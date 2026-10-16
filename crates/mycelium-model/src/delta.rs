//! 模型流式增量。
//!
//! 对应契约：FC-POST-040；装配规则（Planned，v0.2）见 `docs/architecture/lifecycle.md` §6。

use serde::{Deserialize, Serialize};

use mycelium_core::ids::{ToolCallId, ToolName};
use mycelium_core::response::StopReason;

/// 模型流式增量。
///
/// 装配契约（v0.2 运行时实现并强制校验）：
/// - `ToolCallDelta.index` 从 0 开始连续；
/// - 同一 `index` 的首个增量必须携带 `id` 与 `name`，其后仅携带 `arguments_json_delta`；
/// - 成功流必须以 [`ModelDelta::Finished`] 结尾且恰好一条。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelDelta {
    /// 文本增量。
    TextDelta {
        /// 增量文本。
        text: String,
    },
    /// 思考（推理）文本增量（思考模式下由模型产出；装配为 `Thinking` 内容块）。
    ThinkingDelta {
        /// 增量文本。
        text: String,
    },
    /// 工具调用增量（可能分片）。
    ToolCallDelta {
        /// 工具调用序号（从 0 开始）。
        index: u32,
        /// 首次出现时提供调用标识。
        id: Option<ToolCallId>,
        /// 首次出现时提供工具名。
        name: Option<ToolName>,
        /// 参数 JSON 的增量片段（原始字符串，可能不构成合法 JSON）。
        arguments_json_delta: Option<String>,
    },
    /// 用量统计（至多一条，应出现在 `Finished` 之前）。
    Usage {
        /// 输入 Token。
        input_tokens: u64,
        /// 输出 Token。
        output_tokens: u64,
        /// 思考（推理）Token 数（包含在 `output_tokens` 内；提供方未报告时为 `None`）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_tokens: Option<u64>,
    },
    /// 流结束（成功时最后一项）。
    Finished {
        /// 停止原因。
        stop_reason: StopReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_json_roundtrip() {
        let cases = vec![
            ModelDelta::TextDelta {
                text: "hi".to_owned(),
            },
            ModelDelta::ThinkingDelta {
                text: "先想".to_owned(),
            },
            ModelDelta::ToolCallDelta {
                index: 0,
                id: Some(ToolCallId::new("call_1").unwrap()),
                name: Some(ToolName::new("echo").unwrap()),
                arguments_json_delta: Some("{\"a\":".to_owned()),
            },
            ModelDelta::ToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments_json_delta: Some("1}".to_owned()),
            },
            ModelDelta::Usage {
                input_tokens: 10,
                output_tokens: 5,
                reasoning_tokens: Some(3),
            },
            ModelDelta::Usage {
                input_tokens: 1,
                output_tokens: 2,
                reasoning_tokens: None,
            },
            ModelDelta::Finished {
                stop_reason: StopReason::EndTurn,
            },
        ];
        for delta in cases {
            let json = serde_json::to_string(&delta).unwrap();
            let decoded: ModelDelta = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, delta);
        }

        // 非法工具名 / 调用 ID 反序列化必须失败（复用 core 的校验）
        let bad_name = r#"{"type":"tool_call_delta","index":0,"id":"c1","name":"a b","arguments_json_delta":null}"#;
        assert!(serde_json::from_str::<ModelDelta>(bad_name).is_err());
        let blank_id = r#"{"type":"tool_call_delta","index":0,"id":"   ","name":"echo","arguments_json_delta":null}"#;
        assert!(serde_json::from_str::<ModelDelta>(blank_id).is_err());
    }
}
