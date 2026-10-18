//! Embeddings API 映射（mycelium 领域模型 ↔ OpenAI 兼容 `/embeddings`）。
//!
//! 只做纯数据的构造与解析；网络与错误映射由 [`crate::provider::OpenAiProvider`] 负责。
//! 契约见 `docs/spec/core.md` §2.30（FC-POST-241 / FC-POST-242 / FC-ERR-101）。

use serde::Deserialize;
use serde_json::{Map, Value, json};

use mycelium_core::error::ModelError;
use mycelium_model::{EmbeddingRequest, EmbeddingResponse, EmbeddingUsage, EmbeddingVector};

/// 构造 `/embeddings` 请求体（FC-POST-241）：
/// `{model, input}`，`dimensions` 仅在请求显式设置时携带。
pub(crate) fn build_embeddings_request(request: &EmbeddingRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(request.model().as_str()));
    body.insert("input".to_owned(), json!(request.inputs()));
    if let Some(dimensions) = request.dimensions() {
        body.insert("dimensions".to_owned(), json!(dimensions));
    }
    Value::Object(body)
}

/// 解析 `/embeddings` 响应体（FC-POST-242 / FC-ERR-101）。
///
/// # Errors
/// 非法 JSON、缺失必需字段或结构违约（条数 / 下标 / 维度 / 数值）一律返回
/// [`ModelError::InvalidResponse`]（含细节），不静默容忍。
pub(crate) fn parse_embeddings_response(
    body: &[u8],
    input_count: usize,
) -> Result<EmbeddingResponse, ModelError> {
    let raw: RawResponse = serde_json::from_slice(body)
        .map_err(|error| invalid_response(format!("响应体解析失败：{error}")))?;
    let embeddings = raw
        .data
        .into_iter()
        .map(|item| EmbeddingVector {
            index: item.index,
            vector: item.embedding,
        })
        .collect();
    let usage = raw.usage.map(|usage| EmbeddingUsage {
        prompt_tokens: usage.prompt_tokens.unwrap_or(0),
        total_tokens: usage.total_tokens.unwrap_or(0),
    });
    EmbeddingResponse::new(input_count, raw.model, embeddings, usage)
        .map_err(|error| invalid_response(format!("响应违反结构契约：{error}")))
}

/// 结构违约 → `InvalidResponse`。
fn invalid_response(detail: String) -> ModelError {
    ModelError::InvalidResponse { detail }
}

/// `/embeddings` 响应信封（仅取所需字段，多余字段忽略）。
#[derive(Deserialize)]
struct RawResponse {
    /// 提供方回显的模型名（可缺失）。
    model: Option<String>,
    /// 向量数据（必需；缺失即违约）。
    data: Vec<RawEmbedding>,
    /// 用量（可缺失）。
    usage: Option<RawUsage>,
}

/// 单条向量数据。
#[derive(Deserialize)]
struct RawEmbedding {
    /// 对应输入下标。
    index: usize,
    /// 向量数值。
    embedding: Vec<f32>,
}

/// 用量统计（字段可缺失，缺省按 0 处理）。
#[derive(Deserialize)]
struct RawUsage {
    /// 输入 Token 数。
    prompt_tokens: Option<u64>,
    /// 总 Token 数。
    total_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use mycelium_core::ids::ModelId;
    use mycelium_model::EmbeddingRequest;

    use super::*;

    fn request() -> EmbeddingRequest {
        EmbeddingRequest::new(
            ModelId::new("text-embedding-3-small").unwrap(),
            vec!["a".to_owned(), "b".to_owned()],
        )
        .unwrap()
    }

    #[test]
    fn maps_request_body() {
        let body = build_embeddings_request(&request());
        assert_eq!(body["model"], json!("text-embedding-3-small"));
        assert_eq!(body["input"], json!(["a", "b"]));
        assert!(body.get("dimensions").is_none());
        // 不发送多余字段（stream 等聊天参数不得出现）。
        assert_eq!(body.as_object().unwrap().len(), 2);

        let body = build_embeddings_request(&request().with_dimensions(64).unwrap());
        assert_eq!(body["dimensions"], json!(64));
        assert_eq!(body.as_object().unwrap().len(), 3);
    }

