//! 测试辅助（仅在测试构建中编译）。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mycelium_core::message::ToolResultBlock;
use mycelium_core::{AgentError, StopReason, ToolError, ToolName, ToolSpec};
use mycelium_model::{
    ModelCapabilities, ModelDelta, ModelError, ModelProvider, ModelRequest, ModelStream,
    async_trait, stream_from_deltas,
};
use mycelium_tools::{Tool, ToolContext, ToolOutput};

use crate::hooks::{RunContext, RunHooks};
use crate::run::RunOutcome;

/// 脚本化响应。
pub(crate) enum ScriptedResponse {
    /// 按序产出的增量序列。
    Deltas(Vec<ModelDelta>),
    /// 直接返回的模型错误。
    Error(ModelError),
}

/// 按脚本依次响应的提供方；记录调用次数与最近一次请求。
pub(crate) struct ScriptedProvider {
    responses: Mutex<VecDeque<ScriptedResponse>>,
    calls: AtomicUsize,
    last_request: Mutex<Option<ModelRequest>>,
}

impl ScriptedProvider {
    /// 以响应脚本创建。
    pub(crate) fn new(responses: Vec<ScriptedResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            calls: AtomicUsize::new(0),
            last_request: Mutex::new(None),
        }
    }

    /// 已发起的模型调用次数。
    pub(crate) fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// 最近一次收到的请求（克隆）。
    pub(crate) fn last_request(&self) -> Option<ModelRequest> {
        self.last_request.lock().expect("锁未被毒化").clone()
    }
}

#[async_trait]
impl ModelProvider for ScriptedProvider {
    fn id(&self) -> &str {
        "scripted"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_streaming: true,
            supports_tool_calling: true,
            supports_system_prompt: true,
            ..Default::default()
        }
    }

    async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_request.lock().expect("锁未被毒化") = Some(request);
        match self.responses.lock().expect("锁未被毒化").pop_front() {
            Some(ScriptedResponse::Deltas(deltas)) => Ok(stream_from_deltas(deltas)),
            Some(ScriptedResponse::Error(error)) => Err(error),
            None => Err(ModelError::InvalidResponse {
                detail: "ScriptedProvider 脚本已耗尽".to_owned(),
            }),
        }
    }
}

/// 无限挂起的提供方（取消 / 超时测试）。
pub(crate) struct PendingProvider;

#[async_trait]
impl ModelProvider for PendingProvider {
    fn id(&self) -> &str {
        "pending"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_streaming: true,
            ..Default::default()
        }
    }

    async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(futures_util::stream::pending::<
            Result<ModelDelta, ModelError>,
        >()))
    }
}

/// 返回共享的挂起提供方。
pub(crate) fn pending_provider() -> Arc<dyn ModelProvider> {
    Arc::new(PendingProvider)
}

/// 纯文本响应。
pub(crate) fn text_response(text: &str) -> ScriptedResponse {
    ScriptedResponse::Deltas(vec![
        ModelDelta::TextDelta {
            text: text.to_owned(),
        },
        ModelDelta::Finished {
            stop_reason: StopReason::EndTurn,
        },
    ])
}

/// 带用量的纯文本响应。
pub(crate) fn text_response_with_usage(
    text: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> ScriptedResponse {
    ScriptedResponse::Deltas(vec![
        ModelDelta::TextDelta {
            text: text.to_owned(),
        },
        ModelDelta::Usage {
            input_tokens,
            output_tokens,
            reasoning_tokens: None,
        },
        ModelDelta::Finished {
            stop_reason: StopReason::EndTurn,
        },
    ])
}

/// 单个工具调用响应（无用量）。
pub(crate) fn tool_call_response(tool: &str, call_id: &str, arguments: &str) -> ScriptedResponse {
    ScriptedResponse::Deltas(vec![
        ModelDelta::ToolCallDelta {
            index: 0,
            id: Some(mycelium_core::ToolCallId::new(call_id).expect("合法 ID")),
            name: Some(ToolName::new(tool).expect("合法工具名")),
            arguments_json_delta: Some(arguments.to_owned()),
        },
        ModelDelta::Finished {
            stop_reason: StopReason::ToolUse,
        },
    ])
}

/// 单个工具调用响应（带用量）。
pub(crate) fn tool_call_response_with_usage(
    tool: &str,
    call_id: &str,
    arguments: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> ScriptedResponse {
    ScriptedResponse::Deltas(vec![
        ModelDelta::ToolCallDelta {
            index: 0,
            id: Some(mycelium_core::ToolCallId::new(call_id).expect("合法 ID")),
            name: Some(ToolName::new(tool).expect("合法工具名")),
            arguments_json_delta: Some(arguments.to_owned()),
        },
        ModelDelta::Usage {
            input_tokens,
            output_tokens,
            reasoning_tokens: None,
        },
        ModelDelta::Finished {
            stop_reason: StopReason::ToolUse,
        },
    ])
}

/// 回显入参字符串的工具。
pub(crate) struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("echo").expect("合法工具名"),
            "回显入参",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(arguments.to_string()))
    }
}

