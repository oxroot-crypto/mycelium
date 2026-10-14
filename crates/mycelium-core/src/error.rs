//! 错误分类矩阵（单一真实来源）。
//!
//! 约定：
//! - 领域错误使用 `thiserror` 枚举，必须携带定位上下文（哪个工具 / 模型 / 边界）；
//! - 跨层传播保留根因（`#[source]` / `#[from]`）；
//! - 严禁静默失败：所有偏离形式化约束的情况都必须显式返回错误。
//!
//! 对应契约见 `docs/spec/core.md` §2.8。

use crate::ids::{SessionId, ToolName};

/// 顶层的 Agent 运行错误。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AgentError {
    /// 配置非法。
    #[error("配置错误：{0}")]
    Config(#[from] ConfigError),
    /// 模型层错误。
    #[error("模型层错误：{0}")]
    Model(#[from] ModelError),
    /// 工具层错误。
    #[error("工具层错误：{0}")]
    Tool(#[from] ToolError),
    /// 超出预算（步数 / 输出 Token / 时长）。
    #[error("超出预算：{kind} 上限 {limit}，实际 {observed}")]
    BudgetExceeded {
        /// 超出的预算类别。
        kind: BudgetKind,
        /// 配置上限。
        limit: u64,
        /// 实际观测值。
        observed: u64,
    },
    /// 运行被取消。
    #[error("运行已被取消")]
    Cancelled,
    /// 会话不存在。
    #[error("会话不存在：{session_id}")]
    SessionNotFound {
        /// 缺失的会话标识。
        session_id: SessionId,
    },
    /// 功能尚未实现（骨架阶段显式失败，绝不静默成功）。
    #[error("功能尚未实现：{feature}（当前版本为 v0.1 架构骨架）")]
    Unimplemented {
        /// 功能标识（例如 `"AgentRuntime::run"`）。
        feature: &'static str,
    },
    /// 内部不变量被破坏（属于框架缺陷，应报告 issue）。
    #[error("内部错误：{message}")]
    Internal {
        /// 描述信息。
        message: String,
    },
}

/// 预算类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetKind {
    /// 步数（模型调用轮次）。
    Steps,
    /// 输出 Token 数。
    OutputTokens,
    /// 时长（毫秒）。
    DurationMs,
}

impl std::fmt::Display for BudgetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Steps => "steps",
            Self::OutputTokens => "output_tokens",
            Self::DurationMs => "duration_ms",
        };
        f.write_str(name)
    }
}

/// 配置校验错误。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConfigError {
    /// 缺少模型标识。
    #[error("缺少模型标识：请通过 AgentConfigBuilder::model 提供")]
    MissingModel,
    /// `max_steps` 为 0。
    #[error("max_steps 必须 >= 1")]
    ZeroMaxSteps,
    /// `request_timeout` 为 0。
    #[error("request_timeout 必须 > 0")]
    ZeroRequestTimeout,
    /// `max_output_tokens` 为 0。
    #[error("max_output_tokens 必须 >= 1（或省略表示不限制）")]
    ZeroMaxOutputTokens,
    /// `temperature` 越界或为 NaN。
    #[error("temperature 必须位于 [0.0, 2.0] 且非 NaN，实际为 {value}")]
    InvalidTemperature {
        /// 非法取值。
        value: f32,
    },
}

/// 消息内容错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    /// 消息内容为空。
    #[error("消息内容不能为空（至少需要一个内容块）")]
    EmptyContent,
}

/// 非法工具名。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidToolName {
    /// 空字符串。
    #[error("工具名不能为空")]
    Empty,
    /// 超过长度上限。
    #[error("工具名长度 {len} 超过上限 {max}")]
    TooLong {
        /// 实际字符数。
        len: usize,
        /// 允许的最大字符数。
        max: usize,
    },
    /// 含域外字符。
    #[error("工具名包含非法字符（仅允许 ASCII 字母、数字、下划线、连字符）：{value:?}")]
    InvalidCharacter {
        /// 非法值。
        value: String,
    },
}

/// 工具调用 ID 非法（为空或仅含空白）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("工具调用 ID 不能为空或仅含空白")]
pub struct InvalidToolCallId;

/// 模型标识非法（为空或仅含空白）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("模型标识不能为空或仅含空白")]
pub struct InvalidModelId;

/// 模型层错误。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelError {
    /// 传输层失败（网络 / IO）。
    #[error("模型传输失败：{message}")]
    Transport {
        /// 描述信息。
        message: String,
    },
    /// 提供方返回错误响应。
    #[error("模型服务返回错误：HTTP {status} code={code:?}：{message}")]
    Api {
        /// HTTP 状态码。
        status: u16,
        /// 提供方错误码（如 `"rate_limit_exceeded"`）。
        code: Option<String>,
        /// 提供方错误描述。
        message: String,
    },
    /// 限流。
    #[error("模型服务限流：建议 {retry_after_ms:?} 毫秒后重试")]
    RateLimited {
        /// 建议重试间隔（毫秒）；提供方未给出时为 `None`。
        retry_after_ms: Option<u64>,
    },
    /// 请求超时。
    #[error("模型请求超时：超过 {timeout_ms} 毫秒")]
    Timeout {
        /// 超时阈值（毫秒）。
        timeout_ms: u64,
    },
    /// 请求被取消。
    #[error("模型请求被取消")]
    Cancelled,
    /// 上下文长度超限。
    #[error("上下文长度超限：requested={requested:?} limit={limit:?}")]
    ContextLengthExceeded {
        /// 请求的上下文长度（未知时为 `None`）。
        requested: Option<u64>,
        /// 模型上限（未知时为 `None`）。
        limit: Option<u64>,
    },
    /// 提供方响应违反流协议（属于提供方实现缺陷）。
    #[error("模型响应违反协议：{detail}")]
    InvalidResponse {
        /// 违约细节。
        detail: String,
    },
    /// 当前模型 / 提供方不支持该功能。
    #[error("模型提供方不支持该功能：{feature}")]
    Unsupported {
        /// 功能标识（如 `"vision"`、`"tool_calling"`）。
        feature: &'static str,
    },
}

