//! 全栈示例：DeepSeek V4.1-Flash × MCP 文件系统工具 × mycelium 执行循环。
//!
//! 前置：
//! - 仓库根目录 `.env` 配置 `DEEPSEEK_API_KEY`；
//! - 本机可用 `npx`（首次运行会下载官方 MCP 文件系统服务）。
//!
//! 运行：`cargo run -p mycelium --features provider-openai --example agent_deepseek_mcp`
// 示例代码可放宽 unwrap / expect 限制（规范允许：测试与示例代码）。
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use mycelium::mcp::McpClient;
use mycelium::prelude::*;
use mycelium::provider_openai::OpenAiProvider;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1) 准备沙箱与待读取文件
    let sandbox = std::env::temp_dir().join("mycelium-mcp-demo");
    std::fs::create_dir_all(&sandbox)?;
    let hello_path = sandbox.join("hello.txt");
    std::fs::write(&hello_path, "mycelium × MCP：你好，世界！\n")?;
    println!("沙箱目录：{}", sandbox.display());

    // 2) 启动 MCP 文件系统服务并取得工具
    // 注：显式指定公共 registry，避免本机私有镜像配置（可替换为自有镜像）。
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
    let info = client.server_info();
    println!(
        "MCP 服务：{} v{}（协议 {}）",
        info.name, info.version, info.protocol_version
    );
    let tools = client.tools().await?;
    let tool_names: Vec<String> = tools
        .iter()
        .map(|tool| tool.spec().name().as_str().to_owned())
        .collect();
    println!("MCP 工具 {} 个：{}", tools.len(), tool_names.join(", "));

    // 3) 装配 DeepSeek 运行时（思考模式：低强度）
    let dotenv = read_dotenv(".env");
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .or_else(|| dotenv.get("DEEPSEEK_API_KEY").cloned())
        .expect("缺少 DEEPSEEK_API_KEY：请写入仓库根目录 .env 或导出环境变量");
    let provider = OpenAiProvider::builder()
        .api_key(api_key)
        .base_url("https://api.deepseek.com/v1")
        .build()?;
    let mut builder = AgentRuntime::builder()
        .config(
            AgentConfig::builder()
                .model(ModelId::new("deepseek-flash")?)
                .system_prompt("你是简洁的助手，会使用工具完成任务。")
                .max_steps(8)
                .request_timeout(Duration::from_secs(180))
                .reasoning(ReasoningOptions::enabled().with_effort("low"))
                .build()?,
        )
        .provider(Arc::new(provider));
    for tool in tools {
        builder = builder.tool(tool);
    }
    let runtime = builder.build()?;

    // 4) 让模型通过 MCP 工具读取文件
    let prompt = format!(
        "请用可用的 MCP 工具读取文件 {} 的内容，并原样告诉我文件里写了什么。",
        hello_path.display()
    );
    let outcome = runtime.run(RunInput::from_user_text(prompt)).await?;
    println!("steps={} usage={:?}", outcome.steps, outcome.usage);

    let mut final_text = String::new();
    for message in &outcome.messages {
        for block in message.content() {
            match block {
                ContentBlock::ToolUse { name, .. } => println!("  → 调用工具：{name}"),
                ContentBlock::Text { text } => final_text = text.clone(),
                ContentBlock::Thinking { .. } => println!("  → 模型思考（已记录，不回传）"),
                ContentBlock::ToolResult { .. } => {}
            }
        }
    }
    println!("回复：{final_text}");
    if !final_text.contains("mycelium") {
        return Err(format!("最终回复未包含文件内容：{final_text}").into());
    }
    println!("✅ DeepSeek × MCP 全栈验证通过");

    // 5) 关闭 MCP 子进程
    client.shutdown().await?;
    Ok(())
}
