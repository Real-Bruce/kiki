# iCapture

面向 Windows 11 的轻量截图、标注与置顶贴图工具。使用 Rust 原生实现，优先控制体积、响应速度与后台资源占用。

当前状态：产品方案和视觉方向已确认，仓库仅包含交接材料与设计参考，尚未创建应用代码。

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
