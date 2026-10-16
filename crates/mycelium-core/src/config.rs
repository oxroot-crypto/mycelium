//! Agent 配置及其校验。
//!
//! 配置一经构造即不可变（字段私有），所有不变量由 [`AgentConfigBuilder`] 在构建时校验：
//! 非法配置无法表示为 [`AgentConfig`]。对应契约见 `docs/spec/core.md` §2.6。

use std::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use std::time::Duration;

use crate::error::ConfigError;
use crate::ids::ModelId;
use crate::tool::ToolChoice;

/// 默认最大步数（保守取值，防止失控循环）。
pub const DEFAULT_MAX_STEPS: u32 = 8;

/// 默认单次模型请求超时（秒）。
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;

/// 默认工具并发上限（1 = 顺序执行，保持 v0.2 行为）。
pub const DEFAULT_MAX_PARALLEL_TOOL_CALLS: usize = 1;

/// 采样温度下限。
pub const TEMPERATURE_MIN: f32 = 0.0;

/// 采样温度上限。
pub const TEMPERATURE_MAX: f32 = 2.0;

/// `reasoning.effort` 允许的最大长度（字符）。
pub const REASONING_EFFORT_MAX_LEN: usize = 32;

/// 默认首次重试退避（毫秒）。
pub const DEFAULT_RETRY_INITIAL_BACKOFF_MS: u64 = 500;

/// 默认重试退避上限（毫秒）。
pub const DEFAULT_RETRY_MAX_BACKOFF_MS: u64 = 8_000;

/// 模型请求重试策略。
///
/// 仅对 [`crate::ModelError::is_retryable`] 为真的错误生效；退避按
/// `initial * 2^(attempt-1)` 指数增长并封顶 [`RetryPolicy::max_backoff`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: NonZeroU32,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl RetryPolicy {
    /// 以最大尝试次数创建（含首次；默认退避 500ms 起、封顶 8s）。
    ///
    /// # Errors
    /// `max_attempts == 0` 时返回 [`ConfigError::ZeroRetryAttempts`]。
    pub fn new(max_attempts: u32) -> Result<Self, ConfigError> {
        Ok(Self {
            max_attempts: NonZeroU32::new(max_attempts).ok_or(ConfigError::ZeroRetryAttempts)?,
            initial_backoff: Duration::from_millis(DEFAULT_RETRY_INITIAL_BACKOFF_MS),
            max_backoff: Duration::from_millis(DEFAULT_RETRY_MAX_BACKOFF_MS),
        })
    }

    /// 自定义退避区间。
    ///
    /// # Errors
    /// 任一时长为 0 → [`ConfigError::ZeroRetryBackoff`]；
    /// `initial > max` → [`ConfigError::InvalidRetryBackoff`]。
    pub fn with_backoff(
        mut self,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Result<Self, ConfigError> {
        if initial_backoff.is_zero() || max_backoff.is_zero() {
            return Err(ConfigError::ZeroRetryBackoff);
        }
        if initial_backoff > max_backoff {
            return Err(ConfigError::InvalidRetryBackoff {
                initial_ms: millis(initial_backoff),
                max_ms: millis(max_backoff),
            });
        }
        self.initial_backoff = initial_backoff;
        self.max_backoff = max_backoff;
        Ok(self)
    }

    /// 最大尝试次数（含首次）。
    pub fn max_attempts(&self) -> NonZeroU32 {
        self.max_attempts
    }

    /// 首次退避。
    pub fn initial_backoff(&self) -> Duration {
        self.initial_backoff
    }

    /// 退避上限。
    pub fn max_backoff(&self) -> Duration {
        self.max_backoff
    }

    /// 第 `attempt` 次失败后的退避时长（指数增长、封顶，溢出安全）。
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1);
        let factor = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
        self.initial_backoff
            .checked_mul(factor)
            .unwrap_or(self.max_backoff)
            .min(self.max_backoff)
    }
}

/// 时长转毫秒（饱和到 `u64::MAX`）。
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// 模型思考（推理）选项。
///
/// 语义：`enabled` 与 `effort` 均由**提供方定义**；适配层按各自协议映射
/// （DeepSeek：`thinking: {"type": "enabled" | "disabled"}` + `reasoning_effort`）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReasoningOptions {
    /// 是否启用思考模式（`None` = 提供方默认；DeepSeek 默认启用）。
    pub enabled: Option<bool>,
    /// 思考强度（提供方定义的级别；DeepSeek 支持 `low` / `high` / `max`，`none` 表示关闭）。
    pub effort: Option<String>,
}

