//! MCP 客户端集成测试：内存双向流（duplex）上的假服务器。
// 测试代码可放宽 unwrap / expect / panic 限制（规范允许）。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mycelium_core::ids::ToolName;
use mycelium_core::message::ToolResultBlock;
use mycelium_core::tool::ToolSpec;
use mycelium_mcp::{McpClient, McpError};
use mycelium_tools::ToolRegistry;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::task::JoinHandle;

/// 启动假服务器：对每条带 id 的请求回放 `respond` 生成的响应。
async fn fake_server(
    stream: tokio::io::DuplexStream,
    respond: impl Fn(&Value) -> Option<Value> + Send + 'static,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if let Some(response) = respond(&message) {
                let mut text = response.to_string();
                text.push('\n');
                let _ = writer.write_all(text.as_bytes()).await;
                let _ = writer.flush().await;
            }
        }
    })
}

/// 标准假服务器：initialize / tools/list / tools/call。
fn standard_response(message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    match message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": { "name": "fake-server", "version": "1.2.3" }
            }
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    { "name": "echo", "description": "回显", "inputSchema": { "type": "object" } },
                    { "name": "boom", "description": "失败", "inputSchema": { "type": "object" } }
                ]
            }
        })),
        "tools/call" => {
            let tool = message
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let result = match tool {
                "boom" => {
                    json!({ "content": [{ "type": "text", "text": "出错了" }], "isError": true })
                }
                "image" => json!({
                    "content": [
                        { "type": "text", "text": "图片" },
                        { "type": "image", "mimeType": "image/png", "data": "aGVsbG8=" },
                        { "type": "audio", "data": "ignored" }
                    ]
                }),
                _ => {
                    let text = message
                        .pointer("/params/arguments/text")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    json!({ "content": [{ "type": "text", "text": format!("echo:{text}") }] })
                }
            };
            Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
        }
        "resources/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "resources": [
                    { "uri": "file:///data/one.txt", "name": "one", "mimeType": "text/plain" },
                    { "uri": "file:///data/two.bin", "name": "two" }
                ]
            }
        })),
        "resources/read" => {
            let uri = message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let result = match uri {
                "file:///data/one.txt" => {
                    json!({ "contents": [ { "uri": uri, "mimeType": "text/plain", "text": "hello" } ] })
                }
                "file:///data/two.bin" => {
                    json!({ "contents": [ { "uri": uri, "blob": "aGVsbG8=" } ] })
                }
                // 畸形条目：既无 text 也无 blob
                "file:///broken" => json!({ "contents": [ { "uri": uri } ] }),
                _ => json!({ "contents": [] }),
            };
            Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
        }
        "prompts/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "prompts": [
                    {
                        "name": "greet",
                        "description": "打招呼",
                        "arguments": [
                            { "name": "who", "description": "对象", "required": true },
                            { "name": "tone" }
                        ]
                    }
                ]
            }
        })),
        "prompts/get" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "messages": [
                    { "role": "user", "content": { "type": "text", "text": "你好" } },
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": "第二段" },
                            { "type": "image", "mimeType": "image/png", "data": "aGVsbG8=" },
                            { "type": "audio" }
                        ]
                    }
                ]
            }
        })),
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "method not found" }
        })),
    }
}

/// 仅处理 initialize 的响应（其余需求由调用方处理）。
fn init_only_response(message: &Value) -> Option<Value> {
    match message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "initialize" => standard_response(message),
        _ => None,
    }
}

/// 建立一对 connected 客户端 / 假服务器。
async fn connect(
    respond: impl Fn(&Value) -> Option<Value> + Send + 'static,
) -> (McpClient, JoinHandle<()>) {
    let (client_io, server_io) = tokio::io::duplex(16 * 1024);
    let server = fake_server(server_io, respond).await;
    let client = McpClient::connect(client_io).await.expect("握手应成功");
    (client, server)
}

