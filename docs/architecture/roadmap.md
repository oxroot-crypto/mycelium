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

## v0.3 策略与生态

- [ ] 工具并发执行（`join_all`，回灌顺序与调用顺序一致）
- [ ] 权限策略层（审批回调、allow/deny 列表；`ToolError::Denied`）
- [ ] 异步钩子（`RunHooks` 支持异步拦截与改写）与指标导出
- [ ] `#[tool]` derive 宏（由函数签名生成 `ToolSpec`）
- [ ] MCP 客户端适配（把 MCP server 工具接入 `ToolRegistry`）
- [ ] 结构化工具输出（多内容块，图片等）

## v0.4 持久化与多 Agent

- [ ] `Session` 持久化 trait（存储后端可插拔）与恢复
- [ ] 多 Provider 路由与降级（`is_retryable` + 重试策略）
- [ ] 子 Agent / 任务编排（工具化的 agent-as-tool）
- [ ] 长期记忆接口（向量检索等由生态实现）

## v1.0 API 稳定

- [ ] 冻结公共 API，语义化版本承诺（破坏性变更走 RFC + 弃用周期）
- [ ] 文档补全（含迁移指南）、示例集、性能基准（criterion）
- [ ] license 决策与发布流程（crates.io）

## 工程化 backlog

- [ ] `cargo-mutants` 接入 CI（自动化变异测试，替代手工证伪抽查）
- [ ] MSRV CI job（`rust-version` 验证矩阵）
- [ ] `cargo-deny`（依赖许可证与安全公告审计）
- [ ] 文档站点（mdBook / docs.rs 徽章）
