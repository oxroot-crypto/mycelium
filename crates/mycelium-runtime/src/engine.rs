//! 执行循环引擎。
//!
//! 实现 `docs/architecture/lifecycle.md` 定义的状态机与运行语义：工具顺序 / 并发执行、
//! 展开策略与超时、预算 / 取消 / 超时在循环边界强制、事件从统一发射器产出
//! （`seq` 连续且与发送顺序严格一致）。
//!
//! 契约编号见 `docs/spec/core.md` §2.14 / §2.16 / §2.17。
//!
//! 事件是可选观测面：`run` 路径不创建事件通道；`run_events` 路径通过有界通道
//! （[`EVENT_CHANNEL_CAPACITY`]）产生背压，接收端关闭后运行不中断。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{StreamExt, stream};
use mycelium_core::event::{AgentEvent, AgentEventKind};
use mycelium_core::message::{ContentBlock, Message};
use mycelium_core::response::{ModelResponse, Usage};
use mycelium_core::run::{RunEvent, RunState};
use mycelium_core::tool::ToolCall;
use mycelium_core::{
    AgentConfig, AgentError, BudgetKind, CancellationToken, ModelError, RunId, ToolError,
};
use mycelium_model::{DeltaAssembler, ModelDelta, ModelProvider, ModelRequest};
use mycelium_tools::policy::{PolicyDecision, ToolPolicy};
use mycelium_tools::schema::validate_arguments;
use mycelium_tools::{Tool, ToolContext, ToolOutput, ToolRegistry};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::mpsc::Sender;

use crate::hooks::{RunContext, RunHooks};
use crate::run::{RunInput, RunOutcome};

/// 事件通道容量（背压边界）。
pub(crate) const EVENT_CHANNEL_CAPACITY: usize = 256;

/// 事件发射器（可克隆）。
///
/// 并发发射时 `seq` 的分配与发送在同一把锁内完成，保证消费者观察到的事件顺序
/// 与 `seq` 顺序严格一致。
#[derive(Clone)]
pub(crate) struct Emitter {
    run_id: RunId,
    seq: Arc<AsyncMutex<u64>>,
    events: Option<Sender<AgentEvent>>,
    closed: Arc<AtomicBool>,
}

impl Emitter {
    /// 创建发射器。
    pub(crate) fn new(run_id: RunId, events: Option<Sender<AgentEvent>>) -> Self {
        Self {
            run_id,
            seq: Arc::new(AsyncMutex::new(0)),
            events,
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 发射一个事件。
    ///
    /// 返回显式 `Send` 的装箱 future（规避 rustc 对 `&self` 异步方法在
    /// `tokio::spawn` 边界上的 HRTB 限制）。接收端关闭后显式标记并停止发射
    /// （事件为可选观测面，运行继续）。
    pub(crate) fn emit<'a>(
        &'a self,
        kind: AgentEventKind,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if self.closed.load(Ordering::Relaxed) {
                return;
            }
            let mut seq = self.seq.lock().await;
            let event = AgentEvent {
                run_id: self.run_id,
                seq: *seq,
                at_ms: now_ms(),
                kind,
            };
            *seq = seq.saturating_add(1);
            if let Some(sender) = &self.events
                && sender.send(event).await.is_err()
            {
                self.closed.store(true, Ordering::Relaxed);
            }
        })
    }
}

/// 循环内部结果（成功路径）。
struct RunOutput {
    messages: Vec<Message>,
    usage: Usage,
    steps: u32,
}

/// 引擎装配部件（由 [`crate::AgentRuntime`] 提供，避免长参数列表）。
pub(crate) struct RuntimeParts {
    /// Agent 配置。
    pub(crate) config: AgentConfig,
    /// 模型提供方。
    pub(crate) provider: Arc<dyn ModelProvider>,
    /// 工具注册表。
    pub(crate) tools: ToolRegistry,
    /// 权限策略。
    pub(crate) policy: Arc<dyn ToolPolicy>,
    /// 运行钩子。
    pub(crate) hooks: Option<Arc<dyn RunHooks>>,
}

/// 执行引擎：一次运行的全部可变状态。
pub(crate) struct Engine {
    config: AgentConfig,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
    policy: Arc<dyn ToolPolicy>,
    hooks: Option<Arc<dyn RunHooks>>,
    run_id: RunId,
    cancel: CancellationToken,
    emitter: Emitter,
}

impl Engine {
    /// 创建引擎（`run_id` 在此生成）。
    pub(crate) fn new(parts: RuntimeParts) -> Self {
        let RuntimeParts {
            config,
            provider,
            tools,
            policy,
            hooks,
        } = parts;
        let run_id = RunId::generate();
        Self {
            config,
            provider,
            tools,
            policy,
            hooks,
            run_id,
            cancel: CancellationToken::new(),
            emitter: Emitter::new(run_id, None),
        }
    }

    /// 执行一次运行。
    ///
    /// 终态事件（`RunCompleted` / `RunFailed` / `RunCancelled`）在此统一发射，
    /// 保证「恰好一条且为最后一条」。
    pub(crate) async fn run(
        mut self,
        input: RunInput,
        events: Option<Sender<AgentEvent>>,
    ) -> Result<RunOutcome, AgentError> {
        self.emitter = Emitter::new(self.run_id, events);
        self.cancel = input.cancel.clone();
        tracing::debug!(run_id = %self.run_id, provider = self.provider.id(), "运行开始");
        self.emitter
            .emit(AgentEventKind::RunStarted {
                message_count: input.messages.len(),
            })
            .await;

        let context = self.context();
        if let Some(hooks) = &self.hooks {
            hooks.on_run_start(&context).await;
        }

        match self.drive(input).await {
            Ok(output) => {
                self.emitter
                    .emit(AgentEventKind::RunCompleted {
                        usage: output.usage,
                        steps: output.steps,
                    })
                    .await;
                let outcome = RunOutcome {
                    messages: output.messages,
                    usage: output.usage,
                    steps: output.steps,
                };
                if let Some(hooks) = &self.hooks {
                    hooks.on_run_end(&context, Ok(&outcome)).await;
                }
                tracing::debug!(run_id = %self.run_id, steps = outcome.steps, "运行完成");
                Ok(outcome)
            }
            Err(error) => {
                match &error {
                    AgentError::Cancelled => self.emitter.emit(AgentEventKind::RunCancelled).await,
                    other => {
                        self.emitter
                            .emit(AgentEventKind::RunFailed {
                                message: other.to_string(),
                            })
                            .await;
                    }
                }
                if let Some(hooks) = &self.hooks {
                    hooks.on_run_end(&context, Err(&error)).await;
                }
                tracing::debug!(run_id = %self.run_id, error = %error, "运行失败");
                Err(error)
            }
        }
    }

