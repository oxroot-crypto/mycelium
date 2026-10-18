# 形式化约束矩阵（FC-Matrix）：mycelium v0.1 架构骨架

> 本文件是 mycelium 形式化契约的**单一真实来源（Single Source of Truth）**。
> 任何行为变更必须遵循：**先改本契约 → 再改测试 → 再改实现 → 契约漂移审计**（见 §5）。
>
> - 版本：v1.1.0（2026-10-18）
> - 覆盖范围：`mycelium-core`、`mycelium-model`、`mycelium-tools`、`mycelium-runtime`、`mycelium-macros`、`mycelium-mcp`、`mycelium-provider-openai`、`mycelium`（门面）
> - 状态列取值：`Pending`（待验证）/ `Passed`（验证通过）/ `Planned`（未实现，不计入本次覆盖率）

## 0. 豁免判定与严重级别

- 本仓库交付物为**对外契约类 SDK**（公共 API 即契约）。依据 FSVDD §1.1，将以下推荐项升级为**强制**：
  测试与约束 1:1 对齐（100% 追溯）、状态机全路径遍历、基于属性的测试（PBT）、证伪抽查。
- 不适用豁免（FSVDD §1.2）：本仓库承载对外接口与状态流转。
- 覆盖基线：v1.1（本次新增 §2.30 文本向量化）；未实现部分列出 §4「计划约束」，不计入本次交付覆盖率。

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
| `FC-POST-008` | 后置约束 | `Message::tool_result` 生成 `Role::Tool` + 恰一个 `ToolResult` 块，`tool_call_id` / `content` / `is_error` 字段保真；wire 角色为 `"tool"` | `message::tests::tool_result_constructor_shape`、`message::tests::tool_role_wire_format` | Passed |

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
| `FC-POST-053` | 后置约束 | `Session::push` 保持追加顺序；`len()` 与 `messages()` 一致；`new` 后为空 | `session::tests::session_collects_messages_in_order`、`session::tests::session_starts_empty` | Passed |
| `FC-POST-054` | 后置约束 | `RunInput::from_user_text(t)` 生成恰一条 `Role::User` 单文本块消息，文本等于 `t` | `run::tests::run_input_from_user_text_shape` | Passed |

### 2.12 增量装配器（`mycelium-model::assembler`）

装配器把 `ModelStream` 增量装配为 `ModelResponse`；违反流协议一律归约为 `ModelError::InvalidResponse`（不得静默容忍）。

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-060` | 后置约束 | 成功装配：文本增量按序拼接为单个 `Text` 块；工具调用按 `index` 升序，`id` / `name` / `arguments` 精确映射 | `assembler::tests::assembles_text_usage_and_stop_reason`、`assembler::tests::assembles_tool_calls_in_index_order` | Passed |
| `FC-POST-061` | 后置约束 | `stop_reason` 与 `usage` 精确取自流；`Usage` 缺省时用量为 0 | `assembler::tests::assembles_text_usage_and_stop_reason` | Passed |
| `FC-POST-062` | 后置约束 | 无文本且有工具调用 → 消息仅含 `ToolUse` 块；完全为空 → 单空 `Text` 块（消息不变量保持） | `assembler::tests::tool_only_completion_has_no_text_block`、`assembler::tests::empty_completion_yields_empty_text_block` | Passed |
| `FC-POST-063` | 后置约束 | 工具参数为空串 → `{}`；非空 → JSON 解析为 `Value` | `assembler::tests::empty_arguments_fall_back_to_empty_object` | Passed |
| `FC-ERR-010` | 异常处理 | 流在收到 `Finished` 前结束 → `InvalidResponse`（含细节） | `assembler::tests::rejects_stream_without_finished` | Passed |
| `FC-ERR-011` | 异常处理 | `Finished` 之后再推入任何增量 → `InvalidResponse` | `assembler::tests::rejects_push_after_finished` | Passed |
| `FC-ERR-012` | 异常处理 | `Usage` 出现两次 → `InvalidResponse` | `assembler::tests::rejects_duplicate_usage` | Passed |
| `FC-ERR-013` | 异常处理 | `ToolCallDelta.index` 非连续（跳号 / 乱序）→ `InvalidResponse` | `assembler::tests::rejects_non_contiguous_tool_index` | Passed |
| `FC-ERR-014` | 异常处理 | 同一 `index` 的首个增量缺 `id` 或 `name`，或后续增量重复携带 `id` / `name` → `InvalidResponse` | `assembler::tests::rejects_missing_id_or_name_on_first_delta`、`assembler::tests::rejects_repeated_id_or_name` | Passed |
| `FC-ERR-015` | 异常处理 | 累积的工具参数不是合法 JSON → `InvalidResponse` | `assembler::tests::rejects_invalid_arguments_json` | Passed |

### 2.13 工具入参校验（`mycelium-tools::schema`）

校验按 **JSON Schema 子集**执行：`type`（含联合类型）、`properties`、`required`、`additionalProperties: false`、
`items`、`enum`；**未识别的关键字被忽略**（子集语义，文档化）。失败路径使用 JSONPath 风格（`$`、`$.a`、`$[0]`、`$.a[0].b`）。

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-070` | 后置约束 | 合法入参（子集语义）通过校验 | `schema::tests::accepts_valid_object` | Passed |
| `FC-POST-071` | 后置约束 | 数组 `items` 递归校验；失败路径精确到嵌套位置 | `schema::tests::validates_array_items`、`schema::tests::reports_nested_paths` | Passed |
| `FC-POST-072` | 后置约束 | 未识别的 schema 关键字被忽略（不误报） | `schema::tests::ignores_unknown_keywords` | Passed |
| `FC-ERR-020` | 异常处理 | 类型不匹配 → 违规，含期望类型与实际类型 | `schema::tests::rejects_type_mismatch` | Passed |
| `FC-ERR-021` | 异常处理 | 缺必填字段，或 `additionalProperties: false` 时存在多余字段 → 违规 | `schema::tests::rejects_missing_required`、`schema::tests::rejects_additional_property_when_disallowed` | Passed |
| `FC-ERR-022` | 异常处理 | `enum` 不匹配 → 违规（含允许集合） | `schema::tests::rejects_enum_mismatch` | Passed |

### 2.14 执行循环（`mycelium-runtime`）

