//! mycelium 领域内核（Foundation Kernel）。
//!
//! 本 crate 提供跨层共享的**词汇、类型与不变量**，不包含任何 IO 与运行时逻辑：
//!
//! - 标识符：[`SessionId`]、[`RunId`]、[`ToolName`]、[`ToolCallId`]、[`ModelId`]
//! - 消息模型：[`Message`]、[`ContentBlock`]、[`Role`]
//! - 工具协议：[`ToolSpec`]、[`ToolCall`]、[`ToolChoice`]
//! - 响应词汇：[`ModelResponse`]、[`StopReason`]、[`Usage`]
//! - 运行状态机：[`RunState`]、[`RunEvent`]、[`InvalidRunTransition`]
//! - 事件模型：[`AgentEvent`]、[`AgentEventKind`]
//! - 错误分类矩阵：[`AgentError`]、[`ModelError`]、[`ToolError`]、[`ConfigError`]
//! - 配置：[`AgentConfig`]；取消原语：[`CancellationToken`]
//!
//! 形式化约束（Pre / Post / Invariant / State / Error）的单一真实来源见仓库
//! `docs/spec/core.md`；本 crate 的单元测试与属性测试与之一一对应。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp
    )
)]

pub mod config;
pub mod error;
pub mod event;
pub mod ids;
pub mod message;
pub mod response;
pub mod run;
pub mod tool;

#[cfg(test)]
mod properties;

pub use config::{
    AgentConfig, AgentConfigBuilder, DEFAULT_MAX_PARALLEL_TOOL_CALLS, DEFAULT_MAX_STEPS,
    DEFAULT_REQUEST_TIMEOUT_SECS, DEFAULT_RETRY_INITIAL_BACKOFF_MS, DEFAULT_RETRY_MAX_BACKOFF_MS,
    REASONING_EFFORT_MAX_LEN, ReasoningOptions, RetryPolicy,
};
pub use error::{
    AgentError, BudgetKind, ConfigError, ContentError, InvalidModelId, InvalidToolCallId,
    InvalidToolName, ModelError, ToolError,
};
pub use event::{AgentEvent, AgentEventKind};
pub use ids::{ModelId, RunId, SessionId, ToolCallId, ToolName};
pub use message::{ContentBlock, Message, Role, ToolResultBlock};
pub use response::{ModelResponse, StopReason, Usage};
pub use run::{InvalidRunTransition, RunEvent, RunState};
pub use tool::{ToolCall, ToolChoice, ToolSpec};

/// 统一的取消原语（re-export 自 `tokio-util`，保证全生态语义一致）。
///
/// 运行时与所有插件实现（`ModelProvider` / `Tool`）都必须尊重该令牌：
/// 触发后应尽快终止在途工作并返回对应的取消错误。
pub use tokio_util::sync::CancellationToken;
