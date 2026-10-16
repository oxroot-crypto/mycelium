//! OpenAI 兼容 Provider（Chat Completions，流式 SSE）。
//!
//! 面向 OpenAI Chat Completions 协议及其兼容网关（Ollama / vLLM / LM Studio 等）。
//! 通过环境变量或构建器配置；TLS 使用 rustls + ring（无需 cmake）。
//!
//! 已适配的扩展语义：
//! - 思考链：`delta.reasoning_content` → `Thinking` 内容块（不回传历史）；
//! - 思考控制：`ModelRequest.reasoning` → `thinking` / `reasoning_effort`；
//! - 用量细化：`completion_tokens_details.reasoning_tokens` → `Usage.reasoning_tokens`。
//!
//! # Examples
//! ```no_run
//! use std::sync::Arc;
//!
//! use mycelium_core::{AgentConfig, ModelId};
//! use mycelium_provider_openai::OpenAiProvider;
//! use mycelium_runtime::AgentRuntime;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = OpenAiProvider::from_env()?;
//! let config = AgentConfig::builder()
//!     .model(ModelId::new("gpt-4o-mini")?)
//!     .build()?;
//! let runtime = AgentRuntime::builder()
//!     .config(config)
//!     .provider(Arc::new(provider))
//!     .build()?;
//! let _ = runtime;
//! # Ok(())
//! # }
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod provider;
mod request;
mod stream;

pub use provider::{
    API_KEY_ENV, BASE_URL_ENV, DEFAULT_BASE_URL, OpenAiConfigError, OpenAiProvider,
    OpenAiProviderBuilder,
};
