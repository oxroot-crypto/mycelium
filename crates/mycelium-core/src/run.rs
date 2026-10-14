//! 运行（Run）生命周期状态机。
//!
//! 形式化定义 `M = (S, E, δ, Created, {Completed, Failed, Cancelled})` 与转移表明细见
//! `docs/architecture/lifecycle.md`；契约编号见 `docs/spec/core.md` 的
//! FC-STA-001 ~ FC-STA-003。
//!
//! 本模块是纯函数模块：不产生副作用；非法转移必须显式拒绝，禁止静默忽略。

use std::fmt;

/// 运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunState {
    /// 已创建，尚未启动。
    Created,
    /// 运行中（正在处理模型响应或工具结果）。
    Running,
    /// 等待模型响应。
    AwaitingModel,
    /// 正在执行工具调用。
    ExecutingTools,
    /// 正常完成（终止态）。
    Completed,
    /// 失败终止（终止态）。
    Failed,
    /// 被取消（终止态）。
    Cancelled,
}

/// 触发运行状态转移的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunEvent {
    /// 启动运行。
    Start,
    /// 发出模型请求。
    RequestModel,
    /// 模型响应就绪。
    ModelCompleted,
    /// 模型层错误。
    ModelFailed,
    /// 模型要求调用工具。
    ToolUseRequested,
    /// 工具执行完毕。
    ToolsCompleted,
    /// 运行完成。
    Complete,
    /// 运行级失败。
    Fail,
    /// 取消运行。
    Cancel,
}

impl RunState {
    /// 是否为终止状态。
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// 应用一次状态转移。
    ///
    /// # Errors
    /// `(self, event)` 不在合法转移表（`docs/architecture/lifecycle.md` §3）中时，返回
    /// [`InvalidRunTransition`]；本函数无副作用，调用方状态不会被改变。
    ///
    /// # Examples
    /// ```
    /// use mycelium_core::{RunEvent, RunState};
    ///
    /// let running = RunState::Created.transition(RunEvent::Start).unwrap();
    /// assert_eq!(running, RunState::Running);
    /// assert!(running.transition(RunEvent::Start).is_err());
    /// ```
    pub fn transition(self, event: RunEvent) -> Result<Self, InvalidRunTransition> {
        use RunEvent as E;
        use RunState as S;

        let next = match (self, event) {
            (S::Created, E::Start) => S::Running,
            (S::Created, E::Cancel) => S::Cancelled,
            (S::Running, E::RequestModel) => S::AwaitingModel,
            (S::Running, E::ToolUseRequested) => S::ExecutingTools,
            (S::Running, E::Complete) => S::Completed,
            (S::Running, E::Fail) => S::Failed,
            (S::Running, E::Cancel) => S::Cancelled,
            (S::AwaitingModel, E::ModelCompleted) => S::Running,
            (S::AwaitingModel, E::ModelFailed) => S::Failed,
            (S::AwaitingModel, E::Fail) => S::Failed,
            (S::AwaitingModel, E::Cancel) => S::Cancelled,
            (S::ExecutingTools, E::ToolsCompleted) => S::Running,
            (S::ExecutingTools, E::Fail) => S::Failed,
            (S::ExecutingTools, E::Cancel) => S::Cancelled,
            _ => return Err(InvalidRunTransition { from: self, event }),
        };
        Ok(next)
    }
}

impl fmt::Display for RunState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::AwaitingModel => "awaiting_model",
            Self::ExecutingTools => "executing_tools",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        };
        f.write_str(name)
    }
}

impl fmt::Display for RunEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Start => "start",
            Self::RequestModel => "request_model",
            Self::ModelCompleted => "model_completed",
            Self::ModelFailed => "model_failed",
            Self::ToolUseRequested => "tool_use_requested",
            Self::ToolsCompleted => "tools_completed",
            Self::Complete => "complete",
            Self::Fail => "fail",
            Self::Cancel => "cancel",
        };
        f.write_str(name)
    }
}

