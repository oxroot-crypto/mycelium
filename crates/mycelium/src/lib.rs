//! mycelium 门面 crate：统一 re-export、`prelude` 与端到端示例。
//!
//! 本 crate 不承载业务逻辑；文档主体直接引用仓库根目录的 `README.md`
//! （其中的 Rust 代码块会作为 doctest 随 `cargo test` 一起验证）。
#![doc = include_str!("../../../README.md")]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub use mycelium_core as core;
pub use mycelium_model as model;
pub use mycelium_runtime as runtime;
pub use mycelium_tools as tools;

/// 常用类型的统一入口：`use mycelium::prelude::*;`
pub mod prelude {
    pub use crate::core::{
        AgentConfig, AgentError, AgentEvent, AgentEventKind, CancellationToken, ContentBlock,
        Message, ModelError, ModelId, Role, RunEvent, RunId, RunState, SessionId, StopReason,
        ToolCall, ToolCallId, ToolChoice, ToolError, ToolName, ToolSpec, Usage,
    };
    pub use crate::model::{
        ModelCapabilities, ModelDelta, ModelProvider, ModelRequest, ModelStream, async_trait,
        stream_from_deltas,
    };
    pub use crate::runtime::{
        AgentRuntime, AgentRuntimeBuilder, BuildError, RunContext, RunHooks, RunInput, RunOutcome,
        Session,
    };
    pub use crate::tools::{RegistryError, Tool, ToolContext, ToolOutput};
}
