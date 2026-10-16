# RFC-0002：结构化工具结果（多内容块）

- 状态：已接受（v0.4.0 实施）
- 日期：2026-10-16
- 影响面：`mycelium-core` / `mycelium-tools`（wire 格式，**破坏性变更**）

## 动机

v0.3 的 `ContentBlock::ToolResult.content` 是 `String`，无法表达图片（截图、渲染结果）
与结构化多块结果；MCP 的 `content` 数组（含 `image` 块）只能降级为文本，信息丢失。

## 设计

```text
ContentBlock::ToolResult {
    tool_call_id: ToolCallId,
    content: Vec<ToolResultBlock>,   // 由 String 改为块数组
    is_error: bool,
}

ToolResultBlock =
    | Text  { text: String }                        // wire: {"type":"text","text":..}
    | Image { media_type: String, data: String }    // wire: {"type":"image","media_type":..,"data":base64}
```

- 工具侧：`ToolOutput` 持有 `Vec<ToolResultBlock>`；`success/error` 语义不变（单文本块），
  新增 `ToolOutput::blocks` / `error_blocks`；访问器为 `text()`（文本汇总）与
  `content_blocks()`；
- 运行时：工具结果按块原样进入消息历史（不做降级）；
- Provider 适配：OpenAI 兼容协议的工具消息仅支持字符串，映射时展平
  （文本直传；图片降级为 `[image: <media_type>]` 占位并注释说明）；
- MCP 适配：`content` 数组的 `text` / `image` 块一一映射，未知类型忽略。

## 破坏性变更与迁移

| v0.3 | v0.4 |
| :--- | :--- |
| `match block { ContentBlock::ToolResult { content, .. } => content.is_empty() }`（`content: String`） | `content` 为 `Vec<ToolResultBlock>`；用 `ToolOutput::text()` 或自行遍历块 |
| `ToolOutput::content() -> &str` | `ToolOutput::text() -> String`；保留 `content_blocks()` 取原始块 |

## 未纳入本轮

- 音频 / 文件引用（resource）块：等 MCP 资源能力一并设计；
- 图片在 API 历史中的回传策略（当前 OpenAI 兼容映射为占位符，Anthropic 风格适配器可原样透传）。
