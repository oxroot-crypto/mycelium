# RFC-0001：`RunHooks` 异步化

- 状态：已接受（v0.4.0 实施）
- 日期：2026-10-16
- 影响面：`mycelium-runtime`（公开 API，**破坏性变更**）

## 动机

v0.3 的 `RunHooks` 是同步 trait，契约要求实现「不得阻塞等待」。该约束把常见需求
（把审计日志推送到远端、上报指标、落盘 transcript）挤到旁路队列里，实现负担高且容易
误用（在运行线程上做同步 IO 会阻塞整个 Run）。

## 设计

- 四个回调全部改为 `async fn`（`#[async_trait]` 生成 dyn 兼容签名，语义不变）：
  `on_run_start` / `on_step_start` / `on_tool_call` / `on_run_end`；
- 回调节点在运行任务内 `await`，因此**回调本身不得永不完成**（合规实现应尊重取消令牌
  或自行设置超时）；运行不因钩子内部错误而失败（钩子错误由实现自行记录）；
- 默认实现为空，未覆写的回调零开销；
- 「拦截与改写」（hook 修改请求/结果）本轮**不做**：改写需要定义优先级、冲突与重入语义，
  留待后续 RFC（若实现，将通过 `&mut` 上下文或显式的决策类型引入）。

## 破坏性变更与迁移

| v0.3 | v0.4 |
| :--- | :--- |
| `impl RunHooks for X { fn on_run_start(&self, ctx: &RunContext) { .. } }` | `#[async_trait] impl RunHooks for X { async fn on_run_start(&self, ctx: &RunContext) { .. } }` |

迁移成本：加 `#[async_trait]` 与 `async` 关键字；同步逻辑保持不变即可编译。

## 备选方案

1. 保持同步 + 内置旁路队列：隐藏生命周期与背压问题，否决；
2. 回调返回 `BoxFuture`：手写成本高、生态惯例差，否决（`async_trait` 已是事实标准）。
