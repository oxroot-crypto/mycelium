# mycelium 路线图

> 原则：每一档必须保持 `cargo build / test / clippy / fmt` 全绿，且 FC-Matrix 与测试同步演进。

## 进度快照（截至 v0.6.0）

| 版本 | 发布日 | 提交（git tag） | 主题 | 状态 |
| :--- | :--- | :--- | :--- | :--- |
| v0.1.0 | 2026-10-14 | `454cfa6`（`v0.1.0`） | 架构骨架：分层、领域类型、SPI 契约、状态机、FC-Matrix | ✅ 已发布 |
| v0.2.0 | 2026-10-14 | `1c61666`（`v0.2.0`） | 最小可运行循环：执行引擎、流式装配器、事件流、OpenAI 兼容 Provider | ✅ 已发布 |
| v0.3.0 | 2026-10-16 | `d65cc83`（`v0.3.0`） | 策略与生态：并发工具、权限策略、`#[tool]` 宏、MCP、思考过程 | ✅ 已发布 |
| v0.4.0 | 2026-10-16 | `e28205b`（`v0.4.0`） | 结构化工具结果、异步钩子、重试与降级、时长预算、MSRV CI | ✅ 已发布 |
| v0.5.0 | 2026-10-16 | `3fcd1c2`（`v0.5.0`） | 多轮会话持久化（`SessionStore` + `run_session`）、子 Agent 编排（`AgentTool`） | ✅ 已发布 |
| v0.6.0 | 2026-10-16 | `9fbe52a`（`v0.6.0`） | 长期记忆 SPI、JSON Schema 扩充、tracing 结构化 span | ✅ 已发布 |
| v0.7.0 | 2026-10-16 | `PLACEHOLDER`（`v0.7.0`） | MCP 资源 / 提示模板 / streamable HTTP 传输 | ✅ 已发布 |
| v0.7.x | — | — | MCP 采样回调 / 通知订阅、剩余 Schema 关键字、外部记忆后端 | ⏳ 规划中 |
| v1.0.0 | — | — | API 冻结、文档与发布流程 | ⏳ 规划中 |

工程数据（截至 v0.7.0）：

- FC 约束 **155 条**，100% 映射测试（v0.1 43 / v0.2 45 / v0.3 27 / v0.4 16 / v0.5 10 / v0.6 7 / v0.7 7）；
- **229 项测试**（220 单元 / 集成 + 9 doctest）全绿；clippy / fmt / doc 零告警；
- 真实联调原始日志：`docs/integration/`（v0.3 ~ v0.7 五次完整记录，DeepSeek V4.1-Flash × MCP）；
- 契约与变更记录：`docs/spec/core.md`（RFC：`docs/architecture/rfc/`）。

---

## v0.1 架构骨架 — 已完成（2026-10-14，`454cfa6`）

- [x] Cargo workspace 分层：`mycelium-core` / `mycelium-model` / `mycelium-tools` / `mycelium-runtime` / `mycelium`
- [x] 领域类型：消息、内容块、工具协议、事件、标识符、配置
- [x] 三个 SPI 契约：`ModelProvider` / `Tool` / `RunHooks`
- [x] 错误分类矩阵与 `is_retryable` 语义
- [x] 运行状态机（全路径测试 + 负向路径）
- [x] FC-Matrix（`docs/spec/core.md`）与测试 1:1 映射
- [x] PBT（proptest）与证伪抽查
- [x] CI（fmt / clippy / test）+ rust-toolchain + rustfmt 配置

## v0.2 最小可运行循环 — 已完成（2026-10-14，`1c61666`）

- [x] `ModelDelta` 装配器（流协议五项契约与负向测试）
- [x] 执行循环：请求模型 → 顺序执行工具 → 回灌 → 终止条件（无工具调用 / 预算耗尽）
- [x] 事件发射器（`seq` 连续、终态唯一；骨架哨兵已被真实循环取代）
- [x] 取消传播（`CancellationToken` → 在途模型流与工具）与 `request_timeout`
- [x] 预算执行（步数 / 输出 Token）
- [x] `run_events`：以 `Stream<Item = AgentEvent>` 暴露过程事件
- [x] 首个官方 Provider：`mycelium-provider-openai`（OpenAI 兼容 Chat Completions）
- [x] 工具入参 schema 校验（JSON Schema 子集）
- [x] tracing 埋点与脚本化 mock provider 集成测试

## v0.3 策略与生态 — 已完成（2026-10-16，`d65cc83`）

- [x] 工具并发执行（`max_parallel_tool_calls`；`buffered` 保序回灌；事件发射串行化）
- [x] 工具超时（`tool_timeout` → `ToolError::Timeout` 错误结果回灌）
- [x] 权限策略层（`ToolPolicy` SPI + `AllowAll` / `DenyList` / `AllowList`）
- [x] `#[tool]` derive 宏（`mycelium-macros`：编译期校验 + JSON Schema 生成）
- [x] MCP 客户端适配（`mycelium-mcp`：stdio JSON-RPC、分页 `tools/list`、`tools/call` 代理）
- [x] 模型思考过程支持（`Thinking` 块 / `ModelThinkingDelta` / `reasoning_tokens` / `thinking` + `reasoning_effort`）
- [x] 真实模型联调（DeepSeek V4.1-Flash，日志留存 `docs/integration/2026-10-16/`）
- [x] 正式化联调流程（`live_integration` 示例 + 原始交互日志留存）
- ↪️ 顺延项：异步钩子、结构化工具输出（随 v0.4.0 完成，见下）；MCP 资源 / HTTP 传输（顺延至 v0.4.x）