目标形态：单次 Run = 独立 tokio 任务；顺序执行工具；预算与取消在循环边界强制。

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-080` | 后置约束 | 无工具调用：单次模型请求后成功返回；`RunOutcome.messages` 仅含 1 条助手消息 | `engine::tests::completes_without_tools` | Passed |
| `FC-POST-081` | 后置约束 | 工具循环：顺序执行、结果回灌、下一轮请求；消息序列 = 助手(tool_use) + 工具结果 + … + 助手文本 | `engine::tests::executes_tools_sequentially_and_feeds_back` | Passed |
| `FC-POST-082` | 后置约束 | `RunOutcome.usage` / `steps` 与脚本化请求精确吻合 | `engine::tests::outcome_tracks_usage_and_steps` | Passed |
| `FC-POST-083` | 后置约束 | `ModelRequest` 由配置与历史精确构造（model / system_prompt / tools / tool_choice / temperature / max_output_tokens / 消息列表） | `engine::tests::request_matches_config_and_history` | Passed |
| `FC-POST-084` | 后置约束 | 成功路径钩子顺序：`on_run_start` → `on_step_start` → `on_tool_call` → `on_run_end` | `engine::tests::hooks_fire_in_expected_order` | Passed |
| `FC-POST-085` | 后置约束 | `run_events` 产出的流按序给出全部事件；`wait` / `await_result` 与 `run` 返回一致的结果 | `agent::tests::run_events_stream_matches_wait` | Passed |
| `FC-ERR-030` | 异常处理 | 工具未注册 → 错误结果回灌（`is_error=true`），运行继续 | `engine::tests::unknown_tool_error_is_fed_back` | Passed |
| `FC-ERR-031` | 异常处理 | 入参校验失败 → 错误结果回灌（含工具名与路径） | `engine::tests::invalid_arguments_error_is_fed_back` | Passed |
| `FC-ERR-032` | 异常处理 | 工具执行失败 → 错误结果回灌 | `engine::tests::failing_tool_error_is_fed_back` | Passed |
| `FC-ERR-033` | 异常处理 | 模型层错误 → `Err(Model(..))` 终止循环并发出 `RunFailed` 终态事件 | `engine::tests::model_error_terminates_run` | Passed |
| `FC-ERR-034` | 异常处理 | 步数预算：第 `max_steps + 1` 次请求**发起前**终止 `BudgetExceeded{Steps, limit, observed}` | `engine::tests::step_budget_exceeded` | Passed |
| `FC-ERR-035` | 异常处理 | 输出 Token 预算：累计输出超过上限 → `BudgetExceeded{OutputTokens}` | `engine::tests::output_token_budget_exceeded` | Passed |
| `FC-ERR-036` | 异常处理 | 单次请求超时（`request_timeout`）→ `Err(Model(Timeout))` | `engine::tests::request_timeout_maps_to_model_timeout` | Passed |
| `FC-ERR-037` | 异常处理 | 取消（启动前 / 模型流进行中 / 工具阶段）→ `Err(Cancelled)` + `RunCancelled` 终态事件 | `engine::tests::cancel_before_start`、`engine::tests::cancel_during_stream`、`engine::tests::cancel_during_tool` | Passed |
| `FC-ERR-038` | 异常处理 | 工具返回 `ToolError::Cancelled` → `Err(Cancelled)` | `engine::tests::tool_cancelled_error_yields_cancelled` | Passed |
| `FC-INV-020` | 系统不变量 | 事件 `seq` 从 0 连续递增且 `run_id` 与所属 Run 一致 | `engine::tests::events_sequence_invariants` | Passed |
| `FC-INV-021` | 系统不变量 | 首条事件为 `RunStarted`；终态事件恰一条且为最后一条 | `engine::tests::events_sequence_invariants`、`engine::tests::model_error_terminates_run` | Passed |
| `FC-INV-022` | 系统不变量 | `ToolCallStarted` 与 `ToolCallFinished` 以 `call_id` 一一配对（取消路径同样补齐） | `engine::tests::tool_events_are_paired`、`engine::tests::cancel_during_tool` | Passed |

### 2.15 OpenAI 兼容 Provider（`mycelium-provider-openai`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-090` | 后置约束 | 请求体映射：`system` / `user` / `assistant` / `tool` 消息、助手 `tool_calls`、`tools`、`tool_choice` 四态、`max_tokens`、`temperature` | `request::tests::maps_messages_and_tools`、`request::tests::maps_tool_choice_variants` | Passed |
| `FC-POST-091` | 后置约束 | SSE 解析鲁棒性：跨 chunk 分片、`data:` 前缀、`[DONE]` 哨兵、忽略注释行与空行 | `stream::tests::sse_parser_handles_split_chunks` | Passed |
| `FC-POST-092` | 后置约束 | chunk → `ModelDelta` 映射：`content`、`tool_calls` 分片（index/id/name/arguments）、`finish_reason`、`usage` | `stream::tests::maps_chunks_to_deltas` | Passed |
| `FC-POST-093` | 后置约束 | 自身产出的流满足装配器契约：`Usage`（若有）先于 `Finished`，`Finished` 恰一条且为最后一项 | `e2e::e2e_stream_satisfies_assembler_contract` | Passed |
| `FC-POST-094` | 后置约束 | 端到端（本地 stub server）：经 HTTP 流式完成文本响应与工具调用往返 | `e2e::e2e_text_completion`、`e2e::e2e_tool_round_trip` | Passed |
| `FC-ERR-040` | 异常处理 | 非 2xx → `Api{status, code, message}`；429 → `RateLimited{retry_after_ms}` | `stream::tests::maps_error_body`、`provider::tests::maps_429_to_rate_limited` | Passed |
| `FC-ERR-041` | 异常处理 | 网络错误 → `Transport` | `provider::tests::maps_connection_error_to_transport` | Passed |
| `FC-ERR-042` | 异常处理 | 缺失 API key → 构建期显式错误（拒绝启动） | `provider::tests::missing_api_key_is_rejected` | Passed |
| `FC-ERR-043` | 异常处理 | 流在未收到 `finish_reason` 与 `[DONE]` 的情况下中断 → `Transport` | `stream::tests::truncated_stream_is_transport_error` | Passed |

### 2.16 工具并发与超时（`mycelium-runtime`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-100` | 后置约束 | `max_parallel_tool_calls = 1`（默认）：工具严格顺序执行，事件形如 `Started(c1) → Finished(c1) → Started(c2) → Finished(c2)` | `engine::tests::sequential_tool_calls_by_default` | Passed |
| `FC-POST-101` | 后置约束 | 并发上限 `N > 1`：工具**同时**启动（在途数 ≤ N），完成顺序可为任意，但结果按调用顺序回灌且**内容与调用一一对应**（`ToolResult` 的 `call_id` 与 `content` 均不得错位） | `engine::tests::concurrent_tools_are_parallel_and_ordered`、`engine::tests::concurrent_partial_failure_keeps_order` | Passed |
| `FC-POST-102` | 后置约束 | 并发下事件 `seq` 仍从 0 连续且互不重复（发射串行化）；`ToolCallStarted/Finished` 配对完整 | `engine::tests::concurrent_events_stay_serialized` | Passed |
| `FC-ERR-045` | 异常处理 | `tool_timeout` 超限 → `ToolError::Timeout` 语义错误结果回灌，运行继续 | `engine::tests::tool_timeout_is_fed_back` | Passed |
| `FC-ERR-046` | 异常处理 | 并发下个别工具失败/超时不阻塞其他工具；回灌顺序仍按调用顺序 | `engine::tests::concurrent_partial_failure_keeps_order` | Passed |

### 2.17 权限策略（`mycelium-tools::policy`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-110` | 后置约束 | `Allow`（默认 `AllowAll`）：工具正常执行，行为与 v0.2 一致 | `engine::tests::policy_allow_executes_tool`、`policy::tests::allow_all_permits_everything` | Passed |
| `FC-ERR-047` | 异常处理 | `Deny { reason }`：工具**不被执行**（执行次数为 0），拒绝原因以错误结果回灌 | `engine::tests::policy_deny_is_fed_back_without_execution` | Passed |
| `FC-POST-111` | 后置约束 | `DenyList`：列表内工具拒绝、其余允许；`AllowList`：仅列表内工具允许、其余拒绝 | `policy::tests::deny_list_rejects_listed_names`、`policy::tests::allow_list_allows_only_listed_names` | Passed |
| `FC-POST-112` | 后置约束 | 策略在入参校验与工具执行**之前**调用；策略检查支持取消竞争 | `engine::tests::policy_deny_is_fed_back_without_execution`（校验顺序隐含）、`policy::tests::check_called_before_execution` | Passed |

