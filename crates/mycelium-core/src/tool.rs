//! 工具协议：规格（对模型可见）、调用（模型发起）与选择策略。
//!
//! 对应契约见 `docs/spec/core.md` 的 FC-PRE-020 / FC-POST-003 / FC-POST-007。

use serde::{Deserialize, Serialize};

use crate::error::ToolError;
use crate::ids::{ToolCallId, ToolName};
use crate::message::ContentBlock;

/// 工具规格。
///
/// 不变量：`input_schema` 必须是 JSON 对象（主流提供方要求 JSON Schema 顶层为 object）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    name: ToolName,
    description: String,
    input_schema: serde_json::Value,
}

impl ToolSpec {
    /// 构造工具规格。
    ///
    /// # Errors
    /// 当 `input_schema` 不是 JSON 对象时返回 [`ToolError::InvalidSpec`]（含工具名与实际类型）。
    pub fn new(
        name: ToolName,
        description: impl Into<String>,
        input_schema: serde_json::Value,
    ) -> Result<Self, ToolError> {
        if !input_schema.is_object() {
            return Err(ToolError::InvalidSpec {
                tool: name,
                detail: format!(
                    "input_schema 必须是 JSON 对象，实际为 {}",
                    json_kind(&input_schema)
                ),
            });
        }
        Ok(Self {
            name,
            description: description.into(),
            input_schema,
        })
    }

    /// 工具名。
    pub fn name(&self) -> &ToolName {
        &self.name
    }

    /// 工具描述（供模型理解用途）。
    pub fn description(&self) -> &str {
        &self.description
    }

    /// 入参 JSON Schema。
    pub fn input_schema(&self) -> &serde_json::Value {
        &self.input_schema
    }
}

/// 返回 JSON 值的人类可读类型名（用于错误上下文化）。
fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// 一次完整的工具调用（由模型产出、由运行时执行）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// 提供方生成的调用标识。
    pub id: ToolCallId,
    /// 被调用的工具名。
    pub name: ToolName,
    /// 调用参数（JSON）。
    pub arguments: serde_json::Value,
}

impl ToolCall {
    /// 构造工具调用。
    pub fn new(id: ToolCallId, name: ToolName, arguments: serde_json::Value) -> Self {
        Self {
            id,
            name,
            arguments,
        }
    }

    /// 转换为消息内容块（在助手消息中回放工具调用时使用）。
    pub fn into_content_block(self) -> ContentBlock {
        ContentBlock::ToolUse {
            id: self.id,
            name: self.name,
            input: self.arguments,
        }
    }
}

/// 工具选择策略（wire 格式：对象包装，`mode` 字段区分变体）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ToolChoice {
    /// 由模型自行决定是否调用工具（默认）。
    #[default]
    Auto,
    /// 禁止调用工具。
    None,
    /// 必须调用至少一个工具。
    Required,
    /// 强制调用指定工具。
    Tool {
        /// 目标工具名。
        name: ToolName,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn spec(name: &str, schema: serde_json::Value) -> Result<ToolSpec, ToolError> {
        ToolSpec::new(ToolName::new(name).unwrap(), "desc", schema)
    }

    #[test]
    fn spec_rejects_non_object_schema() {
        for schema in [json!(null), json!(true), json!(1), json!("s"), json!([])] {
            let err = spec("echo", schema).unwrap_err();
            assert!(
                matches!(err, ToolError::InvalidSpec { ref tool, .. } if tool.as_str() == "echo"),
                "应拒绝非对象 schema：{err:?}"
            );
        }
    }

    #[test]
    fn spec_accepts_object_schema() {
        let ok = spec("echo", json!({ "type": "object" })).unwrap();
        assert_eq!(ok.name().as_str(), "echo");
        assert_eq!(ok.description(), "desc");
        assert_eq!(ok.input_schema(), &json!({ "type": "object" }));
    }

    #[test]
    fn tool_call_into_block_preserves_fields() {
        let call = ToolCall::new(
            ToolCallId::new("call_1").unwrap(),
            ToolName::new("echo").unwrap(),
            json!({ "text": "hi" }),
        );
        let block = call.clone().into_content_block();
        let ContentBlock::ToolUse { id, name, input } = block else {
            panic!("期望 ToolUse 变体");
        };
        assert_eq!(id, call.id);
        assert_eq!(name, call.name);
        assert_eq!(input, call.arguments);
    }

    #[test]
    fn tool_wire_format_is_stable() {
        let spec = spec("echo", json!({ "type": "object" })).unwrap();
        let spec_roundtrip: ToolSpec =
            serde_json::from_value(serde_json::to_value(&spec).unwrap()).unwrap();
        assert_eq!(spec_roundtrip, spec);

        let call = ToolCall::new(
            ToolCallId::new("c1").unwrap(),
            ToolName::new("echo").unwrap(),
            json!({}),
        );
        let call_roundtrip: ToolCall =
            serde_json::from_value(serde_json::to_value(&call).unwrap()).unwrap();
        assert_eq!(call_roundtrip, call);

        assert_eq!(
            serde_json::to_value(ToolChoice::Auto).unwrap(),
            json!({ "mode": "auto" })
        );
        assert_eq!(
            serde_json::to_value(ToolChoice::None).unwrap(),
            json!({ "mode": "none" })
        );
        assert_eq!(
            serde_json::to_value(ToolChoice::Required).unwrap(),
            json!({ "mode": "required" })
        );

        let forced = ToolChoice::Tool {
            name: ToolName::new("echo").unwrap(),
        };
        assert_eq!(
            serde_json::to_value(&forced).unwrap(),
            json!({ "mode": "tool", "name": "echo" })
        );
        let forced_roundtrip: ToolChoice =
            serde_json::from_value(serde_json::to_value(&forced).unwrap()).unwrap();
        assert_eq!(forced_roundtrip, forced);
    }
}
