# v1.0.5 设置布局验收

日期为 2026-11-13。使用 Windows 11 x64 build 26300、Rust 1.88.0 MSVC。界面与贴图验证在单屏 1920 × 1080、125% 缩放下进行。Release 诊断副本使用独立配置目录和单实例锁，与桌面上已有的 kiki 实例并行运行。

## 原生窗口

| 检查 | 实测结果 | 界面 |
| --- | --- | --- |
| 快捷键页 | 「开始截图」下方的独立分组显示 Ctrl+0、Ctrl+C、Ctrl+S 和 Esc。截图热键录入入口与其恢复默认按钮均可见。 | [浅色](settings-layout-v1.0.5/shortcuts-light.jpg) · [深色](settings-layout-v1.0.5/shortcuts-dark.jpg) |
| 贴图页 | 显示拖动、滚轮、右键菜单说明和「关闭贴图」按钮。四项键盘操作集中在快捷键页。 | [浅色](settings-layout-v1.0.5/pinned-light.jpg) · [深色](settings-layout-v1.0.5/pinned-dark.jpg) |
| 截图页 | 无需滚动即可在「截图提示」上方看到「恢复默认」。按钮可打开原生确认框。取消后圆角仍为 18 px、截图提示关闭。确认后圆角归零、截图提示开启，磁盘配置保留深色主题和 Ctrl+Alt+F9。 | [浅色](settings-layout-v1.0.5/capture-light.jpg) · [深色](settings-layout-v1.0.5/capture-dark.jpg) |
| 截图输出 | 使用隔离热键开始截图，框定设置窗口后按 Ctrl+P，检查实际贴图中的窗口内容及布局，并以 Esc 关闭贴图。实机 PNG 流程检查至另存为对话框。 | 窗口选区为 1184 × 1012 px |

六张证据为 Windows Graphics Capture 返回的设置窗口截图，记录尺寸为 947 × 810。

## 构建与打包

- 格式检查和全特性 Clippy 通过。默认与全特性测试串行运行，各通过 82 项、跳过 6 项。受限环境首次运行有 4 项热键或注册表权限相关失败，桌面环境复跑通过。
- 正式 Windows x64 Release ZIP 为 586,798 字节。解压得到 16 个文件，EXE 为 1,175,040 字节，产品版本为 1.0.5，PE 架构为 x64。核对包内 EXE 与构建产物哈希一致，并检查包内使用说明。
- ZIP SHA-256 为 `404ef20dac7bc9fbae8f033320a40ff5677c1d1364e904c666fe14b51d2ab9b5`。
- EXE SHA-256 为 `c68481b17133e7ada6f6436de260cbcb31c9e4bdbe7d821c95f32289eca057c8`。
- 本次记录包体积，运行时性能数据仍见 [P7 性能验收](P7.md)。多屏、其他 DPI 和 HDR 仍见 [当前进度](../PROGRESS.md#既有环境与功能遗留)。
