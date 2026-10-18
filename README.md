<div align="center">

# mycelium

**Rust agent SDK · tool calling · MCP · sessions · streaming**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](https://github.com/oxroot-crypto/mycelium/blob/main/README.en.md#-license)
[![CI](https://img.shields.io/github/actions/workflow/status/oxroot-crypto/mycelium/ci.yml?style=flat-square)](https://github.com/oxroot-crypto/mycelium/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-architecture-blue?style=flat-square)](https://github.com/oxroot-crypto/mycelium/blob/main/docs/architecture/overview.md)

</div>

---

[English](README.en.md) | 简体中文

> **状态：v1.1.0（公共 API 已冻结）**。执行循环、并发工具、权限策略、重试与降级、MCP（工具 / 资源 / 提示模板，
> stdio 与 HTTP）、思考过程、多轮会话、子 Agent 编排、结构化事件流与文本向量化（Embedding）均已可用，
> 并经 DeepSeek V4.1-Flash（聊天）与 OpenRouter（Embedding）真实联调验证；
> 版本历史见 [CHANGELOG.md](CHANGELOG.md)，后续计划见[路线图](docs/architecture/roadmap.md)。

面向 Rust 的 agent SDK：**分层通用核心**。内核不假设产品形态，同一套抽象既可嵌入应用（框架形态），
也可演进为本地 Agent 运行时（服务形态）。

**目录**

- [功能特性](#-功能特性)
- [架构](#-架构)
- [安装](#-安装)
- [快速开始](#-快速开始)
- [用法](#-用法)
- [配置](#-配置)
- [开发](#-开发)
- [路线图](#-路线图)
- [贡献](#-贡献)
- [许可证](#-许可证)
- [致谢](#-致谢)

## ✨ 功能特性

- **执行引擎** — 模型 → 工具 → 回灌循环，含步数 / Token / 时长预算、取消与超时
- **流式装配** — 文本、思考链与工具调用分片实时装配（`DeltaAssembler`）
- **MCP 客户端** — stdio 与 streamable HTTP，覆盖工具 / 资源 / 提示模板
- **七个 SPI 扩展点** — `ModelProvider`、`Tool`、`ToolPolicy`、`SessionStore`、`Memory`、`RunHooks`、`EmbeddingProvider`
- **文本向量化** — Embedding SPI 与 OpenAI 兼容 `/embeddings`（批量、`dimensions` 降维、请求与响应双重校验）
- **工具生态** — JSON Schema 入参校验、结构化工具结果、并发保序回灌、`#[tool]` 派生宏
- **多轮与多 Agent** — 会话持久化 / 恢复；`AgentTool` 子 Agent 编排
- **可观测** — 结构化事件流（`seq` 连续）、tracing span、原始交互日志留存

## 🧩 架构

```text
mycelium-agent (facade: re-exports + prelude)
   ├── mycelium-agent-runtime (execution loop / events / sessions / hooks)
   │      ├── mycelium-agent-model (ModelProvider / EmbeddingProvider SPI / streaming assembler)
   │      └── mycelium-agent-tools (Tool SPI / registry / validation / policy)
   ├── mycelium-agent-provider-openai (OpenAI-compatible provider, DeepSeek ready)
   ├── mycelium-agent-mcp (MCP client: stdio + HTTP, tools / resources / prompts)
   ├── mycelium-agent-macros (#[tool] derive macro)
   └── mycelium-agent-core (messages / tool protocol / events / state machine / errors / config)
```

> 包名与导入名解耦：crates.io 包名为 `mycelium-agent-*`，代码导入名保持 `mycelium_*`
> （`[lib] name` 固定；例如依赖 `mycelium-agent-core`，代码仍写 `use mycelium_core::…`）。

| crate（crates.io 包名） | 职责 |
| :--- | :--- |
| `mycelium-agent-core` | 领域内核：标识、消息（含思考块）、工具协议、事件、运行状态机、错误分类矩阵、配置 |
| `mycelium-agent-model` | 模型提供方抽象（SPI）：`ModelProvider` / `EmbeddingProvider` / `ModelRequest` / `ModelDelta` / 流式装配器 |
| `mycelium-agent-tools` | 工具抽象（SPI）、注册表、入参校验（JSON Schema 子集）、权限策略、`#[tool]` 宏 |
| `mycelium-agent-runtime` | 执行引擎：`AgentRuntime` / 并发工具 / 预算 / 取消 / `AgentRun` / `Session` / `RunHooks` |
| `mycelium-agent-macros` | 过程宏：`#[tool]` 由函数生成工具实现与 JSON Schema |
| `mycelium-agent-mcp` | MCP 客户端：stdio 与 streamable HTTP；工具 / 资源 / 提示模板 |
| `mycelium-agent-provider-openai` | OpenAI 兼容 Provider（Chat Completions、流式 SSE、思考链、`/embeddings` 向量化） |
| `mycelium-agent` | 门面：统一 re-export 与 `prelude`（可选 feature：`provider-openai`） |

延伸阅读：

- [`docs/architecture/overview.md`](docs/architecture/overview.md) —— 架构总览与关键决策
- [`docs/architecture/lifecycle.md`](docs/architecture/lifecycle.md) —— 运行生命周期状态机
- [`docs/spec/core.md`](docs/spec/core.md) —— 形式化约束矩阵（FC-Matrix，单一真实来源）
- [`docs/integration/`](docs/integration/) —— 真实联调原始交互日志（DeepSeek × MCP，可追溯 / 可重放）
- [`docs/architecture/api-freeze.md`](docs/architecture/api-freeze.md) —— 公共 API 冻结评审与变更政策
- [`docs/architecture/release-process.md`](docs/architecture/release-process.md) —— 发布流程与检查单

## 📦 安装

### 前置要求

| 依赖 | 最低版本 | 说明 |
| :--- | :--- | :--- |
| Rust（stable） | 1.97 | MSRV，已在 stable 1.97.1 验证；通过 [rustup](https://rustup.rs) 安装 |
| Node.js | — | 仅 MCP 示例需要（通过 `npx` 启动 MCP 服务） |
| 模型 API Key | — | 仅真实模型示例需要（见[配置](#-配置)的环境变量） |

### 作为依赖引入

```toml
# Cargo.toml
[dependencies]
# crates.io 包名 mycelium-agent；Rust 导入名固定为 mycelium（use mycelium::prelude::*;）
mycelium-agent = "1.1"
# 可选：OpenAI 兼容 Provider；或改为启用门面 feature（见下）
mycelium-agent-provider-openai = "1.1"
```

```toml
# 等价写法：只依赖门面，用 feature 打开 Provider
mycelium-agent = { version = "1.1", features = ["provider-openai"] }
```

跟随仓库 `main` 的最新开发版本可用 git 依赖：`mycelium-agent = { git = "https://github.com/oxroot-crypto/mycelium" }`。

## 🚀 快速开始

1. 克隆仓库

```bash
git clone https://github.com/oxroot-crypto/mycelium.git
cd mycelium
```

2. 构建 workspace

```bash
cargo build --workspace
```

3. 运行内置示例

```bash
cargo run -p mycelium-agent --example basic
```

预期输出：

```text
装配完成：provider=echo，tools=1
运行完成：steps=1，usage=Usage { input_tokens: 0, output_tokens: 0, reasoning_tokens: 0 }，消息数=1
```

## 📖 用法

### 嵌入应用：最小 Provider 与工具

需要 `mycelium-agent`（导入名 `mycelium`）、`serde_json` 与 `tokio`（`macros`、`rt-multi-thread`）依赖，可作为 `src/main.rs` 直接运行：

```rust
use std::sync::Arc;

use mycelium::prelude::*;

/// 最小 Provider：返回固定文本（演示接线）。
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
            ModelDelta::TextDelta { text: "pong".to_owned() },
            ModelDelta::Finished { stop_reason: StopReason::EndTurn },
        ]))
    }
}

/// 最小工具：回显入参。
struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            ToolName::new("echo").expect("合法工具名"),
            "回显输入参数",
            serde_json::json!({ "type": "object" }),
        )
        .expect("合法 JSON Schema")
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

    // 执行循环：模型回显 "pong"，一次请求即完成。
    let outcome = agent.run(RunInput::from_user_text("ping")).await?;
    assert_eq!(outcome.steps, 1);
    assert_eq!(outcome.messages.len(), 1);

    Ok(())
}
```

### 接入真实模型（OpenAI 兼容）

```bash
cp .env.example .env    # 填入 DEEPSEEK_API_KEY（.env 已在 .gitignore 中）
cargo run -p mycelium-agent-provider-openai --example deepseek
```

预期输出（节选）：

```text
网关：https://api.deepseek.com/v1，模型：deepseek-flash
...
✅ 工具调用往返验证通过
```

- `OpenAiProvider::from_env()` 读取 `OPENAI_API_KEY`（必填）与 `OPENAI_BASE_URL`
  （默认 `https://api.openai.com/v1`，兼容 Ollama / vLLM 等网关）；用法示例见
  [`crates/mycelium-provider-openai/src/lib.rs`](crates/mycelium-provider-openai/src/lib.rs) 的 rustdoc。
- 思考过程：`AgentConfig::reasoning` 控制 `thinking` / `reasoning_effort`；思考内容以 `Thinking`
  内容块与 `ModelThinkingDelta` 事件暴露，`Usage.reasoning_tokens` 统计思考 Token，且**不会**回传给模型。

### 接入 MCP 服务

```rust,no_run
use std::sync::Arc;

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stdio 子进程；HTTP 端点改用 McpClient::connect_http("https://host/mcp")
    let client = Arc::new(
        McpClient::spawn(
            "npx",
            &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
        )
        .await?,
    );
    let tools = client.tools().await?; // Arc<dyn Tool>，可直接注册进 AgentRuntime
    println!("工具数：{}", tools.len());
    let resources = client.list_resources().await?; // 资源与提示模板同样可枚举
    println!("资源数：{}", resources.len());
    client.shutdown().await?;
    Ok(())
}
```

### 多轮会话与子 Agent

```rust,no_run
use std::sync::Arc;

use mycelium::prelude::*;

// 多轮会话：历史自动携带，成功后消息追加回 Session
#[allow(dead_code)]
async fn multi_turn(
    runtime: &AgentRuntime,
    session: &mut Session,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime
        .run_session(session, RunInput::from_user_text("请记住数字 7"))
        .await?;
    // 持久化：内存 / 文件实现（SPI 见 SessionStore）
    let store = FileSessionStore::new("/tmp/mycelium-sessions");
    store.save(session).await?;
    Ok(())
}

// 子 Agent：把子运行时包装成工具，注册进父运行时
#[allow(dead_code)]
fn sub_agent(child: Arc<AgentRuntime>) -> Result<Arc<dyn Tool>, Box<dyn std::error::Error>> {
    let name = ToolName::new("ask_child")?;
    Ok(Arc::new(AgentTool::new(name, "sub agent", child)))
}
```

### 文本向量化（Embedding）

向量化 SPI（`EmbeddingProvider`）与领域类型构造即校验；`mycelium-agent-provider-openai` 的
`OpenAiProvider` 同时实现该 SPI（调用 OpenAI 兼容 `/embeddings`，支持批量与 `dimensions` 降维）：

```rust
use mycelium::prelude::*;

#[allow(dead_code)]
async fn embed_texts(provider: &dyn EmbeddingProvider) -> Result<(), Box<dyn std::error::Error>> {
    // 构造即校验：1..=2048 条输入、逐条非空白、维度非零。
    let request = EmbeddingRequest::new(
        ModelId::new("text-embedding-3-small")?,
        vec!["mycelium".to_owned(), "agent SDK".to_owned()],
    )?
    .with_dimensions(256)?; // 可选：降维；不支持该参数的模型会返回提供方错误

    let response: EmbeddingResponse = provider.embed(request).await?;
    // embeddings[i].index == i（按输入顺序对齐）；model / usage 可能缺失。
    println!("向量维度：{}", response.embeddings[0].vector.len());
    Ok(())
}
```

响应解析会逐条校验（条数 / `index` 排列 / 非空向量 / 维度一致 / 数值有限），任何结构违约显式返回
`ModelError::InvalidResponse`，不会静默容忍。

## ⚙️ 配置

`AgentConfig::builder()` 支持的全部选项（键名为真实方法名）：

| 选项 | 类型 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- |
| `model` | `ModelId` | 必填 | 模型标识，如 `deepseek-flash` |
| `system_prompt` | `String` | 无 | 系统提示 |
| `max_steps` | `u32`（≥ 1） | `8` | 单次运行最大步数 |
| `request_timeout` | `Duration`（> 0） | `60s` | 单次模型请求超时 |
| `max_output_tokens` | `u64`（≥ 1） | 无（提供方默认） | 输出 Token 上限 |
| `temperature` | `f32` ∈ [0, 2] | 无（提供方默认） | 采样温度 |
| `tool_choice` | `ToolChoice` | `Auto` | 工具选择策略 |
| `max_parallel_tool_calls` | `usize`（≥ 1） | `1`（顺序执行） | 工具并发上限，结果按调用顺序回灌 |
| `tool_timeout` | `Duration`（> 0） | 无 | 单个工具调用超时 |
| `reasoning` | `ReasoningOptions` | 无（提供方默认） | 思考模式（`thinking` / `reasoning_effort`） |
| `max_duration` | `Duration`（> 0） | 无 | 单次运行最长时长 |
| `retry` | `RetryPolicy` | 无（不重试） | 可重试模型错误的指数退避（默认 500ms 起 / 封顶 8s） |

示例使用的环境变量（模板见 [`.env.example`](.env.example)）：

| 变量 | 默认值 | 说明 |
| :--- | :--- | :--- |
| `DEEPSEEK_API_KEY` | 必填 | DeepSeek 联调示例的 API Key |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com/v1` | 网关地址 |
| `DEEPSEEK_MODEL` | `deepseek-flash` | 模型名 |
| `OPENAI_API_KEY` | 必填 | `OpenAiProvider::from_env()` 使用 |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | `OpenAiProvider::from_env()` 使用 |
| `OPENROUTER_API_KEY` | 场景 7 必填 | OpenRouter 网关（Embedding 真实联调；DeepSeek 无 embedding 端点） |
| `OPENROUTER_BASE_URL` | `https://openrouter.ai/api/v1` | OpenRouter 网关地址 |
| `OPENROUTER_EMBEDDING_MODEL` | `openai/text-embedding-3-small` | 向量化模型（OpenRouter 模型名） |

## 🧪 开发

```bash
git clone https://github.com/oxroot-crypto/mycelium.git
cd mycelium
cargo build --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo deny check        # 依赖审计（安全公告 / 许可 / 来源）；安装：cargo install cargo-deny
```

- MSRV：Rust 1.97（`rust-version`，已在 stable 1.97.1 验证）。
- CI：[`ci.yml`](.github/workflows/ci.yml)（fmt / clippy / test / doc / MSRV / 依赖审计）与
  [`mutants.yml`](.github/workflows/mutants.yml)（每周变异测试）。
- 变更纪律：公共行为变更必须先更新 `docs/spec/core.md`（契约优先），再改测试，最后改实现。

## 🗺️ 路线图

- 已发布：v0.1 → v1.1 全部里程碑（架构骨架 → API 冻结与 crates.io 首发 → 文本向量化），tag 与快照表见
  [`docs/architecture/roadmap.md`](docs/architecture/roadmap.md)。
- 进行中（v1.x）：MCP 采样回调与服务端通知订阅、JSON Schema 剩余关键字、外部记忆后端。

## 🤝 贡献

- 提交前确保 `cargo test --workspace --all-features`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`
  与 `cargo fmt --all --check` 全绿（CI 另含 MSRV 与 `cargo deny`）。
- 公共行为变更遵循「契约优先」：先更新 `docs/spec/core.md`，再改测试与实现；破坏性变更走
  `docs/architecture/rfc/` 评审。
- 提交信息使用 Conventional Commits；新增依赖需在 `docs/architecture/overview.md` §9 登记理由。

## 📄 许可证

采用 **MIT OR Apache-2.0** 双许可（任选其一）：

- [LICENSE-MIT](LICENSE-MIT)
- [LICENSE-APACHE](LICENSE-APACHE)

## 🙏 致谢

- 本仓库的代码与文档**全部**由 AI 生成：**deepseek-v4.1-flash**；详见 [DISCLOSURE.md](DISCLOSURE.md)。