impl ModelError {
    /// 是否建议重试。
    ///
    /// 规则：`Transport` / `RateLimited` / `Timeout` 以及 HTTP 408、429、5xx 返回 `true`；
    /// 其余一律 `false`（重试无意义或需人工介入）。
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { .. } | Self::RateLimited { .. } | Self::Timeout { .. } => true,
            Self::Api { status, .. } => {
                *status == 408 || *status == 429 || (500..600).contains(status)
            }
            Self::Cancelled
            | Self::ContextLengthExceeded { .. }
            | Self::InvalidResponse { .. }
            | Self::Unsupported { .. } => false,
        }
    }
}

/// 工具层错误。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ToolError {
    /// 工具未注册。
    #[error("工具未注册：{name}")]
    NotFound {
        /// 工具名。
        name: ToolName,
    },
    /// 工具规格非法。
    #[error("工具规格非法（{tool}）：{detail}")]
    InvalidSpec {
        /// 工具名。
        tool: ToolName,
        /// 违规细节。
        detail: String,
    },
    /// 入参不满足工具 schema。
    #[error("工具入参非法（{tool}）：{detail}")]
    InvalidArguments {
        /// 工具名。
        tool: ToolName,
        /// 违规细节。
        detail: String,
    },
    /// 被策略拒绝（权限 / 审批）。
    #[error("工具调用被拒绝（{tool}）：{reason}")]
    Denied {
        /// 工具名。
        tool: ToolName,
        /// 拒绝原因。
        reason: String,
    },
    /// 执行失败。
    #[error("工具执行失败（{tool}）：{message}")]
    Execution {
        /// 工具名。
        tool: ToolName,
        /// 失败描述。
        message: String,
    },
    /// 执行超时。
    #[error("工具执行超时（{tool}）：超过 {timeout_ms} 毫秒")]
    Timeout {
        /// 工具名。
        tool: ToolName,
        /// 超时阈值（毫秒）。
        timeout_ms: u64,
    },
    /// 执行被取消。
    #[error("工具执行被取消：{tool}")]
    Cancelled {
        /// 工具名。
        tool: ToolName,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> ToolName {
        ToolName::new(name).unwrap()
    }

    #[test]
    fn model_error_retryable_matrix() {
        let cases: Vec<(ModelError, bool)> = vec![
            (
                ModelError::Transport {
                    message: "connection reset".to_owned(),
                },
                true,
            ),
            (
                ModelError::RateLimited {
                    retry_after_ms: None,
                },
                true,
            ),
            (ModelError::Timeout { timeout_ms: 1_000 }, true),
            (ModelError::Cancelled, false),
            (
                ModelError::ContextLengthExceeded {
                    requested: Some(10),
                    limit: Some(5),
                },
                false,
            ),
            (
                ModelError::InvalidResponse {
                    detail: "missing finished".to_owned(),
                },
                false,
            ),
            (ModelError::Unsupported { feature: "vision" }, false),
        ];
        for (error, expected) in cases {
            assert_eq!(error.is_retryable(), expected, "{error:?}");
        }
    }

    #[test]
    fn api_status_retryable_rules() {
        let api = |status: u16| ModelError::Api {
            status,
            code: None,
            message: "x".to_owned(),
        };
        for status in [408, 429, 500, 502, 503, 599] {
            assert!(api(status).is_retryable(), "HTTP {status} 应可重试");
        }
        for status in [400, 401, 403, 404, 422] {
            assert!(!api(status).is_retryable(), "HTTP {status} 不应可重试");
        }
    }

    #[test]
    fn error_display_contains_context() {
        let tool_error = ToolError::Execution {
            tool: tool("read_file"),
            message: "boom".to_owned(),
        };
        let text = tool_error.to_string();
        assert!(text.contains("read_file"));
        assert!(text.contains("boom"));

        let api_error = ModelError::Api {
            status: 429,
            code: Some("rate_limit".to_owned()),
            message: "slow down".to_owned(),
        };
        let text = api_error.to_string();
        assert!(text.contains("429"));
        assert!(text.contains("slow down"));

        let unimplemented = AgentError::Unimplemented {
            feature: "AgentRuntime::run",
        };
        assert!(unimplemented.to_string().contains("AgentRuntime::run"));

        let budget = AgentError::BudgetExceeded {
            kind: BudgetKind::Steps,
            limit: 8,
            observed: 9,
        };
        let text = budget.to_string();
        assert!(text.contains("steps"));
        assert!(text.contains('8'));
        assert!(text.contains('9'));
    }

    #[test]
    fn agent_error_source_is_preserved() {
        use std::error::Error;

        let from_model = AgentError::from(ModelError::Cancelled);
        assert!(from_model.source().is_some());

        let from_tool = AgentError::from(ToolError::NotFound { name: tool("nope") });
        assert!(from_tool.source().is_some());

        let from_config = AgentError::from(ConfigError::MissingModel);
        assert!(from_config.source().is_some());
    }
}
