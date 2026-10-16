//! DeepSeek 真实联调示例（OpenAI 兼容协议）。
//!
//! 前置：仓库根目录 `.env`（或进程环境变量）配置 `DEEPSEEK_API_KEY`；
//! 可选 `DEEPSEEK_BASE_URL`（默认 `https://api.deepseek.com/v1`）、
//! `DEEPSEEK_MODEL`（默认 `deepseek-flash`，即 DeepSeek-V4.1-Flash）。
//!
//! 运行：`cargo run -p mycelium-provider-openai --example deepseek`
// 示例代码可放宽 unwrap / expect 限制（规范允许：测试与示例代码）。
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use mycelium_core::event::AgentEventKind;
use mycelium_core::message::{ContentBlock, Message, Role};
use mycelium_core::{AgentConfig, ModelId, ReasoningOptions, ToolError, ToolName, ToolSpec};
use mycelium_provider_openai::OpenAiProvider;
use mycelium_runtime::{AgentRuntime, RunInput, RunOutcome};
use mycelium_tools::{Tool, ToolContext, ToolOutput, async_trait};

/// 默认网关地址（DeepSeek 官方）。
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";
/// 默认模型（DeepSeek-V4.1-Flash）。
const DEFAULT_MODEL: &str = "deepseek-flash";

/// 加法工具：用于验证函数调用往返。
struct AddTool;

#[async_trait]
impl Tool for AddTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("add").expect("合法工具名"),
            "计算两个整数之和",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "integer" },
                    "b": { "type": "integer" }
                },
                "required": ["a", "b"],
                "additionalProperties": false
            }),
        )
        .expect("合法 schema")
    }

    async fn call(
        &self,
        _context: &ToolContext,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        let tool = ToolName::new("add").expect("合法工具名");
        let a = arguments
            .get("a")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: tool.clone(),
                detail: "缺少整数参数 a".to_owned(),
            })?;
        let b = arguments
            .get("b")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| ToolError::InvalidArguments {
                tool,
                detail: "缺少整数参数 b".to_owned(),
            })?;
        Ok(ToolOutput::success(format!("{}", a + b)))
    }
}

/// 读取 `.env` 文件为键值表（不修改进程环境；兼容 `KEY = value` 形式与引号）。
fn read_dotenv(path: &str) -> HashMap<String, String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let mut values = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !key.is_empty() && !value.is_empty() {
            values.insert(key.to_owned(), value.to_owned());
        }
    }
    values
}

/// 取值优先级：进程环境变量 > `.env` 文件。
fn config_value(dotenv: &HashMap<String, String>, key: &str) -> Option<String> {
    std::env::var(key).ok().or_else(|| dotenv.get(key).cloned())
}

/// 提取最后一条助手消息的文本。
fn final_text(outcome: &RunOutcome) -> String {
    collect_assistant_blocks(outcome, |block| match block {
        ContentBlock::Text { text } => Some(text.as_str()),
        _ => None,
    })
}

/// 提取最后一条助手消息的思考内容。
fn final_thinking(outcome: &RunOutcome) -> String {
    collect_assistant_blocks(outcome, |block| match block {
        ContentBlock::Thinking { text } => Some(text.as_str()),
        _ => None,
    })
}

/// 汇总最后一条助手消息中匹配块的内容。
fn collect_assistant_blocks(
    outcome: &RunOutcome,
    pick: impl Fn(&ContentBlock) -> Option<&str>,
) -> String {
    outcome
        .messages
        .iter()
        .rev()
        .find(|message| message.role() == Role::Assistant)
        .map(|message| {
            message
                .content()
                .iter()
                .filter_map(&pick)
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// 打印消息序列摘要。
fn print_messages(messages: &[Message]) {
    for (index, message) in messages.iter().enumerate() {
        let blocks: Vec<&str> = message
            .content()
            .iter()
            .map(|block| match block {
                ContentBlock::Text { .. } => "text",
                ContentBlock::Thinking { .. } => "thinking",
                ContentBlock::ToolUse { .. } => "tool_use",
                ContentBlock::ToolResult { .. } => "tool_result",
            })
            .collect();
        println!("  [{index}] {:?}: {}", message.role(), blocks.join(", "));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dotenv = read_dotenv(".env");
    let api_key = config_value(&dotenv, "DEEPSEEK_API_KEY")
        .expect("缺少 DEEPSEEK_API_KEY：请写入仓库根目录 .env 或导出环境变量");
    let base_url =
        config_value(&dotenv, "DEEPSEEK_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
    let model = config_value(&dotenv, "DEEPSEEK_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_owned());

    println!("网关：{base_url}，模型：{model}");

    let provider = OpenAiProvider::builder()
        .api_key(api_key)
        .base_url(&base_url)
        .build()?;
    let config = AgentConfig::builder()
        .model(ModelId::new(&model)?)
        .system_prompt("你是简洁的助手，回答尽量不超过两句话。")
        .max_steps(6)
        .request_timeout(Duration::from_secs(120))
        // 思考模式：显式启用并降低强度（演示 reasoning 配置）
        .reasoning(ReasoningOptions::enabled().with_effort("low"))
        .build()?;
    let runtime = AgentRuntime::builder()
        .config(config)
        .provider(Arc::new(provider))
        .tool(Arc::new(AddTool))
        .build()?;

    println!("== 场景 1：纯文本对话 ==");
    let outcome = runtime
        .run(RunInput::from_user_text(
            "用一句话介绍你自己，并说明你是什么模型。",
        ))
        .await?;
    println!("steps={} usage={:?}", outcome.steps, outcome.usage);
    let thinking = final_thinking(&outcome);
    if !thinking.is_empty() {
        println!("思考：{}", thinking,);
    }
    println!("reasoning_tokens={}", outcome.usage.reasoning_tokens);
    println!("回复：{}", final_text(&outcome));

    println!("== 场景 2：工具调用往返（add）==");
    let outcome = runtime
        .run(RunInput::from_user_text(
            "请使用 add 工具计算 21 + 21，然后给出结果。",
        ))
        .await?;
    println!("steps={} usage={:?}", outcome.steps, outcome.usage);
    print_messages(&outcome.messages);

    let used_tool = outcome.messages.iter().any(|message| {
        message
            .content()
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
    });
    let text = final_text(&outcome);
    if !used_tool {
        return Err("未观察到工具调用往返（模型未调用 add）".into());
    }
    if !text.contains("42") {
        return Err(format!("最终回复未包含 42：{text}").into());
    }
    println!("最终回复：{text}");
    println!("✅ 工具调用往返验证通过");

    println!("== 场景 3：事件流（run_events）==");
    let (mut events, result) = runtime
        .run_events(RunInput::from_user_text("说一句简短的再见。"))
        .split();
    let mut text_deltas = 0_usize;
    let mut thinking_deltas = 0_usize;
    let mut total_events = 0_usize;
    while let Some(event) = events.next().await {
        total_events += 1;
        match event.kind {
            AgentEventKind::ModelTextDelta { .. } => text_deltas += 1,
            AgentEventKind::ModelThinkingDelta { .. } => thinking_deltas += 1,
            _ => {}
        }
    }
    let outcome = result.await_result().await?;
    println!(
        "事件总数={total_events}，文本增量={text_deltas}，思考增量={thinking_deltas}，steps={}",
        outcome.steps
    );
    if text_deltas == 0 {
        return Err("未观察到流式文本增量".into());
    }
    println!("✅ 流式事件验证通过");

    Ok(())
}
