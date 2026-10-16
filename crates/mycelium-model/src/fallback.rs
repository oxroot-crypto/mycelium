//! 多 Provider 降级组合。
//!
//! 语义（FC-POST-171 / FC-POST-172）：
//! - 按列表顺序尝试；某个提供方返回**可重试**错误（[`ModelError::is_retryable`]）时
//!   静默降级到下一个；
//! - 不可重试错误（鉴权失败、上下文超限等）立即返回，不降级；
//! - 全部失败时返回最后一个错误；
//! - 空列表在构建期拒绝。

use std::sync::Arc;

use mycelium_core::error::ModelError;

use crate::provider::ModelProvider;
use crate::request::ModelRequest;
use crate::{ModelCapabilities, ModelStream, async_trait};

/// [`FallbackProvider`] 构建错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FallbackError {
    /// 提供方列表为空。
    #[error("FallbackProvider 至少需要一个提供方")]
    Empty,
}

/// 按序降级的提供方组合（首个为主提供方）。
pub struct FallbackProvider {
    providers: Vec<Arc<dyn ModelProvider>>,
}

impl FallbackProvider {
    /// 创建降级组合。
    ///
    /// # Errors
    /// 列表为空时返回 [`FallbackError::Empty`]。
    pub fn new(providers: Vec<Arc<dyn ModelProvider>>) -> Result<Self, FallbackError> {
        if providers.is_empty() {
            return Err(FallbackError::Empty);
        }
        Ok(Self { providers })
    }
}

impl std::fmt::Debug for FallbackProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self
            .providers
            .iter()
            .map(|provider| provider.id())
            .collect();
        f.debug_struct("FallbackProvider")
            .field("providers", &ids)
            .finish()
    }
}

#[async_trait]
impl ModelProvider for FallbackProvider {
    fn id(&self) -> &str {
        "fallback"
    }

    fn capabilities(&self) -> ModelCapabilities {
        // 以主提供方为基准（降级方的能力差异由业务方自行约定）。
        self.providers
            .first()
            .map_or_else(ModelCapabilities::default, |provider| {
                provider.capabilities()
            })
    }

    async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
        let mut last_error: Option<ModelError> = None;
        for provider in &self.providers {
            match provider.stream(request.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    if !error.is_retryable() {
                        return Err(error);
                    }
                    last_error = Some(error);
                }
            }
        }
        // 构造期已保证非空，循环必然返回 Ok 或写入 last_error；此分支为防御式兜底。
        Err(last_error.unwrap_or(ModelError::Transport {
            message: "FallbackProvider 无可用提供方".to_owned(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use mycelium_core::ids::ModelId;
    use mycelium_core::message::Message;
    use mycelium_core::response::StopReason;

    use super::*;
    use crate::delta::ModelDelta;
    use crate::provider::stream_from_deltas;

    /// 测试响应。
    enum StubResponse {
        /// 固定文本流。
        Text(&'static str),
        /// 固定错误。
        Error(ModelError),
    }

    /// 固定响应的测试提供方。
    struct StubProvider {
        id: &'static str,
        response: StubResponse,
    }

    #[async_trait]
    impl ModelProvider for StubProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities::default()
        }

        async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
            match &self.response {
                StubResponse::Text(text) => Ok(stream_from_deltas(vec![
                    ModelDelta::TextDelta {
                        text: (*text).to_owned(),
                    },
                    ModelDelta::Finished {
                        stop_reason: StopReason::EndTurn,
                    },
                ])),
                StubResponse::Error(error) => Err(error.clone()),
            }
        }
    }

    fn provider(id: &'static str, response: StubResponse) -> Arc<dyn ModelProvider> {
        Arc::new(StubProvider { id, response })
    }

    fn request() -> ModelRequest {
        ModelRequest::new(
            ModelId::new("stub").unwrap(),
            vec![Message::user_text("hi")],
        )
    }

    #[tokio::test]
    async fn falls_back_on_retryable_error() {
        let fallback = FallbackProvider::new(vec![
            provider(
                "primary",
                StubResponse::Error(ModelError::Transport {
                    message: "net down".to_owned(),
                }),
            ),
            provider("secondary", StubResponse::Text("second")),
        ])
        .unwrap();

        let mut stream = fallback.stream(request()).await.unwrap();
        let first = stream.next().await.unwrap().unwrap();
        assert!(
            matches!(first, ModelDelta::TextDelta { text } if text == "second"),
            "应降级到第二个提供方"
        );
    }

    #[tokio::test]
    async fn does_not_fall_back_on_fatal_error() {
        let fallback = FallbackProvider::new(vec![
            provider(
                "primary",
                StubResponse::Error(ModelError::Unsupported {
                    feature: "tool_calling",
                }),
            ),
            provider("secondary", StubResponse::Text("second")),
        ])
        .unwrap();

        let result = fallback.stream(request()).await;
        assert!(
            matches!(result, Err(ModelError::Unsupported { .. })),
            "不可重试错误必须立即返回"
        );
    }

    #[tokio::test]
    async fn returns_last_error_when_all_fail() {
        let fallback = FallbackProvider::new(vec![
            provider(
                "primary",
                StubResponse::Error(ModelError::Transport {
                    message: "a".to_owned(),
                }),
            ),
            provider(
                "secondary",
                StubResponse::Error(ModelError::Timeout { timeout_ms: 7 }),
            ),
        ])
        .unwrap();

        let result = fallback.stream(request()).await;
        assert!(
            matches!(result, Err(ModelError::Timeout { timeout_ms: 7 })),
            "应返回最后一个错误"
        );
    }

    #[test]
    fn rejects_empty_provider_list() {
        let result = FallbackProvider::new(Vec::new());
        assert!(matches!(result, Err(FallbackError::Empty)));
    }
}
