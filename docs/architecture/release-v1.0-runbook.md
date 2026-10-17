# v1.0.0 发布执行手册（release 分支）

> 前置：`main` 已完成发布准备（发布名迁移 / 元数据 / 许可证 / 冻结评审 / semver 门禁）。
> 通用检查单见 [`release-process.md`](release-process.md) §2；本手册给出 v1.0.0 的精确步骤。

## 0. 前置条件

- [ ] crates.io API Token（`CARGO_REGISTRY_TOKEN`；scope 至少含 `publish-new` 与 `publish-update`）
- [ ] （建议）GitHub Actions 计费解锁：当前账号处于计费锁定，CI 全部 job 无法启动
      （证据：`docs/integration/2026-10-16-readme-e2e/ci-startup-failure.txt`）；解锁后 CI 徽章才会转为真实状态
- [ ] 本机已登录 crates.io（`cargo login`）或导出上述 Token

## 1. 创建 release 分支并最终验证

```bash
git checkout main && git pull
git checkout -b release/v1.0.0

# 全量验证（预期全绿）
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo deny check
cargo package --workspace --no-verify          # 打包预检（8 个 crate，含 README / LICENSE）
```

## 2. 版本号与文档定稿

```bash
# 1) 版本号：0.7.0 -> 1.0.0（工作区版本 + 内部依赖版本一并替换）
sed -i 's/version = "0.7.0"/version = "1.0.0"/g' Cargo.toml
cargo build --workspace        # 让 Cargo.lock 收敛

# 2) CHANGELOG：把 [Unreleased] 内容并入 [1.0.0] - <发布日>，并新增对比链接
#    [1.0.0]: https://github.com/oxroot-crypto/mycelium/compare/v0.7.0...v1.0.0

# 3) 文档收尾
#    - docs/spec/core.md §6：将「v1.0（发布准备）」更新为本发布日的最终记录（工具链 / 测试数 / 联调日志引用）
#    - README.md / README.en.md：状态行 v0.7.0 -> v1.0.0；
#      「安装」段改为 crates.io 版本写法（mycelium-agent = "1.0"），保留 git 依赖作为备选说明
#    - docs/architecture/roadmap.md：v1.0.0 行更新为已发布（发布后补 hash 提交）
```

联调日志：发布准备批次已在 `docs/integration/2026-10-17-v1.0/` 归档六场景原始日志；
若 release 分支相对 main 仅有版本 / 文档变更（无代码变更），可直接引用，无需重跑。

## 3. 提交与打标

```bash
git add -A
git commit -m "feat: v1.0.0 API 稳定（发布名 mycelium-agent-*、冻结生效、155 条约束）"
HASH=$(git rev-parse --short HEAD)     # 记录到 roadmap 快照表
git tag -a v1.0.0 -m "v1.0.0：API 冻结（导入名不变；crates.io 发布名 mycelium-agent-*）"
git push origin release/v1.0.0
git push origin v1.0.0
```

## 4. crates.io 首发（按依赖拓扑顺序，逐步执行）

```text
mycelium-agent-core
  ├── mycelium-agent-macros（被 tools 依赖，先于 tools 发布）
  ├── mycelium-agent-model
  ├── mycelium-agent-tools（依赖 core / macros）
  ├── mycelium-agent-mcp（依赖 core / tools）
  └── mycelium-agent-provider-openai（依赖 core / model）
        └── mycelium-agent-runtime（依赖 core / model / tools）
              └── mycelium-agent（门面）
```

- 每步：`cargo publish -p <包名>`，等待索引可见后再发布其依赖者
  （可先用 `cargo search <包名>` 或 https://crates.io/api/v1/crates/&lt;包名&gt; 确认）。
- **首次发布 bootstrap（实测行为）**：cargo 打包会剥离 `path` 依赖并以注册表解析——
  依赖方在依赖上架前 `cargo package`（含校验）会报 `no matching package named … found`，属预期；
  按上表顺序真实发布即可。预检用 `cargo package --workspace --no-verify`。
- `cargo publish --dry-run` 仅对 `mycelium-agent-core` / `mycelium-agent-macros`
  （无内部依赖）可完整执行；其余 crate 的完整校验发生在真实发布后的构建步骤。

## 5. 发布后

- [ ] GitHub Release：tag 注解 + CHANGELOG v1.0.0 摘要
- [ ] docs.rs 构建验证（`https://docs.rs/mycelium-agent/1.0.0`；门面按 `all-features` 构建）
- [ ] 冒烟验证：新建临时工程 `cargo add mycelium-agent`，运行 README 最小示例
- [ ] roadmap 快照表补提交（发布 commit hash）；CHANGELOG 顶部链接切换到 v1.0.0
- [ ] 若 CI 计费已解锁：确认 `ci.yml` 全 job 绿（含 semver 门禁对 `v1.0.0` 基线生效）

## 6. 回滚

见 [`release-process.md`](release-process.md) §5：代码 `git revert` + patch 重发；
crates.io 只能 `cargo yank`（阻止新解析），yank 后必须发布修复版本并更新 CHANGELOG。
