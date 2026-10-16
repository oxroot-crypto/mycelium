//! MCP 客户端：JSON-RPC 会话、握手与工具能力。

use std::collections::HashMap;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, oneshot};

use mycelium_core::ids::ToolName;
use mycelium_core::tool::ToolSpec;
use mycelium_tools::{Tool, ToolOutput};

use crate::{McpError, McpTool};

/// 客户端协议版本（MCP 规范版本号）。
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// `tools/list` 分页硬上限（防御异常服务端）。
pub const MAX_TOOL_PAGES: usize = 64;

/// 服务端信息（握手结果）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerInfo {
    /// 服务端名称。
    pub name: String,
    /// 服务端版本。
    pub version: String,
    /// 协商后的协议版本。
    pub protocol_version: String,
}

/// 挂起请求表：请求 id → 响应回调。
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, McpError>>>>>;

/// 写入端（装箱，兼容任意流与子进程管道）。
type BoxedWriter = Pin<Box<dyn AsyncWrite + Send>>;

/// MCP 客户端。
pub struct McpClient {
    connection: Connection,
    server_info: McpServerInfo,
    child: Option<Mutex<tokio::process::Child>>,
    reader_task: tokio::task::JoinHandle<()>,
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // 客户端释放即关闭连接：中止读任务会释放读写半部，令对端（子进程 / 假服务器）
        // 立即观察到 EOF；未完成的请求因 oneshot 发送端被丢弃而显式失败。
        self.reader_task.abort();
    }
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("server_info", &self.server_info)
            .field("stdio_child", &self.child.is_some())
            .finish()
    }
}

