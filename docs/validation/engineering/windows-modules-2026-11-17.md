# Windows 源码按功能分目录

日期：2026-11-17。基线提交：`e67a502`，应用版本保持 **v1.0.11**。本次为内部模块结构整理，提交主题为 `refactor(windows): group platform modules by feature`，不创建发布标签或推送。

## 改动范围

- 将原 Windows 目录内 37 个源码文件迁入九个功能目录；`windows/mod.rs` 只声明模块并导出启动和报错入口。
- 应用协调与测试放入 `app`，采集放入 `capture`，截图编辑及长截图放入 `editor`，贴图与设置分别放入 `pinned`、`settings`，成图及编码放入 `output`。共享设备和标注绘制放入 `graphics`，共用控件放入 `ui`，诊断探针放入 `diagnostics`。
- 同步修改模块声明、引用路径、平台内部可见性及许可文本的编译期相对路径。外置测试成为对应功能的普通子模块，移除原来的 `#[path]` 文件映射。
- 更新开发说明、功能计划及九份模块文档的源码链接。未改变应用功能、绘制参数、资源所有权、依赖、工具链或配置格式；未新增测试或修改既有断言。

## 环境与实际检查

Windows x64，系统版本 `10.0.26300.0`，仓库固定 Rust 1.88.0／2024 edition，目标 `x86_64-pc-windows-msvc`。本次检查为编译、原生自动测试及包内容检查，不是新增 GUI 或性能验收。

| 检查 | 实际结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check --workspace --all-targets --all-features --locked` | 通过 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 通过；增量缓存硬链接失败时编译器回退为复制 |
| 默认串行测试，首次受限沙箱 | 78 通过、4 失败、6 跳过；失败详情见下文 |
| `cargo test --workspace --locked -- --test-threads=1`，正常桌面权限 | 82 通过、0 失败、6 跳过 |
| `cargo test --workspace --all-features --locked -- --test-threads=1`，正常桌面权限 | 82 通过、0 失败、6 跳过 |
| `cargo build --release --locked` | 通过；默认特性保留原有 `pinned::open` 未使用告警，该入口用于诊断构建 |
| 迁移审查 | 对照 37 个原文件；除模块／资源路径、声明、可见性及 rustfmt 排版外，保留原逻辑与测试断言 |
| 模块文档源码链接 | 49 个链接目标存在 |
| ZIP 解压及 EXE 检查 | 17 个文件，解压 EXE 与本次构建 SHA-256 相同，产品版本为 1.0.11 |
| `git diff --check` | 通过 |

首次沙箱失败样本保留：

1. `app::hotkey::tests::conflict_preserves_old_registration_and_replacement_releases_it`：无法注册两组测试热键（`need two available test chords`）。
2. `app::startup::tests::startup_writes_are_explicit_and_quote_moved_paths`：隔离注册表写入返回 `0x80070005`，拒绝访问。
3. `editor::tests::reset_capture_defaults_preserves_active_editor_and_seeds_next_preview`：热键未生效，`preferences::active()` 断言失败。
4. `settings::tests::native_settings_apply_rollback_scroll_retry_and_teardown`：测试热键注册返回 `0x80070005`，拒绝访问。

以上四项在正常桌面权限下全部通过。6 项跳过沿用既有测试标记，不计为通过。日志位于忽略目录 `artifacts/windows-modules-default-tests.log`、`artifacts/windows-modules-all-tests.log`、`artifacts/windows-modules-release.log`。

## 验证包与限制

独立验证包：`artifacts/windows-module-refactor/kiki-v1.0.11-windows-x64-refactor.zip`，附 `.sha256`。包含本次 Release EXE、使用说明及完整许可文件；不覆盖 `dist` 中已验收的 v1.0.11 发行包。

- EXE SHA-256：`34785b0b04c5cf25dc34f8b5dee3d7b57788460682e2be15bbc7c69f3112ba26`。
- ZIP SHA-256：`9addbc1ba26e2cb938ea6350c198a3a8b8130fe7a0595ee81f924ad236c716d5`。
- 本次未手动运行解压程序、截图对照、重测时延／内存或执行多屏／HDR 测试。自动测试中原生窗口、绘图和编解码检查通过，不替代完整人工交互验收。
- 既有长截图漏段、真实多屏／混合 DPI、HDR 和性能环境待验项继续保留，详见 [当前进度](../../PROGRESS.md)。

## 用户复测包补充

2026-11-17 按用户要求，以提交 `68f8216` 再次运行 `scripts/Package-Windows.ps1`，Release 增量构建和打包成功。最新包位于 `dist/kiki-v1.0.11-windows-x64.zip`，大小 591,379 字节，附 `.sha256`；包与 EXE 哈希均与上方独立验证包一致。再次解压核对 17 个文件、EXE 版本 1.0.11、构建与解压 EXE 哈希及 ZIP 校验文件均通过，等待用户人工验证。

覆盖前的同名包及校验文件已备份到忽略目录 `artifacts/package-backups/159cc10c-ca91-4cbd-a346-5542ac6326af`。本次仅补充交付记录，未修改应用代码、版本或标签。
