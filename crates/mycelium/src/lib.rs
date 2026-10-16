//! mycelium 门面 crate：统一 re-export、`prelude` 与端到端示例。
//!
//! 本 crate 不承载业务逻辑；文档主体直接引用仓库根目录的 `README.md`
//! （其中的 Rust 代码块会作为 doctest 随 `cargo test` 一起验证）。
#![doc = include_str!("../../../README.md")]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub use mycelium_core as core;
pub use mycelium_mcp as mcp;
pub use mycelium_model as model;
pub use mycelium_runtime as runtime;
pub use mycelium_tools as tools;

#[cfg(feature = "provider-openai")]
pub use mycelium_provider_openai as provider_openai;

/// 常用类型的统一入口：`use mycelium::prelude::*;`
pub mod prelude {
    pub use crate::core::{
        AgentConfig, AgentError, AgentEvent, AgentEventKind, CancellationToken, ContentBlock,
        Message, ModelError, ModelId, ReasoningOptions, RetryPolicy, Role, RunEvent, RunId,
        RunState, SessionId, StopReason, ToolCall, ToolCallId, ToolChoice, ToolError, ToolName,
        ToolResultBlock, ToolSpec, Usage,
    };
    pub use crate::mcp::{McpClient, McpError, McpServerInfo, McpTool};
    pub use crate::model::{
        DeltaAssembler, FallbackError, FallbackProvider, ModelCapabilities, ModelDelta,
        ModelProvider, ModelRequest, ModelResponse, ModelStream, async_trait, stream_from_deltas,
    };
    pub use crate::runtime::{
        AgentEventStream, AgentRun, AgentRuntime, AgentRuntimeBuilder, AgentTool, BuildError,
        FileSessionStore, InMemoryMemory, InMemorySessionStore, Memory, MemoryError, MemoryId,
        MemoryRecord, RunContext, RunFuture, RunHooks, RunInput, RunOutcome, Session, SessionStore,
        SessionStoreError,
    };
    pub use crate::tools::{
        AllowAll, AllowList, DenyList, PolicyDecision, RegistryError, SchemaViolation, Tool,
        ToolContext, ToolOutput, ToolPolicy, validate_arguments,
    };
}
