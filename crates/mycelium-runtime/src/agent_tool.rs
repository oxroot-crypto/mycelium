//! 子 Agent 编排：把运行时封装为工具（agent-as-tool）。
//!
//! 语义（FC-POST-190 ~ FC-ERR-073）：
//! - 子运行以 `prompt` 为唯一用户消息执行，使用**独立预算与独立事件流**
//!   （父事件流只包含工具级事件：`ToolCallStarted` / `ToolCallFinished`）；
//! - 子运行的最终助手文本作为工具结果返回；
//! - 子运行失败以错误结果回灌（父运行继续）；取消传播到子运行（父取消可打断子运行）。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use mycelium_core::error::ToolError;
use mycelium_core::ids::ToolName;
use mycelium_core::message::{ContentBlock, Role};
use mycelium_core::tool::ToolSpec;
use mycelium_core::{AgentError, Message};
use mycelium_tools::{Tool, ToolContext, ToolOutput};

use crate::agent::AgentRuntime;
use crate::run::{RunInput, RunOutcome};

/// 子 Agent 工具的固定输入 schema。
const PROMPT_SCHEMA: &str =
    r#"{"type":"object","properties":{"prompt":{"type":"string"}},"required":["prompt"]}"#;

/// 子 Agent 工具（agent-as-tool）。
///
/// 用法：`AgentTool::new(name, description, Arc<child_runtime>)` 注册进父运行时；
/// 父 Agent 即可通过工具调用委托子 Agent 完成子任务。
pub struct AgentTool {
    name: ToolName,
    description: String,
    spec: ToolSpec,
    runtime: Arc<AgentRuntime>,
}

impl AgentTool {
    /// 创建子 Agent 工具。
    ///
    /// # Panics
    /// 内置 schema 为常量对象字面量，构造失败仅可能源于实现缺陷；此处不会 panic
    /// （由编译期常量与测试双重保证）。
    #[allow(
        clippy::expect_used,
        reason = "schema 为常量对象字面量，失败即实现缺陷"
    )]
    pub fn new(name: ToolName, description: impl Into<String>, runtime: Arc<AgentRuntime>) -> Self {
        let description = description.into();
        let spec = ToolSpec::new(
            name.clone(),
            description.clone(),
            serde_json::from_str(PROMPT_SCHEMA).expect("常量 schema 必为合法 JSON"),
        )
        .expect("常量 schema 必为对象");
        Self {
            name,
            description,
            spec,
            runtime,
        }
    }

    /// 工具名。
    pub fn name(&self) -> &ToolName {
        &self.name
    }

    /// 工具描述。
    pub fn description(&self) -> &str {
        &self.description
    }

    /// 子运行时。
    pub fn runtime(&self) -> &Arc<AgentRuntime> {
        &self.runtime
    }
}

impl std::fmt::Debug for AgentTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentTool")
            .field("name", &self.name)
            .field("child_provider", &self.runtime.provider_id())
            .finish()
    }
}

/// 提取运行结果中最后一条助手消息的文本。
fn final_assistant_text(outcome: &RunOutcome) -> String {
    outcome
        .messages
        .iter()
        .rev()
        .find(|message| message.role() == Role::Assistant)
        .map(|message| {
            message
                .content()
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

#[async_trait]
impl Tool for AgentTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(&self, context: &ToolContext, arguments: Value) -> Result<ToolOutput, ToolError> {
        let prompt = arguments
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: self.name.clone(),
                detail: "缺少字符串参数 prompt".to_owned(),
            })?;

        // 取消传播：父运行的取消令牌注入子运行（FC-ERR-072）。
        let input = RunInput {
            messages: vec![Message::user_text(prompt)],
            cancel: context.cancel.clone(),
        };
        match self.runtime.run(input).await {
            Ok(outcome) => Ok(ToolOutput::success(final_assistant_text(&outcome))),
            Err(AgentError::Cancelled) => Err(ToolError::Cancelled {
                tool: self.name.clone(),
            }),
            // 子运行失败不终止父运行：错误结果回灌（FC-ERR-071）。
            Err(error) => Ok(ToolOutput::error(format!("子 Agent 运行失败：{error}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::stub_provider;

    fn agent_tool() -> AgentTool {
        let runtime = AgentRuntime::builder()
            .config(
                mycelium_core::AgentConfig::builder()
                    .model(mycelium_core::ModelId::new("stub").unwrap())
                    .build()
                    .unwrap(),
            )
            .provider(stub_provider())
            .build()
            .unwrap();
        AgentTool::new(
            ToolName::new("ask_child").unwrap(),
            "向子 Agent 提问（单轮）",
            Arc::new(runtime),
        )
    }

    #[test]
    fn spec_shape() {
        let tool = agent_tool();
        let spec = tool.spec();
        assert_eq!(spec.name().as_str(), "ask_child");
        assert_eq!(spec.description(), "向子 Agent 提问（单轮）");
        assert_eq!(
            spec.input_schema(),
            &serde_json::json!({
                "type": "object",
                "properties": { "prompt": { "type": "string" } },
                "required": ["prompt"]
            })
        );
    }

    #[tokio::test]
    async fn missing_prompt_is_rejected() {
        let tool = agent_tool();
        let context = crate::test_support::tool_context("call_1");
        let error = tool
            .call(&context, serde_json::json!({}))
            .await
            .unwrap_err();
        match error {
            ToolError::InvalidArguments { tool: name, detail } => {
                assert_eq!(name.as_str(), "ask_child");
                assert!(detail.contains("prompt"));
            }
            other => panic!("期望 InvalidArguments，实际：{other:?}"),
        }
    }
}
