# 发布流程（Release Process)

> 适用于所有发布（含 0.x 阶段）。每个版本的发布检查单见 §2；发布顺序见 §4。

## 1. 一次性前置条件

- [x] 许可证：**MIT OR Apache-2.0**（`LICENSE-MIT` / `LICENSE-APACHE`，`Cargo.toml` 的 `license` 字段已设置）
- [ ] crates.io 凭据（`CARGO_REGISTRY_TOKEN`，仅发布 job 可见）
- [ ] 发布前将 `LICENSE-MIT` / `LICENSE-APACHE` 复制（或软链）到各 crate 目录，
      确保 `cargo package` 产物内含许可证文本
- [ ] 移除 `[workspace.package]` 的 `publish = false`（或逐 crate 覆盖）
- [ ] 确认 `description` / `repository` 元数据完整（docs.rs 展示）

## 2. 版本发布检查单

1. **全量验证**（必须全绿）
   - `cargo test --workspace --all-features`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo fmt --all --check`
   - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
   - CI：MSRV job 与 `cargo deny check`（安全公告 / 许可 / 来源）通过
2. **契约一致性**：`docs/spec/core.md` §6 验证报告更新；FC-Matrix 状态 100% `Passed`（0 x `Pending`）
3. **真实联调**：运行 `live_integration`，日志提交到 `docs/integration/<日期>-vX.Y/`，
   并在发布报告（core.md §6）中引用
4. **版本号**：更新 `[workspace.package] version` 与全部内部依赖版本
   （`sed -i 's/version = "OLD"/version = "NEW"/g' Cargo.toml`），运行一次构建让 `Cargo.lock` 收敛
5. **变更记录**：更新 `CHANGELOG.md`（破坏性标 **BREAKING**）与 `docs/spec/core.md` §5
6. **提交与打标**：
   ```bash
   git commit -m "feat: vX.Y  <主题>（N 条约束）"
   HASH=$(git rev-parse --short HEAD)   # 记录到 roadmap 快照表
   git tag -a vX.Y.Z -m "<主题>：<要点>（N 条约束）"
   git push && git push origin vX.Y.Z
   ```
7. **发布到 crates.io**（1.0 前可选；顺序见 §4）：`cargo publish -p <crate>`（逐 crate，等待索引刷新）
8. **发布后**：GitHub Release（tag 注解 + CHANGELOG 摘要）；验证 docs.rs 构建；
   归档联调日志；更新 roadmap 快照表

## 3. 变更分级（与 api-freeze.md 对齐）

- **BREAKING**：公共签名、wire 标签、枚举变体增减 → 需 RFC + CHANGELOG 标注（1.0 后需 major）
- **Feature**：新增类型 / 方法 / 可选字段 → minor
- **Fix / Docs / CI**：patch

## 4. crates.io 发布顺序（依赖拓扑）

```text
mycelium-core
  ├── mycelium-model      ─┐
  ├── mycelium-tools      ─┤（互相独立；先后皆可）
  ├── mycelium-macros     ─┘
  ├── mycelium-mcp
  └── mycelium-provider-openai
        └── mycelium-runtime（依赖 core / model / tools）
              └── mycelium（门面）
```

每个 crate 发布后需等待 crates.io 索引可见再发布其依赖者；`cargo publish --dry-run` 先行验证。

## 5. 回滚策略

- 代码回滚：`git revert` + 新 patch 版本重新发布；
- crates.io 不可撤回：只能 `cargo yank`（阻止新解析，不影响已有锁定）；
  yank 后必须发布修复版本并更新 CHANGELOG 说明。

## 6. 频率与节奏

- 功能批次完成后按 `roadmap.md` 里程碑发布；安全修复随时以 patch 发布；
- 每个发布必须包含：版本化的联调日志 + FC-Matrix 验证报告 + CHANGELOG 条目。
