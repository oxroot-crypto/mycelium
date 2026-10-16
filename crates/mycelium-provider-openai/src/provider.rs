//! Provider 本体与构建器。

use mycelium_core::error::ModelError;
use mycelium_model::{ModelCapabilities, ModelProvider, ModelRequest, ModelStream, async_trait};

use crate::request::build_chat_request;
use crate::stream::parse_sse_stream;

/// 默认网关地址（OpenAI 官方）。
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// API key 环境变量名。
pub const API_KEY_ENV: &str = "OPENAI_API_KEY";

/// 网关地址环境变量名（兼容 Ollama / vLLM 等网关）。
pub const BASE_URL_ENV: &str = "OPENAI_BASE_URL";

/// 构建 / 配置错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenAiConfigError {
    /// 缺少 API key。
    #[error(
        "缺少 API key：请设置环境变量 {API_KEY_ENV} 或通过 OpenAiProviderBuilder::api_key 提供"
    )]
    MissingApiKey,
    /// 网关地址为空。
    #[error("base_url 不能为空")]
    EmptyBaseUrl,
    /// HTTP 客户端构建失败。
    #[error("HTTP 客户端构建失败：{message}")]
    ClientBuild {
        /// 失败描述。
        message: String,
    },
}

/// OpenAI 兼容 Provider。
pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("base_url", &self.base_url)
            // 不泄露密钥：仅标记是否已配置。
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl OpenAiProvider {
    /// 创建构建器。
    pub fn builder() -> OpenAiProviderBuilder {
        OpenAiProviderBuilder::default()
    }

    /// 从环境变量创建（[`API_KEY_ENV`] 必填；[`BASE_URL_ENV`] 可选）。
    ///
    /// # Errors
    /// 缺少 API key 时返回 [`OpenAiConfigError::MissingApiKey`]；其余错误见
    /// [`OpenAiProviderBuilder::build`]。
    pub fn from_env() -> Result<Self, OpenAiConfigError> {
        let api_key = std::env::var(API_KEY_ENV).map_err(|_| OpenAiConfigError::MissingApiKey)?;
        let mut builder = Self::builder().api_key(api_key);
        if let Ok(base_url) = std::env::var(BASE_URL_ENV) {
            builder = builder.base_url(base_url);
        }
        builder.build()
    }
}

/// [`OpenAiProvider`] 构建器。
#[derive(Debug, Default)]
pub struct OpenAiProviderBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
}

impl OpenAiProviderBuilder {
    /// 设置 API key（必填）。
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// 设置网关地址（默认 [`DEFAULT_BASE_URL`]；尾部 `/` 会被去除）。
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// 构建 Provider。
    ///
    /// # Errors
    /// - [`OpenAiConfigError::MissingApiKey`]：未设置 API key 或为空白；
    /// - [`OpenAiConfigError::EmptyBaseUrl`]：`base_url` 为空白；
    /// - [`OpenAiConfigError::ClientBuild`]：HTTP 客户端初始化失败。
    pub fn build(self) -> Result<OpenAiProvider, OpenAiConfigError> {
        let api_key = self
            .api_key
            .filter(|key| !key.trim().is_empty())
            .ok_or(OpenAiConfigError::MissingApiKey)?;
        let base_url = self.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let base_url = base_url.trim().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            return Err(OpenAiConfigError::EmptyBaseUrl);
        }

        // 显式安装 rustls 的 ring 后端（幂等：进程已安装其他后端时忽略）。
        // 选择 ring 的原因：不需要 cmake（aws-lc-rs 需要），降低构建门槛。
        let _ = rustls::crypto::ring::default_provider().install_default();

        let client =
            reqwest::Client::builder()
                .build()
                .map_err(|error| OpenAiConfigError::ClientBuild {
                    message: error.to_string(),
                })?;
        Ok(OpenAiProvider {
            client,
            api_key,
            base_url,
        })
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn id(&self) -> &str {
        "openai"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_streaming: true,
            supports_tool_calling: true,
            supports_system_prompt: true,
            supports_vision: false,
            max_context_tokens: None,
        }
    }

    async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
        let body = build_chat_request(&request);
        let url = format!("{}/chat/completions", self.base_url);
        // 全量原始交互日志（联调可追溯；Authorization 头不落盘）。
        tracing::debug!(target: "mycelium::openai", %url, body = %body, "→ POST /chat/completions");
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(map_request_error)?;

        let status = response.status();
        tracing::debug!(target: "mycelium::openai", status = status.as_u16(), "← HTTP 响应");
        if !status.is_success() {
            return Err(map_error_response(status, response).await);
        }
        Ok(parse_sse_stream(response.bytes_stream()))
    }
}

