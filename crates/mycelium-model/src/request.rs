//! 模型请求。

use mycelium_core::CancellationToken;
use mycelium_core::ids::ModelId;
use mycelium_core::message::Message;
use mycelium_core::tool::{ToolChoice, ToolSpec};

/// 一次模型调用的完整请求。
///
/// 由运行时从 [`mycelium_core::AgentConfig`] 与消息历史构造；字段合法性由构造方保证
/// （`max_output_tokens >= 1`、`temperature ∈ [0.0, 2.0]` 等，见 `AgentConfig` 校验）。
#[derive(Debug, Clone)]
pub struct ModelRequest {
    /// 目标模型。
    pub model: ModelId,
    /// 系统提示（可选）。
    pub system_prompt: Option<String>,
    /// 对话消息（不含系统提示）。
    pub messages: Vec<Message>,
    /// 可用工具规格。
    pub tools: Vec<ToolSpec>,
    /// 工具选择策略。
    pub tool_choice: ToolChoice,
    /// 输出 Token 上限（`None` 表示由提供方默认）。
    pub max_output_tokens: Option<u64>,
    /// 采样温度（`None` 表示提供方默认）。
    pub temperature: Option<f32>,
    /// 取消令牌：触发后提供方应尽快终止请求并让流以
    /// [`mycelium_core::ModelError::Cancelled`] 结束。
    pub cancel: CancellationToken,
}

impl ModelRequest {
    /// 以最小必填项构造请求（其余字段使用默认策略）。
    pub fn new(model: ModelId, messages: Vec<Message>) -> Self {
        Self {
            model,
            system_prompt: None,
            messages,
            tools: Vec::new(),
            tool_choice: ToolChoice::default(),
            max_output_tokens: None,
            temperature: None,
            cancel: CancellationToken::new(),
        }
    }
}
