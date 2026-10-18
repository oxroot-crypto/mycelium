//! 文本向量化（Embedding）SPI 与领域类型。
//!
//! 契约见 `docs/spec/core.md` §2.30：
//! - 请求构造即校验（批量上限、输入非空白、输出维度非零）；
//! - 响应必须经 [`EmbeddingResponse::new`] 校验（条数 / `index` 排列 / 非空向量 /
//!   维度一致 / 数值有限）后返回，提供方实现负责调用并映射为 [`ModelError`]；
//! - 错误统一复用 [`ModelError`]（不新增枚举变体，避免破坏性变更）。

use mycelium_core::error::ModelError;
use mycelium_core::ids::ModelId;

use crate::async_trait;

/// 单次请求允许的最大输入条数（与 OpenAI Embeddings API 的批量上限一致）。
pub const MAX_EMBEDDING_INPUTS: usize = 2048;

/// 一次文本向量化请求。
///
/// 不变量（构造即校验）：输入条数位于 `1..=MAX_EMBEDDING_INPUTS` 且每项非空白；
/// `dimensions` 为 `None` 或 `Some(d)` 且 `d >= 1`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRequest {
    model: ModelId,
    inputs: Vec<String>,
    dimensions: Option<u32>,
}

impl EmbeddingRequest {
    /// 构造向量化请求。
    ///
    /// # Arguments
    /// * `model` - 目标向量化模型标识。
    /// * `inputs` - 待向量化文本列表；按序与响应中的 `index` 对应。
    ///
    /// # Returns
    /// 校验通过（条数 `1..=2048` 且每项非空白）的请求，`dimensions = None`。
    ///
    /// # Errors
    /// - [`EmbeddingRequestError::EmptyInputs`]：`inputs` 为空；
    /// - [`EmbeddingRequestError::TooManyInputs`]：条数超过 [`MAX_EMBEDDING_INPUTS`]；
    /// - [`EmbeddingRequestError::BlankInput`]：存在空白输入（含首个空白项下标）。
    ///
    /// # Examples
    /// ```
    /// use mycelium_core::ModelId;
    /// use mycelium_model::EmbeddingRequest;
    ///
    /// let request = EmbeddingRequest::new(
    ///     ModelId::new("text-embedding-3-small").unwrap(),
    ///     vec!["你好".to_owned(), "hello".to_owned()],
    /// )
    /// .unwrap();
    /// assert_eq!(request.inputs().len(), 2);
    /// ```
    pub fn new(model: ModelId, inputs: Vec<String>) -> Result<Self, EmbeddingRequestError> {
        let len = inputs.len();
        if len == 0 {
            return Err(EmbeddingRequestError::EmptyInputs);
        }
        if len > MAX_EMBEDDING_INPUTS {
            return Err(EmbeddingRequestError::TooManyInputs {
                len,
                max: MAX_EMBEDDING_INPUTS,
            });
        }
        if let Some(index) = inputs.iter().position(|input| input.trim().is_empty()) {
            return Err(EmbeddingRequestError::BlankInput { index });
        }
        Ok(Self {
            model,
            inputs,
            dimensions: None,
        })
    }

    /// 设置期望的输出维度（部分模型支持降维；`None` 表示使用模型默认）。
    ///
    /// # Errors
    /// `dimensions == 0` 返回 [`EmbeddingRequestError::ZeroDimensions`]。
    pub fn with_dimensions(mut self, dimensions: u32) -> Result<Self, EmbeddingRequestError> {
        if dimensions == 0 {
            return Err(EmbeddingRequestError::ZeroDimensions);
        }
        self.dimensions = Some(dimensions);
        Ok(self)
    }

    /// 目标向量化模型标识。
    pub fn model(&self) -> &ModelId {
        &self.model
    }

    /// 待向量化文本列表（与响应 `index` 按位对应）。
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    /// 期望输出维度；未设置时为 `None`（使用模型默认）。
    pub fn dimensions(&self) -> Option<u32> {
        self.dimensions
    }
}

