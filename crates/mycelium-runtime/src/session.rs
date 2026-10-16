//! 会话：多轮对话的消息累积容器。

use serde::{Deserialize, Serialize};

use mycelium_core::{Message, SessionId};

/// 会话。
///
/// 持久化通过 [`crate::store::SessionStore`] 完成；JSON 往返恒等（FC-POST-183）：
/// `{"id": "...", "created_at_ms": 0, "messages": [...]}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    id: SessionId,
    created_at_ms: u64,
    messages: Vec<Message>,
}

impl Session {
    /// 创建新会话。
    ///
    /// # Arguments
    /// * `now_ms` - 创建时间戳（Unix epoch 毫秒），注入以便测试。
    pub fn new(now_ms: u64) -> Self {
        Self {
            id: SessionId::generate(),
            created_at_ms: now_ms,
            messages: Vec::new(),
        }
    }

    /// 会话标识。
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// 创建时间戳（Unix epoch 毫秒）。
    pub fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }

    /// 全部消息（按追加顺序）。
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// 追加一条消息。
    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// 消息数量。
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// 是否尚无消息。
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_empty() {
        let session = Session::new(1_000);
        assert!(session.is_empty());
        assert_eq!(session.len(), 0);
        assert_eq!(session.created_at_ms(), 1_000);
        assert!(session.messages().is_empty());
    }

    #[test]
    fn session_collects_messages_in_order() {
        let mut session = Session::new(0);
        session.push(Message::user_text("first"));
        session.push(Message::assistant_text("second"));

        assert_eq!(session.len(), 2);
        assert!(!session.is_empty());
        assert_eq!(
            session.messages().first(),
            Some(&Message::user_text("first"))
        );
        assert_eq!(
            session.messages().last(),
            Some(&Message::assistant_text("second"))
        );
    }

    #[test]
    fn session_json_roundtrip() {
        let mut session = Session::new(1_700_000_000_000);
        session.push(Message::user_text("你好"));
        session.push(
            Message::new(
                mycelium_core::Role::Assistant,
                vec![
                    mycelium_core::ContentBlock::Thinking {
                        text: "思考".to_owned(),
                    },
                    mycelium_core::ContentBlock::Text {
                        text: "回复".to_owned(),
                    },
                ],
            )
            .unwrap(),
        );

        let json = serde_json::to_string(&session).unwrap();
        let decoded: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, session);
    }
}