### 2.18 `#[tool]` 派生宏（`mycelium-macros`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-120` | 后置约束 | 由函数签名生成 `ToolSpec`：名称 = 函数名（可被 `name` 覆盖）、描述 = 文档注释（可被 `description` 覆盖）；schema 由参数类型映射（`String`/整数/`f32|f64`/`bool`/`Vec<T>`/`Option<T>`），`Option<T>` 不计入 `required`；输出 `{"type":"object","properties":…,"required":[…],"additionalProperties":false}` | `derive::tests::spec_matches_signature`、`derive::tests::attributes_override_name_and_description` | Passed |
| `FC-POST-121` | 后置约束 | 执行路径：入参反序列化 → 调用原函数（async 与 sync 均支持）→ `String` 结果直传、`ToolOutput` 直传、其余类型 JSON 序列化；函数 `Err` → `ToolError::Execution` | `derive::tests::executes_async_function`、`derive::tests::executes_sync_function`、`derive::tests::string_and_tool_output_passthrough` | Passed |
| `FC-ERR-048` | 异常处理 | 入参反序列化失败（类型不符 / 缺必填）→ `ToolError::InvalidArguments`（含参数名与原错误） | `derive::tests::rejects_invalid_arguments` | Passed |
| `FC-POST-122` | 后置约束 | 宏在编译期拒绝非法定义（非法工具名、引用 / 模式 / mut 参数、非 `Result` 返回类型、泛型函数）；属性解析拒绝未知参数与非字面量 | `macros::tests::rejects_invalid_definitions`、`macros::tests::parses_attributes`、`macros::tests::expands_valid_function`（trybuild 编译失败快照计划 v0.4） | Passed |

### 2.19 MCP 客户端适配（`mycelium-mcp`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-130` | 后置约束 | 握手：`initialize`（协议版本 + clientInfo）→ 记录 `serverInfo` → `notifications/initialized`（无 id） | `client::tests::initialize_handshake_records_server_info` | Passed |
| `FC-POST-131` | 后置约束 | `tools/list` → `ToolSpec`（名称经 `ToolName` 校验、`description` 缺省为空、`inputSchema` 必须是对象）；支持 `nextCursor` 分页且页数有硬上限 | `client::tests::list_tools_maps_specs`、`client::tests::list_tools_follows_pagination`、`client::tests::invalid_tool_schema_is_rejected` | Passed |
| `FC-POST-132` | 后置约束 | `tools/call` → `ToolOutput`：`content` 数组中 text 块按序拼接；`isError: true` → 错误结果 | `client::tests::call_tool_maps_text_content`、`client::tests::call_tool_maps_error_flag` | Passed |
| `FC-POST-133` | 后置约束 | `tools()` 产出的 `Arc<dyn Tool>` 可注册进 `ToolRegistry` 并通过 `Tool::call` 调用（代理到 MCP 服务） | `client::tests::mcp_tools_register_into_registry` | Passed |
| `FC-ERR-050` | 异常处理 | JSON-RPC 错误响应 → `McpError::Rpc { code, message }`；工具代理层归约为 `ToolError::Execution` | `client::tests::jsonrpc_error_maps_to_rpc_error` | Passed |
| `FC-ERR-051` | 异常处理 | 传输关闭 / 进程退出 → 所有未完成请求显式失败（闭合已观察时为 `TransportClosed`；写失败先发生时可为携带 IO 详情的 `Transport`），不得悬挂 | `client::tests::transport_close_fails_pending_requests` | Passed |

### 2.20 思考（推理）过程支持

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-140` | 后置约束 | 思考增量装配：`ThinkingDelta` 按序拼接为单个 `Thinking` 块，且位于 `Text` 块**之前**；无思考时消息不含 Thinking 块 | `assembler::tests::assembles_thinking_block_before_text`、`e2e::e2e_text_completion` | Passed |
| `FC-POST-141` | 后置约束 | `Usage.reasoning_tokens` 取自提供方细化统计（缺省 0）；思考 Token 已包含在输出 Token 内，`total_tokens` 不重复累加 | `response::tests::reasoning_tokens_do_not_double_count`、`assembler::tests::usage_reasoning_tokens_propagate`、`e2e::e2e_text_completion` | Passed |
| `FC-POST-142` | 后置约束 | 思考事件：运行时对每个思考增量发射 `ModelThinkingDelta`（逐片、与其他事件共享 `seq` 连续性） | `engine::tests::thinking_deltas_are_emitted_and_assembled` | Passed |
| `FC-POST-143` | 后置约束 | 思考内容不回传：OpenAI 兼容请求映射剔除助手历史中的 `Thinking` 块（只读观测语义） | `request::tests::strips_thinking_blocks_from_history` | Passed |
| `FC-POST-144` | 后置约束 | 请求映射：`ReasoningOptions.enabled` → `thinking: {"type": ...}`；`effort` → `reasoning_effort`；未配置时不发送（使用提供方默认） | `request::tests::maps_reasoning_options` | Passed |
| `FC-POST-145` | 后置约束 | 提供方流映射：`delta.reasoning_content` → `ThinkingDelta`；`completion_tokens_details.reasoning_tokens` → `Usage` 细化字段 | `stream::tests::maps_reasoning_content_and_tokens` | Passed |
| `FC-POST-146` | 后置约束 | `ContentBlock::Thinking` 与 `AgentEventKind::ModelThinkingDelta` 的 wire 标签稳定（`thinking` / `model_thinking_delta`） | `message::tests::thinking_block_wire_format`、`event::tests::event_wire_tags_are_stable` | Passed |
| `FC-PRE-035` | 前置约束 | `AgentConfig` 校验 `reasoning.effort`：`1..=32` 个 ASCII 字母 / 数字 / `_` / `-`，否则 `ConfigError::InvalidReasoningEffort` | `config::tests::reasoning_effort_validation` | Passed |

### 2.21 结构化工具结果（RFC-0002）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-150` | 后置约束 | `ContentBlock::ToolResult.content` 为 `Vec<ToolResultBlock>`；`ToolResultBlock` 的 `text` / `image` wire 标签稳定；`Message::tool_result` 生成单文本块、`tool_result_blocks` 原样携带多块 | `message::tests::tool_result_constructor_shape`、`message::tests::tool_result_blocks_wire_format` | Passed |
| `FC-POST-151` | 后置约束 | `ToolOutput` 块语义：`success/error` 为单文本块；`blocks/error_blocks` 接受多块；`text()` 汇总文本块（换行连接）；`into_blocks` 无损搬运 | `tool::tests::output_text_and_blocks_semantics` | Passed |
| `FC-POST-152` | 后置约束 | 运行时回灌多块：工具返回的文本与图片块**原样**进入消息历史（不降级、不丢弃） | `engine::tests::structured_tool_result_is_fed_back_intact` | Passed |
| `FC-POST-153` | 后置约束 | OpenAI 兼容映射展平：文本块直传、图片块 → `[image: <media_type>]` 占位 | `request::tests::flattens_structured_tool_results` | Passed |
| `FC-POST-154` | 后置约束 | MCP `content` 映射：`text` / `image`（`mimeType`+`data`）一一映射；未知类型与缺字段块忽略（不报错） | `client::tests::call_tool_maps_structured_content` | Passed |

