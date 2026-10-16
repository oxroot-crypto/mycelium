//! MCP HTTP（streamable HTTP）传输集成测试：本地 stub server。
// 测试代码可放宽 unwrap / expect / panic 限制（规范允许）。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mycelium_mcp::{McpClient, McpError};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 脚本化 HTTP 响应（每个 POST 消费一个）。
enum StubResponse {
    /// JSON 响应（可携带 `Mcp-Session-Id`）。
    Json {
        status: u16,
        session: Option<&'static str>,
        body: Value,
    },
    /// SSE 响应。
    Sse { body: String },
    /// 空响应（如 202 Accepted）。
    Empty { status: u16 },
}

/// 本地 stub server。
struct StubServer {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl StubServer {
    /// 启动并按下标顺序回放响应。
    async fn start(responses: Vec<StubResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口");
        let address = listener.local_addr().expect("本地地址");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = tokio::spawn(async move {
            let mut queue: VecDeque<StubResponse> = responses.into();
            while let Some(response) = queue.pop_front() {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let request = read_request(&mut socket).await.unwrap_or_default();
                captured
                    .lock()
                    .expect("锁未被毒化")
                    .push(String::from_utf8_lossy(&request).into_owned());
                let _ = socket.write_all(&render(response)).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            endpoint: format!("http://{address}/mcp"),
            requests,
            handle,
        }
    }

    /// MCP 端点。
    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 已捕获原始请求。
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("锁未被毒化").clone()
    }

    /// 中止服务器（避免脚本未耗尽时等待）。
    fn stop(self) {
        self.handle.abort();
    }
}

/// 渲染响应字节。
fn render(response: StubResponse) -> Vec<u8> {
    match response {
        StubResponse::Json {
            status,
            session,
            body,
        } => {
            let body = body.to_string();
            let session_header = session
                .map(|session| format!("mcp-session-id: {session}\r\n"))
                .unwrap_or_default();
            format!(
                "HTTP/1.1 {status} Stub\r\ncontent-type: application/json\r\n{session_header}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_bytes()
        }
        StubResponse::Sse { body } => format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
        StubResponse::Empty { status } => {
            format!("HTTP/1.1 {status} Stub\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .into_bytes()
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

/// 标准脚本：initialize（带会话头）+ initialized 通知（202）。
fn initialized_script() -> Vec<StubResponse> {
    vec![
        StubResponse::Json {
            status: 200,
            session: Some("sess-1"),
            body: json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "serverInfo": { "name": "http-stub", "version": "0.1.0" }
                }
            }),
        },
        StubResponse::Empty { status: 202 },
    ]
}

#[tokio::test]
async fn handshake_and_session_header() {
    let mut script = initialized_script();
    script.push(StubResponse::Json {
        status: 200,
        session: None,
        body: json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "tools": [{ "name": "echo", "inputSchema": { "type": "object" } }] }
        }),
    });
    let server = StubServer::start(script).await;

    let client = McpClient::connect_http(server.endpoint()).await.unwrap();
    assert_eq!(client.server_info().name, "http-stub");
    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name().as_str(), "echo");

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("initialize"));
    assert!(requests[1].contains("notifications/initialized"));
    // 会话头在 initialize 之后附加到所有请求
    assert!(requests[2].contains("mcp-session-id: sess-1"));

    client.shutdown().await.unwrap();
    server.stop();
}

#[tokio::test]
async fn sse_response_is_matched_by_id() {
    let mut script = initialized_script();
    script.push(StubResponse::Sse {
        body: concat!(
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"p\":1}}\n\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n"
        )
        .to_owned(),
    });
    let server = StubServer::start(script).await;

    let client = McpClient::connect_http(server.endpoint()).await.unwrap();
    // 前置通知被忽略，按 id 匹配到响应
    let tools = client.list_tools().await.unwrap();
    assert!(tools.is_empty());

    client.shutdown().await.unwrap();
    server.stop();
}

#[tokio::test]
async fn jsonrpc_error_over_http() {
    let mut script = initialized_script();
    script.push(StubResponse::Json {
        status: 200,
        session: None,
        body: json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": { "code": -32602, "message": "invalid params" }
        }),
    });
    let server = StubServer::start(script).await;

    let client = McpClient::connect_http(server.endpoint()).await.unwrap();
    let error = client.list_tools().await.unwrap_err();
    match error {
        McpError::Rpc { code, message } => {
            assert_eq!(code, -32602);
            assert_eq!(message, "invalid params");
        }
        other => panic!("期望 Rpc，实际：{other:?}"),
    }

    client.shutdown().await.unwrap();
    server.stop();
}

#[tokio::test]
async fn http_error_status_maps_to_transport() {
    let mut script = initialized_script();
    script.push(StubResponse::Json {
        status: 500,
        session: None,
        body: json!({ "message": "boom" }),
    });
    let server = StubServer::start(script).await;

    let client = McpClient::connect_http(server.endpoint()).await.unwrap();
    let error = client.list_tools().await.unwrap_err();
    match error {
        McpError::Transport { message } => assert!(message.contains("500")),
        other => panic!("期望 Transport，实际：{other:?}"),
    }

    client.shutdown().await.unwrap();
    server.stop();
}

#[tokio::test]
async fn sse_ending_before_response_is_closed() {
    let mut script = initialized_script();
    script.push(StubResponse::Sse {
        body: "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n".to_owned(),
    });
    let server = StubServer::start(script).await;

    let client = McpClient::connect_http(server.endpoint()).await.unwrap();
    let error = client.list_tools().await.unwrap_err();
    assert!(
        matches!(error, McpError::TransportClosed),
        "期望 TransportClosed，实际：{error:?}"
    );

    client.shutdown().await.unwrap();
    server.stop();
}

/// 访问辅助（避免未使用告警；测试内保留以便调试）。
#[allow(dead_code)]
async fn _wait_a_bit() {
    tokio::time::sleep(Duration::from_millis(1)).await;
}
