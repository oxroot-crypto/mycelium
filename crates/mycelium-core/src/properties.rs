//! 基于属性的测试（PBT）：以随机输入验证形式化约束的普适性。
//!
//! 对应契约见 `docs/spec/core.md` §3（FC-INV-010 ~ FC-INV-013）。

use proptest::prelude::*;

use crate::ids::ToolName;
use crate::message::Message;
use crate::response::Usage;
use crate::run::{RunEvent, RunState};

/// 规格谓词：`1 <= len <= 64` 且字符集为 `[A-Za-z0-9_-]`（与 `ToolName` 的契约等价，
/// 有意独立实现且边界值硬编码，保证实现被篡改时属性测试能够变红）。
fn tool_name_is_within_spec(value: &str) -> bool {
    let len = value.chars().count();
    (1..=64).contains(&len)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn any_run_event() -> impl Strategy<Value = RunEvent> {
    prop_oneof![
        Just(RunEvent::Start),
        Just(RunEvent::RequestModel),
        Just(RunEvent::ModelCompleted),
        Just(RunEvent::ModelFailed),
        Just(RunEvent::ToolUseRequested),
        Just(RunEvent::ToolsCompleted),
        Just(RunEvent::Complete),
        Just(RunEvent::Fail),
        Just(RunEvent::Cancel),
    ]
}

proptest! {
    #[test]
    fn prop_tool_name_acceptance_matches_spec(raw in any::<String>()) {
        let result = ToolName::new(raw.clone());
        prop_assert_eq!(result.is_ok(), tool_name_is_within_spec(&raw));
        if let Ok(name) = result {
            prop_assert_eq!(name.as_str(), raw.as_str());
            let json = serde_json::to_string(&name).unwrap();
            let decoded: ToolName = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(decoded, name);
        }
    }

    #[test]
    fn prop_terminal_states_reject_all_events(
        state in prop_oneof![
            Just(RunState::Completed),
            Just(RunState::Failed),
            Just(RunState::Cancelled),
        ],
        event in any_run_event(),
    ) {
        prop_assert!(state.transition(event).is_err());
    }

    #[test]
    fn prop_usage_total_saturating(input in any::<u64>(), output in any::<u64>()) {
        prop_assert_eq!(
            Usage::new(input, output).total_tokens(),
            input.saturating_add(output)
        );
    }

    #[test]
    fn prop_message_roundtrip(text in any::<String>()) {
        let message = Message::user_text(text);
        let json = serde_json::to_string(&message).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(decoded, message);
    }
}