### 2.22 重试策略与时长预算

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-PRE-036` | 前置约束 | `RetryPolicy` 校验：`max_attempts >= 1`、退避 > 0、`initial <= max`；否则对应 `ConfigError` 变体 | `config::tests::retry_policy_validation` | Passed |
| `FC-PRE-037` | 前置约束 | `max_duration == Some(0)` → `ConfigError::ZeroMaxDuration` | `config::tests::max_duration_boundaries` | Passed |
| `FC-POST-160` | 后置约束 | 可重试错误（`ModelError::is_retryable`）在策略内自动重试：每次失败后发射 `ModelRetry { attempt, delay_ms, reason }` 并按指数退避等待；成功后返回 | `engine::tests::retries_retryable_model_errors` | Passed |
| `FC-POST-161` | 后置约束 | 不可重试错误不触发重试（仅一次模型调用） | `engine::tests::non_retryable_error_is_not_retried` | Passed |
| `FC-ERR-060` | 异常处理 | 尝试次数耗尽 → 返回最后一次错误（含事件序列可追溯） | `engine::tests::retry_exhausted_returns_last_error` | Passed |
| `FC-ERR-061` | 异常处理 | 退避等待期间取消 → `Err(Cancelled)`（退避可被取消打断，不得睡满） | `engine::tests::cancel_during_retry_backoff` | Passed |
| `FC-POST-162` | 后置约束 | 退避序列：`initial * 2^(attempt-1)`，封顶 `max_backoff`（溢出安全） | `config::tests::backoff_growth_is_exponential_and_capped` | Passed |
| `FC-ERR-062` | 异常处理 | 时长预算：`elapsed > max_duration` → `BudgetExceeded{DurationMs}`，且**不再发起**新模型请求 | `engine::tests::duration_budget_exceeded` | Passed |

### 2.23 异步钩子与多 Provider 降级（RFC-0001）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-170` | 后置约束 | `RunHooks` 回调为 `async fn` 并在运行任务内 `await`（异步 I/O 不阻塞运行语义）；调用顺序与 v0.3 一致（run_start → step_start → tool_call → run_end） | `engine::tests::hooks_fire_in_expected_order`、`engine::tests::async_hooks_can_await_io` | Passed |
| `FC-POST-171` | 后置约束 | `FallbackProvider` 按序降级：某提供方返回**可重试**错误时调用下一个；不可重试错误立即返回（不降级） | `fallback::tests::falls_back_on_retryable_error`、`fallback::tests::does_not_fall_back_on_fatal_error` | Passed |
| `FC-POST-172` | 后置约束 | 全部提供方失败 → 返回最后一个错误；空列表在构建期拒绝 | `fallback::tests::returns_last_error_when_all_fail`、`fallback::tests::rejects_empty_provider_list` | Passed |

### 2.24 多轮会话与持久化

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-180` | 后置约束 | `run_session`：会话历史 + 新输入消息共同作为运行输入；成功后**新输入与运行新增消息**按序追加回会话；失败/取消时**不修改会话** | `agent::tests::run_session_appends_history_and_new_messages`、`agent::tests::run_session_failure_leaves_session_untouched` | Passed |
| `FC-POST-181` | 后置约束 | `SessionStore` 语义：`load` 缺失 → `Ok(None)`；`save` 覆盖写；`delete` 幂等（不存在视为成功）；`InMemorySessionStore` 满足全部语义 | `store::tests::in_memory_round_trip`、`store::tests::load_missing_is_none`、`store::tests::delete_is_idempotent` | Passed |
| `FC-POST-182` | 后置约束 | `FileSessionStore`：每会话一个 JSON 文件；往返恒等（含消息 / 工具结果块 / 思考块）；原子写（临时文件 + 重命名） | `store::tests::file_round_trip`、`store::tests::file_round_trip_preserves_structured_content` | Passed |
| `FC-ERR-070` | 异常处理 | 存储 IO 失败 → `SessionStoreError::Io`（含路径）；文件损坏 → `SessionStoreError::Corrupted`（显式失败，不静默） | `store::tests::corrupted_file_is_rejected` | Passed |
| `FC-POST-183` | 后置约束 | `Session` JSON 往返恒等（`id` / `created_at_ms` / `messages`） | `session::tests::session_json_roundtrip` | Passed |

### 2.25 子 Agent 编排（agent-as-tool）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-190` | 后置约束 | `AgentTool::spec`：名称 / 描述由调用方给定；schema 固定为 `{"type":"object","properties":{"prompt":{"type":"string"}},"required":["prompt"]}` | `agent_tool::tests::spec_shape` | Passed |
| `FC-POST-191` | 后置约束 | 调用语义：子运行时以 `prompt` 为用户消息运行，最终助手文本作为工具结果；子运行使用独立预算与独立事件流（父事件流仅含工具级事件） | `engine::tests::agent_tool_runs_child_and_returns_final_text` | Passed |
| `FC-ERR-071` | 异常处理 | 子运行失败 → 错误结果回灌（父运行继续）；子运行取消 → `ToolError::Cancelled`（归约为父运行 `Cancelled`） | `engine::tests::agent_tool_child_failure_is_fed_back` | Passed |
| `FC-ERR-072` | 异常处理 | 取消传播：父运行取消令牌注入子运行 `RunInput.cancel`（父取消可打断子运行的在途请求） | `engine::tests::agent_tool_propagates_cancel_to_child` | Passed |
| `FC-ERR-073` | 异常处理 | 入参缺 `prompt`（或非字符串）→ `ToolError::InvalidArguments` | `agent_tool::tests::missing_prompt_is_rejected` | Passed |

### 2.26 长期记忆（`mycelium-runtime::memory`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-200` | 后置约束 | `Memory` SPI：`remember` 追加并返回含生成 `id` 的记录；`recall` 按「相关度降序 → 时间倒序 → id」返回至多 `limit` 条（无匹配时即最近优先）；`forget` 幂等 | `memory::tests::recall_ranks_by_keyword_then_recency`、`memory::tests::recall_respects_limit`、`memory::tests::forget_is_idempotent` | Passed |
| `FC-POST-201` | 后置约束 | 相关度计算确定性：同一存储上重复 `recall` 返回完全相同的顺序；空/纯符号查询等价于「最近优先」 | `memory::tests::recall_is_deterministic`、`memory::tests::empty_query_returns_recent_first` | Passed |
| `FC-POST-202` | 后置约束 | `MemoryRecord` JSON 往返恒等；`MemoryId` 为 UUID v7（时间有序） | `memory::tests::record_json_roundtrip` | Passed |

### 2.27 JSON Schema 子集扩充（`mycelium-tools::schema`）

新增关键字：`const`、`allOf` / `anyOf` / `oneOf`、`minimum` / `maximum` / `exclusiveMinimum` /
`exclusiveMaximum`（数值语义，draft-07）、`minLength` / `maxLength`（按 Unicode 标量计数）、
`minItems` / `maxItems` / `uniqueItems`；未识别关键字仍按子集语义忽略。

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-210` | 后置约束 | 组合关键字语义：`allOf` 全部分支通过；`anyOf` 至少一个分支通过（否则违规）；`oneOf` 恰好一个分支通过（0 或 ≥2 均违规，报实际匹配数） | `schema::tests::all_of_requires_every_branch`、`schema::tests::any_of_requires_one_branch`、`schema::tests::one_of_requires_exactly_one_branch` | Passed |
| `FC-POST-211` | 后置约束 | 数值边界（含边界点）：`minimum` / `maximum` 为闭边界、`exclusiveMinimum` / `exclusiveMaximum` 为开边界；字符串长度按**字符**计数（多字节字符不按字节） | `schema::tests::numeric_bounds_are_inclusive`、`schema::tests::numeric_bounds_exclusive`、`schema::tests::string_length_counts_characters` | Passed |
| `FC-POST-212` | 后置约束 | 数组约束：`minItems` / `maxItems` 按元素数；`uniqueItems: true` 拒绝重复元素（报重复下标）；`const` 要求精确相等 | `schema::tests::array_item_counts_are_enforced`、`schema::tests::unique_items_rejects_duplicates`、`schema::tests::const_requires_exact_match` | Passed |

### 2.28 tracing 结构化 span（`mycelium-runtime::engine`）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-220` | 后置约束 | 运行期发出结构化 span 并携带定位字段：`mycelium.run`（`run_id` / `provider` / `model`）、`mycelium.step`（`step`）、`mycelium.tool`（`tool` / `call_id`）；span 生命周期与实际作用域一致（NEW/CLOSE 可观测） | `engine::tests::run_emits_structured_spans` | Passed |

