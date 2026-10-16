# 真实联调报告（live_integration）

- 网关：`https://api.deepseek.com/v1`
- 模型：`deepseek-flash`（DeepSeek-V4.1-Flash）
- 生成时间（Unix 毫秒）：`1792133914364`
- 隐私：`Authorization` 请求头不进入日志（仅请求体与响应行落盘）
- 重放：`cargo run -p mycelium --features provider-openai --example live_integration -- <目录>`

## 场景与结果

| # | 场景 | 结果 |
|---|------|------|
| 1 | 文本对话 + 思考链 | steps=1，事件=519，usage=Usage { input_tokens: 51, output_tokens: 515, reasoning_tokens: 489 }，思考 851 字，回复：我是你的简洁AI助手，属于由当前平台配置和部署的大语言模型，具体版本请以界面标注为准。 |
| 2 | 工具调用往返（add） | steps=2，事件=32，usage=Usage { input_tokens: 755, output_tokens: 75, reasoning_tokens: 13 }，回复：21 + 21 = **42** |
| 3 | MCP 文件系统（DeepSeek × MCP） | steps=2，事件=66，usage=Usage { input_tokens: 4658, output_tokens: 106, reasoning_tokens: 23 }，调用工具=["read_text_file"] |
| 4 | 多轮会话（持久化 + 恢复） | 两轮完成，会话消息 4 条；第二轮回复：7 |
| 5 | 子 Agent 编排（agent-as-tool） | steps=2，事件=65，子 Agent 结果：2 |
| 6 | MCP 资源与提示模板（server-everything） | 资源 7 个（demo://resource/static/document/architecture.md → 1 内容块）；提示模板 4 个（simple-prompt → 1 条消息） |

## 产物

| 文件 | 内容 |
|---|---|
| `transcript.log` | tracing 全量日志：HTTP 请求体、SSE 原始行、MCP JSON-RPC 双向原文、运行时生命周期 |
| `scenario*_events.jsonl` | `AgentEvent` 全量事件流（`seq` 连续，一行一个 JSON） |
| `scenario*_outcome.json` | 最终消息（含 `thinking` 块）、用量与步数 |
