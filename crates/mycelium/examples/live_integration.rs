//! 完整真实联调驱动：DeepSeek V4.1-Flash ×（MCP 文件系统服务）。
//!
//! 产出可追溯的原始交互日志（全部落盘保留，见 `docs/integration/`）：
//! - `transcript.log`：tracing 全量日志（HTTP 请求体、SSE 原始行、MCP JSON-RPC 双向原文、
//!   运行时生命周期）；
//! - `scenario*_events.jsonl`：`AgentEvent` 全量事件流（一行一个 JSON，`seq` 连续）；
//! - `scenario*_outcome.json`：最终消息（含 `thinking` 块）、用量与步数；
//! - `report.md`：环境、场景与断言结果汇总。
//!
//! 隐私说明：`Authorization` 请求头**不进入日志**（仅请求体与响应行落盘）。
//!
//! 前置：仓库根目录 `.env` 配置 `DEEPSEEK_API_KEY`；本机可用 `npx`；网络可达 api.deepseek.com。
//!
//! 运行：
//! `cargo run -p mycelium --features provider-openai --example live_integration -- docs/integration/2026-10-16`
// 示例代码可放宽 unwrap / expect 限制（规范允许：测试与示例代码）。
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use mycelium::mcp::McpClient;
use mycelium::prelude::*;
use mycelium::provider_openai::OpenAiProvider;
use mycelium::tools::tool;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;

/// 默认网关地址（DeepSeek 官方）。
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";
/// 默认模型（DeepSeek-V4.1-Flash）。
const DEFAULT_MODEL: &str = "deepseek-flash";

