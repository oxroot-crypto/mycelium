//! 模型流式增量的装配器。
//!
//! 把 [`ModelStream`](crate::ModelStream) 的增量按流协议装配为 [`ModelResponse`]；
//! 违反协议一律归约为 [`ModelError::InvalidResponse`]（不得静默容忍）。
//! 契约编号见 `docs/spec/core.md` §2.12，协议定义见 `docs/architecture/lifecycle.md` §6。

use std::collections::BTreeMap;

use mycelium_core::error::ModelError;
use mycelium_core::ids::{ToolCallId, ToolName};
use mycelium_core::message::{ContentBlock, Message, Role};
use mycelium_core::response::{ModelResponse, StopReason, Usage};
use mycelium_core::tool::ToolCall;

use crate::delta::ModelDelta;

/// 装配过程中的工具调用片段（`id` / `name` 在首个增量时保证存在）。
#[derive(Debug)]
struct ToolCallParts {
    id: Option<ToolCallId>,
    name: Option<ToolName>,
    arguments: String,
}

/// 增量装配器。
///
/// 用法：对流的每个成功增量调用 [`DeltaAssembler::push`]，流正常结束后调用
/// [`DeltaAssembler::finish`] 取回完整响应。
#[derive(Debug, Default)]
pub struct DeltaAssembler {
    text: String,
    tool_calls: BTreeMap<u32, ToolCallParts>,
    usage: Option<Usage>,
    stop_reason: Option<StopReason>,
    finished: bool,
}

impl DeltaAssembler {
    /// 创建空装配器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 推入一个增量。
    ///
    /// # Errors
    /// 违反流协议时返回 [`ModelError::InvalidResponse`]：
    /// - `Finished` 之后再推入增量；
    /// - `Usage` 重复出现；
    /// - `ToolCallDelta.index` 非连续（不满足从 0 递增）；
    /// - 同一 `index` 的首个增量缺 `id` / `name`，或后续增量重复携带。
    pub fn push(&mut self, delta: ModelDelta) -> Result<(), ModelError> {
        if self.finished {
            return Err(invalid("Finished 之后不得再产出增量"));
        }
        match delta {
            ModelDelta::TextDelta { text } => self.text.push_str(&text),
            ModelDelta::ToolCallDelta {
                index,
                id,
                name,
                arguments_json_delta,
            } => self.push_tool_call(index, id, name, arguments_json_delta)?,
            ModelDelta::Usage {
                input_tokens,
                output_tokens,
            } => {
                if self.usage.is_some() {
                    return Err(invalid("Usage 至多出现一次"));
                }
                self.usage = Some(Usage::new(input_tokens, output_tokens));
            }
            ModelDelta::Finished { stop_reason } => {
                self.stop_reason = Some(stop_reason);
                self.finished = true;
            }
        }
        Ok(())
    }

    /// 处理一个工具调用增量。
    fn push_tool_call(
        &mut self,
        index: u32,
        id: Option<ToolCallId>,
        name: Option<ToolName>,
        arguments_json_delta: Option<String>,
    ) -> Result<(), ModelError> {
        match self.tool_calls.get_mut(&index) {
            Some(parts) => {
                if id.is_some() || name.is_some() {
                    return Err(invalid(format!(
                        "index {index} 的后续增量不得重复携带 id / name"
                    )));
                }
                if let Some(fragment) = arguments_json_delta {
                    parts.arguments.push_str(&fragment);
                }
            }
            None => {
                let expected = self.tool_calls.len() as u32;
                if index != expected {
                    return Err(invalid(format!(
                        "ToolCallDelta.index 必须从 0 连续：期望 {expected}，实际 {index}"
                    )));
                }
                let id =
                    id.ok_or_else(|| invalid(format!("index {index} 的首个增量必须携带 id")))?;
                let name =
                    name.ok_or_else(|| invalid(format!("index {index} 的首个增量必须携带 name")))?;
                self.tool_calls.insert(
                    index,
                    ToolCallParts {
                        id: Some(id),
                        name: Some(name),
                        arguments: arguments_json_delta.unwrap_or_default(),
                    },
                );
            }
        }
        Ok(())
    }

