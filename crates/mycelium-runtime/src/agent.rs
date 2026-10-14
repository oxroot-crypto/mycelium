//! 运行时核心：装配结果、运行入口与事件流。

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;
use futures_util::stream;
use mycelium_core::event::AgentEvent;
use mycelium_core::{AgentConfig, AgentError};
use mycelium_model::ModelProvider;
use mycelium_tools::ToolRegistry;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::builder::AgentRuntimeBuilder;
use crate::engine::{EVENT_CHANNEL_CAPACITY, Engine};
use crate::hooks::RunHooks;
use crate::run::{RunInput, RunOutcome};

/// 运行事件流。
pub type AgentEventStream = Pin<Box<dyn Stream<Item = AgentEvent> + Send + 'static>>;

/// Agent 运行时。
///
/// 装配完成后不可变；每次运行生成独立 `RunId`，并遵守
/// `docs/architecture/lifecycle.md` 的状态机与事件不变量。
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

    /// 执行一次运行（不返回过程事件）。
    ///
    /// 观测方式：需要过程事件时使用 [`AgentRuntime::run_events`]；需要同步回调时配置
    /// [`RunHooks`]。
    ///
    /// # Errors
    /// - 预算超限：[`AgentError::BudgetExceeded`]；
    /// - 取消：[`AgentError::Cancelled`]；
    /// - 模型 / 工具层错误：[`AgentError::Model`] / [`AgentError::Tool`]；
    /// - 提供方违反流协议：[`AgentError::Model`]（`ModelError::InvalidResponse`）。
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    ///
    /// use mycelium_core::{AgentConfig, ModelId, StopReason};
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
    /// let outcome = runtime.run(RunInput::from_user_text("ping")).await?;
    /// assert_eq!(outcome.steps, 1);
    /// assert_eq!(outcome.messages.len(), 1);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run(&self, input: RunInput) -> Result<RunOutcome, AgentError> {
        Engine::new(
            self.config.clone(),
            self.provider.clone(),
            self.tools.clone(),
            self.hooks.clone(),
        )
        .run(input, None)
        .await
    }

    /// 执行一次运行并返回事件流句柄（内部 spawn 运行任务）。
    ///
    /// 用法：`split()` 后并行消费事件流与结果；或直接 `wait()` 忽略事件只取结果。
    ///
    /// # Panics
    /// 必须在 tokio 运行时上下文中调用（内部依赖 `tokio::spawn`）。
    ///
    /// # Examples
    /// ```
    /// use std::sync::Arc;
    ///
    /// use futures_util::StreamExt;
    /// use mycelium_core::{AgentConfig, ModelId, StopReason};
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
    /// let (mut events, result) = runtime.run_events(RunInput::from_user_text("ping")).split();
    /// let mut seen = 0_usize;
    /// while let Some(_event) = events.next().await {
    ///     seen += 1;
    /// }
    /// assert!(seen > 0);
    /// assert_eq!(result.await_result().await?.steps, 1);
    /// # Ok(())
    /// # }
    /// ```
    pub fn run_events(&self, input: RunInput) -> AgentRun {
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let engine = Engine::new(
            self.config.clone(),
            self.provider.clone(),
            self.tools.clone(),
            self.hooks.clone(),
        );
        let handle = tokio::spawn(async move { engine.run(input, Some(sender)).await });
        let events: AgentEventStream =
            Box::pin(stream::unfold(receiver, |mut receiver| async move {
                receiver.recv().await.map(|event| (event, receiver))
            }));
        AgentRun { events, handle }
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

/// 运行句柄：事件流 + 结果句柄。
pub struct AgentRun {
    events: AgentEventStream,
    handle: JoinHandle<Result<RunOutcome, AgentError>>,
}

impl AgentRun {
    /// 借用事件流。
    pub fn events(&mut self) -> &mut AgentEventStream {
        &mut self.events
    }

    /// 拆分为事件流与结果句柄（可并行消费）。
    pub fn split(self) -> (AgentEventStream, RunFuture) {
        (
            self.events,
            RunFuture {
                handle: self.handle,
            },
        )
    }

    /// 等待运行结束；未消费的事件被显式丢弃（不影响运行正确性）。
    ///
    /// # Errors
    /// 运行失败返回原始 [`AgentError`]；任务异常终止归约为 [`AgentError::Internal`]。
    pub async fn wait(self) -> Result<RunOutcome, AgentError> {
        let AgentRun { events, handle } = self;
        // 显式关闭事件接收端：生产者发送失败后继续运行（事件为可选观测面）。
        drop(events);
        RunFuture { handle }.await_result().await
    }
}

impl fmt::Debug for AgentRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentRun")
            .field("events", &"AgentEventStream")
            .field("handle", &self.handle)
            .finish()
    }
}

/// 运行结果句柄（由 [`AgentRun::split`] 产出）。
pub struct RunFuture {
    handle: JoinHandle<Result<RunOutcome, AgentError>>,
}

impl RunFuture {
    /// 等待运行结束并取回结果。
    ///
    /// # Errors
    /// 运行失败返回原始 [`AgentError`]；任务异常终止（panic）归约为
    /// [`AgentError::Internal`]。
    pub async fn await_result(self) -> Result<RunOutcome, AgentError> {
        match self.handle.await {
            Ok(result) => result,
            Err(join_error) => Err(AgentError::Internal {
                message: format!("运行任务异常终止：{join_error}"),
            }),
        }
    }
}

impl fmt::Debug for RunFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunFuture")
            .field("handle", &self.handle)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use mycelium_core::ModelId;
    use mycelium_core::event::AgentEventKind;

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
    async fn run_events_stream_matches_wait() {
        let runtime = runtime_with_stub();
        let (mut events, result) = runtime.run_events(RunInput::from_user_text("ping")).split();

        let mut collected = Vec::new();
        while let Some(event) = events.next().await {
            collected.push(event);
        }
        let outcome = result.await_result().await.unwrap();

        assert_eq!(outcome.steps, 1);
        assert_eq!(outcome.messages.len(), 1);
        for (index, event) in collected.iter().enumerate() {
            assert_eq!(event.seq, index as u64);
        }
        assert!(matches!(
            collected.first().expect("应有事件").kind,
            AgentEventKind::RunStarted { .. }
        ));
        assert!(matches!(
            collected.last().expect("应有事件").kind,
            AgentEventKind::RunCompleted { steps: 1, .. }
        ));
    }

    #[tokio::test]
    async fn wait_returns_outcome_without_consuming_events() {
        let runtime = runtime_with_stub();
        let outcome = runtime
            .run_events(RunInput::from_user_text("ping"))
            .wait()
            .await
            .unwrap();
        assert_eq!(outcome.steps, 1);
        assert_eq!(outcome.messages.len(), 1);
    }

    #[tokio::test]
    async fn events_accessor_borrows_stream() {
        let runtime = runtime_with_stub();
        let mut run = runtime.run_events(RunInput::from_user_text("ping"));
        let first = run.events().next().await.expect("应有事件");
        assert_eq!(first.seq, 0);
        let outcome = run.wait().await.unwrap();
        assert_eq!(outcome.steps, 1);
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