/// 返回共享的回显工具。
pub(crate) fn echo_tool() -> Arc<dyn Tool> {
    Arc::new(EchoTool)
}

/// 总是失败的工具。
pub(crate) struct FailingTool;

#[async_trait]
impl Tool for FailingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("failing").expect("合法工具名"),
            "总是失败",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Execution {
            tool: ToolName::new("failing").expect("合法工具名"),
            message: "boom".to_owned(),
        })
    }
}

/// 返回共享的失败工具。
pub(crate) fn failing_tool() -> Arc<dyn Tool> {
    Arc::new(FailingTool)
}

/// 要求必填字符串字段 `text` 的工具（入参校验测试）。
pub(crate) struct StrictTool;

#[async_trait]
impl Tool for StrictTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("strict").expect("合法工具名"),
            "严格入参",
            serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
                "additionalProperties": false
            }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success("strict-ok"))
    }
}

/// 返回共享的严格工具。
pub(crate) fn strict_tool() -> Arc<dyn Tool> {
    Arc::new(StrictTool)
}

/// 等待取消令牌触发后返回成功结果的工具。
pub(crate) struct CancelWaitTool;

#[async_trait]
impl Tool for CancelWaitTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("cancel_wait").expect("合法工具名"),
            "等待取消",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        context: &ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        context.cancel.cancelled().await;
        Ok(ToolOutput::success("cancelled"))
    }
}

/// 返回共享的等待取消工具。
pub(crate) fn cancel_wait_tool() -> Arc<dyn Tool> {
    Arc::new(CancelWaitTool)
}

/// 直接返回 `ToolError::Cancelled` 的工具。
pub(crate) struct CancelledTool;

#[async_trait]
impl Tool for CancelledTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("cancelled_tool").expect("合法工具名"),
            "返回取消错误",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Cancelled {
            tool: ToolName::new("cancelled_tool").expect("合法工具名"),
        })
    }
}

/// 返回共享的取消错误工具。
pub(crate) fn cancelled_tool() -> Arc<dyn Tool> {
    Arc::new(CancelledTool)
}

/// 记录调用顺序的钩子。
#[derive(Default)]
pub(crate) struct RecordingHooks {
    calls: Mutex<Vec<String>>,
}

impl RecordingHooks {
    /// 返回已记录的回调序列。
    pub(crate) fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("锁未被毒化").clone()
    }

    /// 追加一条记录。
    fn record(&self, entry: impl Into<String>) {
        self.calls.lock().expect("锁未被毒化").push(entry.into());
    }
}

#[async_trait]
impl RunHooks for RecordingHooks {
    async fn on_run_start(&self, _context: &RunContext) {
        self.record("run_start");
    }

    async fn on_step_start(&self, _context: &RunContext, step: u32) {
        self.record(format!("step_start:{step}"));
    }

    async fn on_tool_call(&self, _context: &RunContext, call: &mycelium_core::ToolCall) {
        self.record(format!("tool_call:{}", call.name));
    }

    async fn on_run_end(&self, _context: &RunContext, _result: Result<&RunOutcome, &AgentError>) {
        self.record(format!("run_end:{}", _result.is_ok()));
    }
}

/// 异步钩子：回调内执行异步 I/O（sleep）后记录顺序（FC-POST-170）。
#[derive(Default)]
pub(crate) struct AsyncRecordingHooks {
    calls: Mutex<Vec<String>>,
}

impl AsyncRecordingHooks {
    /// 返回已记录的回调序列。
    pub(crate) fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("锁未被毒化").clone()
    }

    /// 追加一条记录。
    fn record(&self, entry: impl Into<String>) {
        self.calls.lock().expect("锁未被毒化").push(entry.into());
    }
}

#[async_trait]
impl RunHooks for AsyncRecordingHooks {
    async fn on_run_start(&self, _context: &RunContext) {
        // 证明回调可以执行真实异步 I/O 而不阻塞运行语义。
        tokio::time::sleep(Duration::from_millis(1)).await;
        self.record("run_start");
    }