    /// 结束装配并产出响应。
    ///
    /// # Errors
    /// - 未收到 `Finished`（流提前结束）→ [`ModelError::InvalidResponse`]；
    /// - 累积的工具参数不是合法 JSON → [`ModelError::InvalidResponse`]。
    pub fn finish(self) -> Result<ModelResponse, ModelError> {
        if !self.finished {
            return Err(invalid("流在收到 Finished 之前结束"));
        }
        let stop_reason = self
            .stop_reason
            .ok_or_else(|| invalid("缺少 stop_reason（内部不变量被破坏）"))?;

        let mut blocks = Vec::new();
        if !self.text.is_empty() {
            blocks.push(ContentBlock::Text { text: self.text });
        }
        for (index, parts) in self.tool_calls {
            // push 阶段已保证 id / name 存在；此处防御式复检，不 panic。
            let id = parts
                .id
                .ok_or_else(|| invalid(format!("index {index} 缺少 id（内部不变量被破坏）")))?;
            let name = parts
                .name
                .ok_or_else(|| invalid(format!("index {index} 缺少 name（内部不变量被破坏）")))?;
            let arguments = if parts.arguments.trim().is_empty() {
                serde_json::Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_str(&parts.arguments).map_err(|error| {
                    invalid(format!("index {index} 的工具参数不是合法 JSON：{error}"))
                })?
            };
            blocks.push(ToolCall::new(id, name, arguments).into_content_block());
        }
        if blocks.is_empty() {
            // 完全为空的完成结果：保持「消息至少一个内容块」不变量。
            blocks.push(ContentBlock::Text {
                text: String::new(),
            });
        }

        let message = Message::new(Role::Assistant, blocks)
            .map_err(|error| invalid(format!("装配结果非法：{error}")))?;
        Ok(ModelResponse {
            message,
            stop_reason,
            usage: self.usage.unwrap_or_default(),
        })
    }
}

