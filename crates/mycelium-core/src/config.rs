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
}
