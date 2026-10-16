//! 运行钩子（异步观察者扩展点，RFC-0001）。

use async_trait::async_trait;
use mycelium_core::{AgentError, CancellationToken, RunId, ToolCall};

use crate::run::RunOutcome;

/// 运行上下文（钩子可见的只读信息）。
#[derive(Debug, Clone)]
pub struct RunContext {
    /// 当前运行标识。
    pub run_id: RunId,
    /// 取消令牌。
    pub cancel: CancellationToken,
}

/// 运行钩子。
///
/// 语义（v0.4 起异步化，见 `docs/architecture/rfc/0001-async-hooks.md`）：
/// - 全部方法都有默认空实现，按需覆写；
/// - 回调在运行任务内 `await`，允许非阻塞异步 I/O（审计上报、指标推送等）；
/// - 回调查**不得永不完成**：应尊重 [`RunContext::cancel`] 或自行设置超时；
/// - 钩子不得影响运行正确性：内部错误应自行记录，运行不会因钩子失败而失败；
/// - 「拦截与改写」不在本版本范围内（后续 RFC）。
#[async_trait]
pub trait RunHooks: Send + Sync + 'static {
    /// 运行开始。
    async fn on_run_start(&self, _context: &RunContext) {}

    /// 单步开始（`step` 从 1 起连续递增）。
    async fn on_step_start(&self, _context: &RunContext, _step: u32) {}

    /// 工具调用完成（成功与错误结果均触发；并发模式下按完成顺序）。
    async fn on_tool_call(&self, _context: &RunContext, _call: &ToolCall) {}

    /// 运行结束（成功携带结果；失败携带错误引用）。
    async fn on_run_end(&self, _context: &RunContext, _result: Result<&RunOutcome, &AgentError>) {}
}
