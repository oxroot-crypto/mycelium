//! 会话：多轮对话的消息累积容器。

use mycelium_core::{Message, SessionId};

/// 会话。
///
/// v0.1 为进程内存储；持久化与恢复自 v0.4 起提供（见 roadmap）。
#[derive(Debug, Clone)]
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
}
