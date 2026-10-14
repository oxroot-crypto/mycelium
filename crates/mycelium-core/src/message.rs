//! 对话消息与内容块。
//!
//! 消息是模型、工具与事件之间传递的最小单元；wire 格式契约见
//! `docs/spec/core.md` 的 FC-PRE-010 / FC-POST-001 / FC-POST-002。

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::ContentError;
use crate::ids::{ToolCallId, ToolName};

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 系统提示。
    System,
    /// 用户输入。
    User,
    /// 模型输出。
    Assistant,
}

/// 内容块。
///
/// wire 格式以 `type` 字段区分变体；`text` / `tool_use` / `tool_result` 为稳定标签，
/// 增删或重命名属于破坏性变更（见 FC-Matrix §5）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// 纯文本。
    Text {
        /// 文本内容（允许为空串）。
        text: String,
    },
    /// 模型发起的工具调用。
    ToolUse {
        /// 提供方生成的调用标识。
        id: ToolCallId,
        /// 被调用的工具名。
        name: ToolName,
        /// 调用参数（JSON；校验由工具与运行时负责）。
        input: serde_json::Value,
    },
    /// 工具执行结果（回灌给模型）。
    ToolResult {
        /// 对应的工具调用标识。
        tool_call_id: ToolCallId,
        /// 结果文本。
        content: String,
        /// 是否为错误结果。
        is_error: bool,
    },
}

/// 一条对话消息。
///
/// 不变量：`content` 至少包含一个内容块；构造（含反序列化）路径均强制校验。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Message {
    role: Role,
    content: Vec<ContentBlock>,
}

impl Message {
    /// 构造消息。
    ///
    /// # Errors
    /// `content` 为空时返回 [`ContentError::EmptyContent`]。
    pub fn new(role: Role, content: Vec<ContentBlock>) -> Result<Self, ContentError> {
        if content.is_empty() {
            return Err(ContentError::EmptyContent);
        }
        Ok(Self { role, content })
    }

    /// 构造只含一个文本块的系统消息。
    pub fn system_text(text: impl Into<String>) -> Self {
        Self::text(Role::System, text)
    }

    /// 构造只含一个文本块的用户消息。
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }

    /// 构造只含一个文本块的模型消息。
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::text(Role::Assistant, text)
    }

    /// 构造只含一个文本块的消息（`text` 允许为空串）。
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// 返回消息角色。
    pub fn role(&self) -> Role {
        self.role
    }

    /// 返回内容块切片。
    pub fn content(&self) -> &[ContentBlock] {
        &self.content
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        /// 反序列化中间结构；校验通过后才构造 [`Message`]。
        #[derive(Deserialize)]
        struct RawMessage {
            role: Role,
            content: Vec<ContentBlock>,
        }

        let raw = RawMessage::deserialize(deserializer)?;
        Self::new(raw.role, raw.content).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn assert_single_text(message: &Message, role: Role, text: &str) {
        assert_eq!(message.role(), role);
        assert_eq!(message.content().len(), 1);
        assert_eq!(
            message.content().first(),
            Some(&ContentBlock::Text {
                text: text.to_owned()
            })
        );
    }

    #[test]
    fn new_rejects_empty_content() {
        assert_eq!(
            Message::new(Role::User, Vec::new()).unwrap_err(),
            ContentError::EmptyContent
        );
        // 单文本块（文本可为空串）合法
        assert!(
            Message::new(
                Role::User,
                vec![ContentBlock::Text {
                    text: String::new()
                }]
            )
            .is_ok()
        );
    }

    #[test]
    fn empty_content_deserialization_is_rejected() {
        let raw = r#"{"role":"user","content":[]}"#;
        assert!(serde_json::from_str::<Message>(raw).is_err());
    }

    #[test]
    fn text_constructors_produce_role_and_single_text_block() {
        assert_single_text(&Message::system_text("sys"), Role::System, "sys");
        assert_single_text(&Message::user_text("hi"), Role::User, "hi");
        assert_single_text(&Message::assistant_text("yo"), Role::Assistant, "yo");
        assert_single_text(&Message::text(Role::User, ""), Role::User, "");
    }

    #[test]
    fn message_json_roundtrip() {
        let message = Message::user_text("你好 🌱");
        let json = serde_json::to_string(&message).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn message_wire_format_is_stable() {
        let value = serde_json::to_value(Message::user_text("hi")).unwrap();
        assert_eq!(
            value,
            json!({ "role": "user", "content": [ { "type": "text", "text": "hi" } ] })
        );
    }
}
