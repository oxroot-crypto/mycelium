# mycelium-agent-provider-openai

> OpenAI-compatible model provider (Chat Completions, streaming SSE, reasoning) for the **mycelium** agent SDK.
>
> **mycelium** 的 OpenAI 兼容 Provider：Chat Completions、流式 SSE 与思考链。

- crates.io 包名：`mycelium-agent-provider-openai`；Rust 导入名：`mycelium_provider_openai`（`[lib] name` 固定，`use mycelium_provider_openai::…` 直接可用）
- 使用入口：请依赖门面 crate [`mycelium-agent`](https://crates.io/crates/mycelium-agent)（`features = ["provider-openai"]`）或本 crate
- 兼容 DeepSeek / Ollama / vLLM 等 OpenAI 兼容网关（`OpenAiProvider::from_env` 读取 `OPENAI_API_KEY` / `OPENAI_BASE_URL`）
- 仓库与完整文档：<https://github.com/oxroot-crypto/mycelium>
- 许可证：[MIT](https://github.com/oxroot-crypto/mycelium/blob/main/LICENSE-MIT) OR [Apache-2.0](https://github.com/oxroot-crypto/mycelium/blob/main/LICENSE-APACHE)

本 README 位于 crate 目录内，随 `cargo package` 一起发布；根 README（含双语说明与快速开始）见仓库首页。
