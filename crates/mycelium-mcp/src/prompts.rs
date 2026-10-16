//! MCP 提示模板类型（`prompts/list` / `prompts/get`）。

use mycelium_core::message::ToolResultBlock;
use serde::Serialize;
use serde_json::Value;

use crate::McpError;
use crate::content::blocks_from_content;
use crate::resources::{optional_string, required_string};

/// 提示模板参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpPromptArgument {
    /// 参数名。
    pub name: String,
    /// 描述（可选）。
    pub description: Option<String>,
    /// 是否必填（缺省 `false`）。
    pub required: bool,
}

impl McpPromptArgument {
    /// 从 `prompts/list` 条目解析（`name` 必填）。
    pub(crate) fn from_value(value: &Value) -> Result<Self, McpError> {
        Ok(Self {
            name: required_string(value, "name", "prompt argument")?,
            description: optional_string(value, "description"),
            required: value
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// 提示模板描述（`prompts/list` 条目）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpPrompt {
    /// 模板名。
    pub name: String,
    /// 描述（可选）。
    pub description: Option<String>,
    /// 参数列表。
    pub arguments: Vec<McpPromptArgument>,
}

impl McpPrompt {
    /// 从 `prompts/list` 条目解析（`name` 必填）。
    pub(crate) fn from_value(value: &Value) -> Result<Self, McpError> {
        let arguments = value
            .get("arguments")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(McpPromptArgument::from_value)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            name: required_string(value, "name", "prompt")?,
            description: optional_string(value, "description"),
            arguments,
        })
    }
}

/// 提示模板消息（`prompts/get` 条目）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpPromptMessage {
    /// 角色（如 `user` / `assistant`）。
    pub role: String,
    /// 内容块（`text` / `image` 映射；未知类型忽略）。
    pub content: Vec<ToolResultBlock>,
}

impl McpPromptMessage {
    /// 从 `prompts/get` 条目解析（`role` 必填；`content` 为单块或数组）。
    pub(crate) fn from_value(value: &Value) -> Result<Self, McpError> {
        Ok(Self {
            role: required_string(value, "role", "prompt message")?,
            content: blocks_from_content(value.get("content").unwrap_or(&Value::Null)),
        })
    }
}