    /// 构造钩子上下文。
    fn context(&self) -> RunContext {
        RunContext {
            run_id: self.run_id,
            cancel: self.cancel.clone(),
        }
    }

    /// 主循环：模型请求 → 工具执行 → 回灌，直至无工具调用或到达预算 / 取消边界。
    async fn drive(&mut self, input: RunInput) -> Result<RunOutput, AgentError> {
        let mut state = transition(RunState::Created, RunEvent::Start)?;
        let mut messages = input.messages;
        let history_start = messages.len();
        let mut usage = Usage::default();
        let mut steps: u32 = 0;
        let started = Instant::now();

        loop {
            if self.cancel.is_cancelled() {
                transition(state, RunEvent::Cancel)?;
                return Err(AgentError::Cancelled);
            }

            // 时长预算：在循环边界强制（超限后不再发起新的模型请求）。
            if let Some(limit) = self.config.max_duration() {
                let elapsed = started.elapsed();
                if elapsed > limit {
                    transition(state, RunEvent::Fail)?;
                    return Err(AgentError::BudgetExceeded {
                        kind: BudgetKind::DurationMs,
                        limit: millis_of(limit),
                        observed: millis_of(elapsed),
                    });
                }
            }

            let next_step = steps.checked_add(1).ok_or_else(|| AgentError::Internal {
                message: "步数溢出".to_owned(),
            })?;
            let max_steps = self.config.max_steps().get();
            if next_step > max_steps {
                transition(state, RunEvent::Fail)?;
                return Err(AgentError::BudgetExceeded {
                    kind: BudgetKind::Steps,
                    limit: u64::from(max_steps),
                    observed: u64::from(next_step),
                });
            }
            steps = next_step;

            state = transition(state, RunEvent::RequestModel)?;
            self.emitter
                .emit(AgentEventKind::StepStarted { step: steps })
                .await;
            if let Some(hooks) = &self.hooks {
                hooks.on_step_start(&self.context(), steps).await;
            }
            self.emitter
                .emit(AgentEventKind::ModelRequestStarted {
                    model: self.config.model().clone(),
                    message_count: messages.len(),
                    tool_count: self.tools.len(),
                })
                .await;

            let response = match self.call_model(&messages).await {
                Ok(response) => {
                    state = transition(state, RunEvent::ModelCompleted)?;
                    response
                }
                Err(error) => {
                    transition(state, RunEvent::ModelFailed)?;
                    return Err(error);
                }
            };

            usage = Usage::new(
                usage
                    .input_tokens
                    .saturating_add(response.usage.input_tokens),
                usage
                    .output_tokens
                    .saturating_add(response.usage.output_tokens),
            )
            .with_reasoning_tokens(
                usage
                    .reasoning_tokens
                    .saturating_add(response.usage.reasoning_tokens),
            );
            if let Some(limit) = self.config.max_output_tokens()
                && usage.output_tokens > limit.get()
            {
                transition(state, RunEvent::Fail)?;
                return Err(AgentError::BudgetExceeded {
                    kind: BudgetKind::OutputTokens,
                    limit: limit.get(),
                    observed: usage.output_tokens,
                });
            }

            self.emitter
                .emit(AgentEventKind::ModelRequestFinished {
                    stop_reason: response.stop_reason,
                    usage: response.usage,
                })
                .await;

            let tool_calls: Vec<ToolCall> = response
                .message
                .content()
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some(ToolCall::new(id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect();
            for call in &tool_calls {
                self.emitter
                    .emit(AgentEventKind::ModelToolCall { call: call.clone() })
                    .await;
            }
            messages.push(response.message);

            if tool_calls.is_empty() {
                // 终态：状态机此处仅校验转移合法性（RunEvent::Complete）。
                transition(state, RunEvent::Complete)?;
                return Ok(RunOutput {
                    messages: messages.split_off(history_start),
                    usage,
                    steps,
                });
            }

            state = transition(state, RunEvent::ToolUseRequested)?;
            let results = self.execute_tools(&tool_calls).await;
            for (call, result) in tool_calls.iter().zip(results) {
                let output = match result {
                    Ok(output) => output,
                    Err(AgentError::Cancelled) => {
                        transition(state, RunEvent::Cancel)?;
                        return Err(AgentError::Cancelled);
                    }
                    Err(other) => {
                        transition(state, RunEvent::Fail)?;
                        return Err(other);
                    }
                };
                let is_error = output.is_error();
                messages.push(Message::tool_result_blocks(
                    call.id.clone(),
                    output.into_blocks(),
                    is_error,
                ));
            }
            state = transition(state, RunEvent::ToolsCompleted)?;
        }
    }

    /// 按配置顺序 / 并发执行工具调用；结果保持**调用顺序**。
    async fn execute_tools(&self, calls: &[ToolCall]) -> Vec<Result<ToolOutput, AgentError>> {
        let runtime = ToolRuntime {
            emitter: self.emitter.clone(),
            tools: self.tools.clone(),
            policy: Arc::clone(&self.policy),
            hooks: self.hooks.clone(),
            run_id: self.run_id,
            cancel: self.cancel.clone(),
            tool_timeout: self.config.tool_timeout(),
        };
        let concurrency = self.config.max_parallel_tool_calls().get();
        // 显式循环构造 future 列表（规避 rustc 对高阶闭包返回 future 的 HRTB 限制）。
        let mut futures = Vec::with_capacity(calls.len());
        for call in calls {
            let runtime = runtime.clone();
            let call = call.clone();
            futures.push(async move { runtime.invoke(&call).await });
        }
        stream::iter(futures).buffered(concurrency).collect().await
    }

    /// 调用模型：构造请求、按重试策略执行（含超时与指数退避）、消费流并装配响应。
    ///
    /// 重试语义（FC-POST-160 ~ FC-ERR-061）：
    /// - 仅对 `ModelError::is_retryable()` 为真的错误重试；
    /// - 每次失败后在退避前发射 `ModelRetry` 事件；
    /// - 退避等待可被取消打断；
    /// - 重试不增加步数（步数按逻辑轮次计）。
    async fn call_model(&mut self, messages: &[Message]) -> Result<ModelResponse, AgentError> {
        let request = ModelRequest {
            model: self.config.model().clone(),
            system_prompt: self.config.system_prompt().map(str::to_owned),
            messages: messages.to_vec(),
            tools: self.tools.specs(),
            tool_choice: self.config.tool_choice().clone(),
            max_output_tokens: self.config.max_output_tokens().map(|value| value.get()),
            temperature: self.config.temperature(),
            reasoning: self.config.reasoning().cloned(),
            cancel: self.cancel.clone(),
        };

        let mut attempt: u32 = 1;
        loop {
            match self.attempt_model_call(&request).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    let Some(policy) = self.config.retry() else {
                        return Err(error);
                    };
                    let retryable = matches!(
                        &error,
                        AgentError::Model(model_error) if model_error.is_retryable()
                    );
                    if !retryable || attempt >= policy.max_attempts().get() {
                        return Err(error);
                    }
                    let delay = policy.backoff_for(attempt);
                    self.emitter
                        .emit(AgentEventKind::ModelRetry {
                            attempt,
                            delay_ms: millis_of(delay),
                            reason: error.to_string(),
                        })
                        .await;
                    tracing::debug!(
                        attempt,
                        delay_ms = millis_of(delay),
                        error = %error,
                        "模型请求失败，退避后重试"
                    );
                    let cancel = self.cancel.clone();
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => return Err(AgentError::Cancelled),
                        () = tokio::time::sleep(delay) => {}
                    }
                    attempt += 1;
                }
            }
        }
    }

    /// 单次模型请求（含请求超时）。
    async fn attempt_model_call(
        &mut self,
        request: &ModelRequest,
    ) -> Result<ModelResponse, AgentError> {
        let timeout = self.config.request_timeout();
        let timeout_ms = millis_of(timeout);
        match tokio::time::timeout(timeout, self.consume_model_stream(request.clone())).await {
            Ok(result) => result,
            Err(_elapsed) => Err(ModelError::Timeout { timeout_ms }.into()),
        }
    }

    /// 消费模型流：发射文本增量事件、装配增量、响应取消。
    async fn consume_model_stream(
        &mut self,
        request: ModelRequest,
    ) -> Result<ModelResponse, AgentError> {
        let stream = self.provider.stream(request).await?;
        tokio::pin!(stream);
        let cancel = self.cancel.clone();
        let mut assembler = DeltaAssembler::new();

        loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(AgentError::Cancelled),
                item = stream.next() => {
                    let Some(item) = item else { break };
                    match item {
                        Ok(delta) => {
                            match &delta {
                                ModelDelta::TextDelta { text } => {
                                    self.emitter
                                        .emit(AgentEventKind::ModelTextDelta { text: text.clone() })
                                        .await;
                                }
                                ModelDelta::ThinkingDelta { text } => {
                                    self.emitter
                                        .emit(AgentEventKind::ModelThinkingDelta {
                                            text: text.clone(),
                                        })
                                        .await;
                                }
                                _ => {}
                            }
                            assembler.push(delta)?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        let response = assembler.finish()?;
        Ok(response)
    }
}

/// 工具执行上下文（可克隆进并发任务）。
#[derive(Clone)]
struct ToolRuntime {
    emitter: Emitter,
    tools: ToolRegistry,
    policy: Arc<dyn ToolPolicy>,
    hooks: Option<Arc<dyn RunHooks>>,
    run_id: RunId,
    cancel: CancellationToken,
    tool_timeout: Option<Duration>,
}

impl ToolRuntime {
    /// 执行一次调用：事件配对 + 钩子（并发模式下按完成顺序触发）。
    async fn invoke(&self, call: &ToolCall) -> Result<ToolOutput, AgentError> {
        self.emitter
            .emit(AgentEventKind::ToolCallStarted {
                call_id: call.id.clone(),
                name: call.name.clone(),
            })
            .await;

        let outcome = self.execute(call).await;

        // 无论成功、错误还是取消，都必须补齐 Finished 以维持配对不变量。
        let is_error = match &outcome {
            Ok(output) => output.is_error(),
            Err(_) => true,
        };
        self.emitter
            .emit(AgentEventKind::ToolCallFinished {
                call_id: call.id.clone(),
                name: call.name.clone(),
                is_error,
            })
            .await;
        if let Some(hooks) = &self.hooks {
            hooks
                .on_tool_call(
                    &RunContext {
                        run_id: self.run_id,
                        cancel: self.cancel.clone(),
                    },
                    call,
                )
                .await;
        }
        outcome
    }

    /// 策略检查 → 注册表查找 → 入参校验 → 执行。
    async fn execute(&self, call: &ToolCall) -> Result<ToolOutput, AgentError> {
        if self.cancel.is_cancelled() {
            return Err(AgentError::Cancelled);
        }

        // 权限策略（在入参校验与执行之前；支持取消竞争）。
        let decision = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(AgentError::Cancelled),
            decision = self.policy.check(&call.name, &call.arguments) => decision,
        };
        if let PolicyDecision::Deny { reason } = decision {
            tracing::debug!(tool = %call.name, reason = %reason, "工具调用被策略拒绝");
            return Ok(ToolOutput::error(format!(
                "工具调用被拒绝（{}）：{reason}",
                call.name
            )));
        }

        let Some(tool) = self.tools.get(&call.name) else {
            return Ok(ToolOutput::error(format!("工具未注册：{}", call.name)));
        };
        if let Err(violation) = validate_arguments(tool.spec().input_schema(), &call.arguments) {
            return Ok(ToolOutput::error(format!("工具入参非法：{violation}")));
        }

        let context = ToolContext {
            run_id: self.run_id,
            session_id: None,
            call_id: call.id.clone(),
            cancel: self.cancel.clone(),
        };
        let result = self.call_guarded(tool.as_ref(), &context, call).await;
        match result {
            Ok(output) => Ok(output),
            Err(ToolError::Cancelled { .. }) => Err(AgentError::Cancelled),
            Err(error) => {
                tracing::debug!(tool = %call.name, error = %error, "工具执行失败（作为错误结果回灌）");
                Ok(ToolOutput::error(error.to_string()))
            }
        }
    }

    /// 执行工具本体：取消竞争 + 可选超时。
    async fn call_guarded(
        &self,
        tool: &dyn Tool,
        context: &ToolContext,
        call: &ToolCall,
    ) -> Result<ToolOutput, ToolError> {
        let invoke = tool.call(context, call.arguments.clone());
        match self.tool_timeout {
            Some(limit) => {
                tokio::select! {
                    biased;
                    () = self.cancel.cancelled() => {
                        Err(ToolError::Cancelled { tool: call.name.clone() })
                    }
                    result = tokio::time::timeout(limit, invoke) => match result {
                        Ok(result) => result,
                        Err(_elapsed) => Err(ToolError::Timeout {
                            tool: call.name.clone(),
                            timeout_ms: millis_of(limit),
                        }),
                    },
                }
            }
            None => {
                tokio::select! {
                    biased;
                    () = self.cancel.cancelled() => {
                        Err(ToolError::Cancelled { tool: call.name.clone() })
                    }
                    result = invoke => result,
                }
            }
        }
    }
}

/// 状态转移（非法转移归约为内部缺陷，不 panic）。
fn transition(state: RunState, event: RunEvent) -> Result<RunState, AgentError> {
    state
        .transition(event)
        .map_err(|error| AgentError::Internal {
            message: format!("非法状态转移（引擎缺陷）：{error}"),
        })
}

/// 当前 Unix 时间戳（毫秒）；系统时钟异常时返回 0。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// 时长转毫秒（饱和到 `u64::MAX`）。
fn millis_of(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use futures_util::StreamExt;
    use mycelium_core::event::{AgentEvent, AgentEventKind};
    use mycelium_core::message::ContentBlock;
    use mycelium_core::{
        ModelError, ModelId, RetryPolicy, Role, StopReason, ToolChoice, ToolName, ToolResultBlock,
    };
    use mycelium_model::ModelProvider;
    use mycelium_tools::Tool;
    use mycelium_tools::policy::{AllowAll, DenyList};

    use super::*;
    use crate::AgentRuntime;
    use crate::agent_tool::AgentTool;
    use crate::run::RunInput;
    use crate::test_support::{
        AsyncRecordingHooks, CancelRecorderProvider, RecordingHooks, ScriptedProvider,
        ScriptedResponse, cancel_wait_tool, cancelled_tool, counting_echo_tool, delay_tool,
        echo_tool, failing_tool, pending_provider, slow_tool, strict_tool, structured_tool,
        text_response, text_response_with_usage, tool_call_response, tool_call_response_with_usage,
        tool_calls_response,
    };

    fn base_config() -> AgentConfig {
        AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .build()
            .unwrap()
    }

    fn build_runtime(
        config: AgentConfig,
        provider: Arc<dyn ModelProvider>,
        tools: Vec<Arc<dyn Tool>>,
    ) -> AgentRuntime {
        let mut builder = AgentRuntime::builder().config(config).provider(provider);
        for tool in tools {
            builder = builder.tool(tool);
        }
        builder.build().unwrap()
    }

    async fn collect_events(
        runtime: &AgentRuntime,
        input: RunInput,
    ) -> (Vec<AgentEvent>, Result<RunOutcome, AgentError>) {
        let (mut events, result) = runtime.run_events(input).split();
        let mut collected = Vec::new();
        while let Some(event) = events.next().await {
            collected.push(event);
        }
        (collected, result.await_result().await)
    }

    fn terminal_count(events: &[AgentEvent]) -> usize {
        events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    AgentEventKind::RunCompleted { .. }
                        | AgentEventKind::RunFailed { .. }
                        | AgentEventKind::RunCancelled
                )
            })
            .count()
    }

    /// 汇总工具结果中的文本块（测试辅助）。
    fn tool_text(content: &[ToolResultBlock]) -> String {
        content
            .iter()
            .filter_map(|block| match block {
                ToolResultBlock::Text { text } => Some(text.as_str()),
                ToolResultBlock::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn completes_without_tools() {
        let provider = Arc::new(ScriptedProvider::new(vec![text_response("pong")]));
        let runtime = build_runtime(base_config(), provider.clone(), vec![]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        assert_eq!(outcome.steps, 1);
        assert_eq!(outcome.messages.len(), 1);
        assert_eq!(outcome.messages[0].role(), Role::Assistant);
        assert_eq!(
            outcome.messages[0].content(),
            &[ContentBlock::Text {
                text: "pong".to_owned()
            }]
        );
        assert_eq!(provider.call_count(), 1);
    }

    #[tokio::test]
    async fn executes_tools_sequentially_and_feeds_back() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "call_1", "{\"text\":\"hi\"}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider.clone(), vec![echo_tool()]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        assert_eq!(outcome.steps, 2);
        assert_eq!(outcome.messages.len(), 3);
        assert_eq!(outcome.messages[0].role(), Role::Assistant);
        assert!(matches!(
            outcome.messages[0].content()[0],
            ContentBlock::ToolUse { .. }
        ));
        assert_eq!(outcome.messages[1].role(), Role::Tool);
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_call_id.as_str(), "call_1");
                assert_eq!(tool_text(content), "{\"text\":\"hi\"}");
                assert!(!is_error);
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
        assert_eq!(
            outcome.messages[2].content(),
            &[ContentBlock::Text {
                text: "done".to_owned()
            }]
        );

        // 第二次请求包含工具结果消息与已注册工具
        let request = provider.last_request().expect("应捕获到请求");
        assert!(
            request
                .messages
                .iter()
                .any(|message| message.role() == Role::Tool)
        );
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tool_choice, ToolChoice::Auto);
    }

    #[tokio::test]
    async fn outcome_tracks_usage_and_steps() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response_with_usage("echo", "c1", "{}", 10, 5),
            text_response_with_usage("done", 7, 3),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![echo_tool()]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        assert_eq!(outcome.steps, 2);
        assert_eq!(outcome.usage, Usage::new(17, 8));
    }

    #[tokio::test]
    async fn request_matches_config_and_history() {
        let config = AgentConfig::builder()
            .model(ModelId::new("test-model").unwrap())
            .system_prompt("be nice")
            .temperature(0.5)
            .max_output_tokens(100)
            .tool_choice(ToolChoice::Required)
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(config, provider.clone(), vec![echo_tool()]);

        runtime.run(RunInput::from_user_text("ping")).await.unwrap();

        let request = provider.last_request().expect("应捕获到请求");
        assert_eq!(request.model.as_str(), "test-model");
        assert_eq!(request.system_prompt.as_deref(), Some("be nice"));
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tool_choice, ToolChoice::Required);
        assert_eq!(
            request.temperature.map(f32::to_bits),
            Some(0.5_f32.to_bits())
        );
        assert_eq!(request.max_output_tokens, Some(100));
        assert!(request.messages.len() >= 3);
    }

    #[tokio::test]
    async fn unknown_tool_error_is_fed_back() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("missing_tool", "c1", "{}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(is_error);
                assert!(tool_text(content).contains("工具未注册"));
                assert!(tool_text(content).contains("missing_tool"));
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_arguments_error_is_fed_back() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("strict", "c1", "{\"text\":1}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![strict_tool()]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(is_error);
                assert!(tool_text(content).contains("工具入参非法"));
                assert!(tool_text(content).contains("$.text"));
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn failing_tool_error_is_fed_back() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("failing", "c1", "{}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![failing_tool()]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(is_error);
                assert!(tool_text(content).contains("boom"));
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn model_error_terminates_run() {
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Error(
            ModelError::Transport {
                message: "net down".to_owned(),
            },
        )]));
        let runtime = build_runtime(base_config(), provider, vec![]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        assert!(matches!(
            result,
            Err(AgentError::Model(ModelError::Transport { .. }))
        ));
        assert_eq!(terminal_count(&events), 1);
        match &events.last().expect("应有事件").kind {
            AgentEventKind::RunFailed { message } => assert!(message.contains("net down")),
            other => panic!("期望 RunFailed，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn step_budget_exceeded() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .max_steps(2)
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            tool_call_response("echo", "c2", "{}"),
        ]));
        let runtime = build_runtime(config, provider.clone(), vec![echo_tool()]);

        let result = runtime.run(RunInput::from_user_text("ping")).await;
        assert!(matches!(
            result,
            Err(AgentError::BudgetExceeded {
                kind: BudgetKind::Steps,
                limit: 2,
                observed: 3,
            })
        ));
        assert_eq!(provider.call_count(), 2);
    }

    #[tokio::test]
    async fn output_token_budget_exceeded() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .max_output_tokens(5)
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response_with_usage("echo", "c1", "{}", 1, 3),
            text_response_with_usage("done", 1, 3),
        ]));
        let runtime = build_runtime(config, provider, vec![echo_tool()]);

        let result = runtime.run(RunInput::from_user_text("ping")).await;
        assert!(matches!(
            result,
            Err(AgentError::BudgetExceeded {
                kind: BudgetKind::OutputTokens,
                limit: 5,
                observed: 6,
            })
        ));
    }

    #[tokio::test]
    async fn request_timeout_maps_to_model_timeout() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .request_timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let runtime = build_runtime(config, pending_provider(), vec![]);

        let result = runtime.run(RunInput::from_user_text("ping")).await;
        assert!(matches!(
            result,
            Err(AgentError::Model(ModelError::Timeout { timeout_ms: 50 }))
        ));
    }

    #[tokio::test]
    async fn cancel_before_start() {
        let provider = Arc::new(ScriptedProvider::new(vec![text_response("pong")]));
        let runtime = build_runtime(base_config(), provider.clone(), vec![]);

        let input = RunInput::from_user_text("ping");
        input.cancel.cancel();
        let result = runtime.run(input).await;
        assert!(matches!(result, Err(AgentError::Cancelled)));
        assert_eq!(provider.call_count(), 0);
    }

    #[tokio::test]
    async fn cancel_during_stream() {
        let runtime = build_runtime(base_config(), pending_provider(), vec![]);
        let input = RunInput::from_user_text("ping");
        let cancel = input.cancel.clone();
        let (mut events, result) = runtime.run_events(input).split();

        loop {
            let event = events.next().await.expect("应有事件");
            if matches!(event.kind, AgentEventKind::ModelRequestStarted { .. }) {
                break;
            }
        }
        cancel.cancel();

        let outcome = tokio::time::timeout(Duration::from_secs(2), result.await_result())
            .await
            .expect("取消后应迅速结束");
        assert!(matches!(outcome, Err(AgentError::Cancelled)));

        let mut last = None;
        while let Some(event) = events.next().await {
            last = Some(event);
        }
        assert!(matches!(
            last.expect("应有终态事件").kind,
            AgentEventKind::RunCancelled
        ));
    }

    #[tokio::test]
    async fn cancel_during_tool() {
        let provider = Arc::new(ScriptedProvider::new(vec![tool_call_response(
            "cancel_wait",
            "c1",
            "{}",
        )]));
        let runtime = build_runtime(base_config(), provider, vec![cancel_wait_tool()]);
        let input = RunInput::from_user_text("ping");
        let cancel = input.cancel.clone();
        let (mut events, result) = runtime.run_events(input).split();

        loop {
            let event = events.next().await.expect("应有事件");
            if matches!(event.kind, AgentEventKind::ToolCallStarted { .. }) {
                break;
            }
        }
        cancel.cancel();

        let outcome = tokio::time::timeout(Duration::from_secs(2), result.await_result())
            .await
            .expect("取消后应迅速结束");
        assert!(matches!(outcome, Err(AgentError::Cancelled)));

        let mut remaining = Vec::new();
        while let Some(event) = events.next().await {
            remaining.push(event);
        }
        // 配对不变量：取消路径同样补齐 ToolCallFinished
        assert!(remaining.iter().any(|event| matches!(
            event.kind,
            AgentEventKind::ToolCallFinished { is_error: true, .. }
        )));
        assert!(matches!(
            remaining.last().expect("应有终态事件").kind,
            AgentEventKind::RunCancelled
        ));
    }

    #[tokio::test]
    async fn tool_cancelled_error_yields_cancelled() {
        let provider = Arc::new(ScriptedProvider::new(vec![tool_call_response(
            "cancelled_tool",
            "c1",
            "{}",
        )]));
        let runtime = build_runtime(base_config(), provider, vec![cancelled_tool()]);

        let result = runtime.run(RunInput::from_user_text("ping")).await;
        assert!(matches!(result, Err(AgentError::Cancelled)));
    }

    #[tokio::test]
    async fn events_sequence_invariants() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![echo_tool()]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        let outcome = result.unwrap();
        assert_eq!(outcome.steps, 2);

        // seq 从 0 连续递增，run_id 一致
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event.seq, index as u64, "seq 必须从 0 连续递增");
            assert_eq!(event.run_id, events[0].run_id, "run_id 必须一致");
        }
        // 首条 RunStarted，终态恰一条且为最后一条
        assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
        assert_eq!(terminal_count(&events), 1);
        assert!(matches!(
            events.last().expect("应有事件").kind,
            AgentEventKind::RunCompleted { steps: 2, .. }
        ));
    }

    #[tokio::test]
    async fn tool_events_are_paired() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![echo_tool()]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        result.unwrap();

        let started: Vec<String> = events
            .iter()
            .filter_map(|event| match &event.kind {
                AgentEventKind::ToolCallStarted { call_id, .. } => {
                    Some(call_id.as_str().to_owned())
                }
                _ => None,
            })
            .collect();
        let finished: Vec<String> = events
            .iter()
            .filter_map(|event| match &event.kind {
                AgentEventKind::ToolCallFinished { call_id, .. } => {
                    Some(call_id.as_str().to_owned())
                }
                _ => None,
            })
            .collect();
        assert_eq!(started, vec!["c1"]);
        assert_eq!(started, finished);
    }

    #[tokio::test]
    async fn hooks_fire_in_expected_order() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            text_response("done"),
        ]));
        let hooks = Arc::new(RecordingHooks::default());
        let runtime = AgentRuntime::builder()
            .config(base_config())
            .provider(provider)
            .tool(echo_tool())
            .hooks(hooks.clone())
            .build()
            .unwrap();

        runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        assert_eq!(
            hooks.calls(),
            vec![
                "run_start",
                "step_start:1",
                "tool_call:echo",
                "step_start:2",
                "run_end:true",
            ]
        );
    }

    #[tokio::test]
    async fn sequential_tool_calls_by_default() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_calls_response(&[("echo", "c1", "{}"), ("echo", "c2", "{}")]),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![echo_tool()]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        result.unwrap();

        let sequence: Vec<String> = events
            .iter()
            .filter_map(|event| match &event.kind {
                AgentEventKind::ToolCallStarted { call_id, .. } => {
                    Some(format!("start:{}", call_id.as_str()))
                }
                AgentEventKind::ToolCallFinished { call_id, .. } => {
                    Some(format!("finish:{}", call_id.as_str()))
                }
                _ => None,
            })
            .collect();
        // 顺序执行：每个调用完整结束后才开始下一个
        assert_eq!(
            sequence,
            vec!["start:c1", "finish:c1", "start:c2", "finish:c2"]
        );
    }

    #[tokio::test]
    async fn concurrent_tools_are_parallel_and_ordered() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .max_parallel_tool_calls(3)
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_calls_response(&[
                ("slow_a", "c1", "{}"),
                ("slow_b", "c2", "{}"),
                ("slow_c", "c3", "{}"),
            ]),
            text_response("done"),
        ]));
        let runtime = build_runtime(
            config,
            provider,
            vec![
                delay_tool("slow_a", 120),
                delay_tool("slow_b", 60),
                delay_tool("slow_c", 0),
            ],
        );

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        let outcome = result.unwrap();

        // 并行证据：完成顺序为 c3 → c2 → c1（与调用顺序不同；顺序执行时必为 c1 → c2 → c3）
        let finish_order: Vec<String> = events
            .iter()
            .filter_map(|event| match &event.kind {
                AgentEventKind::ToolCallFinished { call_id, .. } => {
                    Some(call_id.as_str().to_owned())
                }
                _ => None,
            })
            .collect();
        assert_eq!(finish_order, vec!["c3", "c2", "c1"]);

        // 回灌顺序 == 调用顺序，且**内容与调用一一对应**
        let results: Vec<(String, String)> = outcome
            .messages
            .iter()
            .filter_map(|message| match &message.content()[0] {
                ContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => Some((tool_call_id.as_str().to_owned(), tool_text(content))),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("c1".to_owned(), "slow_a 完成".to_owned()),
                ("c2".to_owned(), "slow_b 完成".to_owned()),
                ("c3".to_owned(), "slow_c 完成".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn concurrent_events_stay_serialized() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .max_parallel_tool_calls(3)
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_calls_response(&[
                ("slow_a", "c1", "{}"),
                ("slow_b", "c2", "{}"),
                ("slow_c", "c3", "{}"),
            ]),
            text_response("done"),
        ]));
        let runtime = build_runtime(
            config,
            provider,
            vec![
                delay_tool("slow_a", 60),
                delay_tool("slow_b", 30),
                delay_tool("slow_c", 0),
            ],
        );

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        result.unwrap();

        // seq 连续且互不重复（并发发射串行化）
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event.seq, index as u64);
        }
        let started = events
            .iter()
            .filter(|event| matches!(event.kind, AgentEventKind::ToolCallStarted { .. }))
            .count();
        let finished = events
            .iter()
            .filter(|event| matches!(event.kind, AgentEventKind::ToolCallFinished { .. }))
            .count();
        assert_eq!(started, 3);
        assert_eq!(finished, 3);
        assert_eq!(terminal_count(&events), 1);
    }

    #[tokio::test]
    async fn tool_timeout_is_fed_back() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .tool_timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("slow", "c1", "{}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(config, provider, vec![slow_tool()]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(is_error);
                assert!(tool_text(content).contains("超时"));
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn concurrent_partial_failure_keeps_order() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .max_parallel_tool_calls(3)
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_calls_response(&[
                ("echo", "c1", "{\"x\":1}"),
                ("failing", "c2", "{}"),
                ("delay_ok", "c3", "{}"),
            ]),
            text_response("done"),
        ]));
        let runtime = build_runtime(
            config,
            provider,
            vec![echo_tool(), failing_tool(), delay_tool("delay_ok", 20)],
        );

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        let results: Vec<(String, bool)> = outcome
            .messages
            .iter()
            .filter_map(|message| match &message.content()[0] {
                ContentBlock::ToolResult {
                    tool_call_id,
                    is_error,
                    ..
                } => Some((tool_call_id.as_str().to_owned(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("c1".to_owned(), false),
                ("c2".to_owned(), true),
                ("c3".to_owned(), false),
            ]
        );
    }

    #[tokio::test]
    async fn thinking_deltas_are_emitted_and_assembled() {
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Deltas(vec![
            ModelDelta::ThinkingDelta {
                text: "先想".to_owned(),
            },
            ModelDelta::ThinkingDelta {
                text: "一下".to_owned(),
            },
            ModelDelta::TextDelta {
                text: "答案".to_owned(),
            },
            ModelDelta::Usage {
                input_tokens: 10,
                output_tokens: 20,
                reasoning_tokens: Some(15),
            },
            ModelDelta::Finished {
                stop_reason: StopReason::EndTurn,
            },
        ])]));
        let runtime = build_runtime(base_config(), provider, vec![]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        let outcome = result.unwrap();

        // 思考增量逐片发射为 ModelThinkingDelta 事件
        let thinking_events: Vec<String> = events
            .iter()
            .filter_map(|event| match &event.kind {
                AgentEventKind::ModelThinkingDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(thinking_events, vec!["先想", "一下"]);

        // 装配为 Thinking 块（位于文本块之前），用量含 reasoning_tokens
        assert_eq!(
            outcome.messages[0].content(),
            &[
                ContentBlock::Thinking {
                    text: "先想一下".to_owned()
                },
                ContentBlock::Text {
                    text: "答案".to_owned()
                },
            ]
        );
        assert_eq!(outcome.usage, Usage::new(10, 20).with_reasoning_tokens(15));
    }

    #[tokio::test]
    async fn policy_allow_executes_tool() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{\"x\":1}"),
            text_response("done"),
        ]));
        let counter = Arc::new(AtomicUsize::new(0));
        let runtime = AgentRuntime::builder()
            .config(base_config())
            .provider(provider)
            .tool(counting_echo_tool(Arc::clone(&counter)))
            .policy(Arc::new(AllowAll))
            .build()
            .unwrap();

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert_eq!(tool_text(content), "{\"x\":1}");
                assert!(!is_error);
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn policy_deny_is_fed_back_without_execution() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            text_response("done"),
        ]));
        let counter = Arc::new(AtomicUsize::new(0));
        let policy = DenyList::new([ToolName::new("echo").unwrap()]);
        let runtime = AgentRuntime::builder()
            .config(base_config())
            .provider(provider)
            .tool(counting_echo_tool(Arc::clone(&counter)))
            .policy(Arc::new(policy))
            .build()
            .unwrap();

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        // 工具未被实际执行
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(is_error);
                assert!(tool_text(content).contains("被拒绝"));
                assert!(tool_text(content).contains("拒绝列表"));
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    /// 构造带重试策略的基础配置（测试辅助）。
    fn retry_config(attempts: u32, initial_ms: u64, max_ms: u64) -> AgentConfig {
        let policy = RetryPolicy::new(attempts)
            .unwrap()
            .with_backoff(
                Duration::from_millis(initial_ms),
                Duration::from_millis(max_ms),
            )
            .unwrap();
        AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .retry(policy)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn structured_tool_result_is_fed_back_intact() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("screenshot", "c1", "{}"),
            text_response("done"),
        ]));
        let runtime = build_runtime(base_config(), provider, vec![structured_tool()]);

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(!is_error);
                assert_eq!(content.len(), 2);
                assert!(matches!(
                    &content[0],
                    ToolResultBlock::Text { text } if text == "截图如下"
                ));
                assert!(matches!(
                    &content[1],
                    ToolResultBlock::Image { media_type, data }
                        if media_type == "image/png" && data == "aGVsbG8="
                ));
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn retries_retryable_model_errors() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Error(ModelError::Transport {
                message: "net down".to_owned(),
            }),
            ScriptedResponse::Error(ModelError::RateLimited {
                retry_after_ms: None,
            }),
            text_response("pong"),
        ]));
        let runtime = build_runtime(retry_config(3, 1, 4), provider.clone(), vec![]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        let outcome = result.unwrap();
        assert_eq!(provider.call_count(), 3);
        // 重试不增加步数（步数按逻辑轮次计）
        assert_eq!(outcome.steps, 1);

        let retries: Vec<(u32, u64)> = events
            .iter()
            .filter_map(|event| match &event.kind {
                AgentEventKind::ModelRetry {
                    attempt, delay_ms, ..
                } => Some((*attempt, *delay_ms)),
                _ => None,
            })
            .collect();
        assert_eq!(retries, vec![(1, 1), (2, 2)]);
    }

    #[tokio::test]
    async fn non_retryable_error_is_not_retried() {
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Error(
            ModelError::Unsupported {
                feature: "tool_calling",
            },
        )]));
        let runtime = build_runtime(retry_config(3, 1, 4), provider.clone(), vec![]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        assert!(matches!(
            result,
            Err(AgentError::Model(ModelError::Unsupported { .. }))
        ));
        assert_eq!(provider.call_count(), 1);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.kind, AgentEventKind::ModelRetry { .. }))
        );
    }

    #[tokio::test]
    async fn retry_exhausted_returns_last_error() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Error(ModelError::Transport {
                message: "first".to_owned(),
            }),
            ScriptedResponse::Error(ModelError::Transport {
                message: "second".to_owned(),
            }),
        ]));
        let runtime = build_runtime(retry_config(2, 1, 2), provider.clone(), vec![]);

        let (events, result) = collect_events(&runtime, RunInput::from_user_text("ping")).await;
        match result {
            Err(AgentError::Model(ModelError::Transport { message })) => {
                assert_eq!(message, "second");
            }
            other => panic!("期望最后一次 Transport 错误，实际：{other:?}"),
        }
        assert_eq!(provider.call_count(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.kind, AgentEventKind::ModelRetry { .. }))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn cancel_during_retry_backoff() {
        // 退避 5s：取消必须立刻打断（否则用例会超时）
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Error(
            ModelError::Transport {
                message: "net down".to_owned(),
            },
        )]));
        let runtime = build_runtime(retry_config(3, 5_000, 5_000), provider, vec![]);
        let input = RunInput::from_user_text("ping");
        let cancel = input.cancel.clone();
        let (mut events, result) = runtime.run_events(input).split();

        loop {
            let event = events.next().await.expect("应有事件");
            if matches!(event.kind, AgentEventKind::ModelRetry { .. }) {
                break;
            }
        }
        cancel.cancel();

        let outcome = tokio::time::timeout(Duration::from_secs(2), result.await_result())
            .await
            .expect("退避应被取消打断");
        assert!(matches!(outcome, Err(AgentError::Cancelled)));
    }

    #[tokio::test]
    async fn duration_budget_exceeded() {
        let config = AgentConfig::builder()
            .model(ModelId::new("scripted-model").unwrap())
            .max_duration(Duration::from_millis(50))
            .build()
            .unwrap();
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            text_response("done"),
        ]));
        // 工具耗时 200ms > 预算 50ms：第二轮请求前必须终止
        let runtime = build_runtime(config, provider.clone(), vec![delay_tool("echo", 200)]);

        let result = runtime.run(RunInput::from_user_text("ping")).await;
        match result {
            Err(AgentError::BudgetExceeded {
                kind: BudgetKind::DurationMs,
                limit,
                observed,
            }) => {
                assert_eq!(limit, 50);
                assert!(observed >= 50, "observed={observed}");
            }
            other => panic!("期望 DurationMs 预算错误，实际：{other:?}"),
        }
        assert_eq!(provider.call_count(), 1);
    }

    #[tokio::test]
    async fn async_hooks_can_await_io() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("echo", "c1", "{}"),
            text_response("done"),
        ]));
        let hooks = Arc::new(AsyncRecordingHooks::default());
        let runtime = AgentRuntime::builder()
            .config(base_config())
            .provider(provider)
            .tool(echo_tool())
            .hooks(hooks.clone())
            .build()
            .unwrap();

        runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        assert_eq!(
            hooks.calls(),
            vec![
                "run_start",
                "step_start:1",
                "tool_call:echo",
                "step_start:2",
                "run_end:true",
            ]
        );
    }

    #[tokio::test]
    async fn agent_tool_runs_child_and_returns_final_text() {
        let child_provider = Arc::new(ScriptedProvider::new(vec![text_response("子结果")]));
        let child = build_runtime(base_config(), child_provider.clone(), vec![]);
        let parent_provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("ask_child", "c1", "{\"prompt\":\"1+1?\"}"),
            text_response("父完成"),
        ]));
        let runtime = build_runtime(
            base_config(),
            parent_provider,
            vec![Arc::new(AgentTool::new(
                ToolName::new("ask_child").unwrap(),
                "子 Agent",
                Arc::new(child),
            ))],
        );

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(!is_error);
                assert_eq!(tool_text(content), "子结果");
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
        // 子运行使用独立预算：恰好一次模型调用
        assert_eq!(child_provider.call_count(), 1);
    }

    #[tokio::test]
    async fn agent_tool_child_failure_is_fed_back() {
        let child_provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Error(
            ModelError::Transport {
                message: "child down".to_owned(),
            },
        )]));
        let child = build_runtime(base_config(), child_provider, vec![]);
        let parent_provider = Arc::new(ScriptedProvider::new(vec![
            tool_call_response("ask_child", "c1", "{\"prompt\":\"hi\"}"),
            text_response("父完成"),
        ]));
        let runtime = build_runtime(
            base_config(),
            parent_provider,
            vec![Arc::new(AgentTool::new(
                ToolName::new("ask_child").unwrap(),
                "子 Agent",
                Arc::new(child),
            ))],
        );

        let outcome = runtime.run(RunInput::from_user_text("ping")).await.unwrap();
        match &outcome.messages[1].content()[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(is_error);
                assert!(tool_text(content).contains("子 Agent 运行失败"));
                assert!(tool_text(content).contains("child down"));
            }
            other => panic!("期望 ToolResult，实际：{other:?}"),
        }
    }

    #[tokio::test]
    async fn agent_tool_propagates_cancel_to_child() {
        let child_provider = Arc::new(CancelRecorderProvider::default());
        let child = build_runtime(base_config(), child_provider.clone(), vec![]);
        let parent_provider = Arc::new(ScriptedProvider::new(vec![tool_call_response(
            "ask_child",
            "c1",
            "{\"prompt\":\"hi\"}",
        )]));
        let runtime = build_runtime(
            base_config(),
            parent_provider,
            vec![Arc::new(AgentTool::new(
                ToolName::new("ask_child").unwrap(),
                "子 Agent",
                Arc::new(child),
            ))],
        );

        let input = RunInput::from_user_text("ping");
        let cancel = input.cancel.clone();
        let (mut events, result) = runtime.run_events(input).split();

        // 等待子运行的模型请求上路（记录到父运行注入的取消令牌）
        let child_cancel = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(token) = child_provider.last_cancel() {
                    return token;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("子运行应进入在途状态");

        cancel.cancel();
        let outcome = tokio::time::timeout(Duration::from_secs(2), result.await_result())
            .await
            .expect("取消后应迅速结束");
        assert!(matches!(outcome, Err(AgentError::Cancelled)));
        assert!(
            child_cancel.is_cancelled(),
            "取消必须传播到子运行的在途请求令牌"
        );
        // 消费剩余事件（发送端已关闭）
        while events.next().await.is_some() {}
    }
}
