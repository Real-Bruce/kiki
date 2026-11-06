# kiki

面向 Windows 11 的轻量截图、标注与置顶贴图工具。使用 Rust 原生实现，优先控制体积、响应速度与后台资源占用。

当前状态：已建立 Rust 原生 Windows 工程，正在 P4 可编辑标注阶段，P2/P3 仍有待验收项；P3 按用户要求暂缓。当前程序提供托盘、热键、WGC 捕获和原生选区工具条；已接通矩形、箭头、画笔和文字标注，输出尚未接通，不作为 v0.1.0 成品分发。

## 开发与验证

需要 Windows 11 x64、MSVC Build Tools 和 Windows SDK。固定 Rust 1.88.0，Cargo 会读取仓库内工具链与目标配置。

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

构建结果为 `target/x86_64-pc-windows-msvc/release/kiki.exe`。运行后驻留托盘，Ctrl+Alt+A 或托盘开始截图打开冻结桌面选区；拖动框选，内部拖动移动，调整点改变尺寸，F 选择鼠标所在屏幕，Esc 取消操作或会话。当前工具条已接通选择、矩形、箭头、画笔、文字、撤销、重做与取消；输出按钮暂时禁用。选择工具命中对象后可整体移动，矩形八点调整、箭头端点调整；Delete 删除，Ctrl+Z / Ctrl+Y 撤销/重做。重新选择已有对象后切到对应绘图工具可修改颜色、粗细或字号。文字工具单击创建/重编，选择工具双击文字也可重编；Enter 换行、Ctrl+Enter 提交，Esc 取消本次文字输入。输入框中的 Ctrl+Z、Delete、F 作用于文本。参数条可用 Tab/Shift+Tab 访问，Enter/空格激活。`--capture` 可在首次启动时直接捕获，`--theme=light` / `--theme=dark` 可选择本阶段主题。

开发探针须用 `cargo build --release --features diagnostics` 单独编译，默认构建不包含它们：`kiki.exe --probe artifacts/p1` 写入真实桌面 PNG 与捕获耗时；`kiki.exe --stress artifacts/p1/stress` 执行 100 次捕获、释放及资源采样；`scripts/Measure-Startup.ps1` 采集 30 次独立进程启动到热键和托盘就绪时间。仅显式调用时运行，结果目录不入库。探针会包含真实屏幕内容，请勿将其作为公开测试数据提交。完成后用默认 `cargo build --release` 恢复不含探针的程序。

当前验证结果和缺项见 [P1](docs/validation/P1.md)、[P2](docs/validation/P2.md)、[P3](docs/validation/P3.md)、[P4](docs/validation/P4.md) 和 [文字标注](docs/validation/P4_TEXT.md) 验证报告。`--probe` 还输出中心裁剪 PNG；开发构建的 `--cancel-stress` 验证 100 次实际取消及同进程恢复。电脑操作工具需要时，可在 diagnostics 构建显式使用 `--accessible-capture` 枚举同一画布；默认构建保留 ToolWindow 样式。完整中文输入验收、输出操作、贴图、设置、完整主题体验及 ZIP 交付尚待后续阶段。

开发构建可显式使用 `--trace-window-targets <文件路径>` 记录窗口类别、样式和边界，用于检查透明辅助层筛选；每次捕获覆盖该文件，不记录窗口标题或输入内容。

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