## v0.4 持久化与多 Agent

### v0.4.0 — 已完成（2026-10-16，`e28205b`）

- [x] 【v0.3 遗留】异步钩子（`RunHooks` 异步化，RFC-0001）
- [x] 【v0.3 遗留】结构化工具结果（多内容块 / 图片，RFC-0002；MCP text/image 映射）
- [x] 重试策略（`RetryPolicy`：可重试错误 + 指数退避 + `ModelRetry` 事件；退避可取消）
- [x] 多 Provider 降级（`FallbackProvider`：可重试按序降级，不可重试立即返回）
- [x] 时长预算（`BudgetKind::DurationMs`）
- [x] MSRV CI job（1.97.0 `cargo check --workspace --all-features`）
- [x] 联调重跑（wire 变更后，日志 `docs/integration/2026-10-16-v0.4/`）

### v0.4.x — 规划中（已被 v0.5 取代）

> 会话持久化与子 Agent 编排已随 v0.5.0 交付（见下）；其余条目顺延至 v0.5.x。

## v0.5 多轮会话与多 Agent — v0.5.0 已完成（2026-10-16）

- [x] `Session` 持久化 SPI：`SessionStore` trait + `InMemorySessionStore` / `FileSessionStore`（原子写、损坏显式失败）
- [x] `AgentRuntime::run_session`：历史作为输入、新消息追加回会话；失败 / 取消不修改会话
- [x] 子 Agent 编排（`AgentTool`）：agent-as-tool，取消传播、子运行独立预算与事件流、错误回灌
- [x] 真实联调场景 4/5（会话持久化恢复 / 子 Agent；日志 `docs/integration/2026-10-16-v0.5/`）

## v0.6 记忆与可观测性 — v0.6.0 已完成（2026-10-16）

- [x] 长期记忆 SPI：`Memory` trait + `MemoryRecord` / `MemoryId`（UUID v7）+ `InMemoryMemory`
  （关键字相关度 → 时间倒序 → id 的确定性排序；向量检索由生态实现）
- [x] JSON Schema 子集扩充：`const` / `allOf` / `anyOf` / `oneOf` / 数值边界（含 exclusive）/
  字符串长度（按字符）/ 数组约束（`minItems` / `maxItems` / `uniqueItems`）
- [x] tracing 结构化 span：`mycelium.run`（run_id/provider/model）、`mycelium.step`（step）、
  `mycelium.tool`（tool/call_id）；联调日志可直接观察嵌套 span
- [x] 真实联调重跑（五场景，日志 `docs/integration/2026-10-16-v0.6/`）

## v0.7 MCP 完整化 — v0.7.0 已完成（2026-10-16）

- [x] MCP 资源：`resources/list` / `resources/read`（`McpResource` / `McpResourceContent`；
  缺 `text` / `blob` 显式报错、分页与 tools 一致）
- [x] MCP 提示模板：`prompts/list` / `prompts/get`（`McpPrompt` / `McpPromptArgument` /
  `McpPromptMessage`；内容支持单对象或数组，text / image 映射）
- [x] streamable HTTP 传输：`McpClient::connect_http`（`Mcp-Session-Id` 传播、
  JSON 与 SSE 响应、SSE 按 id 匹配、通知接受 202）
- [x] 传输抽象重构：`Transport` trait（stdio / HTTP 共享全部能力方法）
- [x] 真实联调场景 6：官方 `server-everything` 实测 7 资源 / 4 提示模板
  （日志 `docs/integration/2026-10-16-v0.7/`）

### v0.7.x — 规划中（建议优先级从高到低）

- [ ] MCP 采样回调（server → client 请求）与服务端通知订阅（progress / list_changed）
- [ ] JSON Schema 剩余关键字（`pattern` / `multipleOf` / `format` 等）
- [ ] 外部记忆后端（向量检索 / 数据库，生态实现；SPI 已就绪）

## v1.0 API 稳定 — 规划中

- [ ] API 冻结流程：连续两个小版本无破坏性变更后进入冻结评审；此后破坏性变更走 RFC + 弃用周期
- [ ] 文档补全（含 v0.1 → v1.0 迁移指南）、示例集、性能基准（criterion）
- [ ] license 决策与发布流程（crates.io 首个发布）
- [ ] 发布检查单（SemVer 审计、CHANGELOG 规范化、docs.rs 元数据）

## 工程化 backlog

- [x] MSRV CI job（1.97.0）—— 已并入 v0.4.0
- [ ] `cargo-mutants` 接入 CI（自动化变异测试，替代手工证伪抽查）
- [ ] `cargo-deny`（依赖许可证与安全公告审计）
- [ ] 文档站点（mdBook / docs.rs 徽章）