impl McpClient {
    /// 通过任意双向流连接（测试与自定义传输）。
    ///
    /// # Errors
    /// 握手失败（含传输关闭）返回 [`McpError`]。
    pub async fn connect<S>(stream: S) -> Result<Self, McpError>
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let (reader, writer) = tokio::io::split(stream);
        Self::start(reader, Box::pin(writer), None).await
    }

    /// 通过 stdio 启动并连接子进程（`kill_on_drop`：连接泄漏时子进程随之终止）。
    ///
    /// # Errors
    /// 子进程启动失败或握手失败返回 [`McpError`]。
    pub async fn spawn(command: &str, args: &[&str]) -> Result<Self, McpError> {
        let mut child = tokio::process::Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| McpError::Transport {
                message: format!("启动 MCP 服务失败（{command}）：{error}"),
            })?;
        let stdin = child.stdin.take().ok_or_else(|| McpError::Transport {
            message: "MCP 子进程缺少 stdin".to_owned(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Transport {
            message: "MCP 子进程缺少 stdout".to_owned(),
        })?;
        Self::start(stdout, Box::pin(stdin), Some(child)).await
    }

    /// 组装连接、启动读任务并完成握手。
    async fn start<R>(
        reader: R,
        writer: BoxedWriter,
        child: Option<tokio::process::Child>,
    ) -> Result<Self, McpError>
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let connection = Connection::new(writer);
        let reader_task = Self::spawn_reader_task(
            reader,
            Arc::clone(&connection.pending),
            Arc::clone(&connection.writer),
            Arc::clone(&connection.closed),
        );
        let server_info = match connection.initialize().await {
            Ok(info) => info,
            Err(error) => {
                // 握手失败：中止读任务，避免任务与流泄漏（子进程由 kill_on_drop 回收）。
                reader_task.abort();
                return Err(error);
            }
        };
        Ok(Self {
            connection,
            server_info,
            child: child.map(Mutex::new),
            reader_task,
        })
    }

    /// 读任务：路由响应、忽略通知、显式拒绝未支持的服务端请求；
    /// 传输结束时将全部未完成请求显式失败。返回任务句柄（由客户端在 Drop 时中止）。
    fn spawn_reader_task<R>(
        reader: R,
        pending: PendingMap,
        writer: SharedWriter,
        closed: Arc<AtomicBool>,
    ) -> tokio::task::JoinHandle<()>
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        // 原始 JSON-RPC 入站报文落痕（联调可追溯）。
                        tracing::trace!(target: "mycelium::mcp", direction = "recv", message = line, "JSON-RPC");
                        // 非法 JSON 的行忽略：MCP stdio 传输下服务端不应输出非 JSON 内容，
                        // 但不应因单行脏数据终止整个会话。
                        let Ok(message) = serde_json::from_str::<Value>(line) else {
                            continue;
                        };
                        route_message(&pending, &writer, message).await;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            // 传输关闭：先置关闭标记（与请求注册互斥），再失败全部未完成请求。
            closed.store(true, Ordering::SeqCst);
            let mut guard = pending.lock().await;
            for (_, sender) in guard.drain() {
                let _ = sender.send(Err(McpError::TransportClosed));
            }
        })
    }

    /// 服务端信息（握手结果）。
    pub fn server_info(&self) -> &McpServerInfo {
        &self.server_info
    }

    /// 列出服务端工具（支持 `nextCursor` 分页）。
    ///
    /// # Errors
    /// 传输 / 协议 / RPC 错误，或分页超过 [`MAX_TOOL_PAGES`]。
    pub async fn list_tools(&self) -> Result<Vec<ToolSpec>, McpError> {
        let mut specs = Vec::new();
        let mut cursor: Option<String> = None;
        for _page in 0..MAX_TOOL_PAGES {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.connection.request("tools/list", Some(params)).await?;
            let tools = result
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for tool in tools {
                let raw_name =
                    tool.get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| McpError::Protocol {
                            message: "tools/list 条目缺少 name".to_owned(),
                        })?;
                let name = ToolName::new(raw_name).map_err(|error| McpError::Protocol {
                    message: format!("MCP 工具名非法：{error}"),
                })?;
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let schema = tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object" }));
                let spec = ToolSpec::new(name, description, schema).map_err(|error| {
                    McpError::Protocol {
                        message: format!("MCP 工具 schema 非法：{error}"),
                    }
                })?;
                specs.push(spec);
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|next| !next.is_empty())
                .map(str::to_owned);
            if cursor.is_none() {
                return Ok(specs);
            }
        }
        Err(McpError::Protocol {
            message: format!("tools/list 分页超过上限（{MAX_TOOL_PAGES} 页）"),
        })
    }

    /// 调用服务端工具。
    ///
    /// 语义：`content` 数组中 `type == "text"` 的块按序拼接（换行分隔）为结果文本；
    /// `isError: true` 映射为错误结果（回灌模型，不终止运行）。
    ///
    /// # Errors
    /// 传输 / 协议 / RPC 错误。
    pub async fn call_tool(
        &self,
        name: &ToolName,
        arguments: Value,
    ) -> Result<ToolOutput, McpError> {
        let result = self
            .connection
            .request(
                "tools/call",
                Some(json!({ "name": name.as_str(), "arguments": arguments })),
            )
            .await?;
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut text = String::new();
        if let Some(blocks) = result.get("content").and_then(Value::as_array) {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(part) = block.get("text").and_then(Value::as_str)
                {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(part);
                }
            }
        }
        Ok(if is_error {
            ToolOutput::error(text)
        } else {
            ToolOutput::success(text)
        })
    }

    /// 列出工具并包装为可注册的 [`Tool`] 代理集合。
    ///
    /// # Errors
    /// 见 [`McpClient::list_tools`]。
    pub async fn tools(self: &Arc<Self>) -> Result<Vec<Arc<dyn Tool>>, McpError> {
        let specs = self.list_tools().await?;
        Ok(specs
            .into_iter()
            .map(|spec| Arc::new(McpTool::new(Arc::clone(self), spec)) as Arc<dyn Tool>)
            .collect())
    }

    /// 关闭连接；stdio 模式下终止子进程。
    ///
    /// 语义：先标记关闭并中止读任务（未完成请求显式失败），再回收子进程。
    ///
    /// # Errors
    /// 终止子进程失败时返回 [`McpError::Transport`]。
    pub async fn shutdown(&self) -> Result<(), McpError> {
        self.connection.closed.store(true, Ordering::SeqCst);
        self.reader_task.abort();
        if let Some(child) = &self.child {
            let mut child = child.lock().await;
            child.kill().await.map_err(|error| McpError::Transport {
                message: format!("终止 MCP 子进程失败：{error}"),
            })?;
            // 回收子进程，避免僵尸进程。
            let _ = child.wait().await;
        }
        Ok(())
    }
}

