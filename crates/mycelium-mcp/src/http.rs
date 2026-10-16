//! MCP HTTP（streamable HTTP）传输。
//!
//! 协议要点（FC-POST-234 / FC-ERR-090 / FC-ERR-091）：
//! - 单端点 POST JSON-RPC；`Accept: application/json, text/event-stream`；
//! - 响应为 `application/json`（单条消息）或 `text/event-stream`（SSE，按 id 匹配）；
//! - `initialize` 响应可携带 `Mcp-Session-Id`，客户端附加到后续请求；
//! - 通知接受 2xx（通常 202 Accepted）。

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::McpError;
use crate::transport::Transport;

/// Streamable HTTP 传输。
pub(crate) struct HttpTransport {
    client: reqwest::Client,
    url: String,
    session_id: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl HttpTransport {
    /// 创建传输（显式安装 rustls ring 后端，幂等）。
    pub(crate) fn new(url: impl Into<String>) -> Result<Self, McpError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| McpError::Transport {
                message: format!("HTTP 客户端构建失败：{error}"),
            })?;
        Ok(Self {
            client,
            url: url.into(),
            session_id: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    /// 发送一条 JSON-RPC 消息（POST）。
    async fn post(&self, message: &Value) -> Result<reqwest::Response, McpError> {
        let mut request = self
            .client
            .post(&self.url)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .json(message);
        if let Some(session_id) = self.session_id.lock().await.clone() {
            request = request.header("mcp-session-id", session_id);
        }
        request.send().await.map_err(|error| McpError::Transport {
            message: error.to_string(),
        })
    }

    /// 捕获 `Mcp-Session-Id` 响应头（initialize 时由服务端下发）。
    async fn capture_session(&self, response: &reqwest::Response) {
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            *self.session_id.lock().await = Some(session_id.to_owned());
        }
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn request(&self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params.unwrap_or(Value::Null),
        });
        let response = self.post(&message).await?;
        self.capture_session(&response).await;

        let status = response.status();
        if !status.is_success() {
            return Err(McpError::Transport {
                message: format!("HTTP {status}"),
            });
        }
        let is_sse = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        if is_sse {
            consume_sse(response.bytes_stream(), id).await
        } else {
            let body = response.text().await.map_err(|error| McpError::Transport {
                message: error.to_string(),
            })?;
            let message: Value =
                serde_json::from_str(&body).map_err(|error| McpError::Protocol {
                    message: format!("HTTP 响应不是合法 JSON：{error}"),
                })?;
            interpret_message(&message, id)
        }
    }

    async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(Value::Null),
        });
        let response = self.post(&message).await?;
        self.capture_session(&response).await;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(McpError::Transport {
                message: format!("HTTP {status}"),
            })
        }
    }
}

/// 解释一条 JSON-RPC 响应（与 stdio 相同的语义）。
fn interpret_message(message: &Value, expected_id: u64) -> Result<Value, McpError> {
    if message.get("id").and_then(Value::as_u64) != Some(expected_id) {
        return Err(McpError::Protocol {
            message: "响应 id 与请求不匹配".to_owned(),
        });
    }
    if let Some(result) = message.get("result") {
        return Ok(result.clone());
    }
    if let Some(error) = message.get("error") {
        return Err(McpError::Rpc {
            code: error.get("code").and_then(Value::as_i64).unwrap_or(-32000),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        });
    }
    Err(McpError::Protocol {
        message: "响应缺少 result / error".to_owned(),
    })
}

/// 消费 SSE 流直至匹配响应；其余消息（通知等）忽略；流提前结束 → `TransportClosed`。
async fn consume_sse<S, B>(bytes: S, expected_id: u64) -> Result<Value, McpError>
where
    S: futures_util::Stream<Item = Result<B, reqwest::Error>> + Send,
    B: AsRef<[u8]>,
{
    let mut bytes = Box::pin(bytes);
    let mut buffer = String::new();
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|error| McpError::Transport {
            message: error.to_string(),
        })?;
        buffer.push_str(&String::from_utf8_lossy(chunk.as_ref()));
        // 统一换行符：兼容 CRLF 与 LF。
        if buffer.contains('\r') {
            buffer = buffer.replace("\r\n", "\n");
        }
        while let Some(index) = buffer.find("\n\n") {
            let event: String = buffer.drain(..index + 2).collect();
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                let Ok(message) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                // 只处理匹配 id 的响应；其余消息忽略。
                if message.get("id").and_then(Value::as_u64) == Some(expected_id) {
                    return interpret_message(&message, expected_id);
                }
            }
        }
    }
    Err(McpError::TransportClosed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_handles_result_error_and_mismatch() {
        let result = interpret_message(&json!({ "id": 1, "result": { "ok": true } }), 1).unwrap();
        assert_eq!(result, json!({ "ok": true }));

        let error = interpret_message(
            &json!({ "id": 2, "error": { "code": -32602, "message": "bad" } }),
            2,
        )
        .unwrap_err();
        assert!(matches!(error, McpError::Rpc { code: -32602, .. }));

        assert!(matches!(
            interpret_message(&json!({ "id": 2, "result": {} }), 3),
            Err(McpError::Protocol { .. })
        ));
    }

    #[tokio::test]
    async fn consume_sse_skips_unrelated_messages() {
        let chunks = vec![
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n".to_owned(),
            "data: {\"id\":7,\"result\":{\"value\":42}}\n\n".to_owned(),
        ];
        let stream = futures_util::stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<Vec<u8>, reqwest::Error>(chunk.into_bytes())),
        );
        let result = consume_sse(stream, 7).await.unwrap();
        assert_eq!(result, json!({ "value": 42 }));
    }

    #[tokio::test]
    async fn consume_sse_ending_without_response_is_closed() {
        let chunks = vec!["data: {\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n\n".to_owned()];
        let stream = futures_util::stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<Vec<u8>, reqwest::Error>(chunk.into_bytes())),
        );
        let result = consume_sse(stream, 1).await;
        assert!(matches!(result, Err(McpError::TransportClosed)));
    }
}