/// reqwest 错误 → `Transport`。
fn map_request_error(error: reqwest::Error) -> ModelError {
    ModelError::Transport {
        message: error.to_string(),
    }
}

/// 非 2xx 响应 → `Api` / `RateLimited`。
async fn map_error_response(
    status: reqwest::StatusCode,
    response: reqwest::Response,
) -> ModelError {
    let retry_after_ms = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000));
    let body = response.text().await.unwrap_or_default();
    map_error_status(status.as_u16(), retry_after_ms, &body)
}

/// 纯映射：状态码 + `Retry-After` + 响应体 → 错误。
fn map_error_status(status: u16, retry_after_ms: Option<u64>, body: &str) -> ModelError {
    if status == 429 {
        return ModelError::RateLimited { retry_after_ms };
    }
    let (code, message) = extract_error_fields(body);
    ModelError::Api {
        status,
        code,
        message,
    }
}

/// 解析 OpenAI 错误体 `{"error": {"message": ..., "code": ...}}`；非 JSON 时退回原文。
fn extract_error_fields(body: &str) -> (Option<String>, String) {
    /// 错误信封。
    #[derive(serde::Deserialize)]
    struct ErrorEnvelope {
        error: ErrorBody,
    }

    /// 错误体。
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        message: Option<String>,
        code: Option<String>,
        #[serde(rename = "type")]
        kind: Option<String>,
    }

    match serde_json::from_str::<ErrorEnvelope>(body) {
        Ok(envelope) => (
            envelope.error.code.or(envelope.error.kind),
            envelope.error.message.unwrap_or_else(|| body.to_owned()),
        ),
        Err(_) => (None, body.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use mycelium_core::ids::ModelId;
    use mycelium_core::message::Message;
    use mycelium_model::{ModelProvider, ModelRequest};

    use super::*;

    #[test]
    fn missing_api_key_is_rejected() {
        assert_eq!(
            OpenAiProvider::builder().build().unwrap_err(),
            OpenAiConfigError::MissingApiKey
        );
        assert_eq!(
            OpenAiProvider::builder()
                .api_key("   ")
                .build()
                .unwrap_err(),
            OpenAiConfigError::MissingApiKey
        );
    }

    #[test]
    fn empty_base_url_is_rejected() {
        assert_eq!(
            OpenAiProvider::builder()
                .api_key("k")
                .base_url("  ")
                .build()
                .unwrap_err(),
            OpenAiConfigError::EmptyBaseUrl
        );
    }

    #[test]
    fn base_url_is_normalized() {
        let provider = OpenAiProvider::builder().api_key("k").build().unwrap();
        assert_eq!(provider.base_url, DEFAULT_BASE_URL);

        let provider = OpenAiProvider::builder()
            .api_key("k")
            .base_url("http://127.0.0.1:1/v1/")
            .build()
            .unwrap();
        assert_eq!(provider.base_url, "http://127.0.0.1:1/v1");
    }

    #[test]
    fn maps_error_status_rules() {
        assert_eq!(
            map_error_status(429, Some(1500), "{}"),
            ModelError::RateLimited {
                retry_after_ms: Some(1500)
            }
        );
        assert_eq!(
            map_error_status(
                401,
                None,
                r#"{"error":{"message":"bad key","code":"invalid_api_key"}}"#
            ),
            ModelError::Api {
                status: 401,
                code: Some("invalid_api_key".to_owned()),
                message: "bad key".to_owned(),
            }
        );
        assert_eq!(
            map_error_status(502, None, "gateway down"),
            ModelError::Api {
                status: 502,
                code: None,
                message: "gateway down".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn maps_connection_error_to_transport() {
        let provider = OpenAiProvider::builder()
            .api_key("k")
            .base_url("http://127.0.0.1:1/v1")
            .build()
            .unwrap();
        let request = ModelRequest::new(ModelId::new("m").unwrap(), vec![Message::user_text("hi")]);
        let result = provider.stream(request).await;
        assert!(
            matches!(result, Err(ModelError::Transport { .. })),
            "期望 Transport 错误"
        );
    }
}
