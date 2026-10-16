//! 请求体映射（mycelium 领域模型 → OpenAI Chat Completions）。

use serde_json::{Map, Value, json};

use mycelium_core::message::{ContentBlock, Message, Role};
use mycelium_core::tool::ToolChoice;
use mycelium_model::ModelRequest;

/// 构造 Chat Completions 请求体。
pub(crate) fn build_chat_request(request: &ModelRequest) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(system_prompt) = &request.system_prompt {
        messages.push(json!({ "role": "system", "content": system_prompt }));
    }
    for message in &request.messages {
        messages.extend(map_message(message));
    }

    let mut body = Map::new();
    body.insert("model".to_owned(), json!(request.model.as_str()));
    body.insert("messages".to_owned(), Value::Array(messages));
    body.insert("stream".to_owned(), Value::Bool(true));
    body.insert(
        "stream_options".to_owned(),
        json!({ "include_usage": true }),
    );
    if !request.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|spec| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": spec.name().as_str(),
                                "description": spec.description(),
                                "parameters": spec.input_schema(),
                            }
                        })
                    })
                    .collect(),
            ),
        );
        body.insert(
            "tool_choice".to_owned(),
            map_tool_choice(&request.tool_choice),
        );
    }
    if let Some(max_tokens) = request.max_output_tokens {
        body.insert("max_tokens".to_owned(), json!(max_tokens));
    }
    if let Some(temperature) = request.temperature {
        body.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(reasoning) = &request.reasoning {
        match reasoning.enabled {
            Some(true) => {
                body.insert("thinking".to_owned(), json!({ "type": "enabled" }));
            }
            Some(false) => {
                body.insert("thinking".to_owned(), json!({ "type": "disabled" }));
            }
            None => {}
        }
        if let Some(effort) = &reasoning.effort {
            body.insert("reasoning_effort".to_owned(), json!(effort));
        }
    }
    Value::Object(body)
}

/// 单条 mycelium 消息 → 一条或多条 OpenAI 消息。
///
/// 约定：
/// - `Role::Tool` 消息的每个 `ToolResult` 块映射为一条 `role: "tool"` 消息；
/// - 助手消息的 `ToolUse` 块映射为 `tool_calls` 数组（`arguments` 为 JSON 字符串）；
/// - 其余角色的文本块按角色映射（`system` / `user` / `assistant`）。
fn map_message(message: &Message) -> Vec<Value> {
    let mut text = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut tool_results: Vec<Value> = Vec::new();
    for block in message.content() {
        match block {
            ContentBlock::Text { text: block_text } => text.push_str(block_text),
            // 思考（推理）内容属于只读观测：不回传给提供方（契约 §2.20）。
            ContentBlock::Thinking { .. } => {}
            ContentBlock::ToolUse { id, name, input } => tool_calls.push(json!({
                "id": id.as_str(),
                "type": "function",
                "function": { "name": name.as_str(), "arguments": input.to_string() }
            })),
            ContentBlock::ToolResult {
                tool_call_id,
                content,
                ..
            } => tool_results.push(json!({
                "role": "tool",
                "tool_call_id": tool_call_id.as_str(),
                "content": content,
            })),
        }
    }

    if message.role() == Role::Tool {
        return tool_results;
    }

    let mut mapped = Vec::new();
    if message.role() == Role::Assistant {
        let content = if !text.is_empty() {
            json!(text)
        } else if tool_calls.is_empty() {
            json!("")
        } else {
            Value::Null
        };
        let mut entry = Map::new();
        entry.insert("role".to_owned(), json!("assistant"));
        entry.insert("content".to_owned(), content);
        if !tool_calls.is_empty() {
            entry.insert("tool_calls".to_owned(), Value::Array(tool_calls));
        }
        mapped.push(Value::Object(entry));
        mapped.extend(tool_results);
        return mapped;
    }

    let role = if message.role() == Role::System {
        "system"
    } else {
        "user"
    };
    if !text.is_empty() || tool_results.is_empty() {
        mapped.push(json!({ "role": role, "content": text }));
    }
    mapped.extend(tool_results);
    mapped
}

/// 工具选择策略映射。
fn map_tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Required => json!("required"),
        ToolChoice::Tool { name } => {
            json!({ "type": "function", "function": { "name": name.as_str() } })
        }
    }
}

#[cfg(test)]
mod tests {
    use mycelium_core::ids::{ModelId, ToolCallId, ToolName};
    use mycelium_core::tool::ToolSpec;
    use mycelium_model::ReasoningOptions;

    use super::*;

    fn request_with(messages: Vec<Message>) -> ModelRequest {
        ModelRequest::new(ModelId::new("gpt-test").unwrap(), messages)
    }

