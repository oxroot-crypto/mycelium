# 发布流程（Release Process）

> 适用于所有发布（含 0.x 阶段）。每个版本的发布检查单见 §2；crates.io 发布顺序见 §4；
> v1.0.0 的精确执行步骤见 [`release-v1.0-runbook.md`](release-v1.0-runbook.md)。

## 1. 一次性前置条件（2026-10-17 完成）

- [x] 许可证：**MIT OR Apache-2.0**；`LICENSE-MIT` / `LICENSE-APACHE` 已复制到全部 8 个 crate 目录，
      随 `cargo package` 产物发布（`Cargo.toml` 的 `license` 字段已设置）
- [x] crates.io 发布凭据（`CARGO_REGISTRY_TOKEN`，仅发布者本机 / 发布 job 可见）
- [x] 移除 `[workspace.package]` 的 `publish = false`（连同各 crate 的 `publish.workspace = true`）
- [x] 发布元数据完整（docs.rs 展示）：`description` / `repository` / `readme`（crate 级）/
      `keywords` / `categories`；门面附加 `[package.metadata.docs.rs] all-features = true`
- [x] 发布名迁移：crates.io 包名为 `mycelium-agent-*`，Rust 导入名 `mycelium_*` 保持不变
      （`[lib] name` 固定；映射见 [`migration-v0-to-v1.md`](migration-v0-to-v1.md) §1）

## 2. 版本发布检查单

1. **全量验证**（必须全绿）
   - `cargo test --workspace --all-features`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo fmt --all --check`
   - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
   - `cargo deny check`（安全公告 / 许可 / 来源）
   - CI：fmt/clippy/test、MSRV、打包预检；semver 门禁（基线 tag 存在时）
2. **契约一致性**：`docs/spec/core.md` §6 验证报告更新；FC-Matrix 状态 100% `Passed`（0 x `Pending`）
3. **真实联调**：运行 `live_integration`
   （`cargo run -p mycelium-agent --features provider-openai --example live_integration -- <目录>`；
   `.env` 需配置 `DEEPSEEK_API_KEY`（场景 1-6）与 `OPENROUTER_API_KEY`（场景 7 文本向量化，
   未配置时该场景显式跳过并在报告中记录），
   日志提交到 `docs/integration/<日期>-vX.Y/`，并在发布报告（core.md §6）中引用
4. **版本号**：更新 `[workspace.package] version` 与 `[workspace.dependencies]` 内部依赖版本
   （`sed -i 's/version = "OLD"/version = "NEW"/g' Cargo.toml`），运行一次构建让 `Cargo.lock` 收敛
5. **变更记录**：更新 `CHANGELOG.md`（破坏性标 **BREAKING**）与 `docs/spec/core.md` §5
6. **提交与打标**：
   ```bash
   git commit -m "feat: vX.Y  <主题>（N 条约束）"
   HASH=$(git rev-parse --short HEAD)   # 记录到 roadmap 快照表
   git tag -a vX.Y.Z -m "<主题>：<要点>（N 条约束）"
   git push && git push origin vX.Y.Z
   git branch release/vX.Y.Z && git push origin release/vX.Y.Z   # 发布分支（发布快照）
   ```
7. **发布到 crates.io**（§4）：`cargo publish -p <crate>`（逐 crate，等待索引刷新）
8. **发布后**：GitHub Release（tag 注解 + CHANGELOG 摘要）；验证 docs.rs 构建；
   归档联调日志；更新 roadmap 快照表与 CHANGELOG 链接

## 3. 变更分级（与 api-freeze.md 对齐）

- **BREAKING**：公共签名、wire 标签、枚举变体增减 → 需 RFC + CHANGELOG 标注（1.0 后需 major）
- **Feature**：新增类型 / 方法 / 可选字段 → minor
- **Fix / Docs / CI**：patch

## 4. crates.io 发布顺序（依赖拓扑）

```text
mycelium-agent-core
  ├── mycelium-agent-macros（先于 tools；tools 依赖它）
  ├── mycelium-agent-model
  ├── mycelium-agent-tools（依赖 core / macros）
  ├── mycelium-agent-mcp（依赖 core / tools）
  ├── mycelium-agent-runtime（依赖 core / model / tools）
  ├── mycelium-agent-provider-openai（依赖 core / model；dev-dependencies 含 runtime，须在其后）
  └── mycelium-agent（门面；依赖以上全部）
```

- 每个 crate 发布后需等待 crates.io 索引可见再发布其依赖者；
- **v1.0.0 首次发布的 bootstrap（实测行为，2026-10-17）**：cargo 打包会剥离 `path` 依赖并以注册表解析——
  依赖方（除 `mycelium-agent-core` / `mycelium-agent-macros` 外）在依赖上架前执行 `cargo package`
  （含校验）会报 `no matching package named … found`，属预期；按上表顺序真实发布即可；
- 预检命令：`cargo package --workspace --no-verify`（8 个 crate 全部打包，校验 README / LICENSE 入包）；
  无内部依赖的 crate 可做完整 `cargo package` 验证构建。
- 发布顺序按「依赖 + **dev-dependencies**」拓扑解析：`mycelium-agent-provider-openai` 的 dev-dependencies
  含 `mycelium-agent-runtime`，发布校验会解析 dev-deps，故 runtime 必须先上架（v1.0.0 首发实测：
  顺序错误时报 `no matching package named mycelium-agent-runtime found`）。
- 新包限流：crates.io 对「新包」发布有频率限制（v1.0.0 首发 8 个包期间触发两次 429，错误信息给出
  可重试时间）；续发具备幂等性——对已上传版本重试会返回 `already uploaded`，按已发布跳过即可。

## 5. 回滚策略

- 代码回滚：`git revert` + 新 patch 版本重新发布；
- crates.io 不可撤回：只能 `cargo yank`（阻止新解析，不影响已有锁定）；
  yank 后必须发布修复版本并更新 CHANGELOG 说明。

## 6. 频率与节奏

- 功能批次完成后按 `roadmap.md` 里程碑发布；安全修复随时以 patch 发布；
- 每个发布必须包含：版本化的联调日志 + FC-Matrix 验证报告 + CHANGELOG 条目。