### 2.29 MCP 资源、提示与 HTTP 传输

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-POST-230` | 后置约束 | `resources/list` → `McpResource`（`uri` / `name` 必填；`description` / `mimeType` 可选）；分页语义同 `tools/list`；缺 `name` / `uri` → `Protocol` | `client::tests::list_resources_maps_specs`、`client::tests::list_resources_follows_pagination` | Passed |
| `FC-POST-231` | 后置约束 | `resources/read` → `McpResourceContent`：`uri` 必有；`text` 与 `blob` 至少一项，否则 `Protocol`（显式失败） | `client::tests::read_resource_maps_contents`、`client::tests::read_resource_rejects_malformed_entry` | Passed |
| `FC-POST-232` | 后置约束 | `prompts/list` → `McpPrompt`（`name` 必填；`arguments[].name` 必填、`required` 缺省 `false`） | `client::tests::list_prompts_maps_specs` | Passed |
| `FC-POST-233` | 后置约束 | `prompts/get` → `McpPromptMessage`（`role` + 内容块：单对象或数组；`text` / `image` → `ToolResultBlock`；未知类型忽略） | `client::tests::get_prompt_maps_messages` | Passed |
| `FC-POST-234` | 后置约束 | HTTP 传输（streamable HTTP）：initialize 捕获 `Mcp-Session-Id` 并附加到后续请求；`application/json` 与 `text/event-stream` 响应均可解析（SSE 按 id 匹配响应、其余消息忽略）；通知接受 202 | `http::tests::handshake_and_session_header`、`http::tests::sse_response_is_matched_by_id`、`http::tests::notification_accepts_202` | Passed |
| `FC-ERR-090` | 异常处理 | HTTP 非 2xx → `Transport`（含状态码）；SSE 流在匹配响应前结束 → `TransportClosed` | `http::tests::http_error_status_maps_to_transport`、`http::tests::sse_ending_before_response_is_closed` | Passed |
| `FC-ERR-091` | 异常处理 | HTTP 传输中的 JSON-RPC error 响应 → `McpError::Rpc`（与 stdio 语义一致） | `http::tests::jsonrpc_error_over_http` | Passed |

### 2.30 文本向量化（Embedding，`mycelium-model` / `mycelium-provider-openai`）

向量化 SPI 与 OpenAI 兼容实现：请求**构造即校验**（`EmbeddingRequest`），响应解析后按输入数逐条校验
（`EmbeddingResponse::new`）；任何结构违约一律归约为 `ModelError::InvalidResponse`（禁止静默容忍）。
`EmbeddingRequest` 不携带取消令牌：调用方可自行施加 `tokio::time::timeout`，丢弃 future 即取消在途连接。

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-PRE-040` | 前置约束 | `EmbeddingRequest::new(model, inputs)`：`1 ≤ len(inputs) ≤ 2048` 且每项 `trim ≠ ""` 时 `Ok` 且输入逐项保真；`len = 0` → `EmptyInputs`；`len > 2048` → `TooManyInputs { len, max: 2048 }`；首个空白项下标 → `BlankInput { index }`。边界：1 / 2048 合法，2049 非法 | `embedding::tests::new_accepts_single_and_max_inputs`、`embedding::tests::new_rejects_empty_inputs`、`embedding::tests::new_rejects_too_many_inputs`、`embedding::tests::new_rejects_blank_input` | Passed |
| `FC-PRE-041` | 前置约束 | `EmbeddingRequest::with_dimensions(d)`：`d = 0` → `ZeroDimensions`；`d ≥ 1` → `Ok` 且 `dimensions() == Some(d)`；未调用时为 `None` | `embedding::tests::with_dimensions_rejects_zero`、`embedding::tests::accessors_preserve_values` | Passed |
| `FC-POST-240` | 后置约束 | `EmbeddingResponse::new(input_count, model, embeddings, usage)`：条数与 `input_count` 相等、`index` 恰为 `0..input_count` 的排列（无重复 / 越界）、向量非空、维度一致且数值有限时 `Ok`；结果按 `index` 升序（`embeddings[i].index == i`），`model` / `usage` 原样保留；违约为 6 类显式错误（见 `EmbeddingResponseError`） | `embedding::tests::response_accepts_and_sorts_by_index`、`embedding::tests::response_rejects_length_mismatch`、`embedding::tests::response_rejects_index_out_of_range`、`embedding::tests::response_rejects_duplicate_index`、`embedding::tests::response_rejects_empty_vector`、`embedding::tests::response_rejects_dimension_mismatch`、`embedding::tests::response_rejects_non_finite_value` | Passed |
| `FC-POST-241` | 后置约束 | OpenAI 兼容请求体映射：`{model, input: [全部输入按序]}`；`dimensions` 仅在 `Some` 时携带；不发送多余字段 | `embedding::tests::maps_request_body` | Passed |
| `FC-POST-242` | 后置约束 | 响应映射：`data[].index` / `data[].embedding` 精确映射；`usage.prompt_tokens` / `usage.total_tokens` 取数（字段缺省 0，整个 `usage` 缺失时 `None`）；`model` 回显（可缺失） | `embedding::tests::parses_response_with_usage_and_model`、`embedding::tests::parses_response_without_usage_and_model` | Passed |
| `FC-ERR-100` | 异常处理 | Embedding 路径错误映射与聊天路径一致：非 2xx → `Api{status, code, message}`；429 → `RateLimited{retry_after_ms}`（含 `Retry-After`）；网络失败 → `Transport` | `e2e::e2e_embeddings_error_status_maps_to_api_error`、`provider::tests::embeddings_connection_error_maps_to_transport` | Passed |
| `FC-ERR-101` | 异常处理 | 响应体结构违约（非法 JSON、缺 `data`、条数不符、`index` 越界或重复、空向量、维度不一致、非有限数值）→ `ModelError::InvalidResponse`（含细节），禁止静默容忍 | `embedding::tests::rejects_malformed_json`、`embedding::tests::rejects_missing_data`、`embedding::tests::rejects_out_of_range_index`、`embedding::tests::rejects_duplicate_index`、`embedding::tests::rejects_empty_vector`、`embedding::tests::rejects_inconsistent_dimensions`、`embedding::tests::rejects_non_finite_values` | Passed |