/// 加法工具（演示 `#[tool]` 派生宏）。
#[tool]
async fn add(a: i64, b: i64) -> Result<i64, String> {
    Ok(a + b)
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

/// 装配运行时（思考模式：显式启用、低强度）。
fn build_agent(
    provider: Arc<dyn ModelProvider>,
    tools: Vec<Arc<dyn Tool>>,
) -> Result<AgentRuntime, Box<dyn std::error::Error>> {
    let mut builder = AgentRuntime::builder()
        .config(
            AgentConfig::builder()
                .model(ModelId::new(DEFAULT_MODEL)?)
                .system_prompt("你是简洁的助手，会使用工具完成任务。")
                .max_steps(8)
                .request_timeout(Duration::from_secs(180))
                .reasoning(ReasoningOptions::enabled().with_effort("low"))
                .build()?,
        )
        .provider(provider);
    for tool in tools {
        builder = builder.tool(tool);
    }
    Ok(builder.build()?)
}

/// 执行一个场景：事件流全量落盘，返回结果与事件数量。
async fn run_scenario(
    name: &str,
    runtime: &AgentRuntime,
    prompt: &str,
    output_dir: &Path,
) -> Result<(RunOutcome, usize), Box<dyn std::error::Error>> {
    println!("── 场景 {name} ──");
    let (mut events, result) = runtime.run_events(RunInput::from_user_text(prompt)).split();

    let events_path = output_dir.join(format!("{name}_events.jsonl"));
    let mut events_file = BufWriter::new(File::create(&events_path)?);
    let mut event_count = 0_usize;
    while let Some(event) = events.next().await {
        writeln!(events_file, "{}", serde_json::to_string(&event)?)?;
        event_count += 1;
    }
    events_file.flush()?;

    let outcome = result.await_result().await?;
    let snapshot = serde_json::json!({
        "steps": outcome.steps,
        "usage": outcome.usage,
        "messages": outcome.messages,
    });
    std::fs::write(
        output_dir.join(format!("{name}_outcome.json")),
        serde_json::to_string_pretty(&snapshot)?,
    )?;

    println!(
        "   steps={} usage={:?} 事件 {} 条",
        outcome.steps, outcome.usage, event_count
    );
    Ok((outcome, event_count))
}

/// 提取最后一条助手消息中指定类型块的文本。
fn assistant_block_text(
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "docs/integration/live".to_owned());
    let output_dir = std::path::PathBuf::from(output_dir);
    std::fs::create_dir_all(&output_dir)?;

    // 1) tracing 全量落盘（HTTP 请求体 / SSE 原始行 / MCP JSON-RPC 双向原文 / 运行时生命周期）
    let log_file = File::create(output_dir.join("transcript.log"))?;
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false)
        .with_target(true)
        .with_max_level(LevelFilter::TRACE)
        .finish()
        .init();
    println!("输出目录：{}", output_dir.display());

    // 2) 配置与 Provider（单例复用）
    let dotenv = read_dotenv(".env");
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .or_else(|| dotenv.get("DEEPSEEK_API_KEY").cloned())
        .expect("缺少 DEEPSEEK_API_KEY：请写入仓库根目录 .env 或导出环境变量");
    let base_url = std::env::var("DEEPSEEK_BASE_URL")
        .ok()
        .or_else(|| dotenv.get("DEEPSEEK_BASE_URL").cloned())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
    let provider: Arc<dyn ModelProvider> = Arc::new(
        OpenAiProvider::builder()
            .api_key(api_key)
            .base_url(&base_url)
            .build()?,
    );
    println!("网关：{base_url}，模型：{DEFAULT_MODEL}");

    let mut report_items: Vec<String> = Vec::new();

    // 3) 场景 1：文本对话与思考链
    let runtime = build_agent(Arc::clone(&provider), vec![])?;
    let (outcome, events) = run_scenario(
        "scenario1_text_thinking",
        &runtime,
        "用一句话介绍你自己，并说明你是什么模型。",
        &output_dir,
    )
    .await?;
    let text = assistant_block_text(&outcome, |block| match block {
        ContentBlock::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let thinking = assistant_block_text(&outcome, |block| match block {
        ContentBlock::Thinking { text } => Some(text.as_str()),
        _ => None,
    });
    assert!(!text.is_empty(), "场景 1：最终回复不能为空");
    assert!(
        !thinking.is_empty(),
        "场景 1：应观察到思考链（Thinking 块）"
    );
    assert!(
        outcome.usage.reasoning_tokens > 0,
        "场景 1：reasoning_tokens 应大于 0"
    );
    report_items.push(format!(
        "| 1 | 文本对话 + 思考链 | steps={}，事件={}，usage={:?}，思考 {} 字，回复：{} |",
        outcome.steps,
        events,
        outcome.usage,
        thinking.chars().count(),
        text.replace('\n', " ")
    ));

    // 4) 场景 2：工具调用往返（#[tool] 派生宏生成的 add）
    let runtime = build_agent(Arc::clone(&provider), vec![add_tool()])?;
    let (outcome, events) = run_scenario(
        "scenario2_tool_round_trip",
        &runtime,
        "请使用 add 工具计算 21 + 21，然后给出结果。",
        &output_dir,
    )
    .await?;
    let used_tool = outcome.messages.iter().any(|message| {
        message
            .content()
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
    });
    let text = assistant_block_text(&outcome, |block| match block {
        ContentBlock::Text { text } => Some(text.as_str()),
        _ => None,
    });
    assert!(used_tool, "场景 2：应发生工具调用往返");
    assert!(text.contains("42"), "场景 2：最终回复应包含 42：{text}");
    report_items.push(format!(
        "| 2 | 工具调用往返（add） | steps={}，事件={}，usage={:?}，回复：{} |",
        outcome.steps,
        events,
        outcome.usage,
        text.replace('\n', " ")
    ));

    // 5) 场景 3：MCP 文件系统（DeepSeek × MCP）
    let sandbox = std::env::temp_dir().join("mycelium-mcp-demo");
    std::fs::create_dir_all(&sandbox)?;
    let target = sandbox.join("hello.txt");
    std::fs::write(&target, "mycelium × MCP：你好，世界！\n")?;
    let client = Arc::new(
        McpClient::spawn(
            "npx",
            &[
                "--registry=https://registry.npmjs.org",
                "-y",
                "@modelcontextprotocol/server-filesystem",
                sandbox.to_str().ok_or("沙箱路径不是合法 UTF-8")?,
            ],
        )
        .await?,
    );
    let info = client.server_info().clone();
    let tools = client.tools().await?;
    println!(
        "MCP 服务：{} v{}（协议 {}），工具 {} 个",
        info.name,
        info.version,
        info.protocol_version,
        tools.len()
    );

    let runtime = build_agent(Arc::clone(&provider), tools)?;
    let prompt = format!(
        "请用可用的 MCP 工具读取文件 {} 的内容，并原样告诉我文件里写了什么。",
        target.display()
    );
    let (outcome, events) =
        run_scenario("scenario3_mcp_filesystem", &runtime, &prompt, &output_dir).await?;
    let text = assistant_block_text(&outcome, |block| match block {
        ContentBlock::Text { text } => Some(text.as_str()),
        _ => None,
    });
    let called_tools: Vec<String> = outcome
        .messages
        .iter()
        .flat_map(|message| message.content())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { name, .. } => Some(name.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert!(
        !called_tools.is_empty(),
        "场景 3：模型应通过 MCP 工具读取文件"
    );
    assert!(
        text.contains("mycelium"),
        "场景 3：最终回复应包含文件内容：{text}"
    );
    report_items.push(format!(
        "| 3 | MCP 文件系统（DeepSeek × MCP） | steps={}，事件={}，usage={:?}，调用工具={:?} |",
        outcome.steps, events, outcome.usage, called_tools
    ));
    client.shutdown().await?;

    // 6) 汇总报告
    let generated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let report = format!(
        "# 真实联调报告（live_integration）\n\n\
         - 网关：`{base_url}`\n\
         - 模型：`{DEFAULT_MODEL}`（DeepSeek-V4.1-Flash）\n\
         - 生成时间（Unix 毫秒）：`{generated_at_ms}`\n\
         - 隐私：`Authorization` 请求头不进入日志（仅请求体与响应行落盘）\n\
         - 重放：`cargo run -p mycelium --features provider-openai --example live_integration -- <目录>`\n\n\
         ## 场景与结果\n\n\
         | # | 场景 | 结果 |\n|---|------|------|\n{}\n\n\
         ## 产物\n\n\
         | 文件 | 内容 |\n|---|---|\n\
         | `transcript.log` | tracing 全量日志：HTTP 请求体、SSE 原始行、MCP JSON-RPC 双向原文、运行时生命周期 |\n\
         | `scenario*_events.jsonl` | `AgentEvent` 全量事件流（`seq` 连续，一行一个 JSON） |\n\
         | `scenario*_outcome.json` | 最终消息（含 `thinking` 块）、用量与步数 |\n",
        report_items.join("\n")
    );
    std::fs::write(output_dir.join("report.md"), report)?;

    println!("✅ 全部场景通过，日志已写入 {}", output_dir.display());
    Ok(())
}
