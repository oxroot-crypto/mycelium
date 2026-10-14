# 形式化约束矩阵（FC-Matrix）：mycelium v0.1 架构骨架

> 本文件是 mycelium 形式化契约的**单一真实来源（Single Source of Truth）**。
> 任何行为变更必须遵循：**先改本契约 → 再改测试 → 再改实现 → 契约漂移审计**（见 §5）。
>
> - 版本：v0.1.0（2026-10-14）
> - 覆盖范围：`mycelium-core`、`mycelium-model`、`mycelium-tools`、`mycelium-runtime`、`mycelium`（门面）
> - 状态列取值：`Pending`（待验证）/ `Passed`（验证通过）/ `Planned`（未实现，不计入本次覆盖率）

## 0. 豁免判定与严重级别

- 本仓库交付物为**对外契约类 SDK**（公共 API 即契约）。依据 FSVDD §1.1，将以下推荐项升级为**强制**：
  测试与约束 1:1 对齐（100% 追溯）、状态机全路径遍历、基于属性的测试（PBT）、证伪抽查。
- 不适用豁免（FSVDD §1.2）：本仓库承载对外接口与状态流转。
- v0.1 仅覆盖**已实现**的纯逻辑约束（§2、§3）；执行循环、流装配、并发等未实现部分列入
  §4「计划约束」，不计入本次交付覆盖率，避免"契约已定义但无验证"的假象。

## 1. 术语与记法

| 符号 | 含义 |
| :--- | :--- |
| `TN` | 合法工具名集合：`{ s \| 1 ≤ chars(s) ≤ 64 ∧ ∀c ∈ s: c ∈ [A-Za-z0-9_-] }` |
| `S` | 运行状态集合：`{Created, Running, AwaitingModel, ExecutingTools, Completed, Failed, Cancelled}` |
| `E` | 运行事件集合：`{Start, RequestModel, ModelCompleted, ModelFailed, ToolUseRequested, ToolsCompleted, Complete, Fail, Cancel}` |
| `δ` | 转移函数 `δ(s, e) → Result<S, InvalidRunTransition>`，明细见 `docs/architecture/lifecycle.md` §3 |
| `Terminal` | 终止状态集合：`{Completed, Failed, Cancelled}` |
| `chars(s)` | 按 Unicode 标量（char）计数 |

## 2. 已实现约束（v0.1 可验证部分）

### 2.1 标识符与输入校验（`mycelium-core::ids`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-PRE-001` | 前置约束 | `ToolName::new(s)`：`s ∈ TN` 时 `Ok` 且 `as_str() == s`；否则按优先级返回 `Empty`（`s = ""`）、`TooLong { len, max: 64 }`（`chars(s) > 64`）、`InvalidCharacter`（存在域外字符）。边界：长度 1、64 合法；65 非法 | `ids::tests::tool_name_accepts_boundary_lengths`、`ids::tests::tool_name_rejects_empty`、`ids::tests::tool_name_rejects_too_long`、`ids::tests::tool_name_rejects_invalid_characters` | Passed |
| `FC-PRE-002` | 前置约束 | `ToolCallId::new(s)`：`trim(s) ≠ ""` 时 `Ok`（保留原值）；否则返回 `InvalidToolCallId` | `ids::tests::tool_call_id_rejects_blank`、`ids::tests::tool_call_id_accepts_non_blank` | Passed |
| `FC-PRE-003` | 前置约束 | `ModelId::new(s)`：`trim(s) ≠ ""` 时 `Ok`；否则返回 `InvalidModelId` | `ids::tests::model_id_rejects_blank` | Passed |
| `FC-PRE-004` | 前置约束 | `ToolName` 的反序列化路径与 `ToolName::new` 校验等价（非法输入反序列化必须失败） | `ids::tests::tool_name_deserialize_matches_new_validation` | Passed |

### 2.2 消息模型（`mycelium-core::message`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-PRE-010` | 前置约束 | `Message::new(role, content)`：`len(content) ≥ 1` 时 `Ok`；`content = []` 返回 `ContentError::EmptyContent`。反序列化路径同等校验（`{"role":"user","content":[]}` 必须失败）。`Message::text(role, "")` 合法（单文本块，文本可为空串） | `message::tests::new_rejects_empty_content`、`message::tests::empty_content_deserialization_is_rejected` | Passed |
| `FC-POST-001` | 后置约束 | `system_text` / `user_text` / `assistant_text` 分别生成 `Role::{System, User, Assistant}` 且恰含一个 `ContentBlock::Text`，文本保真（`Output == f(Input)`） | `message::tests::text_constructors_produce_role_and_single_text_block` | Passed |
| `FC-POST-002` | 后置约束 | `Message` JSON 往返（round-trip）恒等，且 wire 格式稳定：`user_text("hi")` ⇔ `{"role":"user","content":[{"type":"text","text":"hi"}]}` | `message::tests::message_json_roundtrip`、`message::tests::message_wire_format_is_stable` | Passed |

