//! [`ModelProvider`] trait、能力声明与流类型。

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use serde::{Deserialize, Serialize};

use mycelium_core::error::ModelError;

use crate::delta::ModelDelta;
use crate::request::ModelRequest;

/// 模型流：按序产出 [`ModelDelta`]。
///
/// 语义：丢弃流即视为取消（提供方应尽快释放资源）；成功流的末尾契约见
/// [`ModelDelta`] 文档。
pub type ModelStream = Pin<Box<dyn Stream<Item = Result<ModelDelta, ModelError>> + Send + 'static>>;

/// 模型能力声明（供运行时做前置校验与降级决策）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// 是否支持流式输出。
    pub supports_streaming: bool,
    /// 是否支持工具调用。
    pub supports_tool_calling: bool,
    /// 是否支持系统提示。
    pub supports_system_prompt: bool,
    /// 是否支持图片输入。
    pub supports_vision: bool,
    /// 上下文窗口上限（Token）；未知时为 `None`。
    pub max_context_tokens: Option<u64>,
}

/// 模型提供方（SPI 扩展点）。
///
/// 实现约定：
/// - 必须支持取消：`request.cancel` 触发后尽快让流以 [`ModelError::Cancelled`] 结束；
/// - 必须遵守流协议（见 [`ModelDelta`] 文档与 `docs/architecture/lifecycle.md` §6）；
/// - 不得 panic；所有失败必须以 [`ModelError`] 返回。
#[async_trait]
pub trait ModelProvider: Send + Sync + 'static {
    /// 提供方标识（用于日志与错误上下文；建议稳定、小写，如 `"anthropic"`）。
    fn id(&self) -> &str;

    /// 模型能力声明。
    fn capabilities(&self) -> ModelCapabilities;

    /// 发起一次流式模型调用。
    ///
    /// # Errors
    /// 网络 / 鉴权 / 限流 / 超时 / 取消等失败返回 [`ModelError`]。
    async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError>;
}

/// 由固定增量序列构造 [`ModelStream`]（测试与简单提供方使用）。
///
/// # Examples
/// ```
/// use mycelium_core::StopReason;
/// use mycelium_model::{ModelDelta, stream_from_deltas};
///
/// let stream = stream_from_deltas(vec![
///     ModelDelta::TextDelta { text: "hello".to_owned() },
///     ModelDelta::Finished { stop_reason: StopReason::EndTurn },
/// ]);
/// // 交由运行时消费；此处仅演示构造。
/// let _ = stream;
/// ```
pub fn stream_from_deltas(deltas: Vec<ModelDelta>) -> ModelStream {
    let items: Vec<Result<ModelDelta, ModelError>> = deltas.into_iter().map(Ok).collect();
    Box::pin(futures_util::stream::iter(items))
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use mycelium_core::StopReason;

    use super::*;

    #[tokio::test]
    async fn stream_from_deltas_preserves_order_and_count() {
        let deltas = vec![
            ModelDelta::TextDelta {
                text: "a".to_owned(),
            },
            ModelDelta::TextDelta {
                text: "b".to_owned(),
            },
            ModelDelta::Finished {
                stop_reason: StopReason::EndTurn,
            },
        ];

        let collected: Vec<Result<ModelDelta, ModelError>> =
            stream_from_deltas(deltas.clone()).collect().await;
        let items: Vec<ModelDelta> = collected
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(items, deltas);
    }
}
