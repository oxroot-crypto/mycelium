# AI 生成声明

[English](DISCLOSURE.en.md) | 简体中文

本仓库（Mycelium）的**全部代码与文档**均由 AI 模型生成，不存在人工逐行编写的部分，
也未经过完整的人工审查与评估，包括但不限于：

- 源码：`crates/`（8 个 crate，含各自 `examples/`）；
- 文档：`README.md`、`README.en.md`、`CHANGELOG.md`、crate 级 `README.md`、
  `docs/`（契约 `docs/spec/`、架构与发布流程 `docs/architecture/`、真实联调日志 `docs/integration/`）；
- 工程与配置：`Cargo.toml`、`Cargo.lock`、`.github/workflows/`、`rust-toolchain.toml`、`rustfmt.toml`、
  `deny.toml`、`mutants.toml`、`LICENSE-MIT`、`LICENSE-APACHE` 等。

> [!NOTE]
> `docs/integration/` 下的真实联调日志由 AI 编写并运行的示例程序与 DeepSeek 真实交互后原样落盘：
> 交互记录是真实的，但断言、结论与文字说明由 AI 编写，不保证完全可信。

本声明文件本身亦由 AI 生成。

参与生成的 AI 模型：

- **deepseek-v4.1-flash**

AI 生成的内容可能存在错误或过时之处，采用、引用或二次分发前请自行审查与评估。
本项目按 **MIT OR Apache-2.0** 双许可（任选其一）发布：
[LICENSE-MIT](LICENSE-MIT) / [LICENSE-APACHE](LICENSE-APACHE)。
