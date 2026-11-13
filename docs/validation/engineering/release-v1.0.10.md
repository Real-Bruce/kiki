# v1.0.10 开源发布验证

日期：2026-11-13。用户接受历史中的姓名与个人邮箱公开，并要求合入 `main`、创建发布标签。本版本发布 MIT 协议、双语介绍、当前截图和模块文档；应用功能沿用 v1.0.9，不将既有功能缺陷或环境缺项标为解决。

## 源码与环境

- `codex/docs-module-guide` 的 `49a38c1` 已快进合入 `main`。
- 被测源码：`e24ee1d4da3a8a884fc96d8a188c907cbb3d6a0d`（`chore(release): prepare MIT open-source release v1.0.10`）。后续放行提交只补充本报告、文档索引与进度，构建输入保持一致。
- 相对 v1.0.9，`src`、`build.rs`、`resources`、`.cargo`、`Cargo.lock` 和 `rust-toolchain.toml` 均无变化；版本来自更新后的 `VERSION`，Cargo 增加 MIT 元数据，打包脚本增加 `LICENSE`。
- 本机 Windows 11 x64，系统 API 报告版本 `10.0.26300.0`；Rust / Cargo 1.88.0，MSVC x64 目标。正式包为无 diagnostics 特性的 Release 构建。本次没有性能采样或新的显示器矩阵验证。

## 实际检查

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 成功；增量缓存硬链接失败后自动复制的环境警告保留在日志中 |
| 受限沙箱首次默认测试 | 78 通过、4 失败、6 跳过；原生热键无法注册，设置／自启注册表访问被拒绝；原始失败未覆盖 |
| 正常桌面权限下默认测试，单线程 | 82 通过、0 失败、6 跳过；上述 4 项复测通过 |
| 正常桌面权限下全特性测试，单线程 | 82 通过、0 失败、6 跳过 |
| `./scripts/Package-Windows.ps1 -Tag v1.0.10` | 真实 Release 构建与打包成功；保留已有 `pinned::open` 未使用警告，未为本次发布修改功能代码 |
| 解压与文件核对 | 17 个条目，EXE 版本 1.0.10；EXE、MIT、快速说明、第三方许可与源文件 SHA-256 一致；Rust 标准库许可保留 |
| ZIP 校验文件 | SHA-256 与 ZIP 一致 |
| `git diff --check` | 通过 |

日志和解压副本保存在忽略的 `artifacts/release-v1.0.10/`，包括 `tests-default.log` 原始失败、`tests-default-desktop.log`、`tests-all-desktop.log`、`clippy.log`、`package.log`、`package-check.json`。本次解压检查为内容与版本核验，没有单独启动解压后的 EXE，也未执行新的手动 GUI 或性能验收；6 项原有忽略测试不计为通过。

## 本地产物

| 文件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `dist/kiki-v1.0.10-windows-x64.zip` | 590957 | `ebe945c52612cb00d9fb2816b1f40c8faeff3653c712a96918c8f8fdb8ddccfe` |
| ZIP 内的 `kiki.exe` | 1182208 | `69858410449ffecd942e2f1db5a6e88009eca30d756bd19d1fba17352e74a738` |

同目录提供 `kiki-v1.0.10-windows-x64.zip.sha256`。既有版本 ZIP 不覆盖。本地文件哈希只绑定上述本机产物，不预先声称 GitHub Actions 重建产物具有相同哈希。

## 放行与遗留

- 本地版本标签为 `v1.0.10`，使用英文标题和 Markdown 更新条目，供现有工作流的 `--notes-from-tag` 使用；标签指向包含本报告的放行提交。不改动已有版本标签。
- 已合并的文档分支可通过 `git branch -d` 清理，保留 `main`；不删除任何独有提交。
- 本次操作不执行推送或发布。用户推送 `main` 与 `v1.0.10` 后，现有标签触发工作流将构建 ZIP、校验文件并创建 Release；本次尚未验证该远端运行结果。
- 既有手动输出补验、多屏／混合 DPI、HDR、长截图 Daily 页面漏段及性能缺项继续见 [当前进度](../../PROGRESS.md)。