impl ReasoningOptions {
    /// 创建空选项（全部使用提供方默认）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 显式启用思考模式的选项。
    pub fn enabled() -> Self {
        Self {
            enabled: Some(true),
            effort: None,
        }
    }

    /// 显式关闭思考模式的选项。
    pub fn disabled() -> Self {
        Self {
            enabled: Some(false),
            effort: None,
        }
    }

    /// 设置思考强度（提供方定义级别，校验见 [`AgentConfigBuilder::build`]）。
    pub fn with_effort(mut self, effort: impl Into<String>) -> Self {
        self.effort = Some(effort.into());
        self
    }
}

/// Agent 配置（不可变）。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentConfig {
    model: ModelId,
    system_prompt: Option<String>,
    max_steps: NonZeroU32,
    request_timeout: Duration,
    max_output_tokens: Option<NonZeroU64>,
    temperature: Option<f32>,
    tool_choice: ToolChoice,
    max_parallel_tool_calls: NonZeroUsize,
    tool_timeout: Option<Duration>,
    reasoning: Option<ReasoningOptions>,
    max_duration: Option<Duration>,
    retry: Option<RetryPolicy>,
}

impl AgentConfig {
    /// 创建配置构建器。
    pub fn builder() -> AgentConfigBuilder {
        AgentConfigBuilder::default()
    }

    /// 目标模型。
    pub fn model(&self) -> &ModelId {
        &self.model
    }

    /// 系统提示。
    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    /// 单次运行最大步数（≥ 1）。
    pub fn max_steps(&self) -> NonZeroU32 {
        self.max_steps
    }

    /// 单次模型请求超时（> 0）。
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// 输出 Token 上限（`None` 表示由提供方默认）。
    pub fn max_output_tokens(&self) -> Option<NonZeroU64> {
        self.max_output_tokens
    }

    /// 采样温度（`None` 表示提供方默认）。
    pub fn temperature(&self) -> Option<f32> {
        self.temperature
    }

    /// 工具选择策略。
    pub fn tool_choice(&self) -> &ToolChoice {
        &self.tool_choice
    }

    /// 单个 Run 内并发执行的工具调用上限（1 = 顺序执行）。
    pub fn max_parallel_tool_calls(&self) -> NonZeroUsize {
        self.max_parallel_tool_calls
    }

    /// 单个工具调用的超时（`None` 表示不限制）。
    pub fn tool_timeout(&self) -> Option<Duration> {
        self.tool_timeout
    }

    /// 思考（推理）选项（`None` 表示全部使用提供方默认）。
    pub fn reasoning(&self) -> Option<&ReasoningOptions> {
        self.reasoning.as_ref()
    }

    /// 单次运行最长时长（`None` 表示不限制）。
    pub fn max_duration(&self) -> Option<Duration> {
        self.max_duration
    }

    /// 模型请求重试策略（`None` 表示不重试）。
    pub fn retry(&self) -> Option<&RetryPolicy> {
        self.retry.as_ref()
    }
}

/// [`AgentConfig`] 构建器。
///
/// `model` 为必填项；其余字段缺省值见 [`DEFAULT_MAX_STEPS`] 与
/// [`DEFAULT_REQUEST_TIMEOUT_SECS`]。
#[derive(Debug, Default)]
pub struct AgentConfigBuilder {
    model: Option<ModelId>,
    system_prompt: Option<String>,
    max_steps: Option<u32>,
    request_timeout: Option<Duration>,
    max_output_tokens: Option<u64>,
    temperature: Option<f32>,
    tool_choice: Option<ToolChoice>,
    max_parallel_tool_calls: Option<usize>,
    tool_timeout: Option<Duration>,
    reasoning: Option<ReasoningOptions>,
    max_duration: Option<Duration>,
    retry: Option<RetryPolicy>,
}

