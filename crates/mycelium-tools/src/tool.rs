//! [`Tool`] trait、执行上下文与输出。

use async_trait::async_trait;

use mycelium_core::CancellationToken;
use mycelium_core::error::ToolError;
use mycelium_core::ids::{RunId, SessionId, ToolCallId};
use mycelium_core::tool::ToolSpec;

/// 工具执行上下文（由运行时注入）。
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// 所属运行。
    pub run_id: RunId,
    /// 所属会话（骨架阶段可能为 `None`；会话接入运行时后填充）。
    pub session_id: Option<SessionId>,
    /// 本次调用的标识。
    pub call_id: ToolCallId,
    /// 取消令牌：工具必须尽快响应取消并返回 [`ToolError::Cancelled`]。
    pub cancel: CancellationToken,
}

/// 工具执行结果。
///
/// 语义：错误结果（[`ToolOutput::error`]）不终止运行，而是以
/// [`mycelium_core::ContentBlock::ToolResult`] 形式回灌给模型继续推理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    content: String,
    is_error: bool,
}

impl ToolOutput {
    /// 构造成功结果。
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    /// 构造错误结果（回灌给模型，参与后续推理）。
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }

    /// 结果文本。
    pub fn content(&self) -> &str {
        &self.content
    }

    /// 是否为错误结果。
    pub fn is_error(&self) -> bool {
        self.is_error
    }
}

/// 工具（SPI 扩展点）。
///
/// 实现约定：
/// - `spec().name()` 必须全局唯一（注册表以此为准，重复注册会被拒绝）；
/// - 必须尊重 [`ToolContext::cancel`]，尽快返回 [`ToolError::Cancelled`]；
/// - 不得 panic；失败必须返回 [`ToolError`]；
/// - 入参不满足 `input_schema` 时应返回 [`ToolError::InvalidArguments`]。
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    /// 工具规格（对模型可见）。
    fn spec(&self) -> ToolSpec;

    /// 执行工具。
    ///
    /// # Arguments
    /// * `context` - 执行上下文（运行 / 会话 / 调用标识 / 取消令牌）。
    /// * `arguments` - 模型给出的原始入参（JSON）。
    ///
    /// # Errors
    /// 入参非法、执行失败、超时、被取消等场景返回 [`ToolError`]。
    async fn call(
        &self,
        context: &ToolContext,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError>;
}
