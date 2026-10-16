# mycelium 路线图

> 原则：每一档必须保持 `cargo build / test / clippy / fmt` 全绿，且 FC-Matrix 与测试同步演进。

## v0.1 架构骨架（本次交付）

- [x] Cargo workspace 分层：`mycelium-core` / `mycelium-model` / `mycelium-tools` / `mycelium-runtime` / `mycelium`
- [x] 领域类型：消息、内容块、工具协议、事件、标识符、配置
- [x] 三个 SPI 契约：`ModelProvider` / `Tool` / `RunHooks`
- [x] 错误分类矩阵与 `is_retryable` 语义
- [x] 运行状态机（全路径测试 + 负向路径）
- [x] FC-Matrix（`docs/spec/core.md`）与 35+ 条测试 1:1 映射
- [x] PBT（proptest）与证伪抽查
- [x] CI（fmt / clippy / test）+ rust-toolchain + rustfmt 配置

## v0.2 最小可运行循环（walking skeleton）— 已完成（2026-10-14）

- [x] `ModelDelta` 装配器（含 §6 流协议五项契约与负向测试）
- [x] 执行循环：请求模型 → 顺序执行工具 → 回灌 → 终止条件（无工具调用 / 预算耗尽）
- [x] 事件发射器（`seq` 连续、终态唯一；`FC-POST-052` 哨兵已被真实循环取代）
- [x] 取消传播（`CancellationToken` → 在途模型流与工具）与 `request_timeout`
- [x] 预算执行（步数 / 输出 Token）
- [x] `run_events`：以 `Stream<Item = AgentEvent>` 暴露过程事件
- [x] 首个官方 Provider 实现：`mycelium-provider-openai`（OpenAI 兼容 Chat Completions）
- [x] 工具入参 schema 校验（`ToolError::InvalidArguments`；JSON Schema 子集）
- [x] tracing 埋点（事件级；run_id / step / 工具名）与脚本化 mock provider 集成测试

## v0.3 策略与生态 — 已完成（2026-10-16）

- [x] 工具并发执行（`max_parallel_tool_calls`；`buffered` 保序回灌；事件发射串行化）
- [x] 工具超时（`tool_timeout` → `ToolError::Timeout` 错误结果回灌）
- [x] 权限策略层（`ToolPolicy` SPI + `AllowAll` / `DenyList` / `AllowList`；`Deny` 不执行并回灌原因）
- [x] `#[tool]` derive 宏（`mycelium-macros`：函数 → `Tool` 实现 + JSON Schema，编译期校验）
- [x] MCP 客户端适配（`mycelium-mcp`：stdio JSON-RPC、分页 `tools/list`、`tools/call`、代理进 `ToolRegistry`）
- [x] 模型思考过程支持（`Thinking` 内容块 / `ModelThinkingDelta` 事件 / `reasoning_tokens` / `thinking` 与
  `reasoning_effort` 请求参数；DeepSeek V4.1-Flash 真实联调验证）
- [x] 真实模型联调（DeepSeek `deepseek-flash`：纯文本、工具往返、流式事件、思考链）
- [x] 异步钩子（`RunHooks` 异步化，RFC-0001）—— 随 v0.4.0 发布
- [x] 结构化工具输出（多内容块 / 图片，RFC-0002）—— 随 v0.4.0 发布
- [ ] MCP 资源 / 提示模板 / HTTP 传输 → v0.4（进行中）

## v0.4 持久化与多 Agent — 进行中（2026-10-16 交付第一批）

- [x] 重试策略（`RetryPolicy`：可重试错误 + 指数退避 + `ModelRetry` 事件；退避可取消）
- [x] 多 Provider 降级（`FallbackProvider`：可重试错误按序降级，不可重试立即返回）
- [x] 时长预算（`BudgetKind::DurationMs`）
- [x] MSRV CI job（1.97.0 `cargo check`）
- [ ] `Session` 持久化 trait（存储后端可插拔）与恢复
- [ ] 子 Agent / 任务编排（工具化的 agent-as-tool）
- [ ] 长期记忆接口（向量检索等由生态实现）

## v1.0 API 稳定

- [ ] 冻结公共 API，语义化版本承诺（破坏性变更走 RFC + 弃用周期）
- [ ] 文档补全（含迁移指南）、示例集、性能基准（criterion）
- [ ] license 决策与发布流程（crates.io）

## 工程化 backlog

- [x] MSRV CI job（1.97.0 `cargo check --workspace --all-features`）
- [ ] `cargo-mutants` 接入 CI（自动化变异测试，替代手工证伪抽查）
- [ ] `cargo-deny`（依赖许可证与安全公告审计）
- [ ] 文档站点（mdBook / docs.rs 徽章）
