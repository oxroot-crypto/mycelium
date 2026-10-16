//! [`Tool`] trait、执行上下文与输出。

use async_trait::async_trait;

use mycelium_core::CancellationToken;
use mycelium_core::error::ToolError;
use mycelium_core::ids::{RunId, SessionId, ToolCallId};
use mycelium_core::message::ToolResultBlock;
use mycelium_core::tool::ToolSpec;

/// 工具执行上下文（由运行时注入）。
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// 所属运行。
    pub run_id: RunId,
    /// 所属会话（当前运行时尚未绑定会话，为 `None`；v0.3 引入会话接入后填充）。
    pub session_id: Option<SessionId>,
    /// 本次调用的标识。
    pub call_id: ToolCallId,
    /// 取消令牌：工具必须尽快响应取消并返回 [`ToolError::Cancelled`]。
    pub cancel: CancellationToken,
}

/// 工具执行结果。
///
/// 语义：错误结果（[`ToolOutput::error`] / [`ToolOutput::error_blocks`]）不终止运行，
/// 而是以 [`mycelium_core::ContentBlock::ToolResult`] 形式回灌给模型继续推理。
///
/// 结果为多内容块（RFC-0002）：`success` / `error` 构造单文本块；需要返回图片等
/// 结构化内容时使用 `blocks` / `error_blocks`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    blocks: Vec<ToolResultBlock>,
    is_error: bool,
}

impl ToolOutput {
    /// 构造成功结果（单文本块）。
    pub fn success(content: impl Into<String>) -> Self {
        Self::blocks(vec![ToolResultBlock::Text {
            text: content.into(),
        }])
    }

    /// 构造错误结果（单文本块；回灌给模型，参与后续推理）。
    pub fn error(content: impl Into<String>) -> Self {
        Self::error_blocks(vec![ToolResultBlock::Text {
            text: content.into(),
        }])
    }

    /// 由多内容块构造成功结果。
    pub fn blocks(blocks: Vec<ToolResultBlock>) -> Self {
        Self {
            blocks,
            is_error: false,
        }
    }

    /// 由多内容块构造错误结果。
    pub fn error_blocks(blocks: Vec<ToolResultBlock>) -> Self {
        Self {
            blocks,
            is_error: true,
        }
    }

    /// 汇总文本块（换行连接；图片块不参与）。
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                ToolResultBlock::Text { text } => Some(text.as_str()),
                ToolResultBlock::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 结果内容块（原样，供报文进入消息历史）。
    pub fn content_blocks(&self) -> &[ToolResultBlock] {
        &self.blocks
    }

    /// 取出内容块（移动语义，避免克隆）。
    pub fn into_blocks(self) -> Vec<ToolResultBlock> {
        self.blocks
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_text_and_blocks_semantics() {
        // success/error 为单文本块
        let success = ToolOutput::success("ok");
        assert_eq!(success.content_blocks().len(), 1);
        assert_eq!(success.text(), "ok");
        assert!(!success.is_error());

        let error = ToolOutput::error("boom");
        assert!(error.is_error());
        assert_eq!(error.text(), "boom");

        // 多块：文本汇总用换行连接，图片不参与
        let structured = ToolOutput::blocks(vec![
            ToolResultBlock::Text {
                text: "第一行".to_owned(),
            },
            ToolResultBlock::Image {
                media_type: "image/png".to_owned(),
                data: "aGVsbG8=".to_owned(),
            },
            ToolResultBlock::Text {
                text: "第二行".to_owned(),
            },
        ]);
        assert_eq!(structured.text(), "第一行\n第二行");
        assert_eq!(structured.content_blocks().len(), 3);
        assert!(!structured.is_error());

        // into_blocks 无损搬运
        let blocks = structured.into_blocks();
        assert_eq!(blocks.len(), 3);
        assert!(blocks.iter().any(|block| matches!(
            block,
            ToolResultBlock::Image { media_type, .. } if media_type == "image/png"
        )));

        let error_blocks = ToolOutput::error_blocks(vec![ToolResultBlock::Text {
            text: "失败".to_owned(),
        }]);
        assert!(error_blocks.is_error());
    }
}