impl AgentConfigBuilder {
    /// 设置目标模型（必填）。
    pub fn model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// 设置系统提示。
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// 设置单次运行最大步数（必须 ≥ 1）。
    pub fn max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = Some(max_steps);
        self
    }

    /// 设置单次模型请求超时（必须 > 0）。
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// 设置输出 Token 上限（必须 ≥ 1）。
    pub fn max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// 设置采样温度（必须 ∈ `[0.0, 2.0]` 且非 NaN）。
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// 设置工具选择策略。
    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// 设置单个 Run 内并发执行的工具调用上限（必须 ≥ 1；1 = 顺序执行）。
    pub fn max_parallel_tool_calls(mut self, max_parallel_tool_calls: usize) -> Self {
        self.max_parallel_tool_calls = Some(max_parallel_tool_calls);
        self
    }

    /// 设置单个工具调用的超时（必须 > 0；省略表示不限制）。
    pub fn tool_timeout(mut self, timeout: Duration) -> Self {
        self.tool_timeout = Some(timeout);
        self
    }

    /// 设置思考（推理）选项。
    pub fn reasoning(mut self, reasoning: ReasoningOptions) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    /// 设置单次运行最长时长（必须 > 0；省略表示不限制）。
    pub fn max_duration(mut self, max_duration: Duration) -> Self {
        self.max_duration = Some(max_duration);
        self
    }

    /// 设置模型请求重试策略（省略表示不重试）。
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// 构建并校验配置。
    ///
    /// # Errors
    /// - 未设置 `model`：[`ConfigError::MissingModel`]；
    /// - `max_steps == 0`：[`ConfigError::ZeroMaxSteps`]；
    /// - `max_output_tokens == Some(0)`：[`ConfigError::ZeroMaxOutputTokens`]；
    /// - `request_timeout` 为零：[`ConfigError::ZeroRequestTimeout`]；
    /// - `max_parallel_tool_calls == 0`：[`ConfigError::ZeroMaxParallelToolCalls`]；
    /// - `tool_timeout == Some(0)`：[`ConfigError::ZeroToolTimeout`]；
    /// - `temperature` 越界或为 NaN：[`ConfigError::InvalidTemperature`]。
    pub fn build(self) -> Result<AgentConfig, ConfigError> {
        let model = self.model.ok_or(ConfigError::MissingModel)?;

        let max_steps = NonZeroU32::new(self.max_steps.unwrap_or(DEFAULT_MAX_STEPS))
            .ok_or(ConfigError::ZeroMaxSteps)?;

        let max_output_tokens = match self.max_output_tokens {
            None => None,
            Some(value) => Some(NonZeroU64::new(value).ok_or(ConfigError::ZeroMaxOutputTokens)?),
        };

        let request_timeout = self
            .request_timeout
            .unwrap_or(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS));
        if request_timeout.is_zero() {
            return Err(ConfigError::ZeroRequestTimeout);
        }

        let max_parallel_tool_calls = NonZeroUsize::new(
            self.max_parallel_tool_calls
                .unwrap_or(DEFAULT_MAX_PARALLEL_TOOL_CALLS),
        )
        .ok_or(ConfigError::ZeroMaxParallelToolCalls)?;

        if let Some(value) = self.tool_timeout
            && value.is_zero()
        {
            return Err(ConfigError::ZeroToolTimeout);
        }

        if let Some(value) = self.temperature {
            // NaN 会 fail contains 检查，必须一并拒绝。
            if !(TEMPERATURE_MIN..=TEMPERATURE_MAX).contains(&value) {
                return Err(ConfigError::InvalidTemperature { value });
            }
        }

        if let Some(effort) = self
            .reasoning
            .as_ref()
            .and_then(|options| options.effort.as_ref())
            && !is_valid_effort(effort)
        {
            return Err(ConfigError::InvalidReasoningEffort {
                value: effort.clone(),
            });
        }

        if let Some(value) = self.max_duration
            && value.is_zero()
        {
            return Err(ConfigError::ZeroMaxDuration);
        }

        Ok(AgentConfig {
            model,
            system_prompt: self.system_prompt,
            max_steps,
            request_timeout,
            max_output_tokens,
            temperature: self.temperature,
            tool_choice: self.tool_choice.unwrap_or_default(),
            max_parallel_tool_calls,
            tool_timeout: self.tool_timeout,
            reasoning: self.reasoning,
            max_duration: self.max_duration,
            retry: self.retry,
        })
    }
}

