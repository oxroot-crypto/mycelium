//! [`AgentRuntime`] 构建器。

use std::fmt;
use std::sync::Arc;

use mycelium_core::AgentConfig;
use mycelium_model::ModelProvider;
use mycelium_tools::{RegistryError, Tool, ToolRegistry};

use crate::agent::AgentRuntime;
use crate::hooks::RunHooks;

/// 运行时构建错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    /// 缺少配置。
    #[error("缺少 AgentConfig：请通过 AgentRuntimeBuilder::config 提供")]
    MissingConfig,
    /// 缺少模型提供方。
    #[error("缺少 ModelProvider：请通过 AgentRuntimeBuilder::provider 提供")]
    MissingProvider,
    /// 工具注册失败（例如重名）。
    #[error("工具注册失败：{0}")]
    Registry(#[from] RegistryError),
}

/// [`AgentRuntime`] 构建器。
///
/// `config` 与 `provider` 为必填；工具可在构建时批量注册，重名在
/// [`AgentRuntimeBuilder::build`] 阶段被拒绝。
#[derive(Default)]
pub struct AgentRuntimeBuilder {
    config: Option<AgentConfig>,
    provider: Option<Arc<dyn ModelProvider>>,
    tools: Vec<Arc<dyn Tool>>,
    hooks: Option<Arc<dyn RunHooks>>,
}

impl AgentRuntimeBuilder {
    /// 设置配置（必填）。
    pub fn config(mut self, config: AgentConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// 设置模型提供方（必填）。
    pub fn provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    /// 追加一个工具（可多次调用）。
    pub fn tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// 追加多个工具。
    pub fn tools(mut self, tools: impl IntoIterator<Item = Arc<dyn Tool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    /// 设置运行钩子。
    pub fn hooks(mut self, hooks: Arc<dyn RunHooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// 构建运行时。
    ///
    /// # Errors
    /// - 未设置 `config`：[`BuildError::MissingConfig`]；
    /// - 未设置 `provider`：[`BuildError::MissingProvider`]；
    /// - 工具重名：`BuildError::Registry(RegistryError::DuplicateName)`。
    pub fn build(self) -> Result<AgentRuntime, BuildError> {
        let config = self.config.ok_or(BuildError::MissingConfig)?;
        let provider = self.provider.ok_or(BuildError::MissingProvider)?;

        let mut tools = ToolRegistry::new();
        for tool in self.tools {
            tools.register(tool)?;
        }

        Ok(AgentRuntime::from_parts(
            config, provider, tools, self.hooks,
        ))
    }
}

impl fmt::Debug for AgentRuntimeBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentRuntimeBuilder")
            .field("config", &self.config)
            .field("provider_id", &self.provider.as_ref().map(|p| p.id()))
            .field("tool_count", &self.tools.len())
            .field("hooks_configured", &self.hooks.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use mycelium_core::{AgentConfig, ModelId, ToolName};

    use super::*;
    use crate::test_support::{stub_provider, stub_tool};

    fn config() -> AgentConfig {
        AgentConfig::builder()
            .model(ModelId::new("stub-model").unwrap())
            .build()
            .unwrap()
    }

    #[test]
    fn missing_config_is_rejected() {
        let result = AgentRuntimeBuilder::default()
            .provider(stub_provider())
            .build();
        assert_eq!(result.unwrap_err(), BuildError::MissingConfig);
    }

    #[test]
    fn missing_provider_is_rejected() {
        let result = AgentRuntimeBuilder::default().config(config()).build();
        assert_eq!(result.unwrap_err(), BuildError::MissingProvider);
    }

    #[test]
    fn complete_builder_succeeds() {
        let runtime = AgentRuntimeBuilder::default()
            .config(config())
            .provider(stub_provider())
            .tool(stub_tool("echo"))
            .build()
            .unwrap();
        assert_eq!(runtime.tools().len(), 1);
    }

    #[test]
    fn duplicate_tool_name_is_rejected() {
        let result = AgentRuntimeBuilder::default()
            .config(config())
            .provider(stub_provider())
            .tool(stub_tool("echo"))
            .tool(stub_tool("echo"))
            .build();
        assert_eq!(
            result.unwrap_err(),
            BuildError::Registry(RegistryError::DuplicateName {
                name: ToolName::new("echo").unwrap()
            })
        );
    }
}