### 2.3 工具协议（`mycelium-core::tool`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-PRE-020` | 前置约束 | `ToolSpec::new(name, desc, schema)`：`schema` 为 JSON 对象时 `Ok`；`null` / 布尔 / 数字 / 字符串 / 数组时返回 `ToolError::InvalidSpec`（含工具名与类型描述） | `tool::tests::spec_rejects_non_object_schema`、`tool::tests::spec_accepts_object_schema` | Passed |
| `FC-POST-003` | 后置约束 | `ToolSpec` / `ToolCall` / `ToolChoice` JSON 往返恒等；wire 标签稳定（`ToolChoice` 使用 `{"mode": ...}` 包装） | `tool::tests::tool_wire_format_is_stable` | Passed |
| `FC-POST-007` | 后置约束 | `ToolCall::into_content_block()` 产出 `ContentBlock::ToolUse`，`id` / `name` / `input` 与调用方字段逐一相等 | `tool::tests::tool_call_into_block_preserves_fields` | Passed |

### 2.4 响应词汇（`mycelium-core::response`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-005` | 后置约束 | `Usage::total_tokens() == input_tokens + output_tokens`（常规范围） | `response::tests::total_tokens_sums` | Passed |
| `FC-POST-006` | 后置约束 | `Usage::total_tokens()` 溢出时**饱和**到 `u64::MAX`（不得 panic，不得回绕） | `response::tests::total_tokens_saturates` | Passed |
| `FC-POST-004` | 后置约束 | `StopReason` wire 格式为 snake_case 字符串且往返恒等 | `response::tests::stop_reason_wire_format_is_snake_case` | Passed |

### 2.5 运行状态机（`mycelium-core::run`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-STA-001` | 状态机 | `δ` 对全部 14 条合法转移边返回精确目标状态（S×E 全组合 63 组逐一比对） | `run::tests::transition_table_is_exhaustive` | Passed |
| `FC-STA-002` | 状态机拦截 | 表中不存在的 `(s, e)` 必须返回 `InvalidRunTransition { from: s, event: e }`（禁止静默忽略）。特别地：终止状态吸收一切事件 | `run::tests::terminal_states_absorb_all_events`、`run::tests::transition_table_is_exhaustive`（非法分支） | Passed |
| `FC-STA-003` | 状态机 | `is_terminal()` 对且仅对 `Terminal` 集合返回 `true`；合法边永不从终止状态出发 | `run::tests::is_terminal_classification` | Passed |

### 2.6 配置（`mycelium-core::config`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-PRE-030` | 前置约束 | 未提供 `model` 时 `build()` 返回 `ConfigError::MissingModel` | `config::tests::build_requires_model` | Passed |
| `FC-PRE-031` | 前置约束 | `max_steps`：`0` → `ConfigError::ZeroMaxSteps`；`1` 与 `u32::MAX` 合法 | `config::tests::max_steps_boundaries` | Passed |
| `FC-PRE-032` | 前置约束 | `request_timeout`：`0` → `ConfigError::ZeroRequestTimeout`；`1ns` 至 `u64::MAX` 秒合法 | `config::tests::request_timeout_boundaries` | Passed |
| `FC-PRE-033` | 前置约束 | `max_output_tokens`：`Some(0)` → `ConfigError::ZeroMaxOutputTokens`；`None` 与 `Some(1)` 合法 | `config::tests::max_output_tokens_boundaries` | Passed |
| `FC-PRE-034` | 前置约束 | `temperature`：`∈ [0.0, 2.0]` 且非 NaN 合法（含边界 `0.0`、`2.0`）；越界、`NaN`、`±∞` → `ConfigError::InvalidTemperature { value }` | `config::tests::temperature_boundaries` | Passed |
| `FC-POST-020` | 后置约束 | 最小合法构造的默认值恒定：`max_steps = 8`、`request_timeout = 60s`、`temperature = None`、`max_output_tokens = None`、`system_prompt = None`、`tool_choice = Auto` | `config::tests::defaults_are_stable` | Passed |