## 3. 属性测试（PBT，对应 FSVDD §4.2.1）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-INV-010` | 不变量（属性） | 对任意 Unicode 字符串 `s`：`ToolName::new(s).is_ok() ⇔ s ∈ TN`；且合法时 `as_str()` 保真、serde 往返恒等 | `properties::prop_tool_name_acceptance_matches_spec` | Passed |
| `FC-INV-011` | 不变量（属性） | 对任意 `t ∈ Terminal` 与任意 `e ∈ E`：`δ(t, e)` 必须为 `Err`（终止状态无出边） | `properties::prop_terminal_states_reject_all_events` | Passed |
| `FC-INV-012` | 不变量（属性） | 对任意 `(a, b) ∈ u64 × u64`：`Usage::new(a, b).total_tokens() == a.saturating_add(b)` | `properties::prop_usage_total_saturating` | Passed |
| `FC-INV-013` | 不变量（属性） | 对任意文本 `t`：`Message::user_text(t)` serde 往返恒等 | `properties::prop_message_roundtrip` | Passed |
| `FC-INV-030` | 不变量（属性） | 对任意文本增量序列 `v`：装配结果 = 单 `Text` 块且文本等于 `v` 的顺序拼接 | `assembler::tests::prop_text_deltas_concatenate` | Passed |
| `FC-INV-040` | 不变量（属性） | 对任意输入列表 `v`：`EmbeddingRequest::new(model, v).is_ok() ⇔ (1 ≤ len(v) ≤ 2048 ∧ ∀s ∈ v: trim(s) ≠ "")`；合法时输入逐项保真 | `embedding::tests::prop_embedding_request_validation_matches_spec` | Passed |

## 4. 计划约束（v0.7+ 未实现；不计入本次覆盖率）

以下条目在实现对应功能时**必须**移入 §2 并补齐测试映射：

- MCP：采样回调（server → client 请求）与服务端通知订阅（progress / list_changed）。
- JSON Schema 剩余关键字（`pattern` / `multipleOf` / `format` 等，见 §2.27 说明）。
- 向量检索等外部记忆后端（SPI 已在 §2.26 定义，由生态实现；向量化能力见 §2.30 `EmbeddingProvider`）。
- rerank（重排序）模型支持：暂缓（OpenRouter 账号级 ZDR 策略拦截 rerank 端点，见 roadmap v1.1）。
- 序列化兼容：核心类型新增字段需走非破坏性变更评审；枚举新增变体视为破坏性变更。
- 工程化：`cargo-mutants` 自动化变异测试、`cargo-deny` 依赖审计、文档站点。

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

### 5.1 变更记录

| 版本 | 日期 | 变更内容 | 类型 |
| :--- | :--- | :--- | :--- |
| v0.1.0 | 2026-10-14 | 初始契约：领域类型、状态机、错误矩阵、配置（44 条约束） | 初始 |
| v0.2.0 | 2026-10-14 | 新增 `Role::Tool` 与 `Message::tool_result`（FC-POST-008） | 破坏性（枚举新增变体） |
| v0.2.0 | 2026-10-14 | 新增装配器契约（§2.12）：流协议 5 项 + 负向拒绝 6 条 | 新增 |
| v0.2.0 | 2026-10-14 | 新增入参校验契约（§2.13）：JSON Schema 子集语义 | 新增 |
| v0.2.0 | 2026-10-14 | 新增执行循环契约（§2.14）：循环语义、预算、取消、事件不变量 | 新增 |
| v0.2.0 | 2026-10-14 | 移除 `FC-POST-052`（骨架哨兵），由 §2.14 取代；`AgentRuntime::run` 行为从「显式未实现」变为「真实执行循环」 | 行为变更 |
| v0.2.0 | 2026-10-14 | 新增 OpenAI 兼容 Provider 契约（§2.15） | 新增 |
| v0.3.0 | 2026-10-16 | 新增工具并发与超时契约（§2.16）；`AgentConfig` 新增 `max_parallel_tool_calls`、`tool_timeout` 字段（默认值保持 v0.2 行为） | 新增（非破坏性） |
| v0.3.0 | 2026-10-16 | 新增权限策略契约（§2.17）：`ToolPolicy` SPI、`DenyList` / `AllowList` / `AllowAll` | 新增 |
| v0.3.0 | 2026-10-16 | 新增 `#[tool]` 派生宏契约（§2.18） | 新增 |
| v0.3.0 | 2026-10-16 | 新增 MCP 客户端契约（§2.19） | 新增 |
| v0.3.0 | 2026-10-16 | 新增思考（推理）支持契约（§2.20）：`ContentBlock::Thinking`、`ModelDelta::ThinkingDelta`、`AgentEventKind::ModelThinkingDelta`、`Usage.reasoning_tokens`、`ModelRequest.reasoning`、`AgentConfig.reasoning` | 破坏性（枚新增变体 / 结构新增字段） |
| v0.4.0 | 2026-10-16 | 结构化工具结果（RFC-0002）：`ContentBlock::ToolResult.content` 由 `String` 改为 `Vec<ToolResultBlock>`；`ToolOutput` 改为多块模型（`text()` / `content_blocks()` / `into_blocks()`） | 破坏性（wire + API） |
| v0.4.0 | 2026-10-16 | `RunHooks` 异步化（RFC-0001）：四个回调改为 `async fn`，在运行任务内 `await` | 破坏性（API） |
| v0.4.0 | 2026-10-16 | 新增重试策略（`RetryPolicy` / `AgentConfig.retry` / `AgentEventKind::ModelRetry`）与时长预算（`AgentConfig.max_duration` / `BudgetKind::DurationMs`） | 新增（非破坏性） |
| v0.4.0 | 2026-10-16 | 新增多 Provider 降级 `FallbackProvider`（`mycelium-model`） | 新增 |
| v0.5.0 | 2026-10-16 | 新增多轮会话契约（§2.24）：`Session` serde、`SessionStore` SPI、`InMemorySessionStore` / `FileSessionStore`、`AgentRuntime::run_session` | 新增（非破坏性） |
| v0.5.0 | 2026-10-16 | 新增子 Agent 编排契约（§2.25）：`AgentTool`（agent-as-tool，取消传播 + 子运行独立预算 / 事件流） | 新增 |
| v0.6.0 | 2026-10-16 | 新增长期记忆契约（§2.26）：`Memory` SPI、`MemoryRecord` / `MemoryId`、`InMemoryMemory`（关键字相关度） | 新增（非破坏性） |
| v0.6.0 | 2026-10-16 | JSON Schema 子集扩充（§2.27）：`const` / `allOf` / `anyOf` / `oneOf` / 数值边界 / 字符串长度 / 数组约束 | 新增（校验更严格，属行为增强） |
| v0.6.0 | 2026-10-16 | tracing 结构化 span（§2.28）：`mycelium.run` / `mycelium.step` / `mycelium.tool` | 新增 |
| v0.7.0 | 2026-10-16 | MCP 完整化（§2.29）：`resources/list` / `resources/read` / `prompts/list` / `prompts/get` 与 HTTP（streamable HTTP）传输 | 新增（非破坏性） |
| v1.0.0 | 2026-10-17 | crates.io 发布名迁移（`mycelium-agent-*`；导入名不变）与发布工程（元数据 / 许可证 / crate README / 打包预检） | 非破坏性（导入名不变；未发布无存量影响） |
| v1.0.0 | 2026-10-17 | 公共 API 冻结正式生效：基线 = v0.7.0 API（semver-checks 全量无破坏差异）+ 发布名迁移；CI 增加 semver 门禁 | 冻结 |
| v1.1.0 | 2026-10-18 | 新增文本向量化契约（§2.30）：`EmbeddingProvider` SPI、`EmbeddingRequest` / `EmbeddingResponse` 构造即校验、OpenAI 兼容 `/embeddings` 实现 | 新增（非破坏性） |

## 6. 验证报告（FC-Verification Report）

### 6.1 v1.1（2026-10-18）