/// 写入端共享句柄。
type SharedWriter = Arc<Mutex<BoxedWriter>>;

/// JSON-RPC 连接：请求 / 通知 / 错误路由。
struct Connection {
    writer: SharedWriter,
    pending: PendingMap,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
}

impl Connection {
    /// 创建连接。
    fn new(writer: BoxedWriter) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 发送请求并等待响应。
    async fn request(&self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        {
            // 注册与关闭检查在同一临界区内，避免「注册后传输才关闭」的悬挂窗口。
            let mut pending = self.pending.lock().await;
            if self.closed.load(Ordering::SeqCst) {
                return Err(McpError::TransportClosed);
            }
            pending.insert(id, sender);
        }
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params.unwrap_or(Value::Null),
        });
        if let Err(error) = self.write_message(&message).await {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(McpError::TransportClosed),
        }
    }

    /// 发送通知（无 id，无响应）。
    async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(Value::Null),
        });
        self.write_message(&message).await
    }

    /// 写入一条换行分隔的 JSON-RPC 消息。
    async fn write_message(&self, message: &Value) -> Result<(), McpError> {
        let mut line = serde_json::to_string(message).map_err(|error| McpError::Protocol {
            message: error.to_string(),
        })?;
        line.push('\n');
        // 原始 JSON-RPC 出站报文落痕（联调可追溯）。
        tracing::trace!(target: "mycelium::mcp", direction = "send", message = line.trim_end(), "JSON-RPC");
        let mut writer = self.writer.lock().await;
        if let Err(error) = writer.write_all(line.as_bytes()).await {
            // 写失败意味着会话不可继续：标记关闭并显式失败（后续请求立即返回 TransportClosed）。
            self.closed.store(true, Ordering::SeqCst);
            return Err(McpError::Transport {
                message: error.to_string(),
            });
        }
        if let Err(error) = writer.flush().await {
            self.closed.store(true, Ordering::SeqCst);
            return Err(McpError::Transport {
                message: error.to_string(),
            });
        }
        Ok(())
    }

    /// 初始化握手：`initialize` → 记录服务端信息 → `notifications/initialized`。
    async fn initialize(&self) -> Result<McpServerInfo, McpError> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "mycelium", "version": env!("CARGO_PKG_VERSION") }
        });
        let result = self.request("initialize", Some(params)).await?;
        let server_info = McpServerInfo {
            name: result
                .pointer("/serverInfo/name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            version: result
                .pointer("/serverInfo/version")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            protocol_version: result
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSION)
                .to_owned(),
        };
        self.notify("notifications/initialized", None).await?;
        Ok(server_info)
    }
}

/// 路由一条入站消息。
async fn route_message(pending: &PendingMap, writer: &SharedWriter, message: Value) {
    let id = message.get("id").and_then(Value::as_u64);

    // 服务端发起的请求 / 通知。
    if message.get("method").is_some() {
        if let Some(id) = id {
            // 当前客户端只声明工具能力；其余方法显式拒绝（不得静默丢弃请求）。
            let reply = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "mycelium MCP 客户端不支持该方法" }
            });
            if let Ok(mut line) = serde_json::to_string(&reply) {
                line.push('\n');
                let mut writer = writer.lock().await;
                // 写回失败时连接大概率已关闭：未完成请求由读任务统一失败。
                let _ = writer.write_all(line.as_bytes()).await;
                let _ = writer.flush().await;
            }
        }
        return;
    }

    // 响应：按 id 路由。
    let Some(id) = id else {
        return;
    };
    let Some(sender) = pending.lock().await.remove(&id) else {
        return;
    };
    let outcome = if let Some(result) = message.get("result") {
        Ok(result.clone())
    } else if let Some(error) = message.get("error") {
        Err(McpError::Rpc {
            code: error.get("code").and_then(Value::as_i64).unwrap_or(-32000),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        })
    } else {
        Err(McpError::Protocol {
            message: "响应缺少 result / error".to_owned(),
        })
    };
    let _ = sender.send(outcome);
}
