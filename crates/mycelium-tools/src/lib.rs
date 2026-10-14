//! mycelium 工具抽象（Tool SPI）与注册表。
//!
//! 工具是模型可调用的能力单元：实现 [`Tool`] 并提供 [`ToolSpec`]
//! （名称 / 描述 / 入参 JSON Schema），注册进 [`ToolRegistry`] 后即可被模型调用。
//!
//! 执行契约（Planned，v0.2 起由运行时强制）见 `docs/architecture/lifecycle.md` §7。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod registry;
pub mod tool;

pub use registry::{RegistryError, ToolRegistry};
pub use tool::{Tool, ToolContext, ToolOutput};

pub use async_trait::async_trait;
pub use mycelium_core::error::ToolError;
pub use mycelium_core::tool::ToolSpec;