- 验证日期：2026-10-18
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace --all-features`：**248 个单元 / 集成测试 + 16 个 doctest** 全部通过
    （1 个 doctest 为 `ignore`）；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过；
  - `cargo deny check`：`advisories / bans / licenses / sources` 全部通过。
- **公共 API 兼容性**：`cargo-semver-checks 0.51.0`，7 个冻结 crate 对 `v1.0.0` 基线全量验证，
  全部 `no semver update required`（Embedding 属非破坏性新增，无枚举变体与 wire 变更）。
- **真实联调**（七场景全通过；原始日志 `docs/integration/2026-10-18-v1.1/`）：
  - 场景 1-6（DeepSeek V4.1-Flash）：文本 + 思考链（`reasoning_tokens=68`）、`add` 往返（42）、
    MCP 文件系统（14 工具，`read_text_file`）、多轮会话持久化 / 恢复、子 Agent 编排、
    MCP 资源 7 个 / 提示模板 4 个（`server-everything`）；
  - 场景 7（新增，OpenRouter × `openai/text-embedding-3-small`）：3 条输入默认 1536 维、
    `dimensions=64` 降维生效、同义句余弦相似度 **0.9180 > 0.1068**（无关句）、usage 回传
    （`prompt_tokens=34`）；`transcript.log` 含两次 `/embeddings` 请求体（含 `dimensions`）
    与运行时校验摘要（`vectors=3 dimensions=1536`、`vectors=1 dimensions=64`）。
- 追溯覆盖率：§2 与 §3 共 **163 条**约束（155 + §2.30 的 7 条 + §3 的 `FC-INV-040`），
  100% 映射且全部 `Passed`（见状态列）。
- **安装验证**（crates.io 1.1.0）：独立 consumer 工程经注册表依赖门面
  （`features = ["provider-openai"]`）编译运行通过（Embedding 构造校验、`EmbeddingProvider`
  动态调用）；docs.rs 构建通过（8 个包按依赖拓扑发布，见 roadmap v1.1 记录）。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F29 | 批量上限放宽 1 步（2049 仍接受） | `embedding::tests::new_rejects_too_many_inputs` | 变红 ✓（已还原） |
| F30 | 响应不再按 `index` 排序（维持到达顺序） | `embedding::tests::response_accepts_and_sorts_by_index` | 变红 ✓（已还原） |
| F31 | 重复 `index` 不再拒绝（静默接受） | `embedding::tests::response_rejects_duplicate_index` | 变红 ✓（已还原） |
| F32 | `dimensions = 0` 不再拒绝 | `embedding::tests::with_dimensions_rejects_zero` | 变红 ✓（已还原） |

### 6.2 v1.0（2026-10-17）

- 验证日期：2026-10-17
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace --all-features`：**220 个单元 / 集成测试 + 11 个 doctest** 全部通过
    （20 个测试套件；1 个 doctest 为 `ignore`）；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过；
  - `cargo deny check`：`advisories / bans / licenses / sources` 全部通过；
  - MSRV：1.93 工具链被 `rust-version = "1.97"` 显式拒绝（`mycelium-agent-core@0.7.0 requires rustc 1.97`）。
- **公共 API 兼容性（冻结评审）**：`cargo-semver-checks 0.51.0`，7 个冻结 crate 对 `v0.7.0` 基线
  全量验证，全部 `no semver update required`（无破坏性差异）。
- **打包验证**：
  - `cargo package --workspace --no-verify`：8 个 `.crate` 产物，全部含 README / LICENSE-MIT / LICENSE-APACHE；
  - `cargo package -p mycelium-agent-core` 与 `-p mycelium-agent-macros`（无内部依赖）：
    完整 verification 构建通过；依赖方首次发布 bootstrap 行为已实测并记录于 release-process.md §4。
- **真实模型联调**（六场景全通过，29.5s；原始日志 `docs/integration/2026-10-17-v1.0/`）：
  ① 文本 + 思考链（reasoning_tokens=181）② `#[tool]` 往返（42）③ MCP 文件系统（`secure-filesystem-server`，14 工具）
  ④ 多轮会话持久化 / 恢复 ⑤ 子 Agent 编排 ⑥ MCP 资源 7 个 / 提示模板 4 个（`server-everything`）。
- **安装验证**（新发布名 git 依赖）：两个独立 consumer 工程——README 用法示例原文（71 行）
  与 `features = ["provider-openai"]` 写法——编译运行通过。
- 追溯覆盖率：§2 与 §3 共 **155 条**约束，100% 映射且全部 `Passed`（本批次无契约行为变更）。
- 发布阻断记录（外部）：CI 计费锁定（账号级），证据见
  `docs/integration/2026-10-16-readme-e2e/ci-startup-failure.txt`；本地全套验证通过，
  解锁后 CI（fmt/clippy/test、MSRV、打包预检、semver 门禁）即可真实生效。

### 6.3 v0.7（历史）

- 验证日期：2026-10-16
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace --all-features`：**220 个单元 / 集成测试 + 9 个 doctest** 全部通过；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过。
- **真实模型联调**（六场景；原始日志 `docs/integration/2026-10-16-v0.7/`）：
  - 前五场景保持通过；场景 6 经官方 `server-everything` 实测：
    **资源 7 个**（首条 `demo://resource/static/document/architecture.md` → 1 内容块）、
    **提示模板 4 个**（`simple-prompt` → 1 条消息）；
  - filesystem 服务不实现 resources / prompts（-32601），客户端按协议显式上报并可继续。
- 追溯覆盖率：§2 与 §3 共 **155 条**约束（43+45+27+16+10+7+7），100% 映射且全部 `Passed`。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F25 | HTTP 会话头不再传播 | `http::tests::handshake_and_session_header` | 变红 ✓（已还原） |
| F26 | 资源内容缺失字段被静默接受 | `client::tests::read_resource_rejects_malformed_entry` | 变红 ✓（已还原） |
| F27 | SSE 不按 id 匹配（取首条） | `http::tests::sse_response_is_matched_by_id` | 变红 ✓（已还原） |
| F28 | 提示参数 `required` 缺省改为 `true` | `client::tests::list_prompts_maps_specs` | 变红 ✓（已还原） |

### 6.4 v0.6（历史）

- 验证日期：2026-10-16
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace --all-features`：**205 个单元 / 集成测试 + 9 个 doctest** 全部通过；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过。
- **真实模型联调**（五场景全通过；原始日志 `docs/integration/2026-10-16-v0.6/`）：
  - `transcript.log` 中可直接观察嵌套结构化 span：
    `mycelium.run{run_id=… provider="openai" model=deepseek-flash}:mycelium.step{step=1}: …`。
- 追溯覆盖率：§2 与 §3 共 **148 条**约束（v0.1 43 + v0.2 45 + v0.3 27 + v0.4 16 + v0.5 10 + v0.6 7），
  100% 映射到测试且全部 `Passed`（见状态列）。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F21 | 记忆相关度失效（退化为纯时间序） | `memory::tests::recall_ranks_by_keyword_then_recency` | 变红 ✓（已还原） |
| F22 | `minimum` 边界放宽 1 步 | `schema::tests::numeric_bounds_are_inclusive` | 变红 ✓（已还原） |
| F23 | `oneOf` 允许多分支同时命中 | `schema::tests::one_of_requires_exactly_one_branch` | 变红 ✓（已还原） |
| F24 | 工具 span 移除 `instrument` | `engine::tests::run_emits_structured_spans` | 变红 ✓（已还原） |

### 6.5 v0.5（历史）

- 验证日期：2026-10-16
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace --all-features`：**188 个单元 / 集成测试 + 10 个 doctest** 全部通过；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过。
- **真实模型联调**（DeepSeek V4.1-Flash，五场景；原始日志随仓库保留
  `docs/integration/2026-10-16-v0.5/`，含 `sessions/` 会话文件）：
  - 场景 1 文本 + 思考链：800 事件 / `reasoning_tokens=778`；
  - 场景 2 工具往返：`add → 42`；
  - 场景 3 MCP 文件系统：14 工具 / `read_text_file`；
  - 场景 4 **多轮会话**：两轮完成，`FileSessionStore` 持久化并在新 store 实例中恢复（会话 4 条消息）；
  - 场景 5 **子 Agent 编排**：`ask_child` 工具委托子运行并返回「2」。
