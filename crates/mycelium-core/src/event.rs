//! 运行事件模型。
//!
//! 事件是可序列化的 value object，作为观测与传输的统一格式；事件顺序不变量
//! （`seq` 连续、终态唯一等）属于 v0.2 运行时契约，见 `docs/spec/core.md` §4。
//!
//! 对应契约：FC-POST-010 / FC-POST-011。

use serde::{Deserialize, Serialize};

use crate::ids::{ModelId, RunId, ToolCallId, ToolName};
use crate::response::{StopReason, Usage};
use crate::tool::ToolCall;

/// 事件信封：元信息 + 载荷。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    /// 所属运行。
    pub run_id: RunId,
    /// 运行内序号（运行时保证从 0 连续递增，见 FC-Matrix §4）。
    pub seq: u64,
    /// 事件发生时间（Unix epoch 毫秒）。
    pub at_ms: u64,
    /// 事件载荷（wire 格式展平到信封同层，以 `type` 字段区分变体）。
    #[serde(flatten)]
    pub kind: AgentEventKind,
}

/// 事件载荷；wire 格式以 `type` 字段区分变体（稳定标签，破坏性变更需走契约变更流程）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEventKind {
    /// 运行开始。
    RunStarted {
        /// 输入消息数量。
        message_count: usize,
    },
    /// 一步（一轮模型调用及其工具执行）开始。
    StepStarted {
        /// 步序号（运行时保证从 1 开始连续递增）。
        step: u32,
    },
    /// 模型请求已发出。
    ModelRequestStarted {
        /// 目标模型。
        model: ModelId,
        /// 消息数量。
        message_count: usize,
        /// 可用工具数量。
        tool_count: usize,
    },
    /// 模型文本增量。
    ModelTextDelta {
        /// 增量文本。
        text: String,
    },
    /// 模型思考（推理）文本增量。
    ModelThinkingDelta {
        /// 增量文本。
        text: String,
    },
    /// 模型产出的完整工具调用（装配完成）。
    ModelToolCall {
        /// 装配后的工具调用。
        call: ToolCall,
    },
    /// 模型请求完成。
    ModelRequestFinished {
        /// 停止原因。
        stop_reason: StopReason,
        /// 本次请求的用量。
        usage: Usage,
    },
    /// 工具调用开始执行。
    ToolCallStarted {
        /// 调用标识。
        call_id: ToolCallId,
        /// 工具名。
        name: ToolName,
    },
    /// 工具调用执行结束（成功或错误结果均在此上报）。
    ToolCallFinished {
        /// 调用标识。
        call_id: ToolCallId,
        /// 工具名。
        name: ToolName,
        /// 结果是否为错误。
        is_error: bool,
    },
    /// 运行成功完成（终态事件）。
    RunCompleted {
        /// 运行累计用量。
        usage: Usage,
        /// 运行总步数。
        steps: u32,
    },
    /// 运行失败（终态事件）。
    RunFailed {
        /// 人类可读的失败描述（结构化错误经 `Result` 返回链路传递）。
        message: String,
    },
    /// 运行被取消（终态事件）。
    RunCancelled,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sample_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            run_id: RunId::generate(),
            seq: 0,
            at_ms: 1_700_000_000_000,
            kind,
        }
    }

    fn tool_call() -> ToolCall {
        ToolCall::new(
            ToolCallId::new("call_1").unwrap(),
            ToolName::new("echo").unwrap(),
            json!({ "text": "hi" }),
        )
    }

    fn all_kinds() -> Vec<AgentEventKind> {
        vec![
            AgentEventKind::RunStarted { message_count: 1 },
            AgentEventKind::StepStarted { step: 1 },
            AgentEventKind::ModelRequestStarted {
                model: ModelId::new("test-model").unwrap(),
                message_count: 1,
                tool_count: 1,
            },
            AgentEventKind::ModelTextDelta {
                text: "hel".to_owned(),
            },
            AgentEventKind::ModelThinkingDelta {
                text: "先想".to_owned(),
            },
            AgentEventKind::ModelToolCall { call: tool_call() },
            AgentEventKind::ModelRequestFinished {
                stop_reason: StopReason::ToolUse,
                usage: Usage::new(10, 5),
            },
            AgentEventKind::ToolCallStarted {
                call_id: ToolCallId::new("call_1").unwrap(),
                name: ToolName::new("echo").unwrap(),
            },
            AgentEventKind::ToolCallFinished {
                call_id: ToolCallId::new("call_1").unwrap(),
                name: ToolName::new("echo").unwrap(),
                is_error: false,
            },
            AgentEventKind::RunCompleted {
                usage: Usage::new(10, 5),
                steps: 1,
            },
            AgentEventKind::RunFailed {
                message: "model exploded".to_owned(),
            },
            AgentEventKind::RunCancelled,
        ]
    }

    #[test]
    fn event_json_roundtrip_all_kinds() {
        for kind in all_kinds() {
            let event = sample_event(kind);
            let json = serde_json::to_string(&event).unwrap();
            let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, event);
        }
    }

    #[test]
    fn event_wire_tags_are_stable() {
        let cases: Vec<(AgentEventKind, &str)> = vec![
            (
                AgentEventKind::RunStarted { message_count: 0 },
                "run_started",
            ),
            (
                AgentEventKind::ModelTextDelta {
                    text: String::new(),
                },
                "model_text_delta",
            ),
            (
                AgentEventKind::ModelThinkingDelta {
                    text: String::new(),
                },
                "model_thinking_delta",
            ),
            (
                AgentEventKind::ToolCallFinished {
                    call_id: ToolCallId::new("c1").unwrap(),
                    name: ToolName::new("echo").unwrap(),
                    is_error: false,
                },
                "tool_call_finished",
            ),
            (
                AgentEventKind::RunCompleted {
                    usage: Usage::default(),
                    steps: 1,
                },
                "run_completed",
            ),
            (AgentEventKind::RunCancelled, "run_cancelled"),
        ];
        for (kind, tag) in cases {
            let value = serde_json::to_value(sample_event(kind)).unwrap();
            assert_eq!(value.get("type"), Some(&json!(tag)));
        }

        // 信封字段契约：run_id / seq / at_ms 必须存在
        let value = serde_json::to_value(sample_event(AgentEventKind::RunCancelled)).unwrap();
        assert!(value.get("run_id").is_some());
        assert!(value.get("seq").is_some());
        assert!(value.get("at_ms").is_some());
    }
}
