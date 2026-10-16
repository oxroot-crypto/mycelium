# README 端到端真实验证（2026-10-16）

以「新用户视角」对 `README.md` / `README.en.md` 中的全部可执行声明逐条复现：

- 源码：`git clone https://github.com/oxroot-crypto/mycelium.git` 全新克隆（commit `740fce4`，与远端 HEAD 一致）
- 环境：Rust stable 1.97.1、Cargo 1.97.1、Node v26.7.0、npm 12.0.2
- 说明：本机默认 npm registry 为私有镜像（证书不匹配），仓库示例已显式使用
  `--registry=https://registry.npmjs.org`；本目录为其原始输出归档。

## 结果矩阵

| # | README 声明 | 复现方式 | 结果 |
| :--- | :--- | :--- | :--- |
| 1 | 快速开始① 克隆仓库 | 无凭据全新克隆 | 通过 |
| 2 | 快速开始② `cargo build --workspace` | 全新 target 冷构建 | 通过（1m09s） |
| 3 | 快速开始③ 预期输出 | `cargo run -p mycelium --example basic` | 通过，与 README 预期**逐字一致**（`quickstart-basic.log`） |
| 4 | 接入真实模型 + `.env` 流程 | `cp .env.example .env` → `cargo run -p mycelium-provider-openai --example deepseek` | 通过，三场景（文本+思考链 / 工具往返 / 流式事件），`deepseek-real.log` |
| 5 | 接入 MCP 服务 | `cargo run --features provider-openai --example agent_deepseek_mcp` | 通过，`secure-filesystem-server v1.0.0` × 14 工具，模型经 MCP 读文件，`mcp-real.log` |
| 6 | 多轮会话 / 子 Agent | `cargo run --features provider-openai --example live_integration -- <dir>` | 通过，六场景全部通过（`live-integration/`） |
| 7 | 安装：门面 feature 写法 | 独立 consumer：`mycelium = { git=…, features=["provider-openai"] }` | 通过，README 用法示例**原文**（71 行）编译运行、断言通过 |
| 8 | 安装：直连 provider 写法 | 独立 consumer：`mycelium` + `mycelium-provider-openai` git 依赖 | 通过，`OpenAiProvider::from_env()` 对 DeepSeek 真实请求成功（steps=1） |
| 9 | 开发命令链 | test / clippy `-D warnings` / fmt / doc / deny | 通过：20 个测试套件 231 passed / 0 failed（1 ignored）；clippy、fmt、doc 零告警；`cargo deny check` 全绿 |
| 10 | MSRV 1.97 | 当前 stable 1.97.1 全量构建；`cargo +1.93.0 check` 被 `rust-version` 拒绝 | 通过，声明生效 |
| 11 | CI 徽章 | GitHub Actions 运行记录 | **失败（账号级）**：`oxroot-crypto` 处于 Actions 计费锁定，全部 11 次运行 0 步骤即失败，见 `ci-startup-failure.txt` |

## 环境差异说明

- README 的 MCP 片段为通用写法（依赖默认 registry）；本机镜像不可用时改用仓库示例
  （自带公共 registry 参数），功能等价。
- 两个 consumer 共享 `CARGO_TARGET_DIR` 复用依赖编译，不影响结论。

## 备注

- 日志不含 API Key：已验证无 `sk-` 模式、无 `Authorization` 行；
  `live-integration/report.md` 中的 `Authorization` 仅为隐私说明文字。
- `live_integration` 六场景：① 文本+思考链 ② `#[tool]` 往返 ③ MCP 文件系统
  ④ 多轮会话（持久化 + 恢复） ⑤ 子 Agent 编排 ⑥ MCP 资源与提示模板。
