//! 执行循环引擎。
//!
//! 实现 `docs/architecture/lifecycle.md` 定义的状态机与 v0.2 运行语义：顺序执行工具、
//! 预算 / 取消 / 超时在循环边界强制、事件从唯一发射点产出（`seq` 连续、终态唯一）。
//! 契约编号见 `docs/spec/core.md` §2.14。
//!
//! 事件是可选观测面：`run` 路径不创建事件通道；`run_events` 路径通过有界通道
//! （[`EVENT_CHANNEL_CAPACITY`]）产生背压，接收端关闭后运行不中断。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use mycelium_core::event::{AgentEvent, AgentEventKind};
use mycelium_core::message::{ContentBlock, Message};
use mycelium_core::response::{ModelResponse, Usage};
use mycelium_core::run::{RunEvent, RunState};
use mycelium_core::tool::ToolCall;
use mycelium_core::{
    AgentConfig, AgentError, BudgetKind, CancellationToken, ModelError, RunId, ToolError,
};
use mycelium_model::{DeltaAssembler, ModelDelta, ModelProvider, ModelRequest};
use mycelium_tools::schema::validate_arguments;
use mycelium_tools::{ToolContext, ToolOutput, ToolRegistry};
use tokio::sync::mpsc::Sender;

use crate::hooks::{RunContext, RunHooks};
use crate::run::{RunInput, RunOutcome};

/// 事件通道容量（背压边界）。
pub(crate) const EVENT_CHANNEL_CAPACITY: usize = 256;

/// 循环内部结果（成功路径）。
struct RunOutput {
    messages: Vec<Message>,
    usage: Usage,
    steps: u32,
}

/// 执行引擎：一次运行的全部可变状态。
pub(crate) struct Engine {
    config: AgentConfig,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
    hooks: Option<Arc<dyn RunHooks>>,
    run_id: RunId,
    cancel: CancellationToken,
    seq: u64,
    events: Option<Sender<AgentEvent>>,
    events_closed: bool,
}