/// [`EmbeddingRequest`] 构造错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EmbeddingRequestError {
    /// 输入列表为空。
    #[error("向量化输入不能为空（至少需要 1 条文本）")]
    EmptyInputs,
    /// 输入条数超过上限。
    #[error("向量化输入条数 {len} 超过上限 {max}")]
    TooManyInputs {
        /// 实际条数。
        len: usize,
        /// 允许的最大条数。
        max: usize,
    },
    /// 存在空白输入。
    #[error("向量化输入第 {index} 条为空或仅含空白")]
    BlankInput {
        /// 首个空白输入的下标（从 0 开始）。
        index: usize,
    },
    /// 期望维度为 0。
    #[error("输出维度必须 >= 1（0 非法）")]
    ZeroDimensions,
}

/// 单条输入的向量结果。
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingVector {
    /// 对应输入在请求列表中的下标。
    pub index: usize,
    /// 向量数值（维度由模型决定）。
    pub vector: Vec<f32>,
}

/// 向量化用量（与聊天 [`mycelium_core::response::Usage`] 语义不同，单独定义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmbeddingUsage {
    /// 输入（提示）Token 数；提供方未报告时为 0。
    pub prompt_tokens: u64,
    /// 总 Token 数；提供方未报告时为 0。
    pub total_tokens: u64,
}

/// 一次文本向量化响应。
///
/// 不变量（构造即校验，见 [`EmbeddingResponse::new`]）：`embeddings` 条数与输入一致，
/// `embeddings[i].index == i`（按 `index` 升序），所有向量非空、维度一致且数值有限。
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingResponse {
    /// 提供方回显的模型名（可能缺失；不代表请求模型一定被原样满足）。
    pub model: Option<String>,
    /// 向量结果（按 `index` 升序，`embeddings[i].index == i`）。
    pub embeddings: Vec<EmbeddingVector>,
    /// 用量统计（提供方未报告时为 `None`）。
    pub usage: Option<EmbeddingUsage>,
}

impl EmbeddingResponse {
    /// 构造并校验向量化响应。
    ///
    /// # Arguments
    /// * `input_count` - 请求的输入条数（响应必须逐条对应）。
    /// * `model` - 提供方回显的模型名（可缺失）。
    /// * `embeddings` - 向量结果（允许乱序，构造时按 `index` 升序重排）。
    /// * `usage` - 用量统计（可缺失）。
    ///
    /// # Returns
    /// 校验通过的响应；`embeddings[i].index == i` 恒成立。
    ///
    /// # Errors
    /// - [`EmbeddingResponseError::LengthMismatch`]：条数与 `input_count` 不一致；
    /// - [`EmbeddingResponseError::IndexOutOfRange`]：`index >= input_count`；
    /// - [`EmbeddingResponseError::DuplicateIndex`]：`index` 重复（排序后不连续即重复）；
    /// - [`EmbeddingResponseError::EmptyVector`]：某条向量为空；
    /// - [`EmbeddingResponseError::DimensionMismatch`]：向量维度不一致；
    /// - [`EmbeddingResponseError::NonFiniteValue`]：向量含 `NaN` / `±inf`。
    ///
    /// # Examples
    /// ```
    /// use mycelium_model::{EmbeddingResponse, EmbeddingVector};
    ///
    /// let response = EmbeddingResponse::new(
    ///     2,
    ///     None,
    ///     vec![
    ///         EmbeddingVector { index: 1, vector: vec![0.5, 0.25] },
    ///         EmbeddingVector { index: 0, vector: vec![1.0, 0.0] },
    ///     ],
    ///     None,
    /// )
    /// .unwrap();
    /// assert_eq!(response.embeddings[0].index, 0);
    /// assert_eq!(response.embeddings[1].vector, vec![0.5, 0.25]);
    /// ```
    pub fn new(
        input_count: usize,
        model: Option<String>,
        mut embeddings: Vec<EmbeddingVector>,
        usage: Option<EmbeddingUsage>,
    ) -> Result<Self, EmbeddingResponseError> {
        if embeddings.len() != input_count {
            return Err(EmbeddingResponseError::LengthMismatch {
                expected: input_count,
                actual: embeddings.len(),
            });
        }
        embeddings.sort_by_key(|embedding| embedding.index);
        let mut dimensions: Option<usize> = None;
        for (position, embedding) in embeddings.iter().enumerate() {
            validate_embedding(embedding, position, input_count)?;
            match dimensions {
                None => dimensions = Some(embedding.vector.len()),
                Some(expected) if expected != embedding.vector.len() => {
                    return Err(EmbeddingResponseError::DimensionMismatch {
                        expected,
                        actual: embedding.vector.len(),
                    });
                }
                Some(_) => {}
            }
        }
        Ok(Self {
            model,
            embeddings,
            usage,
        })
    }
}

