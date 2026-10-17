# Changelog

本文件记录每个版本的显著变更；形式化契约的逐条变更记录见
[`docs/spec/core.md`](docs/spec/core.md) §5。版本遵循语义化版本：`0.x` 阶段的破坏性条目以 **BREAKING** 标注；
自 v1.0.0 起遵循 SemVer，破坏性变更需 major（政策见 [`api-freeze.md`](docs/architecture/api-freeze.md) §3）。

## [Unreleased]

## [1.0.0] - 2026-10-17

**公共 API 冻结**

- **发布名迁移**：crates.io 包名改为 `mycelium-agent` / `mycelium-agent-core` / `-model` / `-tools` /
  `-macros` / `-runtime` / `-mcp` / `-provider-openai`；Rust 导入名保持不变（`[lib] name` 固定），
  源码零改动。背景：`mycelium` / `mycelium-core` / `mycelium-runtime` / `mycelium-macros` 已被第三方占用
- **API 冻结正式生效**（[`api-freeze.md`](docs/architecture/api-freeze.md)，基线 v1.0.0）：7 个冻结 crate 经
  `cargo-semver-checks` 对 `v0.7.0` 全量验证——无破坏性差异
- 发布工程：全部 crate 目录内置双许可证与 crate 级 README；元数据补齐
  （`keywords` / `categories` / `readme` / docs.rs `all-features`）；工作区移除 `publish = false`
- CI 新增：API 兼容门禁（semver job，基线 tag `v1.0.0`）与打包预检（`cargo package --workspace --no-verify`）
- 新增文档：[迁移指南](docs/architecture/migration-v0-to-v1.md) / [v1.0 发布手册](docs/architecture/release-v1.0-runbook.md)
- 新增 AI 生成声明：[`DISCLOSURE.md`](DISCLOSURE.md) / [`DISCLOSURE.en.md`](DISCLOSURE.en.md)
  （全部代码与文档由 **deepseek-v4.1-flash** 生成）
- 历史条目补注：0.2.0 / 0.3.0 的枚举新增变体属破坏性变更（此前未按 **BREAKING** 标注，现补注）

## [0.7.0] - 2026-10-16

**MCP 完整化**

- 新增 `Transport` 抽象：stdio 与 streamable HTTP 共享全部能力方法
- 新增 MCP 资源：`resources/list` / `resources/read`（`McpResource` / `McpResourceContent`）
- 新增 MCP 提示模板：`prompts/list` / `prompts/get`（`McpPrompt` / `McpPromptMessage`）
- 新增 streamable HTTP 传输：`McpClient::connect_http`（会话头传播、JSON 与 SSE 响应、SSE 按 id 匹配）
- **BREAKING**：`MAX_TOOL_PAGES` 更名为 `MAX_LIST_PAGES`（语义扩展到全部 `*/list`）
- 联调：新增场景 6（官方 `server-everything`：7 资源 / 4 提示模板）

## [0.6.0] - 2026-10-16

**记忆与可观测性**

- 新增长期记忆 SPI：`Memory` / `MemoryRecord` / `MemoryId` / `InMemoryMemory`
- JSON Schema 子集扩充：`const` / `allOf` / `anyOf` / `oneOf` / 数值边界 / 字符串长度 / 数组约束
- 新增 tracing 结构化 span：`mycelium.run` / `mycelium.step` / `mycelium.tool`

## [0.5.0] - 2026-10-16

**多轮会话与多 Agent**

- 新增 `SessionStore` SPI（内存 / 文件实现，原子写）与 `AgentRuntime::run_session`
- 新增 `AgentTool`（agent-as-tool：取消传播、子运行独立预算与事件流）
- 联调：新增场景 4（会话持久化恢复）与场景 5（子 Agent 编排）

## [0.4.0] - 2026-10-16

**结构化工具结果、异步钩子、重试与降级**

- **BREAKING**：`ContentBlock::ToolResult.content` 由 `String` 改为 `Vec<ToolResultBlock>`（RFC-0002）；
  `ToolOutput` 改为多块模型（`text()` / `content_blocks()` / `into_blocks()`）
- **BREAKING**：`RunHooks` 异步化（RFC-0001），四个回调改为 `async fn`
- 新增重试策略（`RetryPolicy` + `ModelRetry` 事件）与时长预算（`BudgetKind::DurationMs`）
- 新增多 Provider 降级 `FallbackProvider`
- CI：新增 MSRV job（1.97.0）

## [0.3.0] - 2026-10-16

**策略与生态**

- 工具并发执行（`max_parallel_tool_calls`，保序回灌）与工具超时（`tool_timeout`）
- 权限策略层：`ToolPolicy` SPI（`AllowAll` / `DenyList` / `AllowList`）
- 新增 `mycelium-macros`：`#[tool]` 派生宏
- 新增 `mycelium-mcp`：MCP 客户端（stdio、工具能力）
- 思考过程支持：`Thinking` 内容块 / `ModelThinkingDelta` / `reasoning_tokens` / `thinking` + `reasoning_effort`
- **BREAKING**：枚举新增变体——`ContentBlock::Thinking` / `ModelDelta::ThinkingDelta` /
  `AgentEventKind::ModelThinkingDelta`；`Usage` 新增 `reasoning_tokens`（结构体新增字段带默认值）
- 联调：DeepSeek V4.1-Flash 首次真实联调，原始日志随仓库保留

## [0.2.0] - 2026-10-14

**最小可运行循环**

- 执行引擎（顺序工具、预算、取消、超时）与流式增量装配器
- 事件系统（`run_events` 流式 API）与工具入参 JSON Schema 子集校验
- 新增 `mycelium-provider-openai`（OpenAI 兼容 Provider）
- **BREAKING**：新增 `Role::Tool` 与 `Message::tool_result`（枚举新增变体；0.x 未发布，无外部影响）

## [0.1.0] - 2026-10-14

**架构骨架**

- Cargo workspace 分层（core / model / tools / runtime / 门面）与三个 SPI 契约
- 运行状态机（全路径测试）、错误分类矩阵、FC-Matrix 与测试 1:1 映射
- CI（fmt / clippy / test）与 PBT（proptest）

[Unreleased]: https://github.com/oxroot-crypto/mycelium/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.7.0...v1.0.0
[0.7.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/oxroot-crypto/mycelium/releases/tag/v0.1.0
