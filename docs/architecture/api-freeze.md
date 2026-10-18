# 公共 API 冻结评审（v1.0）

> 状态：**已冻结**（2026-10-17）。冻结基线：**v1.0.0**；冻结面于该版本定型，证据见 §4。
> 冻结前证据：7 个冻结 crate 经 `cargo-semver-checks 0.51.0` 对 `v0.7.0` 基线全量验证——
> 全部 `no semver update required`（无破坏性差异），检查单执行记录见 §4。
> 自 v1.0.0 起，§1 冻结面受 SemVer 承诺保护；破坏性变更需 major + RFC + 弃用期（§3）。

## 1. 冻结范围（Stable：v1.0 后受 SemVer 承诺保护）

> 下表为 **crates.io 发布名**；Rust 导入名保持 `mycelium_*` 不变（各 crate `[lib] name` 固定），
> 映射与依赖写法见 [`migration-v0-to-v1.md`](migration-v0-to-v1.md) §1。

| crate（发布名） | 冻结面 |
| :--- | :--- |
| `mycelium-agent-core` | `Message` / `ContentBlock` / `ToolResultBlock` / `Role` / `ToolSpec` / `ToolCall` / `ToolChoice` / `AgentEvent` / `AgentEventKind` / `RunState` / `RunEvent` / `InvalidRunTransition` / `AgentConfig` / `AgentConfigBuilder` / `ReasoningOptions` / `RetryPolicy` / `StopReason` / `Usage` / `ModelResponse` / 全部 ID 类型 / 错误类型（变体与数据字段，文案不冻结）/ serde wire 标签与字段名 |
| `mycelium-agent-model` | `ModelProvider` / `EmbeddingProvider` / `ModelRequest` / `EmbeddingRequest` / `EmbeddingRequestError` / `ModelDelta` / `EmbeddingResponse` / `EmbeddingResponseError` / `EmbeddingUsage` / `EmbeddingVector` / `MAX_EMBEDDING_INPUTS` / `ModelStream` / `ModelCapabilities` / `DeltaAssembler` / `FallbackProvider` / `FallbackError` / `stream_from_deltas` |
| `mycelium-agent-tools` | `Tool` / `ToolContext` / `ToolOutput` / `ToolRegistry` / `RegistryError` / `ToolPolicy` 家族（`AllowAll` / `AllowList` / `DenyList` / `PolicyDecision`）/ `schema::validate_arguments` / `SchemaViolation` / `#[tool]` 宏的**生成约定**（`<Fn>Tool` 结构体与 `<fn>_tool()` 帮助函数） |
| `mycelium-agent-runtime` | `AgentRuntime` / `AgentRuntimeBuilder` / `BuildError` / `AgentRun` / `RunFuture` / `AgentEventStream` / `RunHooks` / `RunContext` / `RunInput` / `RunOutcome` / `Session` / `SessionStore` 家族 / `Memory` 家族 / `AgentTool`；`mycelium.run` / `mycelium.step` / `mycelium.tool` 结构化 span 的名称与关键字段 |
| `mycelium-agent-mcp` | `McpClient` / `McpServerInfo` / `McpTool` / `McpError` / `McpResource` / `McpResourceContent` / `McpPrompt` / `McpPromptArgument` / `McpPromptMessage` / `PROTOCOL_VERSION` / `MAX_LIST_PAGES` |
| `mycelium-agent-provider-openai` | `OpenAiProvider` / `OpenAiProviderBuilder` / `OpenAiConfigError` / `DEFAULT_BASE_URL` / `API_KEY_ENV` / `BASE_URL_ENV` |
| `mycelium-agent`（门面） | `prelude` 导出集合（**新增属非破坏性；移除属破坏性**）与 re-export 模块路径（`core` / `model` / `tools` / `runtime` / `mcp` / `provider_openai`） |

统计口径参考（2026-10-16，`pub` 定义粗计）：约 264 项（不含 `pub(crate)` 与 re-export）。

## 2. 明确不冻结（Unstable）

- `mycelium_tools::__private`（仅宏生成代码使用；随时可变，禁止手写依赖）；
- `#[doc(hidden)]` 项目与 `mycelium-agent-macros` 的内部实现细节（属性解析、代码生成结构）；
- 错误 `Display` 的具体文案与 `tracing` 日志消息文本 / level（结构化 span 名称与字段名除外）；
- 测试、示例与 `docs/integration/` 产物；
- `mycelium-agent-macros` 自身的公共过程宏签名（`#[tool]` 的属性参数集属 Stable，内部展开不经 `__private` 之外的路径者不冻结）。

## 3. 破坏性变更政策

| 阶段 | 规则 |
| :--- | :--- |
| 0.x（历史，至 v0.7.0） | minor 版本可包含破坏性变更，必须记录于 `docs/spec/core.md` §5 与 `CHANGELOG.md`（**BREAKING** 标注），并同步更新 FC-Matrix 与迁移说明 |
| 1.0+（自 v1.0.0 起生效） | 破坏性变更需 major 版本；至少保留一个 minor 版本的弃用期（`#[deprecated]` + 迁移指引）；变更前须经 RFC（`docs/architecture/rfc/`）评审，并通过 CI 的 semver 门禁 |
| wire 格式 | 新增字段必须可选且带默认值（非破坏性）；枚举新增变体视为**破坏性**；标签重命名按破坏性处理 |
| MSRV | 提升 MSRV 属 minor 变更，须在 CHANGELOG 标注并给出理由 |

## 4. 冻结检查单（2026-10-17 执行记录）

- [x] 遍历冻结面逐项确认签名与语义：7 个冻结 crate 经 `cargo-semver-checks` 对 `v0.7.0` 基线全量验证，
      全部 `no semver update required`；`cargo doc --workspace --no-deps --all-features` 零告警
- [x] 引入 `cargo-semver-checks` 作为 CI 门禁：`.github/workflows/ci.yml` 的 `semver` job，
      基线 tag `v1.0.0`（tag 创建后自动生效；此前诚实跳过）；v1.1.0 发布后基线更新为 `v1.1.0`；
      `cargo-public-api` 未引入（0.52.0 于当前工具链编译失败，semver-checks 已覆盖门禁需求）
- [x] 确认 `__private` 未被任何文档或示例引用（仅 `mycelium-agent-macros` 的生成代码内部引用）
- [x] 确认 CHANGELOG 覆盖全部历史破坏性变更（0.2.0 / 0.3.0 的枚举新增变体条目已补注 **BREAKING**）
- [x] 撰写迁移指南：`docs/architecture/migration-v0-to-v1.md`（v0.1 → v1.0）
- [x] 记录基线与日期：**v1.0.0**（发布 tag），2026-10-17；冻结面 = v0.7.0 API + 零破坏性差异 + 发布名迁移

## 5. 冻结面增量记录（发布后）

| 版本 | 增量 | 类型 | 验证 |
| :--- | :--- | :--- | :--- |
| v1.1.0（2026-10-18） | `mycelium-agent-model` 新增 Embedding 类型（`EmbeddingProvider` / `EmbeddingRequest` / `EmbeddingRequestError` / `EmbeddingResponse` / `EmbeddingResponseError` / `EmbeddingUsage` / `EmbeddingVector` / `MAX_EMBEDDING_INPUTS`）；门面 `prelude` 对应导出；`mycelium-agent-provider-openai` 的 `OpenAiProvider` 新增 `EmbeddingProvider` 实现 | 非破坏性新增（新增 trait / 类型 / impl；无枚举变体与 wire 变更），纳入冻结面 | `cargo-semver-checks` 对基线 `v1.0.0`：`no semver update required` |
