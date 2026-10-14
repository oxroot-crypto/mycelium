//! 运行时核心：装配结果与运行入口。

use std::fmt;
use std::sync::Arc;

use mycelium_core::{AgentConfig, AgentError};
use mycelium_model::ModelProvider;
use mycelium_tools::ToolRegistry;

use crate::builder::AgentRuntimeBuilder;
use crate::hooks::RunHooks;
use crate::run::{RunInput, RunOutcome};

/// Agent 运行时。
///
/// 装配完成后不可变；一次 [`AgentRuntime::run`] 对应一段运行生命周期
/// （状态机见 `docs/architecture/lifecycle.md`）。
pub struct AgentRuntime {
    config: AgentConfig,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
    hooks: Option<Arc<dyn RunHooks>>,
}

impl AgentRuntime {
    /// 创建构建器。
    pub fn builder() -> AgentRuntimeBuilder {
        AgentRuntimeBuilder::default()
    }

    /// 由 [`AgentRuntimeBuilder`] 调用的内部构造。
    pub(crate) fn from_parts(
        config: AgentConfig,
        provider: Arc<dyn ModelProvider>,
        tools: ToolRegistry,
        hooks: Option<Arc<dyn RunHooks>>,
    ) -> Self {
        Self {
            config,
            provider,
            tools,
            hooks,
        }
    }

    /// 配置。
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// 工具注册表。
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    /// 模型提供方标识。
    pub fn provider_id(&self) -> &str {
        self.provider.id()
    }

    /// 运行钩子（若已配置）。
    pub fn hooks(&self) -> Option<&dyn RunHooks> {
        self.hooks.as_deref()
    }

    /// 执行一次运行。
    ///
    /// # 详细说明
    /// v0.1 骨架阶段尚未实现执行循环：调用**必然**返回
    /// [`AgentError::Unimplemented`]，不会静默成功。该行为由 `FC-POST-052`
    /// 契约与 `agent::tests::run_returns_unimplemented_in_skeleton` 守护；
    /// 实现循环时必须先更新契约与测试（契约漂移哨兵）。
    ///
    /// # Errors
    /// - [`AgentError::Unimplemented`]：执行循环尚未实现（v0.1）；
    /// - 实现后：预算超限 [`AgentError::BudgetExceeded`]、取消
    ///   [`AgentError::Cancelled`]、模型 / 工具层错误等。
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    ///
    /// use mycelium_core::{AgentConfig, AgentError, ModelId, StopReason};
    /// use mycelium_model::{
    ///     ModelCapabilities, ModelDelta, ModelError, ModelProvider, ModelRequest, ModelStream,
    ///     async_trait, stream_from_deltas,
    /// };
    /// use mycelium_runtime::{AgentRuntime, RunInput};
    ///
    /// struct EchoProvider;
    ///
    /// #[async_trait]
    /// impl ModelProvider for EchoProvider {
    ///     fn id(&self) -> &str {
    ///         "echo"
    ///     }
    ///
    ///     fn capabilities(&self) -> ModelCapabilities {
    ///         ModelCapabilities {
    ///             supports_streaming: true,
    ///             ..Default::default()
    ///         }
    ///     }
    ///
    ///     async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
    ///         Ok(stream_from_deltas(vec![
    ///             ModelDelta::TextDelta { text: "pong".to_owned() },
    ///             ModelDelta::Finished { stop_reason: StopReason::EndTurn },
    ///         ]))
    ///     }
    /// }
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = AgentConfig::builder().model(ModelId::new("echo")?).build()?;
    /// let runtime = AgentRuntime::builder()
    ///     .config(config)
    ///     .provider(Arc::new(EchoProvider))
    ///     .build()?;
    ///
    /// let result = runtime.run(RunInput::from_user_text("ping")).await;
    /// assert!(matches!(result, Err(AgentError::Unimplemented { .. })));
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run(&self, _input: RunInput) -> Result<RunOutcome, AgentError> {
        Err(AgentError::Unimplemented {
            feature: "AgentRuntime::run",
        })
    }
}

impl fmt::Debug for AgentRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentRuntime")
            .field("config", &self.config)
            .field("provider_id", &self.provider.id())
            .field("tools", &self.tools)
            .field("hooks_configured", &self.hooks.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use mycelium_core::{AgentConfig, AgentError, ModelId};

    use super::*;
    use crate::test_support::stub_provider;

    fn runtime_with_stub() -> AgentRuntime {
        let config = AgentConfig::builder()
            .model(ModelId::new("stub-model").unwrap())
            .build()
            .unwrap();
        AgentRuntime::builder()
            .config(config)
            .provider(stub_provider())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn run_returns_unimplemented_in_skeleton() {
        let runtime = runtime_with_stub();
        let result = runtime.run(RunInput::from_user_text("ping")).await;
        assert!(matches!(
            result,
            Err(AgentError::Unimplemented { feature }) if feature.contains("AgentRuntime::run")
        ));
    }

    #[test]
    fn runtime_exposes_wiring() {
        let runtime = runtime_with_stub();
        assert_eq!(runtime.provider_id(), "stub");
        assert_eq!(runtime.config().model().as_str(), "stub-model");
        assert!(runtime.tools().is_empty());
        assert!(runtime.hooks().is_none());
    }
}
