//! 端到端集成测试：本地 stub HTTP server 上验证 Provider × 运行时协作。
// 测试代码可放宽 unwrap / expect / panic 限制（规范允许）。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use mycelium_core::error::ModelError;
use mycelium_core::message::{ContentBlock, Role};
use mycelium_core::{AgentConfig, Message, ModelId, ToolError, ToolName, ToolSpec, Usage};
use mycelium_model::{ModelDelta, ModelProvider, ModelRequest, async_trait};
use mycelium_provider_openai::OpenAiProvider;
use mycelium_runtime::{AgentRuntime, RunInput};
use mycelium_tools::{Tool, ToolContext, ToolOutput};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 按脚本顺序应答的本地 HTTP/1.1 服务器（零外部依赖）。
struct StubServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: tokio::task::JoinHandle<()>,
}

/// 脚本化响应。
enum StubResponse {
    /// 200 + `text/event-stream`。
    Sse(String),
    /// 指定状态码 + JSON 体。
    Error { status: u16, body: String },
}

impl StubServer {
    /// 启动服务器，按顺序为每次连接返回一个响应。
    async fn start(responses: Vec<StubResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口");
        let address = listener.local_addr().expect("读取本地地址");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = tokio::spawn(async move {
            for response in responses {
                let Ok((mut socket, _peer)) = listener.accept().await else {
                    break;
                };
                let request = read_request(&mut socket).await.unwrap_or_default();
                captured
                    .lock()
                    .expect("锁未被毒化")
                    .push(String::from_utf8_lossy(&request).into_owned());
                let _ = socket.write_all(&response.render()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            base_url: format!("http://{address}/v1"),
            requests,
            handle,
        }
    }

    /// OpenAI 兼容 base URL。
    fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 已捕获的原始请求（按到达顺序）。
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("锁未被毒化").clone()
    }

    /// 等待服务器脚本执行完毕。
    async fn join(self) {
        let _ = self.handle.await;
    }
}

impl StubResponse {
    /// 渲染为完整 HTTP 响应字节。
    fn render(self) -> Vec<u8> {
        match self {
            Self::Sse(body) => format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .into_bytes(),
            Self::Error { status, body } => format!(
                "HTTP/1.1 {status} StubError\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .into_bytes(),
        }
    }
}

/// 读取完整请求（请求行 + 头部 + `Content-Length` 正文）。
async fn read_request(socket: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut header_end = None;
    loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            header_end = find_subslice(&buffer, b"\r\n\r\n").map(|index| index + 4);
        }
        if let Some(end) = header_end {
            let content_length = parse_content_length(&buffer[..end]).unwrap_or(0);
            if buffer.len() >= end + content_length {
                break;
            }
        }
    }
    Ok(buffer)
}

/// 子串查找。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 解析 `Content-Length`（大小写不敏感）。
fn parse_content_length(head: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(head);
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            return value.trim().parse().ok();
        }
    }
    None
}

/// 把 JSON 事件组装为 SSE 响应体（自动追加 `[DONE]`）。
fn sse(events: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for event in events {
        body.push_str("data: ");
        body.push_str(&event.to_string());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    body
}

/// 回显入参的工具。
struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("echo").expect("合法工具名"),
            "回显入参",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(arguments.to_string()))
    }
}

/// 组装指向 stub server 的运行时。
fn build_runtime(base_url: &str, tools: Vec<Arc<dyn Tool>>) -> AgentRuntime {
    let provider = OpenAiProvider::builder()
        .api_key("test-key")
        .base_url(base_url)
        .build()
        .expect("构建 Provider");
    let config = AgentConfig::builder()
        .model(ModelId::new("gpt-test").expect("模型标识"))
        .build()
        .expect("配置");
    let mut builder = AgentRuntime::builder()
        .config(config)
        .provider(Arc::new(provider));
    for tool in tools {
        builder = builder.tool(tool);
    }
    builder.build().expect("装配运行时")
}

