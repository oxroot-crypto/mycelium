# 真实联调报告（live_integration）

- 网关：`https://api.deepseek.com/v1`
- 模型：`deepseek-flash`（DeepSeek-V4.1-Flash）
- 生成时间（Unix 毫秒）：`1792126785962`
- 隐私：`Authorization` 请求头不进入日志（仅请求体与响应行落盘）
- 重放：`cargo run -p mycelium --features provider-openai --example live_integration -- <目录>`

## 场景与结果

| # | 场景 | 结果 |
|---|------|------|
| 1 | 文本对话 + 思考链 | steps=1，事件=451，usage=Usage { input_tokens: 51, output_tokens: 447, reasoning_tokens: 422 }，思考 735 字，回复：我是一个简洁、能使用工具协助你的AI助手，属于大语言模型（具体版本以当前平台配置为准）。 |
| 2 | 工具调用往返（add） | steps=2，事件=47，usage=Usage { input_tokens: 764, output_tokens: 90, reasoning_tokens: 28 }，回复：21 + 21 = **42** |
| 3 | MCP 文件系统（DeepSeek × MCP） | steps=2，事件=74，usage=Usage { input_tokens: 4641, output_tokens: 114, reasoning_tokens: 6 }，调用工具=["read_text_file"] |

## 产物

| 文件 | 内容 |
|---|---|
| `transcript.log` | tracing 全量日志：HTTP 请求体、SSE 原始行、MCP JSON-RPC 双向原文、运行时生命周期 |
| `scenario*_events.jsonl` | `AgentEvent` 全量事件流（`seq` 连续，一行一个 JSON） |
| `scenario*_outcome.json` | 最终消息（含 `thinking` 块）、用量与步数 |