#[tokio::test]
async fn initialize_handshake_records_server_info() {
    let (client, server) = connect(standard_response).await;
    let info = client.server_info();
    assert_eq!(info.name, "fake-server");
    assert_eq!(info.version, "1.2.3");
    assert_eq!(info.protocol_version, "2025-06-18");
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn list_tools_maps_specs() {
    let (client, server) = connect(standard_response).await;
    let specs = client.list_tools().await.unwrap();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].name().as_str(), "echo");
    assert_eq!(specs[0].description(), "回显");
    assert_eq!(specs[0].input_schema(), &json!({ "type": "object" }));
    assert_eq!(specs[1].name().as_str(), "boom");
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn list_tools_follows_pagination() {
    let pages = Arc::new(AtomicUsize::new(0));
    let pages_for_server = Arc::clone(&pages);
    let (client, server) = connect(move |message| {
        if message.get("method").and_then(Value::as_str) == Some("tools/list") {
            let id = message.get("id")?.clone();
            let page = pages_for_server.fetch_add(1, Ordering::SeqCst);
            let result = if page == 0 {
                json!({
                    "tools": [{ "name": "page_one", "description": "", "inputSchema": { "type": "object" } }],
                    "nextCursor": "next"
                })
            } else {
                json!({
                    "tools": [{ "name": "page_two", "description": "", "inputSchema": { "type": "object" } }]
                })
            };
            return Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        }
        init_only_response(message)
    })
    .await;

    let specs = client.list_tools().await.unwrap();
    let names: Vec<&str> = specs.iter().map(|spec| spec.name().as_str()).collect();
    assert_eq!(names, vec!["page_one", "page_two"]);
    assert_eq!(pages.load(Ordering::SeqCst), 2);
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn invalid_tool_schema_is_rejected() {
    let (client, server) = connect(|message| {
        if message.get("method").and_then(Value::as_str) == Some("tools/list") {
            let id = message.get("id")?.clone();
            // inputSchema 不是对象 → 必须显式拒绝
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [{ "name": "bad", "inputSchema": [] }] }
            }));
        }
        init_only_response(message)
    })
    .await;

    let error = client.list_tools().await.unwrap_err();
    assert!(
        matches!(error, McpError::Protocol { .. }),
        "期望 Protocol 错误，实际：{error:?}"
    );
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn call_tool_maps_text_content() {
    let (client, server) = connect(standard_response).await;
    let output = client
        .call_tool(&ToolName::new("echo").unwrap(), json!({ "text": "hi" }))
        .await
        .unwrap();
    assert_eq!(output.text(), "echo:hi");
    assert!(!output.is_error());
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn call_tool_maps_error_flag() {
    let (client, server) = connect(standard_response).await;
    let output = client
        .call_tool(&ToolName::new("boom").unwrap(), json!({}))
        .await
        .unwrap();
    assert!(output.is_error());
    assert_eq!(output.text(), "出错了");
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn call_tool_maps_structured_content() {
    let (client, server) = connect(standard_response).await;
    let output = client
        .call_tool(&ToolName::new("image").unwrap(), json!({}))
        .await
        .unwrap();
    assert!(!output.is_error());
    let blocks = output.content_blocks();
    // text + image 一一映射；audio 等未知类型忽略
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], ToolResultBlock::Text { text } if text == "图片"));
    assert!(matches!(
        &blocks[1],
        ToolResultBlock::Image { media_type, data }
            if media_type == "image/png" && data == "aGVsbG8="
    ));
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn jsonrpc_error_maps_to_rpc_error() {
    let (client, server) = connect(|message| {
        if message.get("method").and_then(Value::as_str) == Some("tools/list") {
            let id = message.get("id")?.clone();
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32602, "message": "invalid params" }
            }));
        }
        init_only_response(message)
    })
    .await;

    let error = client.list_tools().await.unwrap_err();
    match error {
        McpError::Rpc { code, message } => {
            assert_eq!(code, -32602);
            assert_eq!(message, "invalid params");
        }
        other => panic!("期望 Rpc 错误，实际：{other:?}"),
    }
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn transport_close_fails_pending_requests() {
    let (client_io, server_io) = tokio::io::duplex(1024);
    // 服务端立即关闭：握手请求必须显式失败，不得悬挂
    drop(server_io);
    let result = McpClient::connect(client_io).await;
    match result {
        // 闭合已被观察（读任务先看到 EOF）
        Err(McpError::TransportClosed) => {}
        // 写失败先于 EOF 观察：携带 IO 详情的显式失败同样满足契约
        Err(McpError::Transport { .. }) => {}
        other => panic!("期望显式传输失败，实际：{other:?}"),
    }
}