/// 构造 `InvalidResponse` 错误。
fn invalid(detail: impl Into<String>) -> ModelError {
    ModelError::InvalidResponse {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use mycelium_core::StopReason;
    use proptest::prelude::*;

    use super::*;

    fn text_delta(text: &str) -> ModelDelta {
        ModelDelta::TextDelta {
            text: text.to_owned(),
        }
    }

    fn finished(reason: StopReason) -> ModelDelta {
        ModelDelta::Finished {
            stop_reason: reason,
        }
    }

    fn tool_delta(
        index: u32,
        id: Option<&str>,
        name: Option<&str>,
        arguments: Option<&str>,
    ) -> ModelDelta {
        ModelDelta::ToolCallDelta {
            index,
            id: id.map(|value| ToolCallId::new(value).unwrap()),
            name: name.map(|value| ToolName::new(value).unwrap()),
            arguments_json_delta: arguments.map(str::to_owned),
        }
    }

    #[track_caller]
    fn assert_invalid(error: ModelError, contains: &str) {
        match error {
            ModelError::InvalidResponse { detail } => {
                assert!(
                    detail.contains(contains),
                    "细节未包含 {contains:?}：{detail}"
                );
            }
            other => panic!("期望 InvalidResponse，实际：{other:?}"),
        }
    }

    #[test]
    fn assembles_text_usage_and_stop_reason() {
        let mut assembler = DeltaAssembler::new();
        assembler.push(text_delta("hel")).unwrap();
        assembler.push(text_delta("lo")).unwrap();
        assembler
            .push(ModelDelta::Usage {
                input_tokens: 7,
                output_tokens: 3,
            })
            .unwrap();
        assembler.push(finished(StopReason::EndTurn)).unwrap();

        let response = assembler.finish().unwrap();
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(response.usage, Usage::new(7, 3));
        assert_eq!(response.message.role(), Role::Assistant);
        assert_eq!(
            response.message.content(),
            &[ContentBlock::Text {
                text: "hello".to_owned()
            }]
        );
    }

    #[test]
    fn assembles_tool_calls_in_index_order() {
        let mut assembler = DeltaAssembler::new();
        assembler
            .push(tool_delta(0, Some("c1"), Some("alpha"), Some("{\"a\":")))
            .unwrap();
        assembler
            .push(tool_delta(1, Some("c2"), Some("beta"), None))
            .unwrap();
        assembler
            .push(tool_delta(0, None, None, Some("1}")))
            .unwrap();
        assembler.push(finished(StopReason::ToolUse)).unwrap();

        let response = assembler.finish().unwrap();
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage, Usage::default());
        assert_eq!(
            response.message.content(),
            &[
                ContentBlock::ToolUse {
                    id: ToolCallId::new("c1").unwrap(),
                    name: ToolName::new("alpha").unwrap(),
                    input: serde_json::json!({ "a": 1 }),
                },
                ContentBlock::ToolUse {
                    id: ToolCallId::new("c2").unwrap(),
                    name: ToolName::new("beta").unwrap(),
                    input: serde_json::json!({}),
                },
            ]
        );
    }

    #[test]
    fn tool_only_completion_has_no_text_block() {
        let mut assembler = DeltaAssembler::new();
        assembler
            .push(tool_delta(0, Some("c1"), Some("echo"), Some("{}")))
            .unwrap();
        assembler.push(finished(StopReason::ToolUse)).unwrap();

        let response = assembler.finish().unwrap();
        assert_eq!(response.message.content().len(), 1);
        assert!(matches!(
            response.message.content()[0],
            ContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn empty_completion_yields_empty_text_block() {
        let mut assembler = DeltaAssembler::new();
        assembler.push(finished(StopReason::EndTurn)).unwrap();

        let response = assembler.finish().unwrap();
        assert_eq!(
            response.message.content(),
            &[ContentBlock::Text {
                text: String::new()
            }]
        );
    }

    #[test]
    fn empty_arguments_fall_back_to_empty_object() {
        let mut assembler = DeltaAssembler::new();
        assembler
            .push(tool_delta(0, Some("c1"), Some("echo"), None))
            .unwrap();
        assembler
            .push(tool_delta(1, Some("c2"), Some("echo"), Some("")))
            .unwrap();
        assembler.push(finished(StopReason::ToolUse)).unwrap();

        let response = assembler.finish().unwrap();
        for block in response.message.content() {
            match block {
                ContentBlock::ToolUse { input, .. } => {
                    assert_eq!(input, &serde_json::json!({}));
                }
                other => panic!("期望 ToolUse，实际：{other:?}"),
            }
        }
    }

    #[test]
    fn rejects_stream_without_finished() {
        let mut assembler = DeltaAssembler::new();
        assembler.push(text_delta("partial")).unwrap();
        assert_invalid(assembler.finish().unwrap_err(), "Finished");
    }

    #[test]
    fn rejects_push_after_finished() {
        let mut assembler = DeltaAssembler::new();
        assembler.push(finished(StopReason::EndTurn)).unwrap();
        assert_invalid(
            assembler.push(text_delta("late")).unwrap_err(),
            "Finished 之后",
        );
    }

    #[test]
    fn rejects_duplicate_usage() {
        let mut assembler = DeltaAssembler::new();
        assembler
            .push(ModelDelta::Usage {
                input_tokens: 1,
                output_tokens: 1,
            })
            .unwrap();
        assert_invalid(
            assembler
                .push(ModelDelta::Usage {
                    input_tokens: 2,
                    output_tokens: 2,
                })
                .unwrap_err(),
            "Usage",
        );
    }

    #[test]
    fn rejects_non_contiguous_tool_index() {
        let mut assembler = DeltaAssembler::new();
        assert_invalid(
            assembler
                .push(tool_delta(1, Some("c1"), Some("alpha"), None))
                .unwrap_err(),
            "连续",
        );

        let mut assembler = DeltaAssembler::new();
        assembler
            .push(tool_delta(0, Some("c1"), Some("alpha"), None))
            .unwrap();
        assert_invalid(
            assembler
                .push(tool_delta(2, Some("c2"), Some("beta"), None))
                .unwrap_err(),
            "连续",
        );
    }

    #[test]
    fn rejects_missing_id_or_name_on_first_delta() {
        let mut assembler = DeltaAssembler::new();
        assert_invalid(
            assembler
                .push(tool_delta(0, None, Some("alpha"), None))
                .unwrap_err(),
            "必须携带 id",
        );

        let mut assembler = DeltaAssembler::new();
        assert_invalid(
            assembler
                .push(tool_delta(0, Some("c1"), None, None))
                .unwrap_err(),
            "必须携带 name",
        );
    }

    #[test]
    fn rejects_repeated_id_or_name() {
        let mut assembler = DeltaAssembler::new();
        assembler
            .push(tool_delta(0, Some("c1"), Some("alpha"), None))
            .unwrap();
        assert_invalid(
            assembler
                .push(tool_delta(0, Some("c1"), None, None))
                .unwrap_err(),
            "不得重复携带",
        );
    }

    #[test]
    fn rejects_invalid_arguments_json() {
        let mut assembler = DeltaAssembler::new();
        assembler
            .push(tool_delta(0, Some("c1"), Some("alpha"), Some("{oops")))
            .unwrap();
        assembler.push(finished(StopReason::ToolUse)).unwrap();
        assert_invalid(assembler.finish().unwrap_err(), "合法 JSON");
    }

    proptest! {
        #[test]
        fn prop_text_deltas_concatenate(fragments in proptest::collection::vec(any::<String>(), 0..8)) {
            let mut assembler = DeltaAssembler::new();
            for fragment in &fragments {
                assembler.push(ModelDelta::TextDelta { text: fragment.clone() }).unwrap();
            }
            assembler.push(ModelDelta::Finished { stop_reason: StopReason::EndTurn }).unwrap();
            let response = assembler.finish().unwrap();

            let expected_text: String = fragments.concat();
            let expected_block = ContentBlock::Text { text: expected_text };
            prop_assert_eq!(response.message.content(), &[expected_block]);
        }
    }
}