    #[test]
    fn maps_messages_and_tools() {
        let mut request = request_with(vec![
            Message::system_text("sys"),
            Message::user_text("hi"),
            Message::new(
                Role::Assistant,
                vec![
                    ContentBlock::Text {
                        text: "let me check".to_owned(),
                    },
                    ContentBlock::ToolUse {
                        id: ToolCallId::new("c1").unwrap(),
                        name: ToolName::new("read").unwrap(),
                        input: json!({ "path": "a" }),
                    },
                ],
            )
            .unwrap(),
            Message::tool_result(ToolCallId::new("c1").unwrap(), "file content", false),
        ]);
        request.system_prompt = Some("global system".to_owned());
        request.tools = vec![
            ToolSpec::new(
                ToolName::new("read").unwrap(),
                "read file",
                json!({ "type": "object" }),
            )
            .unwrap(),
        ];
        request.temperature = Some(0.5);
        request.max_output_tokens = Some(64);

        let body = build_chat_request(&request);
        assert_eq!(body["model"], json!("gpt-test"));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["temperature"], json!(0.5));
        assert_eq!(body["max_tokens"], json!(64));

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0]["role"], json!("system"));
        assert_eq!(messages[0]["content"], json!("global system"));
        assert_eq!(messages[1]["role"], json!("system"));
        assert_eq!(messages[1]["content"], json!("sys"));
        assert_eq!(messages[2]["role"], json!("user"));
        assert_eq!(messages[2]["content"], json!("hi"));
        assert_eq!(messages[3]["role"], json!("assistant"));
        assert_eq!(messages[3]["content"], json!("let me check"));
        assert_eq!(messages[3]["tool_calls"][0]["id"], json!("c1"));
        assert_eq!(messages[3]["tool_calls"][0]["type"], json!("function"));
        assert_eq!(
            messages[3]["tool_calls"][0]["function"]["name"],
            json!("read")
        );
        assert_eq!(
            messages[3]["tool_calls"][0]["function"]["arguments"],
            json!("{\"path\":\"a\"}")
        );
        assert_eq!(messages[4]["role"], json!("tool"));
        assert_eq!(messages[4]["tool_call_id"], json!("c1"));
        assert_eq!(messages[4]["content"], json!("file content"));

        assert_eq!(body["tools"][0]["type"], json!("function"));
        assert_eq!(body["tools"][0]["function"]["name"], json!("read"));
        assert_eq!(body["tool_choice"], json!("auto"));
    }

    #[test]
    fn maps_tool_choice_variants() {
        let mut request = request_with(vec![Message::user_text("hi")]);
        request.tools = vec![
            ToolSpec::new(
                ToolName::new("read").unwrap(),
                "",
                json!({ "type": "object" }),
            )
            .unwrap(),
        ];

        for (choice, expected) in [
            (ToolChoice::Auto, json!("auto")),
            (ToolChoice::None, json!("none")),
            (ToolChoice::Required, json!("required")),
            (
                ToolChoice::Tool {
                    name: ToolName::new("read").unwrap(),
                },
                json!({ "type": "function", "function": { "name": "read" } }),
            ),
        ] {
            request.tool_choice = choice;
            let body = build_chat_request(&request);
            assert_eq!(body["tool_choice"], expected);
        }

        // 无工具时不携带 tools / tool_choice
        request.tools.clear();
        let body = build_chat_request(&request);
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn maps_reasoning_options() {
        let mut request = request_with(vec![Message::user_text("hi")]);

        // 未配置：不发送 thinking / reasoning_effort（使用提供方默认）
        let body = build_chat_request(&request);
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());

        // 显式关闭
        request.reasoning = Some(ReasoningOptions::disabled());
        let body = build_chat_request(&request);
        assert_eq!(body["thinking"], json!({ "type": "disabled" }));
        assert!(body.get("reasoning_effort").is_none());

        // 显式启用 + 强度
        request.reasoning = Some(ReasoningOptions::enabled().with_effort("max"));
        let body = build_chat_request(&request);
        assert_eq!(body["thinking"], json!({ "type": "enabled" }));
        assert_eq!(body["reasoning_effort"], json!("max"));

        // 仅强度
        request.reasoning = Some(ReasoningOptions::default().with_effort("low"));
        let body = build_chat_request(&request);
        assert!(body.get("thinking").is_none());
        assert_eq!(body["reasoning_effort"], json!("low"));
    }

    #[test]
    fn strips_thinking_blocks_from_history() {
        let request = request_with(vec![
            Message::new(
                Role::Assistant,
                vec![
                    ContentBlock::Thinking {
                        text: "内部推理".to_owned(),
                    },
                    ContentBlock::Text {
                        text: "答案".to_owned(),
                    },
                ],
            )
            .unwrap(),
        ]);
        let body = build_chat_request(&request);
        let serialized = body["messages"][0].to_string();
        // 思考内容不回传给提供方（DeepSeek 契约：reasoning_content 仅出站）
        assert!(!serialized.contains("内部推理"));
        assert_eq!(body["messages"][0]["content"], json!("答案"));
    }
}