- 追溯覆盖率：§2 与 §3 共 **141 条**约束（v0.1 43 + v0.2 45 + v0.3 27 + v0.4 16 + v0.5 10），
  100% 映射到测试且全部 `Passed`（见状态列）。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F17 | `run_session` 不追加用户输入 | `agent::tests::run_session_appends_history_and_new_messages` | 变红 ✓（已还原） |
| F18 | 文件存储 `load` 永远返回空 | `store::tests::file_round_trip` | 变红 ✓（已还原） |
| F19 | 取消不传播到子运行 | `engine::tests::agent_tool_propagates_cancel_to_child` | 变红 ✓（已还原） |
| F20 | `AgentTool` 缺 `prompt` 不再校验 | `agent_tool::tests::missing_prompt_is_rejected` | 变红 ✓（已还原） |

### 6.6 v0.4（历史）

- 验证日期：2026-10-16
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace --all-features`：**175 个单元 / 集成测试 + 8 个 doctest** 全部通过
    （单元 / 集成：core 50 + tools 18 + derive 7 + macros 4 + runtime 45 + model 21 +
    provider 16 + e2e 4 + mcp 10）；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过；
  - CI 新增 **MSRV job**（`cargo check --workspace --all-features` on 1.97.0）。
- **真实模型联调**（DeepSeek V4.1-Flash，wire 变更后重跑）：
  - 原始交互日志随仓库保留：`docs/integration/2026-10-16-v0.4/`
    （场景 1 文本 + 思考链：350 事件 / `reasoning_tokens=320`；场景 2 工具往返 `add → 42`；
    场景 3 MCP 文件系统：14 工具 / 自主调用 `read_text_file`）。
- 追溯覆盖率：§2 与 §3 共 **131 条**约束（v0.1 43 + v0.2 45 + v0.3 27 + v0.4 16），
  100% 映射到测试且全部 `Passed`（见状态列）。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F13 | 重试失效（attempts 恒为 1） | `engine::tests::retries_retryable_model_errors` | 变红 ✓（已还原） |
| F14 | 时长预算放宽 1000 倍 | `engine::tests::duration_budget_exceeded` | 变红 ✓（已还原） |
| F15 | 结构化工具结果降级为纯文本 | `engine::tests::structured_tool_result_is_fed_back_intact` | 变红 ✓（已还原） |
| F16 | 降级判据反转（可重试不降级） | `fallback::tests::falls_back_on_retryable_error` | 变红 ✓（已还原） |

### 6.7 v0.3（历史）

- 验证日期：2026-10-16
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace --all-features`：**157 个单元 / 集成测试 + 8 个 doctest** 全部通过
    （单元 / 集成：core 46 + tools 17 + derive 7 + macros 4 + runtime 38 + model 17 + provider 15 + e2e 4 + mcp 9；
    macros 文档示例 1 个按 `ignore` 处理，非可执行契约）；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过；
  - `cargo run -p mycelium --example basic`：端到端成功。
- **真实模型联调**（DeepSeek V4.1-Flash，`deepseek-flash`）：
  - 正式联调（**完整原始交互日志随仓库保留**，见 [`docs/integration/2026-10-16/`](../../integration/2026-10-16/)）：
    - 场景 1 文本对话 + 思考链：451 条事件，`reasoning_tokens=422`，思考 735 字；
    - 场景 2 `#[tool]` 工具往返：`add` → 42（steps=2）；
    - 场景 3 MCP 文件系统：14 个工具，模型自主调用 `read_text_file` 并正确回答文件内容（steps=2）；
    - 产物：`transcript.log`（HTTP 请求体 / SSE 逐行 / MCP JSON-RPC 双向原文）、
      `scenario*_events.jsonl`、`scenario*_outcome.json`、`report.md`；
  - 复现：`cargo run -p mycelium --features provider-openai --example live_integration -- <目录>`。
- 追溯覆盖率：§2 与 §3 共 **115 条**约束（v0.1 的 43 条 + v0.2 的 45 条 + v0.3 的 27 条），
  100% 映射到测试且全部 `Passed`（见状态列）。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F9 | 并发保序 `buffered` → `buffer_unordered` | `engine::tests::concurrent_tools_are_parallel_and_ordered` | 首轮**未变红** → 测试已强化（断言结果内容与调用一一对应）后复验变红 ✓ |
| F10 | 权限拒绝失效（`Deny` 分支不生效） | `engine::tests::policy_deny_is_fed_back_without_execution` | 变红 ✓（已还原） |
| F11 | 思考块顺序颠倒（`Text` 先于 `Thinking`） | `assembler::tests::assembles_thinking_block_before_text` | 变红 ✓（已还原） |
| F12 | MCP 非法 schema 静默跳过 | `client::tests::invalid_tool_schema_is_rejected` | 变红 ✓（已还原） |

> F9 首轮未变红暴露了原测试的弱点：它只校验 `call_id` 顺序，而 `call_id` 本身由调用顺序 zip 生成，
> 内容错位无法被发现。已按 FSVDD 要求强化测试（校验 `call_id` 与 `content` 的对应关系）并复验通过。

### 6.8 v0.2（历史）

- 验证日期：2026-10-14
- 工具链：rustc / cargo 1.97.1（stable，x86_64-unknown-linux-gnu）
- 执行命令与结果：
  - `cargo test --workspace`：**115 个单元 / 集成测试 + 7 个 doctest** 全部通过
    （单元 / 属性测试 115 = core 41 + model 15 + tools 12 + runtime 30 + provider 13 + e2e 集成 4）；
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`：零告警；
  - `cargo fmt --all --check`：通过；
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`：通过；
  - `cargo run -p mycelium --example basic`：端到端成功（steps=1）。
- 追溯覆盖率：§2 与 §3 共 **88 条**约束（v0.1 的 43 条 + v0.2 新增 45 条），100% 映射到测试且全部 `Passed`（见状态列）。
- 证伪抽查（Falsification Check，FSVDD §4.2.4）：

| # | 故意破坏的约束 | 预期变红测试 | 结果 |
| :--- | :--- | :--- | :--- |
| F5 | 装配器文本拼接改为覆盖写入 | `assembler::tests::assembles_text_usage_and_stop_reason` | 变红 ✓（已还原） |
| F6 | 步数预算边界放宽 `max_steps + 1` | `engine::tests::step_budget_exceeded` | 变红 ✓（已还原） |
| F7 | 工具错误改为直接终止（不回灌） | `engine::tests::failing_tool_error_is_fed_back` | 变红 ✓（已还原） |
| F8 | 429 限流映射改为普通 Api 错误 | `provider::tests::maps_error_status_rules` | 变红 ✓（已还原） |

### 6.9 v0.1（历史）

- 验证日期：2026-10-14；结果：55 个单元 / 属性测试 + 4 个 doctest 全部通过；clippy / fmt / doc 零告警。
- 证伪抽查 F1 ~ F4（工具名长度上限、重名注册覆盖、转移边删除、饱和加法改回绕）全部按预期变红后还原。

- 备注：自动化变异测试（`cargo-mutants`）计划接入 CI，替代手工证伪抽查（见 roadmap 工程化 backlog）。
