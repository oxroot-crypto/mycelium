# 真实联调日志（docs/integration/）

本目录保存**可追溯、可重放**的真实模型联调原始交互日志（随仓库永久保留）。

## 目录结构

```text
docs/integration/
├── README.md
├── 2026-10-16/                              # v0.3 联调（DeepSeek-V4.1-Flash）
│   ├── report.md
│   ├── transcript.log
│   ├── scenario1_text_thinking_events.jsonl
│   ├── scenario1_text_thinking_outcome.json
│   ├── scenario2_tool_round_trip_events.jsonl
│   ├── scenario2_tool_round_trip_outcome.json
│   ├── scenario3_mcp_filesystem_events.jsonl
│   └── scenario3_mcp_filesystem_outcome.json
└── 2026-10-16-v0.4/                         # v0.4 联调（结构化工具结果 / 重试版本重跑）
│   └── （同上结构）
└── 2026-10-16-v0.5/                         # v0.5 联调（场景 4 多轮会话 / 场景 5 子 Agent）
│   ├── （同上结构）
│   └── sessions/                            # 场景 4 的会话持久化文件（FileSessionStore）
└── 2026-10-16-v0.6/                         # v0.6 联调（含结构化 span 的完整 transcript）
    └── （同上结构）
```

## 产物说明

| 文件 | 内容 |
| :--- | :--- |
| `transcript.log` | tracing TRACE 全量：HTTP 请求体（含 `thinking` / `reasoning_effort` 参数）、SSE 逐行原文、MCP JSON-RPC 双向原文、运行时生命周期（`run_id` 等）、TLS/HTTP 连接细节 |
| `*_events.jsonl` | `AgentEvent` 事件流逐条 JSON（`seq` 从 0 连续、终态唯一） |
| `*_outcome.json` | 最终消息序列（含 `Thinking` 块）、`usage`（含 `reasoning_tokens`）、步数 |
| `report.md` | 场景、断言与结果一览 |

## 隐私与安全

- `Authorization` 请求头**不落盘**：埋点只记录请求体与响应行；
- 每次生成后已检查 API Key 未出现在任何日志中。

## 复现方式

```bash
# 前置：仓库根目录 .env 配置 DEEPSEEK_API_KEY；本机可用 npx；网络可达 api.deepseek.com
cargo run -p mycelium --features provider-openai --example live_integration -- docs/integration/<日期>
```

覆盖场景：

1. 文本对话 + 思考链（`Thinking` 块 / `ModelThinkingDelta` 事件 / `reasoning_tokens`）；
2. `#[tool]` 派生宏生成工具的调用往返（`add`）；
3. MCP 文件系统服务（npx `@modelcontextprotocol/server-filesystem`）：模型自主选择
   `read_text_file` 读取沙箱文件并回答内容；
4. 多轮会话（`run_session` + `FileSessionStore` 持久化并在新 store 实例中恢复）；
5. 子 Agent 编排（`AgentTool`：父 Agent 委托子运行并转述结果）。