    #[test]
    fn parses_response_with_usage_and_model() {
        let body = br#"{
            "object": "list",
            "model": "text-embedding-3-small",
            "data": [
                {"object": "embedding", "index": 1, "embedding": [0.5, 0.25]},
                {"object": "embedding", "index": 0, "embedding": [1.0, 0.0]}
            ],
            "usage": {"prompt_tokens": 4, "total_tokens": 4}
        }"#;
        let response = parse_embeddings_response(body, 2).unwrap();

        assert_eq!(response.model.as_deref(), Some("text-embedding-3-small"));
        assert_eq!(response.embeddings[0].index, 0);
        assert_eq!(response.embeddings[0].vector, vec![1.0, 0.0]);
        assert_eq!(response.embeddings[1].index, 1);
        assert_eq!(response.embeddings[1].vector, vec![0.5, 0.25]);
        assert_eq!(
            response.usage,
            Some(EmbeddingUsage {
                prompt_tokens: 4,
                total_tokens: 4
            })
        );
    }

    #[test]
    fn parses_response_without_usage_and_model() {
        let body = br#"{"data": [{"index": 0, "embedding": [1.0]}]}"#;
        let response = parse_embeddings_response(body, 1).unwrap();
        assert_eq!(response.model, None);
        assert_eq!(response.usage, None);
        assert_eq!(response.embeddings[0].vector, vec![1.0]);

        // usage 存在但字段缺失：缺省按 0。
        let body = br#"{"data": [{"index": 0, "embedding": [1.0]}], "usage": {"total_tokens": 5}}"#;
        let response = parse_embeddings_response(body, 1).unwrap();
        assert_eq!(
            response.usage,
            Some(EmbeddingUsage {
                prompt_tokens: 0,
                total_tokens: 5
            })
        );
    }

    #[test]
    fn rejects_malformed_json() {
        let result = parse_embeddings_response(b"not json", 1);
        assert!(
            matches!(result, Err(ModelError::InvalidResponse { .. })),
            "非法 JSON 必须显式失败"
        );
    }

    #[test]
    fn rejects_missing_data() {
        let result = parse_embeddings_response(br#"{"object": "list"}"#, 1);
        assert!(
            matches!(result, Err(ModelError::InvalidResponse { .. })),
            "缺 data 必须显式失败"
        );
    }

    #[test]
    fn rejects_out_of_range_index() {
        let body = br#"{"data": [{"index": 2, "embedding": [1.0]}]}"#;
        let result = parse_embeddings_response(body, 1);
        assert!(
            matches!(
                result,
                Err(ModelError::InvalidResponse { ref detail }) if detail.contains("下标")
            ),
            "越界下标必须显式失败"
        );
    }

    #[test]
    fn rejects_duplicate_index() {
        let body = br#"{"data": [
            {"index": 0, "embedding": [1.0]},
            {"index": 0, "embedding": [2.0]}
        ]}"#;
        let result = parse_embeddings_response(body, 2);
        assert!(
            matches!(result, Err(ModelError::InvalidResponse { .. })),
            "重复下标必须显式失败"
        );
    }

    #[test]
    fn rejects_empty_vector() {
        let body = br#"{"data": [{"index": 0, "embedding": []}]}"#;
        let result = parse_embeddings_response(body, 1);
        assert!(
            matches!(result, Err(ModelError::InvalidResponse { .. })),
            "空向量必须显式失败"
        );
    }

    #[test]
    fn rejects_inconsistent_dimensions() {
        let body = br#"{"data": [
            {"index": 0, "embedding": [1.0]},
            {"index": 1, "embedding": [1.0, 2.0]}
        ]}"#;
        let result = parse_embeddings_response(body, 2);
        assert!(
            matches!(result, Err(ModelError::InvalidResponse { .. })),
            "维度不一致必须显式失败"
        );
    }

    #[test]
    fn rejects_non_finite_values() {
        // `1e39` 超出 f32 有限范围（解析为 ±inf 时由结构校验拒绝）。
        let body = br#"{"data": [{"index": 0, "embedding": [1e39]}]}"#;
        let result = parse_embeddings_response(body, 1);
        assert!(
            matches!(result, Err(ModelError::InvalidResponse { .. })),
            "非有限数值必须显式失败"
        );
    }

    #[test]
    fn rejects_length_mismatch() {
        let body = br#"{"data": [{"index": 0, "embedding": [1.0]}]}"#;
        let result = parse_embeddings_response(body, 2);
        assert!(
            matches!(result, Err(ModelError::InvalidResponse { .. })),
            "条数不符必须显式失败"
        );
    }
}
