# 迁移指南：v0.x → v1.0

> 适用对象：从 0.1–0.7 升级到 v1.0.0 的使用者。自 v1.0 起公共 API 冻结
> （冻结面见 [`api-freeze.md`](api-freeze.md) §1）；后续破坏性变更需 major + RFC + 弃用期。

## 1. crates.io 发布名迁移（v1.0 首发）

包名与导入名解耦（`[lib] name` 固定）：**依赖名变化，`use` 导入路径不变**。

| crates.io 包名（v1.0 起） | Rust 导入名（不变） | 备注 |
| :--- | :--- | :--- |
| `mycelium-agent` | `mycelium` | 门面（推荐入口） |
| `mycelium-agent-core` | `mycelium_core` | 领域内核 |
| `mycelium-agent-model` | `mycelium_model` | 模型 SPI |
| `mycelium-agent-tools` | `mycelium_tools` | 工具 SPI 与注册表 |
| `mycelium-agent-macros` | `mycelium_macros` | `#[tool]` 宏（经 tools 再导出） |
| `mycelium-agent-runtime` | `mycelium_runtime` | 执行引擎 |
| `mycelium-agent-mcp` | `mycelium_mcp` | MCP 客户端 |
| `mycelium-agent-provider-openai` | `mycelium_provider_openai` | 可选 feature `provider-openai` |

背景：原名称 `mycelium` / `mycelium-core` / `mycelium-runtime` / `mycelium-macros`
已被第三方项目占用，无法在 crates.io 发布。

```toml
# crates.io 依赖（v1.0 起）
[dependencies]
mycelium-agent = "1.0"
# 等价写法：门面 + feature（代替直接依赖 mycelium-agent-provider-openai）
# mycelium-agent = { version = "1.0", features = ["provider-openai"] }
```

```rust
// 代码零改动：导入名保持 mycelium
use mycelium::prelude::*;
```

## 2. API 破坏性变更（0.x 历史）

v1.0 冻结面内**没有新增破坏性变更**（`cargo-semver-checks` 对 `v0.7.0` 基线验证：无破坏性差异）。
若从早期 0.x 版本升级，按下表逐项迁移。

| 版本 | 破坏性变更 | 迁移方法 |
| :--- | :--- | :--- |
| v0.7.0 | `MAX_TOOL_PAGES` 更名为 `MAX_LIST_PAGES` | 全局替换常量名（语义扩展到全部 `*/list`） |
| v0.4.0 | `ContentBlock::ToolResult.content`：`String` → `Vec<ToolResultBlock>`（RFC-0002） | 构造：`ToolOutput::success(text)` 或 `ToolOutput::blocks(vec![ToolResultBlock::Text { text }])`；读取文本：`output.text()` |
| v0.4.0 | `RunHooks` 四个回调改为 `async fn`（RFC-0001：`on_run_start` / `on_step_start` / `on_tool_call` / `on_run_end`） | 实现处加 `async`（`#[async_trait]` 随 SPI 导出）；回调在运行任务内 `await` |
| v0.3.0 | 枚举新增变体：`ContentBlock::Thinking`、`ModelDelta::ThinkingDelta`、`AgentEventKind::ModelThinkingDelta`；`Usage` 新增 `reasoning_tokens` | 穷举 `match` 需新增分支（或用 `_`）；结构体新增字段带默认值，wire 向后兼容 |
| v0.2.0 | 枚举新增变体：`Role::Tool`；新增 `Message::tool_result` | 穷举 `match` 需覆盖 `Role::Tool` |

## 3. wire 兼容性承诺

v1.0 起 serde 标签与字段名冻结（`api-freeze.md` §1）：新增字段必须可选且带默认值；
枚举新增变体视为破坏性（需 major）。0.x 阶段未发布到 crates.io，无存量 wire 兼容负担。

## 4. 升级建议

1. 从 0.7.0 升级：无需改动 API，仅更新依赖声明（§1）；
2. 从 ≤ 0.6 升级：先按 §2 完成迁移，再更新依赖；
3. 迁移后运行 `cargo test`；`#[tool]` 宏与 MCP 能力在 0.x 已稳定并纳入冻结面。
