# kiki

面向 Windows 11 的轻量截图、标注与置顶贴图工具。使用 Rust 原生实现，优先控制体积、响应速度与后台资源占用。

## Windows 11 x64 免安装包

从本地 dist 目录解压 **kiki-v0.1.0-windows-x64-settings-reference.zip**，双击 kiki.exe。程序驻留托盘，默认按 **Ctrl+Alt+A** 开始截图。可用 Get-FileHash 核对同目录的 .zip.sha256。

[快速使用说明](docs/QUICK_START.txt) · [版本变更](CHANGELOG.md) · [P8 交付与验收](docs/validation/P8.md)

## 开发与验证

需要 Windows 11 x64、MSVC Build Tools 和 Windows SDK。固定 Rust 1.88.0，Cargo 会读取仓库内工具链与目标配置。

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

构建结果为 `target/x86_64-pc-windows-msvc/release/kiki.exe`。运行后驻留托盘，Ctrl+Alt+A 或托盘开始截图打开冻结桌面选区；拖动框选，内部拖动移动，调整点改变尺寸，F 选择鼠标所在屏幕，Esc 取消操作或会话。当前工具条已接通选择、矩形、箭头、画笔、文字、撤销、重做、贴图、保存、复制与取消。选择工具命中对象后可整体移动，矩形八点调整、箭头端点调整；Delete 删除，Ctrl+Z / Ctrl+Y 撤销/重做。重新选择已有对象后切到对应绘图工具可修改颜色、粗细或字号。文字工具单击创建/重编。文字框透明并随内容伸缩。用选择工具单击已有文字，拖动文字移动或拖动顶部圆点旋转，双击可重编；文字输入时 Enter 换行、Ctrl+Enter 提交，Esc 取消本次文字输入；非文本输入时 Enter 复制，Ctrl+S 打开原生 PNG 保存对话框，Ctrl+P 生成置顶贴图。成功才结束截图，取消保存或输出失败保留编辑。输入框中的 Ctrl+Z、Delete、F 作用于文本。参数条可用 Tab/Shift+Tab 访问，Enter/空格激活。`--capture` 可在首次启动时直接捕获，`--theme=light` / `--theme=dark` 可选择本阶段主题。

开发探针须用 `cargo build --release --features diagnostics` 单独编译，默认构建不包含它们：`kiki.exe --probe artifacts/p1` 写入真实桌面 PNG 与捕获耗时；`kiki.exe --stress artifacts/p1/stress` 执行 100 次捕获、释放及资源采样；`scripts/Measure-Startup.ps1` 采集 30 次独立进程启动到热键和托盘就绪时间。仅显式调用时运行，结果目录不入库。探针会包含真实屏幕内容，请勿将其作为公开测试数据提交。完成后用默认 `cargo build --release` 恢复不含探针的程序。

当前验证结果和缺项见 [P1](docs/validation/P1.md)、[P2](docs/validation/P2.md)、[P3](docs/validation/P3.md)、[P4](docs/validation/P4.md) 、[文字标注](docs/validation/P4_TEXT.md) 、[P5 输出](docs/validation/P5.md) 和 [截图界面优化](docs/validation/UI_POLISH.md) 验证报告。`--probe` 还输出中心裁剪 PNG；开发构建的 `--cancel-stress` 验证 100 次实际取消及同进程恢复。电脑操作工具需要时，可在 diagnostics 构建显式使用 `--accessible-capture` 枚举同一画布；默认构建保留 ToolWindow 样式。完整输入法、高 DPI、多屏、HDR 及更多应用兼容性验收仍待补齐。

开发构建可显式使用 `--trace-window-targets <文件路径>` 记录窗口类别、样式和边界，用于检查透明辅助层筛选；每次捕获覆盖该文件，不记录窗口标题或输入内容。

成图开发探针：diagnostics 构建可用 `kiki.exe --annotation-probe artifacts/p4-composition` 输出一张明确的测试底图和一张真实桌面裁剪，各自叠加诊断标注，用于检查共享绘制、透明区域和 PNG 内容。它是独立开发探针，不触发普通用户的保存/复制操作，输出含真实屏幕内容。详见 [P4 成图记录](docs/validation/P4_COMPOSITION.md)。


贴图支持拖动、滚轮缩放、Ctrl+0 恢复原比例、Ctrl+C 复制、Ctrl+S 保存、Esc 关闭。悬停显示复制/保存/关闭，右键提供相同操作及恢复原比例；托盘可关闭所有贴图。缩放只改变显示，输出保持原始图像尺寸。当前最多 64 张、累计原始图像 512 MiB，显示边长/面积另有上限；大图恢复 100% 受限时显示提示。已完成共享设备与缓存回收修正、Word 粘贴及原生失败恢复验收，见 [P5 完成记录](docs/validation/P5_COMPLETION.md)。多屏等原遗留与 P7 的资源指标继续单列。

开发构建另提供 `--pin-demo`（两张明确测试图）、`--pin-stress <目录>`（100 轮，每轮四张独立 4K 渐变图，隐藏窗口绘制/关闭及 CPU/GPU 采样）、`--pin-4k-demo`（四张可见 4K 测试图），可加 `--pin-sample <目录>` 记录有界的可见窗口资源采样，普通构建不包含这些入口。

可从托盘或 `--settings` 打开设置；`--capture-screen` 可选择鼠标所在屏幕。设置包括热键冲突回滚、自动保存、主题 / 提示 / 默认样式与显式开机启动。配置位于当前用户 LocalAppData/kiki；开机启动默认关闭，仅在用户开启时写入。详见 [P6 验证](docs/validation/P6.md)。

P7 的采样口径、性能数字和未通过项见 [验证报告](docs/validation/P7.md)。`scripts/Measure-Performance.ps1` 可分别采集 30 个进程首次截图、200 轮真实截图 / 取消 / 贴图 / 关闭和 60 秒闲置 CPU；`scripts/Measure-Idle.ps1` 可对指定进程读取外部闲置指标。时延探针是 diagnostics 构建的应用端基准，不能混称物理键盘全链路时延。

## 新会话执行入口

请先阅读 [AGENTS.md](AGENTS.md)，然后按 [实施计划](docs/IMPLEMENTATION_PLAN.md) 开始工作。

- [产品规格](docs/PRODUCT_SPEC.md)
- [设计规格](docs/DESIGN_SPEC.md)
- [实施计划](docs/IMPLEMENTATION_PLAN.md)
- [进度与交接记录](docs/PROGRESS.md)
- [设计参考说明](docs/design/README.md)

## 已确认范围

- 区域、窗口、全屏和跨显示器截图，兼容混合缩放。
- 矩形、箭头、画笔、文字，可编辑标注，撤销与重做。
- 独立的复制、PNG 保存和置顶贴图。
- 多张贴图的拖动、缩放、复制、保存和关闭。
- 默认 Ctrl+Alt+A，全局热键冲突提示，托盘与设置窗口。
- Win11 圆角半透明外观，跟随系统或手动选择深浅色。
- Windows 11 x64 ZIP 免安装首版；后续保留接入 macOS 的结构。

## 设计稿

![截图与标注](docs/design/01-capture.png)

![浅色与深色工具条](docs/design/02-toolbars.png)

![功能设置](docs/design/03-settings.png)

![多张贴图](docs/design/04-pinned.png)

本仓库中的设计稿由内置图像生成工具制作，用户已认可整体方向。真实控件行为以产品及设计规格为准。
