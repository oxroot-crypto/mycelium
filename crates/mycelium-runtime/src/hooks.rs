//! 运行钩子（同步观察者扩展点）。

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
/// 语义：
/// - 全部方法都有默认空实现，按需覆写；
/// - 在运行线程上**同步**调用，实现不得阻塞等待（耗时操作请自行投递到任务队列）；
/// - 钩子不得 panic；内部异常应自行记录，不得影响运行正确性。
pub trait RunHooks: Send + Sync + 'static {
    /// 运行开始。
    fn on_run_start(&self, _context: &RunContext) {}

    /// 单步开始（`step` 从 1 起连续递增）。
    fn on_step_start(&self, _context: &RunContext, _step: u32) {}

    /// 工具调用完成（成功与错误结果均触发）。
    fn on_tool_call(&self, _context: &RunContext, _call: &ToolCall) {}

    /// 运行结束（成功携带结果；失败携带错误引用）。
    fn on_run_end(&self, _context: &RunContext, _result: Result<&RunOutcome, &AgentError>) {}
}