#[tokio::test]
async fn mcp_tools_register_into_registry() {
    let (client, server) = connect(standard_response).await;
    let client = Arc::new(client);
    let tools = client.tools().await.unwrap();
    assert_eq!(tools.len(), 2);

    let mut registry = ToolRegistry::new();
    for tool in tools {
        registry.register(tool).unwrap();
    }
    let echo = registry.get(&ToolName::new("echo").unwrap()).unwrap();
    let output = echo
        .call(
            &mycelium_tools::ToolContext {
                run_id: mycelium_core::RunId::generate(),
                session_id: None,
                call_id: mycelium_core::ToolCallId::new("call_1").unwrap(),
                cancel: mycelium_core::CancellationToken::new(),
            },
            json!({ "text": "via-registry" }),
        )
        .await
        .unwrap();
    assert_eq!(output.text(), "echo:via-registry");

    // 释放全部持有 `Arc<McpClient>` 的句柄（工具依赖客户端，RAII 语义）
    drop(echo);
    drop(registry);
    drop(client);
    let _ = server.await;
}

/// 供编译器确认 `ToolSpec` 类型在测试中的可见性（类型由 `list_tools` 返回）。
#[allow(dead_code)]
fn _assert_spec_type(spec: &ToolSpec) -> &ToolName {
    spec.name()
}

#[tokio::test]
async fn list_resources_maps_specs() {
    let (client, server) = connect(standard_response).await;
    let resources = client.list_resources().await.unwrap();
    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0].uri, "file:///data/one.txt");
    assert_eq!(resources[0].name, "one");
    assert_eq!(resources[0].mime_type.as_deref(), Some("text/plain"));
    assert_eq!(resources[1].description, None);
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn list_resources_follows_pagination() {
    let pages = Arc::new(AtomicUsize::new(0));
    let pages_for_server = Arc::clone(&pages);
    let (client, server) = connect(move |message| {
        if message.get("method").and_then(Value::as_str) == Some("resources/list") {
            let id = message.get("id")?.clone();
            let page = pages_for_server.fetch_add(1, Ordering::SeqCst);
            let result = if page == 0 {
                json!({
                    "resources": [{ "uri": "file:///a", "name": "a" }],
                    "nextCursor": "next"
                })
            } else {
                json!({ "resources": [{ "uri": "file:///b", "name": "b" }] })
            };
            return Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        }
        init_only_response(message)
    })
    .await;

    let resources = client.list_resources().await.unwrap();
    let uris: Vec<&str> = resources
        .iter()
        .map(|resource| resource.uri.as_str())
        .collect();
    assert_eq!(uris, vec!["file:///a", "file:///b"]);
    assert_eq!(pages.load(Ordering::SeqCst), 2);
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn read_resource_maps_contents() {
    let (client, server) = connect(standard_response).await;

    let text = client.read_resource("file:///data/one.txt").await.unwrap();
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].text.as_deref(), Some("hello"));
    assert_eq!(text[0].mime_type.as_deref(), Some("text/plain"));
    assert_eq!(text[0].blob, None);

    let blob = client.read_resource("file:///data/two.bin").await.unwrap();
    assert_eq!(blob[0].blob.as_deref(), Some("aGVsbG8="));
    assert_eq!(blob[0].text, None);

    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn read_resource_rejects_malformed_entry() {
    let (client, server) = connect(standard_response).await;
    let error = client.read_resource("file:///broken").await.unwrap_err();
    match error {
        McpError::Protocol { message } => assert!(message.contains("text 与 blob")),
        other => panic!("期望 Protocol，实际：{other:?}"),
    }
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn list_prompts_maps_specs() {
    let (client, server) = connect(standard_response).await;
    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, "greet");
    assert_eq!(prompts[0].description.as_deref(), Some("打招呼"));
    assert_eq!(prompts[0].arguments.len(), 2);
    assert_eq!(prompts[0].arguments[0].name, "who");
    assert!(prompts[0].arguments[0].required);
    // required 缺省 false
    assert!(!prompts[0].arguments[1].required);
    drop(client);
    let _ = server.await;
}

#[tokio::test]
async fn get_prompt_maps_messages() {
    let (client, server) = connect(standard_response).await;
    let messages = client.get_prompt("greet", None).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    // 单对象内容
    assert_eq!(
        messages[0].content,
        vec![ToolResultBlock::Text {
            text: "你好".to_owned()
        }]
    );
    // 数组内容：text / image 映射，audio 忽略
    assert_eq!(messages[1].content.len(), 2);
    assert!(matches!(
        &messages[1].content[1],
        ToolResultBlock::Image { media_type, data }
            if media_type == "image/png" && data == "aGVsbG8="
    ));
    drop(client);
    let _ = server.await;
}
