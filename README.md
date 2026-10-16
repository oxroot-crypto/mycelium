# mycelium

面向 Rust 的 agent SDK：**分层通用核心**。内核不假设产品形态，同一套抽象既可嵌入应用
（框架形态），也可演进为本地 Agent 运行时（服务形态）。

> **状态：v0.5。** 执行循环、并发工具、权限策略、`#[tool]` 派生宏、MCP 客户端、思考过程、
> 结构化工具结果、异步钩子、重试与多 Provider 降级、**多轮会话持久化与子 Agent 编排**均已可用，
> 并经 DeepSeek V4.1-Flash 五场景真实联调验证（原始日志见 `docs/integration/`）；会话持久化与
> 子 Agent 编排见 [`docs/architecture/roadmap.md`](docs/architecture/roadmap.md)。

## 架构

```text
mycelium（门面：re-export + prelude）
   ├── mycelium-runtime（执行循环 / 事件流 / Session / RunHooks）
   │      ├── mycelium-model（ModelProvider SPI / 流式装配器）
   │      └── mycelium-tools（Tool SPI / 注册表 / 入参校验 / 权限策略 / #[tool] 宏再导出）
   ├── mycelium-provider-openai（OpenAI 兼容 Provider，含 DeepSeek）
   ├── mycelium-mcp（MCP 客户端：stdio JSON-RPC / tools）
   ├── mycelium-macros（#[tool] 派生宏）
   └── mycelium-core（消息 / 工具协议 / 事件 / 状态机 / 错误 / 配置）
```

| crate | 职责 |
| :--- | :--- |
| `mycelium-core` | 领域内核：标识、消息（含思考块）、工具协议、事件、运行状态机、错误分类矩阵、配置 |
| `mycelium-model` | 模型提供方抽象（SPI）：`ModelProvider` / `ModelRequest` / `ModelDelta` / 流式装配器 |
| `mycelium-tools` | 工具抽象（SPI）、注册表、入参校验（JSON Schema 子集）、权限策略、`#[tool]` 宏 |
| `mycelium-runtime` | 执行引擎：`AgentRuntime` / 并发工具 / 预算 / 取消 / `AgentRun` / `Session` / `RunHooks` |
| `mycelium-macros` | 过程宏：`#[tool]` 由函数生成工具实现与 JSON Schema |
| `mycelium-mcp` | MCP 客户端：stdio 子进程、握手、分页 `tools/list`、`tools/call` 代理 |
| `mycelium-provider-openai` | OpenAI 兼容 Provider（Chat Completions、流式 SSE、思考链） |
| `mycelium` | 门面：统一 re-export 与 `prelude`（可选 feature：`provider-openai`） |

## 快速开始

```rust
use std::sync::Arc;

use mycelium::prelude::*;

/// 最小 Provider：返回固定文本（演示接线；真实实现见 roadmap v0.2）。
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

    // v0.2：执行循环真实可用 —— 模型回显 "pong"，一次请求即完成。
    let outcome = agent.run(RunInput::from_user_text("ping")).await?;
    assert_eq!(outcome.steps, 1);
    assert_eq!(outcome.messages.len(), 1);

    Ok(())
}
```

## 接入真实模型（OpenAI 兼容）

```bash
cargo add mycelium-provider-openai   # 或使用门面 feature：mycelium = { features = ["provider-openai"] }
```

```bash
export OPENAI_API_KEY=sk-...          # 必填
export OPENAI_BASE_URL=http://localhost:11434/v1   # 可选：兼容 Ollama / vLLM 等网关
```

`OpenAiProvider::from_env()` 读取上述环境变量；用法示例见
[`crates/mycelium-provider-openai/src/lib.rs`](crates/mycelium-provider-openai/src/lib.rs) 的 rustdoc。

DeepSeek V4.1-Flash 真实联调（纯文本 / 工具往返 / 流式事件 / 思考链）：

```bash
cargo run -p mycelium-provider-openai --example deepseek
```

思考过程支持：`AgentConfig::reasoning`（`thinking` / `reasoning_effort`）控制思考模式；
思考内容以 `Thinking` 内容块与 `ModelThinkingDelta` 事件暴露，`Usage.reasoning_tokens`
给出思考 Token 统计，且思考内容**不会**回传给模型。

## 文档

- [`docs/architecture/overview.md`](docs/architecture/overview.md) —— 架构总览与关键决策
- [`docs/architecture/lifecycle.md`](docs/architecture/lifecycle.md) —— 运行生命周期状态机
- [`docs/spec/core.md`](docs/spec/core.md) —— 形式化约束矩阵（FC-Matrix，单一真实来源）
- [`docs/architecture/roadmap.md`](docs/architecture/roadmap.md) —— 路线图
- [`docs/integration/`](docs/integration/) —— 真实联调原始交互日志（DeepSeek × MCP，可追溯 / 可重放）

## 开发

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

- MSRV：Rust 1.97（`rust-version`，已在 stable 1.97.1 验证）。
- 变更纪律：公共行为变更必须先更新 `docs/spec/core.md`（契约优先），再改测试，最后改实现。

## 许可证

待定（发布前决策；Rust 生态惯例为 `MIT OR Apache-2.0`）。
