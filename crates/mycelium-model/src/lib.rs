//! mycelium 模型提供方抽象（Provider SPI）。
//!
//! 提供方实现 [`ModelProvider`] 后即可接入运行时；本 crate 不包含任何具体提供方实现。
//! 错误类型 [`ModelError`] 与响应词汇 [`StopReason`]、[`Usage`] 由 `mycelium-core`
//! 定义并在此 re-export。
//!
//! 流协议契约（装配规则）见 `docs/architecture/lifecycle.md` §6 与 FC-Matrix §4。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod assembler;
pub mod delta;
pub mod provider;
pub mod request;

pub use assembler::DeltaAssembler;
pub use delta::ModelDelta;
pub use provider::{ModelCapabilities, ModelProvider, ModelStream, stream_from_deltas};
pub use request::ModelRequest;

pub use async_trait::async_trait;
pub use mycelium_core::config::ReasoningOptions;
pub use mycelium_core::error::ModelError;
pub use mycelium_core::response::{ModelResponse, StopReason, Usage};