/// 校验单条向量（条数一致已在外层保证，`position` 为排序后的期望下标）。
fn validate_embedding(
    embedding: &EmbeddingVector,
    position: usize,
    input_count: usize,
) -> Result<(), EmbeddingResponseError> {
    if embedding.index >= input_count {
        return Err(EmbeddingResponseError::IndexOutOfRange {
            index: embedding.index,
            input_count,
        });
    }
    if embedding.index != position {
        return Err(EmbeddingResponseError::DuplicateIndex {
            index: embedding.index,
        });
    }
    if embedding.vector.is_empty() {
        return Err(EmbeddingResponseError::EmptyVector {
            index: embedding.index,
        });
    }
    if !embedding.vector.iter().all(|value| value.is_finite()) {
        return Err(EmbeddingResponseError::NonFiniteValue {
            index: embedding.index,
        });
    }
    Ok(())
}

/// [`EmbeddingResponse`] 结构校验错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EmbeddingResponseError {
    /// 向量条数与输入条数不一致。
    #[error("向量条数 {actual} 与输入条数 {expected} 不一致")]
    LengthMismatch {
        /// 期望条数（等于输入条数）。
        expected: usize,
        /// 实际条数。
        actual: usize,
    },
    /// `index` 越界。
    #[error("向量下标 {index} 超出输入范围（共 {input_count} 条）")]
    IndexOutOfRange {
        /// 越界的下标。
        index: usize,
        /// 输入条数。
        input_count: usize,
    },
    /// `index` 重复。
    #[error("向量下标重复：{index}")]
    DuplicateIndex {
        /// 重复出现的下标。
        index: usize,
    },
    /// 向量为空。
    #[error("下标 {index} 的向量为空")]
    EmptyVector {
        /// 对应下标。
        index: usize,
    },
    /// 向量维度不一致。
    #[error("向量维度不一致：期望 {expected}，实际 {actual}")]
    DimensionMismatch {
        /// 期望维度（首条向量的维度）。
        expected: usize,
        /// 实际维度。
        actual: usize,
    },
    /// 向量含非有限数值。
    #[error("下标 {index} 的向量包含非有限数值（NaN / ±inf）")]
    NonFiniteValue {
        /// 对应下标。
        index: usize,
    },
}

