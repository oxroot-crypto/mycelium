<div align="center">

# mycelium

**Rust agent SDK · tool calling · MCP · sessions · streaming**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat-square)](https://github.com/oxroot-crypto/mycelium/blob/main/README.en.md#-license)
[![CI](https://img.shields.io/github/actions/workflow/status/oxroot-crypto/mycelium/ci.yml?style=flat-square)](https://github.com/oxroot-crypto/mycelium/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-architecture-blue?style=flat-square)](https://github.com/oxroot-crypto/mycelium/blob/main/docs/architecture/overview.md)

</div>

---

English | [简体中文](README.md)

> **Status: v0.7.0.** Execution loop, concurrent tools, tool policies, retry & fallback, MCP
> (tools / resources / prompts over stdio and HTTP), reasoning, multi-turn sessions, sub-agent
> orchestration and a structured event stream are all available, verified against DeepSeek
> V4.1-Flash; see [CHANGELOG.md](CHANGELOG.md) and the
> [roadmap](docs/architecture/roadmap.md).

A Rust agent SDK built around a **layered, product-agnostic core**: the same abstractions embed
into an application (framework shape) and can grow into a local agent runtime (service shape).

**Table of contents**

- [Features](#-features)
- [Architecture](#-architecture)
- [Installation](#-installation)
- [Quick Start](#-quick-start)
- [Usage](#-usage)
- [Configuration](#-configuration)
- [Development](#-development)
- [Roadmap](#-roadmap)
- [Contributing](#-contributing)
- [License](#-license)

## ✨ Features

- **Execution loop** — model → tools → feedback, with step / token / duration budgets, cancellation and timeouts
- **Streaming assembly** — text, reasoning and tool-call deltas assembled incrementally (`DeltaAssembler`)
- **MCP client** — stdio and streamable HTTP; tools / resources / prompts
- **Six SPI extension points** — `ModelProvider`, `Tool`, `ToolPolicy`, `SessionStore`, `Memory`, `RunHooks`
- **Tool ecosystem** — JSON Schema argument validation, structured tool results, order-preserving concurrency, `#[tool]` derive macro
- **Sessions & sub-agents** — persist / restore conversations; sub-agent orchestration via `AgentTool`
- **Observability** — structured event stream (contiguous `seq`), tracing spans, raw interaction logs

## 🧩 Architecture

```text
mycelium (facade: re-exports + prelude)
   ├── mycelium-runtime (execution loop / events / sessions / hooks)
   │      ├── mycelium-model (ModelProvider SPI / streaming assembler)
   │      └── mycelium-tools (Tool SPI / registry / validation / policy)
   ├── mycelium-provider-openai (OpenAI-compatible provider, DeepSeek ready)
   ├── mycelium-mcp (MCP client: stdio + HTTP, tools / resources / prompts)
   ├── mycelium-macros (#[tool] derive macro)
   └── mycelium-core (messages / tool protocol / events / state machine / errors / config)
```

| Crate | Responsibility |
| :--- | :--- |
| `mycelium-core` | Domain kernel: ids, messages (incl. thinking blocks), tool protocol, events, run state machine, error taxonomy, configuration |
| `mycelium-model` | Model provider SPI: `ModelProvider` / `ModelRequest` / `ModelDelta` / streaming assembler |
| `mycelium-tools` | Tool SPI, registry, argument validation (JSON Schema subset), tool policies, `#[tool]` macro |
| `mycelium-runtime` | Execution engine: `AgentRuntime` / concurrent tools / budgets / cancellation / `AgentRun` / `Session` / `RunHooks` |
| `mycelium-macros` | Proc macros: `#[tool]` generates the tool impl and JSON Schema from a function |
| `mycelium-mcp` | MCP client: stdio and streamable HTTP; tools / resources / prompts |
| `mycelium-provider-openai` | OpenAI-compatible provider (Chat Completions, streaming SSE, reasoning) |
| `mycelium` | Facade: unified re-exports and `prelude` (optional feature: `provider-openai`) |

Further reading:

- [`docs/architecture/overview.md`](docs/architecture/overview.md) — architecture overview and key decisions
- [`docs/architecture/lifecycle.md`](docs/architecture/lifecycle.md) — run lifecycle state machine
- [`docs/spec/core.md`](docs/spec/core.md) — formal constraint matrix (FC-Matrix, single source of truth)
- [`docs/integration/`](docs/integration/) — raw end-to-end integration logs (DeepSeek × MCP, traceable / replayable)
- [`docs/architecture/api-freeze.md`](docs/architecture/api-freeze.md) — public API freeze review and change policy
- [`docs/architecture/release-process.md`](docs/architecture/release-process.md) — release process and checklists

## 📦 Installation

### Prerequisites

| Dependency | Minimum | Notes |
| :--- | :--- | :--- |
| Rust (stable) | 1.97 | MSRV, verified on stable 1.97.1; install via [rustup](https://rustup.rs) |
| Node.js | — | Only needed for the MCP example (starts MCP servers via `npx`) |
| Model API key | — | Only needed for real-model examples (see env vars in [Configuration](#-configuration)) |

### Add as a dependency

The project is not published on crates.io yet (`publish = false`); use a git dependency (or a
`path` dependency during local development):

```toml
# Cargo.toml
[dependencies]
mycelium = { git = "https://github.com/oxroot-crypto/mycelium" }
# Optional: OpenAI-compatible provider; or enable the facade feature instead (below)
mycelium-provider-openai = { git = "https://github.com/oxroot-crypto/mycelium" }
```

```toml
# Equivalent: facade only, provider behind a feature flag
mycelium = { git = "https://github.com/oxroot-crypto/mycelium", features = ["provider-openai"] }
```

## 🚀 Quick Start

1. Clone the repository

```bash
git clone https://github.com/oxroot-crypto/mycelium.git
cd mycelium
```

2. Build the workspace

```bash
cargo build --workspace
```

3. Run the built-in example

```bash
cargo run -p mycelium --example basic
```

Expected output:

```text
装配完成：provider=echo，tools=1
运行完成：steps=1，usage=Usage { input_tokens: 0, output_tokens: 0, reasoning_tokens: 0 }，消息数=1
```

## 📖 Usage

### Embed in your app: a minimal provider and tool

Requires the `mycelium`, `serde_json` and `tokio` (`macros`, `rt-multi-thread`) dependencies;
the snippet runs as-is in `src/main.rs`:

```rust
use std::sync::Arc;

use mycelium::prelude::*;

/// Minimal provider: returns fixed text (for wiring demos).
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

/// Minimal tool: echoes its arguments.
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

    // Execution loop: the model echoes "pong" and finishes in a single step.
    let outcome = agent.run(RunInput::from_user_text("ping")).await?;
    assert_eq!(outcome.steps, 1);
    assert_eq!(outcome.messages.len(), 1);

    Ok(())
}
```

### Connect to a real model (OpenAI-compatible)

```bash
cp .env.example .env    # fill in DEEPSEEK_API_KEY (.env is gitignored)
cargo run -p mycelium-provider-openai --example deepseek
```

Expected output (excerpt):

```text
网关：https://api.deepseek.com/v1，模型：deepseek-flash
...
✅ 工具调用往返验证通过
```

- `OpenAiProvider::from_env()` reads `OPENAI_API_KEY` (required) and `OPENAI_BASE_URL`
  (defaults to `https://api.openai.com/v1`; works with Ollama / vLLM and other gateways); usage
  examples live in the rustdoc of
  [`crates/mycelium-provider-openai/src/lib.rs`](crates/mycelium-provider-openai/src/lib.rs).
- Reasoning: `AgentConfig::reasoning` controls `thinking` / `reasoning_effort`; thinking content
  is exposed as `Thinking` content blocks and `ModelThinkingDelta` events, counted by
  `Usage.reasoning_tokens`, and is **never** sent back to the model.

### Connect an MCP server

```rust,no_run
use std::sync::Arc;

use mycelium::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stdio child process; for HTTP endpoints use McpClient::connect_http("https://host/mcp")
    let client = Arc::new(
        McpClient::spawn(
            "npx",
            &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
        )
        .await?,
    );
    let tools = client.tools().await?; // Arc<dyn Tool> values, ready to register in AgentRuntime
    println!("工具数：{}", tools.len());
    let resources = client.list_resources().await?; // resources and prompts are enumerable too
    println!("资源数：{}", resources.len());
    client.shutdown().await?;
    Ok(())
}
```

### Multi-turn sessions and sub-agents

```rust,no_run
use std::sync::Arc;

use mycelium::prelude::*;

// Multi-turn session: history is carried automatically, messages are appended back on success
#[allow(dead_code)]
async fn multi_turn(
    runtime: &AgentRuntime,
    session: &mut Session,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime
        .run_session(session, RunInput::from_user_text("请记住数字 7"))
        .await?;
    // Persistence: in-memory / file implementations (SPI: SessionStore)
    let store = FileSessionStore::new("/tmp/mycelium-sessions");
    store.save(session).await?;
    Ok(())
}

// Sub-agent: wrap a child runtime as a tool and register it on the parent runtime
#[allow(dead_code)]
fn sub_agent(child: Arc<AgentRuntime>) -> Result<Arc<dyn Tool>, Box<dyn std::error::Error>> {
    let name = ToolName::new("ask_child")?;
    Ok(Arc::new(AgentTool::new(name, "sub agent", child)))
}
```

## ⚙️ Configuration

All options supported by `AgentConfig::builder()` (keys are the real method names):

| Option | Type | Default | Notes |
| :--- | :--- | :--- | :--- |
| `model` | `ModelId` | required | Model identifier, e.g. `deepseek-flash` |
| `system_prompt` | `String` | none | System prompt |
| `max_steps` | `u32` (≥ 1) | `8` | Maximum steps per run |
| `request_timeout` | `Duration` (> 0) | `60s` | Timeout per model request |
| `max_output_tokens` | `u64` (≥ 1) | none (provider default) | Output token limit |
| `temperature` | `f32` ∈ [0, 2] | none (provider default) | Sampling temperature |
| `tool_choice` | `ToolChoice` | `Auto` | Tool selection strategy |
| `max_parallel_tool_calls` | `usize` (≥ 1) | `1` (sequential) | Tool concurrency limit; results are fed back in call order |
| `tool_timeout` | `Duration` (> 0) | none | Timeout per tool call |
| `reasoning` | `ReasoningOptions` | none (provider default) | Reasoning mode (`thinking` / `reasoning_effort`) |
| `max_duration` | `Duration` (> 0) | none | Maximum duration per run |
| `retry` | `RetryPolicy` | none (no retry) | Exponential backoff for retryable model errors (starts at 500ms, caps at 8s) |

Environment variables used by the examples (template: [`.env.example`](.env.example)):

| Variable | Default | Notes |
| :--- | :--- | :--- |
| `DEEPSEEK_API_KEY` | required | API key for the DeepSeek integration example |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com/v1` | Gateway URL |
| `DEEPSEEK_MODEL` | `deepseek-flash` | Model name |
| `OPENAI_API_KEY` | required | Used by `OpenAiProvider::from_env()` |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Used by `OpenAiProvider::from_env()` |

## 🧪 Development

```bash
git clone https://github.com/oxroot-crypto/mycelium.git
cd mycelium
cargo build --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo deny check        # dependency audit (advisories / licenses / sources); install: cargo install cargo-deny
```

- MSRV: Rust 1.97 (`rust-version`, verified on stable 1.97.1).
- CI: [`ci.yml`](.github/workflows/ci.yml) (fmt / clippy / test / doc / MSRV / dependency audit) and
  [`mutants.yml`](.github/workflows/mutants.yml) (weekly mutation testing).
- Change discipline: public behaviour changes must first update `docs/spec/core.md` (contract
  first), then tests, then the implementation.

## 🗺️ Roadmap

- Released: all milestones v0.1 → v0.7 (architecture skeleton → complete MCP); tags and snapshot
  table in [`docs/architecture/roadmap.md`](docs/architecture/roadmap.md).
- In progress: MCP sampling callbacks and server notification subscriptions, remaining JSON
  Schema keywords, external memory backends.
- v1.0 prep: API freeze review ([`api-freeze.md`](docs/architecture/api-freeze.md)) and the
  crates.io release ([release process](docs/architecture/release-process.md)).

## 🤝 Contributing

- Before submitting, make sure `cargo test --workspace --all-features`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings` and
  `cargo fmt --all --check` are green (CI also runs MSRV and `cargo deny`).
- Public behaviour changes follow the contract-first rule: update `docs/spec/core.md`, then tests
  and the implementation; breaking changes go through a review under `docs/architecture/rfc/`.
- Use Conventional Commits; register new dependencies with a rationale in
  `docs/architecture/overview.md` §9.

## 📄 License

Dual-licensed under **MIT OR Apache-2.0** (pick either):

- [LICENSE-MIT](LICENSE-MIT)
- [LICENSE-APACHE](LICENSE-APACHE)