#[tokio::test]
async fn e2e_text_completion() {
    let server = StubServer::start(vec![StubResponse::Sse(sse(&[
        serde_json::json!({"choices":[{"delta":{"reasoning_content":"想一下"},"finish_reason":null}]}),
        serde_json::json!({"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}),
        serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
        serde_json::json!({"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":2,"completion_tokens_details":{"reasoning_tokens":1}}}),
    ]))])
    .await;

    let runtime = build_runtime(server.base_url(), vec![]);
    let outcome = runtime
        .run(RunInput::from_user_text("hi"))
        .await
        .expect("运行应成功");

    assert_eq!(outcome.steps, 1);
    assert_eq!(outcome.usage, Usage::new(5, 2).with_reasoning_tokens(1));
    assert_eq!(outcome.messages[0].role(), Role::Assistant);
    assert_eq!(
        outcome.messages[0].content(),
        &[
            ContentBlock::Thinking {
                text: "想一下".to_owned()
            },
            ContentBlock::Text {
                text: "你好".to_owned()
            },
        ]
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("\"stream\":true"));
    server.join().await;
}

#[tokio::test]
async fn e2e_tool_round_trip() {
    let first = sse(&[
        serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"echo","arguments":"{\"text\":"}}]},"finish_reason":null}]}),
        serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ping\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        serde_json::json!({"choices":[],"usage":{"prompt_tokens":6,"completion_tokens":4}}),
    ]);
    let second = sse(&[
        serde_json::json!({"choices":[{"delta":{"content":"done"},"finish_reason":null}]}),
        serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
    ]);
    let server = StubServer::start(vec![StubResponse::Sse(first), StubResponse::Sse(second)]).await;

    let runtime = build_runtime(server.base_url(), vec![Arc::new(EchoTool)]);
    let outcome = runtime
        .run(RunInput::from_user_text("ping"))
        .await
        .expect("运行应成功");

    assert_eq!(outcome.steps, 2);
    assert_eq!(outcome.messages.len(), 3);
    assert_eq!(outcome.messages[1].role(), Role::Tool);
    match &outcome.messages[1].content()[0] {
        ContentBlock::ToolResult {
            content, is_error, ..
        } => {
            assert_eq!(content, "{\"text\":\"ping\"}");
            assert!(!is_error);
        }
        other => panic!("期望 ToolResult，实际：{other:?}"),
    }
    assert_eq!(
        outcome.messages[2].content(),
        &[ContentBlock::Text {
            text: "done".to_owned()
        }]
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].contains("\"role\":\"tool\""));
    assert!(requests[1].contains("call_1"));
    server.join().await;
}

#[tokio::test]
async fn e2e_stream_satisfies_assembler_contract() {
    let server = StubServer::start(vec![StubResponse::Sse(sse(&[
        serde_json::json!({"choices":[{"delta":{"content":"a"},"finish_reason":null}]}),
        serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
        serde_json::json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1}}),
    ]))])
    .await;

    let provider = OpenAiProvider::builder()
        .api_key("k")
        .base_url(server.base_url())
        .build()
        .expect("构建 Provider");
    let request = ModelRequest::new(
        ModelId::new("gpt-test").expect("模型标识"),
        vec![Message::user_text("hi")],
    );
    let mut stream = provider.stream(request).await.expect("建立流");

    let mut deltas = Vec::new();
    while let Some(item) = stream.next().await {
        deltas.push(item.expect("流不应出错"));
    }

    let usage_position = deltas.iter().position(|d| {
        matches!(
            d,
            ModelDelta::Usage {
                input_tokens: 1,
                output_tokens: 1,
                reasoning_tokens: None,
            }
        )
    });
    let finished_position = deltas
        .iter()
        .position(|d| matches!(d, ModelDelta::Finished { .. }));

    assert_eq!(deltas.len(), 3);
    assert_eq!(
        finished_position,
        Some(deltas.len() - 1),
        "Finished 必须为最后一项"
    );
    assert!(
        usage_position.expect("应有 Usage") < finished_position.expect("应有 Finished"),
        "Usage 必须先于 Finished"
    );
    server.join().await;
}

#[tokio::test]
async fn e2e_error_status_maps_to_api_error() {
    let server = StubServer::start(vec![StubResponse::Error {
        status: 401,
        body: r#"{"error":{"message":"bad key","code":"invalid_api_key"}}"#.to_owned(),
    }])
    .await;

    let provider = OpenAiProvider::builder()
        .api_key("bad")
        .base_url(server.base_url())
        .build()
        .expect("构建 Provider");
    let request = ModelRequest::new(
        ModelId::new("gpt-test").expect("模型标识"),
        vec![Message::user_text("hi")],
    );
    let result = provider.stream(request).await;
    assert!(
        matches!(
            result,
            Err(ModelError::Api {
                status: 401,
                ref code,
                ..
            }) if code.as_deref() == Some("invalid_api_key")
        ),
        "期望 401 Api 错误"
    );
    server.join().await;
}
