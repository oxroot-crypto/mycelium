//! 模型响应的共享词汇：停止原因与 Token 用量。
//!
//! 对应契约见 `docs/spec/core.md` 的 FC-POST-004 / FC-POST-005 / FC-POST-006。

use serde::{Deserialize, Serialize};

use crate::message::Message;

/// Token 用量。
///
/// `reasoning_tokens` 为思考（推理）Token 数，语义上**包含在** `output_tokens` 内
/// （提供方未报告时为 0）；[`Usage::total_tokens`] 不会重复累加。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// 输入 Token 数。
    pub input_tokens: u64,
    /// 输出 Token 数。
    pub output_tokens: u64,
    /// 思考（推理）Token 数（0 = 未知或未启用思考）。
    pub reasoning_tokens: u64,
}

impl Usage {
    /// 构造用量统计（`reasoning_tokens = 0`）。
    pub fn new(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
            reasoning_tokens: 0,
        }
    }

    /// 设置思考 Token 数。
    pub fn with_reasoning_tokens(mut self, reasoning_tokens: u64) -> Self {
        self.reasoning_tokens = reasoning_tokens;
        self
    }

    /// 总 Token 数；溢出时饱和到 [`u64::MAX`]（不 panic、不回绕）。
    ///
    /// `reasoning_tokens` 已包含在 `output_tokens` 内，不重复累加。
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// 模型停止原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// 模型自然结束本轮输出。
    EndTurn,
    /// 模型请求调用工具。
    ToolUse,
    /// 达到输出 Token 上限。
    MaxTokens,
    /// 命中停止序列。
    StopSequence,
    /// 其他（提供方特有原因）。
    Other,
}

/// 一次模型调用的完整结果（由流式增量装配得到）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelResponse {
    /// 助手消息（文本与工具调用）。
    pub message: Message,
    /// 停止原因。
    pub stop_reason: StopReason,
    /// 本次请求的用量（流未提供时为 0）。
    pub usage: Usage,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn total_tokens_sums() {
        assert_eq!(Usage::new(10, 5).total_tokens(), 15);
        assert_eq!(Usage::default().total_tokens(), 0);
    }

    #[test]
    fn reasoning_tokens_do_not_double_count() {
        let usage = Usage::new(10, 20).with_reasoning_tokens(15);
        assert_eq!(usage.reasoning_tokens, 15);
        // reasoning ⊆ output，不重复累加
        assert_eq!(usage.total_tokens(), 30);
    }

    #[test]
    fn total_tokens_saturates() {
        assert_eq!(Usage::new(u64::MAX, 1).total_tokens(), u64::MAX);
        assert_eq!(Usage::new(u64::MAX, u64::MAX).total_tokens(), u64::MAX);
    }

    #[test]
    fn stop_reason_wire_format_is_snake_case() {
        for (reason, expected) in [
            (StopReason::EndTurn, "end_turn"),
            (StopReason::ToolUse, "tool_use"),
            (StopReason::MaxTokens, "max_tokens"),
            (StopReason::StopSequence, "stop_sequence"),
            (StopReason::Other, "other"),
        ] {
            assert_eq!(serde_json::to_value(reason).unwrap(), json!(expected));
            let roundtrip: StopReason = serde_json::from_value(json!(expected)).unwrap();
            assert_eq!(roundtrip, reason);
        }
    }
}