/// 校验思考强度：`1..=32` 个 ASCII 字母 / 数字 / `_` / `-`。
fn is_valid_effort(effort: &str) -> bool {
    let len = effort.chars().count();
    (1..=REASONING_EFFORT_MAX_LEN).contains(&len)
        && effort
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn minimal() -> AgentConfigBuilder {
        AgentConfig::builder().model(ModelId::new("test-model").unwrap())
    }

    #[test]
    fn build_requires_model() {
        assert_eq!(
            AgentConfig::builder().build().unwrap_err(),
            ConfigError::MissingModel
        );
    }

    #[test]
    fn defaults_are_stable() {
        let config = minimal().build().unwrap();
        assert_eq!(config.model().as_str(), "test-model");
        assert_eq!(config.max_steps().get(), DEFAULT_MAX_STEPS);
        assert_eq!(config.max_steps().get(), 8);
        assert_eq!(
            config.request_timeout(),
            Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS)
        );
        assert_eq!(config.request_timeout(), Duration::from_secs(60));
        assert_eq!(config.max_output_tokens(), None);
        assert_eq!(config.temperature(), None);
        assert_eq!(config.system_prompt(), None);
        assert_eq!(config.tool_choice(), &ToolChoice::Auto);
        assert_eq!(
            config.max_parallel_tool_calls().get(),
            DEFAULT_MAX_PARALLEL_TOOL_CALLS
        );
        assert_eq!(config.max_parallel_tool_calls().get(), 1);
        assert_eq!(config.tool_timeout(), None);
        assert_eq!(config.reasoning(), None);
        assert_eq!(config.max_duration(), None);
        assert!(config.retry().is_none());
    }

    #[test]
    fn max_steps_boundaries() {
        assert_eq!(
            minimal().max_steps(0).build().unwrap_err(),
            ConfigError::ZeroMaxSteps
        );
        assert_eq!(minimal().max_steps(1).build().unwrap().max_steps().get(), 1);
        assert_eq!(
            minimal()
                .max_steps(u32::MAX)
                .build()
                .unwrap()
                .max_steps()
                .get(),
            u32::MAX
        );
    }

    #[test]
    fn max_output_tokens_boundaries() {
        assert_eq!(
            minimal().max_output_tokens(0).build().unwrap_err(),
            ConfigError::ZeroMaxOutputTokens
        );
        assert_eq!(
            minimal()
                .max_output_tokens(1)
                .build()
                .unwrap()
                .max_output_tokens(),
            NonZeroU64::new(1)
        );
        assert_eq!(minimal().build().unwrap().max_output_tokens(), None);
    }

    #[test]
    fn request_timeout_boundaries() {
        assert_eq!(
            minimal()
                .request_timeout(Duration::ZERO)
                .build()
                .unwrap_err(),
            ConfigError::ZeroRequestTimeout
        );
        assert_eq!(
            minimal()
                .request_timeout(Duration::from_nanos(1))
                .build()
                .unwrap()
                .request_timeout(),
            Duration::from_nanos(1)
        );
        assert_eq!(
            minimal()
                .request_timeout(Duration::from_secs(u64::MAX))
                .build()
                .unwrap()
                .request_timeout(),
            Duration::from_secs(u64::MAX)
        );
    }

    #[test]
    fn temperature_boundaries() {
        for ok in [0.0_f32, 1.0, 2.0] {
            let config = minimal().temperature(ok).build().unwrap();
            assert_eq!(config.temperature().map(f32::to_bits), Some(ok.to_bits()));
        }
        for bad in [-0.1_f32, 2.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            match minimal().temperature(bad).build() {
                Err(ConfigError::InvalidTemperature { value }) => {
                    assert_eq!(value.to_bits(), bad.to_bits());
                }
                other => panic!("应拒绝 temperature = {bad}，实际：{other:?}"),
            }
        }
    }

    #[test]
    fn max_parallel_tool_calls_boundaries() {
        assert_eq!(
            minimal().max_parallel_tool_calls(0).build().unwrap_err(),
            ConfigError::ZeroMaxParallelToolCalls
        );
        assert_eq!(
            minimal()
                .max_parallel_tool_calls(1)
                .build()
                .unwrap()
                .max_parallel_tool_calls()
                .get(),
            1
        );
        assert_eq!(
            minimal()
                .max_parallel_tool_calls(8)
                .build()
                .unwrap()
                .max_parallel_tool_calls()
                .get(),
            8
        );
    }

    #[test]
    fn tool_timeout_boundaries() {
        assert_eq!(
            minimal().tool_timeout(Duration::ZERO).build().unwrap_err(),
            ConfigError::ZeroToolTimeout
        );
        assert_eq!(
            minimal()
                .tool_timeout(Duration::from_millis(1))
                .build()
                .unwrap()
                .tool_timeout(),
            Some(Duration::from_millis(1))
        );
        assert_eq!(minimal().build().unwrap().tool_timeout(), None);
    }

    #[test]
    fn reasoning_effort_validation() {
        // 合法：典型级别与边界长度
        for effort in ["low", "high", "max", "none", "medium", &"a".repeat(32)] {
            let options = ReasoningOptions::enabled().with_effort(effort);
            let config = minimal().reasoning(options).build().unwrap();
            assert_eq!(
                config.reasoning().and_then(|value| value.effort.as_deref()),
                Some(effort)
            );
        }

        // 非法：空、超长、非法字符
        for bad in ["", " ", &"a".repeat(33), "hig h", "高"] {
            let options = ReasoningOptions::default().with_effort(bad);
            assert_eq!(
                minimal().reasoning(options).build().unwrap_err(),
                ConfigError::InvalidReasoningEffort {
                    value: bad.to_owned()
                }
            );
        }

        // 关闭思考
        let config = minimal()
            .reasoning(ReasoningOptions::disabled())
            .build()
            .unwrap();
        assert_eq!(
            config.reasoning().and_then(|value| value.enabled),
            Some(false)
        );
    }

    #[test]
    fn max_duration_boundaries() {
        assert_eq!(
            minimal().max_duration(Duration::ZERO).build().unwrap_err(),
            ConfigError::ZeroMaxDuration
        );
        assert_eq!(
            minimal()
                .max_duration(Duration::from_millis(1))
                .build()
                .unwrap()
                .max_duration(),
            Some(Duration::from_millis(1))
        );
        assert_eq!(minimal().build().unwrap().max_duration(), None);
    }

    #[test]
    fn retry_policy_validation() {
        assert_eq!(
            RetryPolicy::new(0).unwrap_err(),
            ConfigError::ZeroRetryAttempts
        );

        let policy = RetryPolicy::new(3).unwrap();
        assert_eq!(policy.max_attempts().get(), 3);
        assert_eq!(
            policy.initial_backoff(),
            Duration::from_millis(DEFAULT_RETRY_INITIAL_BACKOFF_MS)
        );

        assert_eq!(
            RetryPolicy::new(2)
                .unwrap()
                .with_backoff(Duration::ZERO, Duration::from_secs(1))
                .unwrap_err(),
            ConfigError::ZeroRetryBackoff
        );
        assert_eq!(
            RetryPolicy::new(2)
                .unwrap()
                .with_backoff(Duration::from_secs(2), Duration::from_secs(1))
                .unwrap_err(),
            ConfigError::InvalidRetryBackoff {
                initial_ms: 2_000,
                max_ms: 1_000
            }
        );

        // 配置集成
        let config = minimal()
            .retry(RetryPolicy::new(3).unwrap())
            .build()
            .unwrap();
        assert_eq!(
            config.retry().map(|policy| policy.max_attempts().get()),
            Some(3)
        );
    }

    #[test]
    fn backoff_growth_is_exponential_and_capped() {
        let policy = RetryPolicy::new(10)
            .unwrap()
            .with_backoff(Duration::from_millis(100), Duration::from_millis(800))
            .unwrap();
        assert_eq!(policy.backoff_for(1), Duration::from_millis(100));
        assert_eq!(policy.backoff_for(2), Duration::from_millis(200));
        assert_eq!(policy.backoff_for(3), Duration::from_millis(400));
        assert_eq!(policy.backoff_for(4), Duration::from_millis(800));
        // 封顶与溢出安全
        assert_eq!(policy.backoff_for(5), Duration::from_millis(800));
        assert_eq!(policy.backoff_for(u32::MAX), Duration::from_millis(800));
    }
}
