//! 端到端接线示例：构建一个最小 Agent 并执行一次运行。
//!
//! 运行方式：`cargo run -p mycelium --example basic`
// 示例代码可放宽 unwrap / expect 限制（规范允许：测试与示例代码）。
#![allow(clippy::expect_used)]

use std::sync::Arc;

use mycelium::prelude::*;

/// 最小 Provider：返回固定文本（真实 Provider 见 roadmap v0.2）。
struct EchoProvider;

#[async_trait]
impl ModelProvider for EchoProvider {
    fn id(&self) -> &str {
        "echo"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_streaming: true,
            ..Default::default()
        }
    }

    async fn stream(&self, _request: ModelRequest) -> Result<ModelStream, ModelError> {
        Ok(stream_from_deltas(vec![
            ModelDelta::TextDelta {
                text: "pong".to_owned(),
            },
            ModelDelta::Finished {
                stop_reason: StopReason::EndTurn,
            },
        ]))
    }
}

/// 最小工具：回显入参。
struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("echo").expect("工具名 echo 合法"),
            "回显输入参数",
            serde_json::json!({ "type": "object" }),
        )
        .expect("schema 为 JSON 对象")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success(arguments.to_string()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AgentConfig::builder()
        .model(ModelId::new("mycelium-echo")?)
        .max_steps(4)
        .build()?;

    let agent = AgentRuntime::builder()
        .config(config)
        .provider(Arc::new(EchoProvider))
        .tool(Arc::new(EchoTool))
        .build()?;

    println!(
        "装配完成：provider={}，tools={}",
        agent.provider_id(),
        agent.tools().len()
    );

    let outcome = agent.run(RunInput::from_user_text("ping")).await?;
    println!(
        "运行完成：steps={}，usage={:?}，消息数={}",
        outcome.steps,
        outcome.usage,
        outcome.messages.len()
    );

    Ok(())
}