impl Engine {
    /// 创建引擎（`run_id` 在此生成）。
    pub(crate) fn new(
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
            run_id: RunId::generate(),
            cancel: CancellationToken::new(),
            seq: 0,
            events: None,
            events_closed: false,
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
        self.events = events;
        self.cancel = input.cancel.clone();
        tracing::debug!(run_id = %self.run_id, provider = self.provider.id(), "运行开始");
        self.emit(AgentEventKind::RunStarted {
            message_count: input.messages.len(),
        })
        .await;

        let context = self.context();
        if let Some(hooks) = &self.hooks {
            hooks.on_run_start(&context);
        }

        match self.drive(input).await {
            Ok(output) => {
                self.emit(AgentEventKind::RunCompleted {
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
                    hooks.on_run_end(&context, Ok(&outcome));
                }
                tracing::debug!(run_id = %self.run_id, steps = outcome.steps, "运行完成");
                Ok(outcome)
            }
            Err(error) => {
                match &error {
                    AgentError::Cancelled => self.emit(AgentEventKind::RunCancelled).await,
                    other => {
                        self.emit(AgentEventKind::RunFailed {
                            message: other.to_string(),
                        })
                        .await;
                    }
                }
                if let Some(hooks) = &self.hooks {
                    hooks.on_run_end(&context, Err(&error));
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

    /// 发射一个事件。
    ///
    /// 接收端已关闭时显式记录并停止发射（事件是可选观测面，运行继续）。
    async fn emit(&mut self, kind: AgentEventKind) {
        if self.events_closed {
            return;
        }
        let event = AgentEvent {
            run_id: self.run_id,
            seq: self.seq,
            at_ms: now_ms(),
            kind,
        };
        self.seq = self.seq.saturating_add(1);
        if let Some(sender) = &self.events
            && sender.send(event).await.is_err()
        {
            self.events_closed = true;
        }
    }

    /// 主循环：模型请求 → 工具执行 → 回灌，直至无工具调用或到达预算 / 取消边界。
    async fn drive(&mut self, input: RunInput) -> Result<RunOutput, AgentError> {
        let mut state = transition(RunState::Created, RunEvent::Start)?;
        let mut messages = input.messages;
        let history_start = messages.len();
        let mut usage = Usage::default();
        let mut steps: u32 = 0;

        loop {
            if self.cancel.is_cancelled() {
                transition(state, RunEvent::Cancel)?;
                return Err(AgentError::Cancelled);
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
            self.emit(AgentEventKind::StepStarted { step: steps }).await;
            if let Some(hooks) = &self.hooks {
                hooks.on_step_start(&self.context(), steps);
            }
            self.emit(AgentEventKind::ModelRequestStarted {
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

            self.emit(AgentEventKind::ModelRequestFinished {
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
                self.emit(AgentEventKind::ModelToolCall { call: call.clone() })
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
            for call in tool_calls {
                let output = match self.execute_tool(&call).await {
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
                messages.push(Message::tool_result(
                    call.id.clone(),
                    output.content(),
                    output.is_error(),
                ));
            }
            state = transition(state, RunEvent::ToolsCompleted)?;
        }
    }

    /// 调用模型：构造请求、施加超时、消费流并装配响应。
    async fn call_model(&mut self, messages: &[Message]) -> Result<ModelResponse, AgentError> {
        let request = ModelRequest {
            model: self.config.model().clone(),
            system_prompt: self.config.system_prompt().map(str::to_owned),
            messages: messages.to_vec(),
            tools: self.tools.specs(),
            tool_choice: self.config.tool_choice().clone(),
            max_output_tokens: self.config.max_output_tokens().map(|value| value.get()),
            temperature: self.config.temperature(),
            cancel: self.cancel.clone(),
        };

        let timeout = self.config.request_timeout();
        let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
        match tokio::time::timeout(timeout, self.consume_model_stream(request)).await {
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
                            if let ModelDelta::TextDelta { text } = &delta {
                                self.emit(AgentEventKind::ModelTextDelta { text: text.clone() }).await;
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

    /// 执行一次工具调用（含事件配对与钩子；错误结果此处回灌来源）。
    async fn execute_tool(&mut self, call: &ToolCall) -> Result<ToolOutput, AgentError> {
        self.emit(AgentEventKind::ToolCallStarted {
            call_id: call.id.clone(),
            name: call.name.clone(),
        })
        .await;

        let outcome = self.invoke_tool(call).await;

        // 无论成功、错误还是取消，都必须补齐 Finished 以维持配对不变量。
        let is_error = match &outcome {
            Ok(output) => output.is_error(),
            Err(_) => true,
        };
        self.emit(AgentEventKind::ToolCallFinished {
            call_id: call.id.clone(),
            name: call.name.clone(),
            is_error,
        })
        .await;
        if let Some(hooks) = &self.hooks {
            hooks.on_tool_call(&self.context(), call);
        }
        outcome
    }

    /// 实际调用工具：注册表查找、入参校验、取消竞争与错误归一化。
    async fn invoke_tool(&mut self, call: &ToolCall) -> Result<ToolOutput, AgentError> {
        if self.cancel.is_cancelled() {
            return Err(AgentError::Cancelled);
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
        let cancel = self.cancel.clone();
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => Err(ToolError::Cancelled { tool: call.name.clone() }),
            result = tool.call(&context, call.arguments.clone()) => result,
        };

        match result {
            Ok(output) => Ok(output),
            Err(ToolError::Cancelled { .. }) => Err(AgentError::Cancelled),
            Err(error) => {
                tracing::debug!(tool = %call.name, error = %error, "工具执行失败（作为错误结果回灌）");
                Ok(ToolOutput::error(error.to_string()))
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::StreamExt;
    use mycelium_core::event::{AgentEvent, AgentEventKind};
    use mycelium_core::message::ContentBlock;
    use mycelium_core::{ModelError, ModelId, Role, ToolChoice};
    use mycelium_model::ModelProvider;
    use mycelium_tools::Tool;

    use super::*;
    use crate::AgentRuntime;
    use crate::run::RunInput;
    use crate::test_support::{
        RecordingHooks, ScriptedProvider, ScriptedResponse, cancel_wait_tool, cancelled_tool,
        echo_tool, failing_tool, pending_provider, strict_tool, text_response,
        text_response_with_usage, tool_call_response, tool_call_response_with_usage,
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
                assert_eq!(content, "{\"text\":\"hi\"}");
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
                assert!(content.contains("工具未注册"));
                assert!(content.contains("missing_tool"));
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
                assert!(content.contains("工具入参非法"));
                assert!(content.contains("$.text"));
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
                assert!(content.contains("boom"));
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
}