    async fn on_step_start(&self, _context: &RunContext, step: u32) {
        tokio::time::sleep(Duration::from_millis(1)).await;
        self.record(format!("step_start:{step}"));
    }

    async fn on_tool_call(&self, _context: &RunContext, call: &mycelium_core::ToolCall) {
        self.record(format!("tool_call:{}", call.name));
    }

    async fn on_run_end(&self, _context: &RunContext, _result: Result<&RunOutcome, &AgentError>) {
        tokio::time::sleep(Duration::from_millis(1)).await;
        self.record(format!("run_end:{}", _result.is_ok()));
    }
}

/// 返回文本 + 图片块的工具（结构化输出测试，RFC-0002）。
pub(crate) struct StructuredTool;

#[async_trait]
impl Tool for StructuredTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("screenshot").expect("合法工具名"),
            "返回文本与图片块",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::blocks(vec![
            ToolResultBlock::Text {
                text: "截图如下".to_owned(),
            },
            ToolResultBlock::Image {
                media_type: "image/png".to_owned(),
                data: "aGVsbG8=".to_owned(),
            },
        ]))
    }
}

/// 返回共享的结构化输出工具。
pub(crate) fn structured_tool() -> Arc<dyn Tool> {
    Arc::new(StructuredTool)
}

/// 固定产出 "pong" 文本流的桩提供方（API 冒烟测试用）。
pub(crate) struct StubProvider;

#[async_trait]
impl ModelProvider for StubProvider {
    fn id(&self) -> &str {
        "stub"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_streaming: true,
            ..Default::default()
        }
    }

    async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
        Ok(stream_from_deltas(vec![
            ModelDelta::TextDelta {
                text: "pong".to_owned(),
            },
            ModelDelta::Finished {
                stop_reason: StopReason::EndTurn,
            },
        ]))
    }
}

/// 返回共享的桩提供方。
pub(crate) fn stub_provider() -> Arc<dyn ModelProvider> {
    Arc::new(StubProvider)
}

/// 固定返回文本的桩工具。
pub(crate) struct StubTool {
    spec: ToolSpec,
}

#[async_trait]
impl Tool for StubTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(
        &self,
        _context: &ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success("stub"))
    }
}

/// 构造指定名称的桩工具。
pub(crate) fn stub_tool(name: &str) -> Arc<dyn Tool> {
    Arc::new(StubTool {
        spec: ToolSpec::new(
            ToolName::new(name).expect("合法工具名"),
            "stub tool",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema"),
    })
}

/// 多个工具调用响应（index 按数组顺序从 0 递增）。
pub(crate) fn tool_calls_response(calls: &[(&str, &str, &str)]) -> ScriptedResponse {
    let mut deltas = Vec::new();
    for (index, (tool, call_id, arguments)) in calls.iter().enumerate() {
        deltas.push(ModelDelta::ToolCallDelta {
            index: index as u32,
            id: Some(mycelium_core::ToolCallId::new(*call_id).expect("合法 ID")),
            name: Some(ToolName::new(*tool).expect("合法工具名")),
            arguments_json_delta: Some((*arguments).to_owned()),
        });
    }
    deltas.push(ModelDelta::Finished {
        stop_reason: StopReason::ToolUse,
    });
    ScriptedResponse::Deltas(deltas)
}

/// 固定延迟后返回的确定性工具（并发 / 超时测试）。
pub(crate) struct DelayTool {
    name: ToolName,
    delay: Duration,
}

#[async_trait]
impl Tool for DelayTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            self.name.clone(),
            "延迟工具",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        tokio::time::sleep(self.delay).await;
        Ok(ToolOutput::success(format!("{} 完成", self.name)))
    }
}

/// 构造带延迟的确定性工具。
pub(crate) fn delay_tool(name: &str, delay_ms: u64) -> Arc<dyn Tool> {
    Arc::new(DelayTool {
        name: ToolName::new(name).expect("合法工具名"),
        delay: Duration::from_millis(delay_ms),
    })
}

/// 一贯很慢的工具（工具超时测试）。
pub(crate) fn slow_tool() -> Arc<dyn Tool> {
    delay_tool("slow", 10_000)
}

/// 统计执行次数的回显工具（权限策略测试）。
pub(crate) struct CountingEchoTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for CountingEchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("echo").expect("合法工具名"),
            "计数回显",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput::success(arguments.to_string()))
    }
}

/// 构造带执行计数的回显工具。
pub(crate) fn counting_echo_tool(calls: Arc<AtomicUsize>) -> Arc<dyn Tool> {
    Arc::new(CountingEchoTool { calls })
}