/// 非法状态转移错误（携带来源状态与事件，供审计与定位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("非法的运行状态转移：{from} --{event}--> ?")]
pub struct InvalidRunTransition {
    /// 转移前的状态。
    pub from: RunState,
    /// 触发事件。
    pub event: RunEvent,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [RunState; 7] = [
        RunState::Created,
        RunState::Running,
        RunState::AwaitingModel,
        RunState::ExecutingTools,
        RunState::Completed,
        RunState::Failed,
        RunState::Cancelled,
    ];

    const ALL_EVENTS: [RunEvent; 9] = [
        RunEvent::Start,
        RunEvent::RequestModel,
        RunEvent::ModelCompleted,
        RunEvent::ModelFailed,
        RunEvent::ToolUseRequested,
        RunEvent::ToolsCompleted,
        RunEvent::Complete,
        RunEvent::Fail,
        RunEvent::Cancel,
    ];

    /// 合法转移表（与 `docs/architecture/lifecycle.md` §3 一一对应）。
    const LEGAL_TABLE: [(RunState, RunEvent, RunState); 14] = [
        (RunState::Created, RunEvent::Start, RunState::Running),
        (RunState::Created, RunEvent::Cancel, RunState::Cancelled),
        (
            RunState::Running,
            RunEvent::RequestModel,
            RunState::AwaitingModel,
        ),
        (
            RunState::Running,
            RunEvent::ToolUseRequested,
            RunState::ExecutingTools,
        ),
        (RunState::Running, RunEvent::Complete, RunState::Completed),
        (RunState::Running, RunEvent::Fail, RunState::Failed),
        (RunState::Running, RunEvent::Cancel, RunState::Cancelled),
        (
            RunState::AwaitingModel,
            RunEvent::ModelCompleted,
            RunState::Running,
        ),
        (
            RunState::AwaitingModel,
            RunEvent::ModelFailed,
            RunState::Failed,
        ),
        (RunState::AwaitingModel, RunEvent::Fail, RunState::Failed),
        (
            RunState::AwaitingModel,
            RunEvent::Cancel,
            RunState::Cancelled,
        ),
        (
            RunState::ExecutingTools,
            RunEvent::ToolsCompleted,
            RunState::Running,
        ),
        (RunState::ExecutingTools, RunEvent::Fail, RunState::Failed),
        (
            RunState::ExecutingTools,
            RunEvent::Cancel,
            RunState::Cancelled,
        ),
    ];

    fn expected(state: RunState, event: RunEvent) -> Option<RunState> {
        LEGAL_TABLE
            .iter()
            .find(|(from, trigger, _)| *from == state && *trigger == event)
            .map(|(_, _, to)| *to)
    }

    #[test]
    fn transition_table_is_exhaustive() {
        for state in ALL_STATES {
            for event in ALL_EVENTS {
                match (state.transition(event), expected(state, event)) {
                    (Ok(actual), Some(expect)) => {
                        assert_eq!(actual, expect, "{state} --{event}--> 期望 {expect}");
                    }
                    (Err(err), None) => {
                        assert_eq!(err.from, state);
                        assert_eq!(err.event, event);
                    }
                    (actual, expect) => panic!(
                        "转移结果与契约不符：{state} --{event}--> 实际 {actual:?}，期望 {expect:?}"
                    ),
                }
            }
        }
    }

    #[test]
    fn terminal_states_absorb_all_events() {
        for state in [RunState::Completed, RunState::Failed, RunState::Cancelled] {
            for event in ALL_EVENTS {
                assert!(
                    state.transition(event).is_err(),
                    "{state} 作为终止态不应接受 {event}"
                );
            }
        }
    }

    #[test]
    fn is_terminal_classification() {
        for state in ALL_STATES {
            let expected_terminal = matches!(
                state,
                RunState::Completed | RunState::Failed | RunState::Cancelled
            );
            assert_eq!(
                state.is_terminal(),
                expected_terminal,
                "{state} 的终止态判定不符"
            );
        }
    }
}
