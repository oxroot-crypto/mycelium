//! mycelium 执行引擎：装配、会话、钩子与运行入口。
//!
//! v0.1 骨架：提供 [`AgentRuntime`] 的完整装配与签名契约。执行循环
//! （[`AgentRuntime::run`]）自 v0.2 起实现，当前**必然**返回
//! [`mycelium_core::AgentError::Unimplemented`]（显式失败，绝不静默成功）。
//!
//! 生命周期与事件契约见 `docs/architecture/lifecycle.md`。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agent;
pub mod builder;
pub mod hooks;
pub mod run;
pub mod session;

#[cfg(test)]
mod test_support;

pub use agent::AgentRuntime;
pub use builder::{AgentRuntimeBuilder, BuildError};
pub use hooks::{RunContext, RunHooks};
pub use run::{RunInput, RunOutcome};
pub use session::Session;