### 2.7 事件模型（`mycelium-core::event`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-010` | 后置约束 | `AgentEvent`（含全部 `AgentEventKind` 变体）JSON 往返恒等 | `event::tests::event_json_roundtrip_all_kinds` | Passed |
| `FC-POST-011` | 后置约束 | `AgentEventKind` wire 标签稳定（展平至事件信封同层，`type` 字段，snake_case；`run_started` / `model_text_delta` / `tool_call_finished` / `run_completed` 为固定契约） | `event::tests::event_wire_tags_are_stable` | Passed |

### 2.8 错误分类矩阵（`mycelium-core::error`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-ERR-001` | 异常处理 | `ModelError::is_retryable()`：`Transport` / `RateLimited` / `Timeout` 为 `true`；其余变体为 `false` | `error::tests::model_error_retryable_matrix` | Passed |
| `FC-ERR-002` | 异常处理 | HTTP 状态重试规则：`408` / `429` / `5xx` 为 `true`；`4xx`（其余）为 `false` | `error::tests::api_status_retryable_rules` | Passed |
| `FC-ERR-003` | 异常处理 | 错误 `Display` 必须包含定位上下文（工具名 / 模型状态码 / 功能名等），禁止空信息 | `error::tests::error_display_contains_context` | Passed |
| `FC-ERR-004` | 异常处理 | `AgentError` 自子错误转换（`From`）后 `source()` 保留根因 | `error::tests::agent_error_source_is_preserved` | Passed |

### 2.9 工具注册表（`mycelium-tools::registry`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-030` | 后置约束 | 重名注册返回 `RegistryError::DuplicateName`，且注册表内容不变（原工具仍可 `get`，`len` 不变） | `registry::tests::register_rejects_duplicate_and_keeps_original` | Passed |
| `FC-POST-031` | 后置约束 | `specs()` 按工具名字典序返回（确定性输出） | `registry::tests::specs_are_sorted_by_name` | Passed |
| `FC-POST-032` | 后置约束 | `get` 未注册名称返回 `None` | `registry::tests::get_unknown_returns_none` | Passed |
| `FC-POST-033` | 后置约束 | `len()` 与 `is_empty()` 与注册数量严格一致 | `registry::tests::len_and_is_empty_track_registrations` | Passed |
| `FC-INV-001` | 系统不变量 | 任意注册序列后，注册表内工具名始终唯一（由 `register` 维持） | `FC-POST-030` 同测试 + `properties`（区间组合） | Passed |

### 2.10 模型流类型（`mycelium-model`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-040` | 后置约束 | `ModelDelta`（含全部变体）JSON 往返恒等，非法 `ToolCallId` / `ToolName` 反序列化失败 | `delta::tests::delta_json_roundtrip` | Passed |
| `FC-POST-041` | 后置约束 | `stream_from_deltas(v)` 产出的流按原始顺序、不增不减地产出 `v` 的每一项（`Item = Ok(...)`） | `provider::tests::stream_from_deltas_preserves_order_and_count` | Passed |

### 2.11 运行时装配（`mycelium-runtime`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-050` | 后置约束 | `build()`：缺少 config → `BuildError::MissingConfig`；缺少 provider → `BuildError::MissingProvider`；两者齐备 → `Ok` | `builder::tests::missing_config_is_rejected`、`builder::tests::missing_provider_is_rejected`、`builder::tests::complete_builder_succeeds` | Passed |
| `FC-POST-051` | 后置约束 | builder 注册重复工具名 → `BuildError::Registry(DuplicateName)` | `builder::tests::duplicate_tool_name_is_rejected` | Passed |
| `FC-POST-052` | 后置约束 | 骨架阶段 `AgentRuntime::run` **必然**返回 `AgentError::Unimplemented { feature }`（显式失败，禁止静默成功）。实现循环时必须先更新本契约与对应测试（契约漂移哨兵） | `agent::tests::run_returns_unimplemented_in_skeleton` | Passed |
| `FC-POST-053` | 后置约束 | `Session::push` 保持追加顺序；`len()` 与 `messages()` 一致；`new` 后为空 | `session::tests::session_collects_messages_in_order`、`session::tests::session_starts_empty` | Passed |
| `FC-POST-054` | 后置约束 | `RunInput::from_user_text(t)` 生成恰一条 `Role::User` 单文本块消息，文本等于 `t` | `run::tests::run_input_from_user_text_shape` | Passed |

