# 形式化约束矩阵（FC-Matrix）：mycelium v0.1 架构骨架

> 本文件是 mycelium 形式化契约的**单一真实来源（Single Source of Truth）**。
> 任何行为变更必须遵循：**先改本契约 → 再改测试 → 再改实现 → 契约漂移审计**（见 §5）。
>
> - 版本：v0.2.0（2026-10-14）
> - 覆盖范围：`mycelium-core`、`mycelium-model`、`mycelium-tools`、`mycelium-runtime`、`mycelium-provider-openai`、`mycelium`（门面）
> - 状态列取值：`Pending`（待验证）/ `Passed`（验证通过）/ `Planned`（未实现，不计入本次覆盖率）

## 0. 豁免判定与严重级别

- 本仓库交付物为**对外契约类 SDK**（公共 API 即契约）。依据 FSVDD §1.1，将以下推荐项升级为**强制**：
  测试与约束 1:1 对齐（100% 追溯）、状态机全路径遍历、基于属性的测试（PBT）、证伪抽查。
- 不适用豁免（FSVDD §1.2）：本仓库承载对外接口与状态流转。
- v0.2 覆盖**已实现**的纯逻辑与运行时行为（§2、§3）：领域类型、装配器、入参校验、执行循环、
  OpenAI 兼容 Provider 与事件不变量；其余未实现部分（工具并发、权限策略、时长预算等）列入
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

## 3. 属性测试（PBT，对应 FSVDD §4.2.1）

| 编号 | 类型 | 形式化规范描述 | 对应测试用例 / 测试套件 | 验证状态 |
| :--- | :--- | :--- | :--- | :--- |
| `FC-INV-010` | 不变量（属性） | 对任意 Unicode 字符串 `s`：`ToolName::new(s).is_ok() ⇔ s ∈ TN`；且合法时 `as_str()` 保真、serde 往返恒等 | `properties::prop_tool_name_acceptance_matches_spec` | Passed |
| `FC-INV-011` | 不变量（属性） | 对任意 `t ∈ Terminal` 与任意 `e ∈ E`：`δ(t, e)` 必须为 `Err`（终止状态无出边） | `properties::prop_terminal_states_reject_all_events` | Passed |
| `FC-INV-012` | 不变量（属性） | 对任意 `(a, b) ∈ u64 × u64`：`Usage::new(a, b).total_tokens() == a.saturating_add(b)` | `properties::prop_usage_total_saturating` | Passed |
| `FC-INV-013` | 不变量（属性） | 对任意文本 `t`：`Message::user_text(t)` serde 往返恒等 | `properties::prop_message_roundtrip` | Passed |
| `FC-INV-030` | 不变量（属性） | 对任意文本增量序列 `v`：装配结果 = 单 `Text` 块且文本等于 `v` 的顺序拼接 | `assembler::tests::prop_text_deltas_concatenate` | Passed |

## 4. 计划约束（v0.3+ 未实现；不计入本次覆盖率）

以下条目在实现对应功能时**必须**移入 §2 并补齐测试映射：

- 时长预算（`BudgetKind::DurationMs`）：单次 Run 总时长超限终止。
- 工具超时：由运行时策略层统一施加（`ToolError::Timeout`）。
- 权限策略：策略拒绝返回 `ToolError::Denied`；内核默认不隐式放行。
- 工具并发执行（v0.3）：结果回灌顺序与调用顺序一致；跨 Run 无共享可变状态。
- 结构化工具输出（多内容块、图片等）。
- 完整 JSON Schema 支持（超出 §2.13 子集的关键字）。
- 重试策略：依据 `ModelError::is_retryable` 的自动重试与退避。
- 会话持久化与恢复；tracing 结构化 span（当前仅事件级埋点）。
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

## 6. 验证报告（FC-Verification Report）

### 6.1 v0.2（当前）

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

### 6.2 v0.1（历史）

- 验证日期：2026-10-14；结果：55 个单元 / 属性测试 + 4 个 doctest 全部通过；clippy / fmt / doc 零告警。
- 证伪抽查 F1 ~ F4（工具名长度上限、重名注册覆盖、转移边删除、饱和加法改回绕）全部按预期变红后还原。

- 备注：自动化变异测试（`cargo-mutants`）计划接入 CI，替代手工证伪抽查（见 roadmap 工程化 backlog）。
