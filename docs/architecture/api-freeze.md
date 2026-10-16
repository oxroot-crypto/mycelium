# 公共 API 冻结评审（v1.0 准备）

> 状态：评审草案（2026-10-16）。冻结正式生效条件：连续两个小版本无破坏性变更后，
> 由维护者按下文检查单完成冻结，并将本文件状态改为「已冻结」。

## 1. 冻结范围（Stable：v1.0 后受 SemVer 承诺保护）

| crate | 冻结面 |
| :--- | :--- |
| `mycelium-core` | `Message` / `ContentBlock` / `ToolResultBlock` / `Role` / `ToolSpec` / `ToolCall` / `ToolChoice` / `AgentEvent` / `AgentEventKind` / `RunState` / `RunEvent` / `InvalidRunTransition` / `AgentConfig` / `AgentConfigBuilder` / `ReasoningOptions` / `RetryPolicy` / `StopReason` / `Usage` / `ModelResponse` / 全部 ID 类型 / 错误类型（变体与数据字段，文案不冻结）/ serde wire 标签与字段名 |
| `mycelium-model` | `ModelProvider` / `ModelRequest` / `ModelDelta` / `ModelStream` / `ModelCapabilities` / `DeltaAssembler` / `FallbackProvider` / `FallbackError` / `stream_from_deltas` |
| `mycelium-tools` | `Tool` / `ToolContext` / `ToolOutput` / `ToolRegistry` / `RegistryError` / `ToolPolicy` 家族（`AllowAll` / `AllowList` / `DenyList` / `PolicyDecision`）/ `schema::validate_arguments` / `SchemaViolation` / `#[tool]` 宏的**生成约定**（`<Fn>Tool` 结构体与 `<fn>_tool()` 帮助函数） |
| `mycelium-runtime` | `AgentRuntime` / `AgentRuntimeBuilder` / `BuildError` / `AgentRun` / `RunFuture` / `AgentEventStream` / `RunHooks` / `RunContext` / `RunInput` / `RunOutcome` / `Session` / `SessionStore` 家族 / `Memory` 家族 / `AgentTool`；`mycelium.run` / `mycelium.step` / `mycelium.tool` 结构化 span 的名称与关键字段 |
| `mycelium-mcp` | `McpClient` / `McpServerInfo` / `McpTool` / `McpError` / `McpResource` / `McpResourceContent` / `McpPrompt` / `McpPromptArgument` / `McpPromptMessage` / `PROTOCOL_VERSION` / `MAX_LIST_PAGES` |
| `mycelium-provider-openai` | `OpenAiProvider` / `OpenAiProviderBuilder` / `OpenAiConfigError` / `DEFAULT_BASE_URL` / `API_KEY_ENV` / `BASE_URL_ENV` |
| `mycelium`（门面） | `prelude` 导出集合（**新增属非破坏性；移除属破坏性**）与 re-export 模块路径（`core` / `model` / `tools` / `runtime` / `mcp` / `provider_openai`） |

统计口径参考（2026-10-16，`pub` 定义粗计）：约 264 项（不含 `pub(crate)` 与 re-export）。

## 2. 明确不冻结（Unstable）

- `mycelium_tools::__private`（仅宏生成代码使用；随时可变，禁止手写依赖）；
- `#[doc(hidden)]` 项目与 `mycelium-macros` 的内部实现细节（属性解析、代码生成结构）；
- 错误 `Display` 的具体文案与 `tracing` 日志消息文本 / level（结构化 span 名称与字段名除外）；
- 测试、示例与 `docs/integration/` 产物；
- `mycelium-macros` 自身的公共过程宏签名（`#[tool]` 的属性参数集属 Stable，内部展开不经 `__private` 之外的路径者不冻结）。

## 3. 破坏性变更政策

| 阶段 | 规则 |
| :--- | :--- |
| 0.x（当前） | minor 版本可包含破坏性变更，必须记录于 `docs/spec/core.md` §5 与 `CHANGELOG.md`（**BREAKING** 标注），并同步更新 FC-Matrix 与迁移说明 |
| 1.0+ | 破坏性变更需 major 版本；至少保留一个 minor 版本的弃用期（`#[deprecated]` + 迁移指引）；变更前须经 RFC（`docs/architecture/rfc/`）评审 |
| wire 格式 | 新增字段必须可选且带默认值（非破坏性）；枚举新增变体视为**破坏性**；标签重命名按破坏性处理 |
| MSRV | 提升 MSRV 属 minor 变更，须在 CHANGELOG 标注并给出理由 |

## 4. 冻结检查单（正式冻结时逐项执行）

- [ ] 遍历上表逐项确认签名与语义（对照 `cargo doc` 输出）；
- [ ] 引入 `cargo-public-api` / `cargo-semver-checks` 作为 CI 门禁（roadmap backlog），生成基线快照；
- [ ] 确认 `__private` 未被任何文档或示例引用；
- [ ] 确认 CHANGELOG 覆盖全部历史破坏性变更；
- [ ] 撰写 v0.1 → v1.0 迁移指南（基于 CHANGELOG 聚合）；
- [ ] 修改本文件状态为「已冻结」并记录日期与基线版本。
