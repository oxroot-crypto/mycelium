//! 一次运行的输入与结果。

use mycelium_core::{CancellationToken, Message, Usage};

/// 运行输入。
#[derive(Debug, Clone)]
pub struct RunInput {
    /// 输入消息（作为对话起点；系统提示来自 [`mycelium_core::AgentConfig`]）。
    pub messages: Vec<Message>,
    /// 取消令牌：触发后运行应尽快进入 `Cancelled` 终态。
    pub cancel: CancellationToken,
}

impl RunInput {
    /// 由消息列表构造。
    pub fn from_messages(messages: Vec<Message>) -> Self {
        Self {
            messages,
            cancel: CancellationToken::new(),
        }
    }

    /// 由单条用户文本构造。
    pub fn from_user_text(text: impl Into<String>) -> Self {
        Self::from_messages(vec![Message::user_text(text)])
    }
}

/// 运行结果（成功路径）。
///
/// 语义：`Ok` 即代表运行以 `Completed` 终态结束；取消 / 失败 / 预算超限均以
/// [`mycelium_core::AgentError`] 返回。`messages` 为本次运行**新增**的消息
/// （助手消息与工具结果），不含输入。
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// 本次运行新增的消息。
    pub messages: Vec<Message>,
    /// 累计用量。
    pub usage: Usage,
    /// 实际执行的步数。
    pub steps: u32,
}

impl RunOutcome {
    /// 最后一条消息（通常为模型的收尾回复）。
    pub fn final_message(&self) -> Option<&Message> {
        self.messages.last()
    }
}

#[cfg(test)]
mod tests {
    use mycelium_core::{ContentBlock, Role};

    use super::*;

    #[test]
    fn run_input_from_user_text_shape() {
        let input = RunInput::from_user_text("ping");
        assert_eq!(input.messages.len(), 1);
        let message = input.messages.first().unwrap();
        assert_eq!(message.role(), Role::User);
        assert_eq!(
            message.content(),
            &[ContentBlock::Text {
                text: "ping".to_owned()
            }]
        );
    }

    #[test]
    fn run_outcome_final_message_is_last() {
        let outcome = RunOutcome {
            messages: vec![Message::assistant_text("done")],
            usage: Usage::new(1, 2),
            steps: 1,
        };
        assert_eq!(
            outcome.final_message(),
            Some(&Message::assistant_text("done"))
        );
    }
}
