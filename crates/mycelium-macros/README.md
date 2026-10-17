# mycelium-agent-macros

> Procedural macros for the **mycelium** agent SDK: `#[tool]` generates a `Tool` implementation and JSON Schema from a plain function.
>
> **mycelium** 过程宏：`#[tool]` 由普通函数生成 `Tool` 实现与 JSON Schema。

- crates.io 包名：`mycelium-agent-macros`；Rust 导入名：`mycelium_macros`（`[lib] name` 固定）
- 使用入口：由 [`mycelium-agent-tools`](https://crates.io/crates/mycelium-agent-tools) 与门面 [`mycelium-agent`](https://crates.io/crates/mycelium-agent) 再导出，通常无需直接依赖
- 仓库与完整文档：<https://github.com/oxroot-crypto/mycelium>
- 许可证：[MIT](https://github.com/oxroot-crypto/mycelium/blob/main/LICENSE-MIT) OR [Apache-2.0](https://github.com/oxroot-crypto/mycelium/blob/main/LICENSE-APACHE)

本 README 位于 crate 目录内，随 `cargo package` 一起发布；根 README（含双语说明与快速开始）见仓库首页。