## 3. 属性测试（PBT，对应 FSVDD §4.2.1）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-INV-010` | 不变量（属性） | 对任意 Unicode 字符串 `s`：`ToolName::new(s).is_ok() ⇔ s ∈ TN`；且合法时 `as_str()` 保真、serde 往返恒等 | `properties::prop_tool_name_acceptance_matches_spec` | Passed |
| `FC-INV-011` | 不变量（属性） | 对任意 `t ∈ Terminal` 与任意 `e ∈ E`：`δ(t, e)` 必须为 `Err`（终止状态无出边） | `properties::prop_terminal_states_reject_all_events` | Passed |
| `FC-INV-012` | 不变量（属性） | 对任意 `(a, b) ∈ u64 × u64`：`Usage::new(a, b).total_tokens() == a.saturating_add(b)` | `properties::prop_usage_total_saturating` | Passed |
| `FC-INV-013` | 不变量（属性） | 对任意文本 `t`：`Message::user_text(t)` serde 往返恒等 | `properties::prop_message_roundtrip` | Passed |

## 4. 计划约束（v0.2+，未实现；不计入 v0.1 覆盖率）

以下条目在实现对应功能时**必须**移入 §2 并补齐测试映射：

- 事件顺序：单个 Run 内 `seq` 从 0 开始连续单调递增；每条事件信封的 `run_id` 与所属 Run 一致。
- 终态唯一性：每个 Run 恰好产生一条终态事件（`RunCompleted` / `RunFailed` / `RunCancelled`），且为最后一条。
- 工具配对：每个 `ToolCallStarted` 恰有唯一 `ToolCallFinished` 与之配对（`call_id` 相同）；不存在裸 start / 裸 finish。
- 模型流装配：成功流中 `Finished` 恰好一条且为最后一项；`Usage` 至多一条且在 `Finished` 之前；`ToolCallDelta.index` 从 0 连续；同一 `index` 的首个增量必带 `id` 与 `name`；`Finished` 之后不得再产出任何项；违反者归约为 `ModelError::InvalidResponse`。
- 预算不变量：`steps` / `Usage` 在单个 Run 内单调不减；任一超限立即以 `AgentError::BudgetExceeded` 终止且不再发起新模型请求。
- 取消及时性：`RunInput.cancel` 触发后，所有在途模型请求与工具执行必须在有限时间内结束，Run 进入 `Cancelled`。
- 工具入参校验：执行前按 `input_schema` 校验，不通过返回 `ToolError::InvalidArguments`（含工具名与失败字段路径）。
- 权限策略：策略拒绝返回 `ToolError::Denied`；内核默认不隐式放行。
- 并发（v0.3）：同一 Run 内工具结果顺序与调用顺序一致；跨 Run 无共享可变状态。
- 序列化兼容：核心类型新增字段需走非破坏性变更评审；枚举新增变体视为破坏性变更。

## 5. 变更协议与契约漂移审计

```mermaid
flowchart LR
    A[需求 / Bug] --> B[更新本契约]
    B --> C[更新测试]
    C --> D[更新实现]
    D --> E[漂移审计]
    E --> F[交付]
```

- **破坏性变更**：wire 标签、公开签名、枚举变体增减、错误变体语义变化。必须在本文件递增版本号并记录迁移说明。
- **非破坏性变更**：新增可选字段（默认值向后兼容）、新增测试。仍需同步更新本矩阵。
- **审计**：每次交付前核对「契约 - 测试 - 实现」三位一体：无孤儿实现、无失效契约、无孤立测试。

## 6. v0.1 验证报告（FC-Verification Report）

- 验证日期：2026-10-14
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace`：55 个单元 / 属性测试 + 4 个 doctest 全部通过；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过；
  - `cargo run -p mycelium --example basic`：装配成功，`run` 显式返回 `Unimplemented`（符合 FC-POST-052）。
- 追溯覆盖率：§2 与 §3 共 **44 条**约束，100% 映射到测试且全部 `Passed`（见状态列）。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F1 | `ToolName` 长度上限 64 → 65 | `ids::tests::tool_name_rejects_too_long` | 变红 ✓（已还原） |
| F2 | 重名注册改为静默覆盖 | `registry::tests::register_rejects_duplicate_and_keeps_original` | 变红 ✓（已还原） |
| F3 | 删除 `AwaitingModel --Cancel--> Cancelled` 转移边 | `run::tests::transition_table_is_exhaustive` | 变红 ✓（已还原） |
| F4 | `Usage::total_tokens` 饱和加法 → 回绕加法 | `response::tests::total_tokens_saturates` | 变红 ✓（已还原） |

- 备注：自动化变异测试（`cargo-mutants`）计划接入 CI，替代手工证伪抽查（见 roadmap 工程化 backlog）。