/// 文本向量化提供方（SPI 扩展点）。
///
/// 实现约定：
/// - 不得 panic；所有失败必须以 [`ModelError`] 返回；
/// - 返回前必须通过 [`EmbeddingResponse::new`] 完成结构校验，不得把未校验数据交给调用方；
/// - 错误映射：网络 / IO → [`ModelError::Transport`]；提供方错误响应 →
///   [`ModelError::Api`] / [`ModelError::RateLimited`]；结构违约 →
///   [`ModelError::InvalidResponse`]。
///
/// # Examples
/// ```
/// use mycelium_core::error::ModelError;
/// use mycelium_model::{
///     EmbeddingProvider, EmbeddingRequest, EmbeddingResponse, EmbeddingVector, async_trait,
/// };
///
/// /// 最小实现：固定返回全零向量（演示接线）。
/// struct ZeroProvider;
///
/// #[async_trait]
/// impl EmbeddingProvider for ZeroProvider {
///     fn id(&self) -> &str {
///         "zero"
///     }
///
///     async fn embed(
///         &self,
///         request: EmbeddingRequest,
///     ) -> Result<EmbeddingResponse, ModelError> {
///         let embeddings = request
///             .inputs()
///             .iter()
///             .enumerate()
///             .map(|(index, _)| EmbeddingVector {
///                 index,
///                 vector: vec![0.0, 0.0],
///             })
///             .collect();
///         EmbeddingResponse::new(request.inputs().len(), None, embeddings, None).map_err(|error| {
///             ModelError::InvalidResponse {
///                 detail: error.to_string(),
///             }
///         })
///     }
/// }
/// ```
#[async_trait]
pub trait EmbeddingProvider: Send + Sync + 'static {
    /// 提供方标识（用于日志与错误上下文；建议稳定、小写，如 `"openai"`）。
    fn id(&self) -> &str;

    /// 对一批文本执行向量化。
    ///
    /// # Arguments
    /// * `request` - 已通过构造校验的向量化请求。
    ///
    /// # Returns
    /// 校验通过的向量化响应（逐条对应输入，按 `index` 升序）。
    ///
    /// # Errors
    /// 网络 / 鉴权 / 限流 / 超时等失败返回 [`ModelError`]；响应结构违约返回
    /// [`ModelError::InvalidResponse`]。
    async fn embed(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse, ModelError>;
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn model() -> ModelId {
        ModelId::new("embed-test").unwrap()
    }

    fn vector(index: usize, values: &[f32]) -> EmbeddingVector {
        EmbeddingVector {
            index,
            vector: values.to_vec(),
        }
    }

    #[test]
    fn new_accepts_single_and_max_inputs() {
        // 边界值独立硬编码（1 / 2048），保证实现常量被篡改时测试变红（证伪原则）。
        let single = EmbeddingRequest::new(model(), vec!["a".to_owned()]).unwrap();
        assert_eq!(single.inputs(), ["a".to_owned()]);

        let max_inputs: Vec<String> = (0..2048).map(|i| format!("t{i}")).collect();
        let request = EmbeddingRequest::new(model(), max_inputs).unwrap();
        assert_eq!(request.inputs().len(), 2048);
    }

    #[test]
    fn new_rejects_empty_inputs() {
        assert_eq!(
            EmbeddingRequest::new(model(), Vec::new()).unwrap_err(),
            EmbeddingRequestError::EmptyInputs
        );
    }

    #[test]
    fn new_rejects_too_many_inputs() {
        let too_many: Vec<String> = (0..2049).map(|i| format!("t{i}")).collect();
        assert_eq!(
            EmbeddingRequest::new(model(), too_many).unwrap_err(),
            EmbeddingRequestError::TooManyInputs {
                len: 2049,
                max: 2048
            }
        );
    }

    #[test]
    fn new_rejects_blank_input() {
        for (inputs, expected_index) in [
            (vec!["", "b"], 0),
            (vec!["ok", "  "], 1),
            (vec!["ok", "\t\n", "x"], 1),
        ] {
            let inputs: Vec<String> = inputs.into_iter().map(str::to_owned).collect();
            assert_eq!(
                EmbeddingRequest::new(model(), inputs).unwrap_err(),
                EmbeddingRequestError::BlankInput {
                    index: expected_index
                }
            );
        }
    }

    #[test]
    fn with_dimensions_rejects_zero() {
        let request = EmbeddingRequest::new(model(), vec!["a".to_owned()]).unwrap();
        assert_eq!(
            request.clone().with_dimensions(0).unwrap_err(),
            EmbeddingRequestError::ZeroDimensions
        );
        // 失败时请求保持未设置（可继续使用原请求）。
        assert_eq!(request.dimensions(), None);
    }

    #[test]
    fn accessors_preserve_values() {
        let request = EmbeddingRequest::new(model(), vec!["a".to_owned(), "b".to_owned()]).unwrap();
        assert_eq!(request.model().as_str(), "embed-test");
        assert_eq!(request.inputs(), ["a".to_owned(), "b".to_owned()]);
        assert_eq!(request.dimensions(), None);

        let request = request.with_dimensions(64).unwrap();
        assert_eq!(request.dimensions(), Some(64));
        assert_eq!(
            request.clone().with_dimensions(1).unwrap().dimensions(),
            Some(1)
        );
    }

    #[test]
    fn response_accepts_and_sorts_by_index() {
        let response = EmbeddingResponse::new(
            2,
            Some("embed-test".to_owned()),
            vec![vector(1, &[0.5, 0.25]), vector(0, &[1.0, 0.0])],
            Some(EmbeddingUsage {
                prompt_tokens: 3,
                total_tokens: 3,
            }),
        )
        .unwrap();

        assert_eq!(response.embeddings[0].index, 0);
        assert_eq!(response.embeddings[0].vector, vec![1.0, 0.0]);
        assert_eq!(response.embeddings[1].index, 1);
        assert_eq!(response.embeddings[1].vector, vec![0.5, 0.25]);
        assert_eq!(response.model.as_deref(), Some("embed-test"));
        assert_eq!(
            response.usage,
            Some(EmbeddingUsage {
                prompt_tokens: 3,
                total_tokens: 3
            })
        );
    }

    #[test]
    fn response_rejects_length_mismatch() {
        assert_eq!(
            EmbeddingResponse::new(2, None, vec![vector(0, &[1.0])], None).unwrap_err(),
            EmbeddingResponseError::LengthMismatch {
                expected: 2,
                actual: 1
            }
        );
    }

    #[test]
    fn response_rejects_index_out_of_range() {
        assert_eq!(
            EmbeddingResponse::new(1, None, vec![vector(1, &[1.0])], None).unwrap_err(),
            EmbeddingResponseError::IndexOutOfRange {
                index: 1,
                input_count: 1
            }
        );
    }

    #[test]
    fn response_rejects_duplicate_index() {
        assert_eq!(
            EmbeddingResponse::new(2, None, vec![vector(0, &[1.0]), vector(0, &[2.0])], None)
                .unwrap_err(),
            EmbeddingResponseError::DuplicateIndex { index: 0 }
        );
    }

    #[test]
    fn response_rejects_empty_vector() {
        assert_eq!(
            EmbeddingResponse::new(1, None, vec![vector(0, &[])], None).unwrap_err(),
            EmbeddingResponseError::EmptyVector { index: 0 }
        );
    }

    #[test]
    fn response_rejects_dimension_mismatch() {
        assert_eq!(
            EmbeddingResponse::new(
                2,
                None,
                vec![vector(0, &[1.0]), vector(1, &[1.0, 2.0])],
                None
            )
            .unwrap_err(),
            EmbeddingResponseError::DimensionMismatch {
                expected: 1,
                actual: 2
            }
        );
    }

    #[test]
    fn response_rejects_non_finite_value() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                EmbeddingResponse::new(1, None, vec![vector(0, &[value])], None).unwrap_err(),
                EmbeddingResponseError::NonFiniteValue { index: 0 }
            );
        }
    }

    proptest! {
        /// FC-INV-040：接受集与规范一致，且合法时输入逐项保真。
        #[test]
        fn prop_embedding_request_validation_matches_spec(
            inputs in proptest::collection::vec(any::<String>(), 0..8),
        ) {
            let model = model();
            let result = EmbeddingRequest::new(model.clone(), inputs.clone());
            let expected_ok = !inputs.is_empty()
                && inputs.len() <= MAX_EMBEDDING_INPUTS
                && inputs.iter().all(|input| !input.trim().is_empty());
            prop_assert_eq!(result.is_ok(), expected_ok);
            if let Ok(request) = result {
                prop_assert_eq!(request.inputs(), inputs.as_slice());
                prop_assert_eq!(request.model(), &model);
                prop_assert_eq!(request.dimensions(), None);
            }
        }
    }
}
