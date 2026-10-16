//! mycelium 执行引擎：装配、会话、钩子与运行入口。
//!
//! v0.2 起执行循环真实可用：[`AgentRuntime::run`] 同步返回最终结果，
//! [`AgentRuntime::run_events`] 以事件流暴露过程（事件为可选观测面）。
//!
//! 生命周期与事件契约见 `docs/architecture/lifecycle.md`。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agent;
pub mod agent_tool;
pub mod builder;
pub(crate) mod engine;
pub mod hooks;
pub mod run;
pub mod session;
pub mod store;

#[cfg(test)]
mod test_support;

pub use agent::{AgentEventStream, AgentRun, AgentRuntime, RunFuture};
pub use agent_tool::AgentTool;
pub use builder::{AgentRuntimeBuilder, BuildError};
pub use hooks::{RunContext, RunHooks};
pub use run::{RunInput, RunOutcome};
pub use session::Session;
pub use store::{FileSessionStore, InMemorySessionStore, SessionStore, SessionStoreError};
