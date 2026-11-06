# 应用图标

- 来源：用户于 2026-11-06 提供的 `iCapture-icons.zip`，其中 `iCapture-icons/iCapture.ico` 原样提取。
- 图案：透明圆角蓝色底板与白色猫爪；本次用户提供的图标作为应用标识。
- 内含 16、20、24、32、40、48、64、128、256 px 九种尺寸。
- SHA-256：`E5AC712D38BB829F79CA60A37CD2B69E8DB8E8CD11422A9C2A89BFB9F8149FED`。
- Windows 版只引入 ICO；macOS、网页及原始设计图片不参与本次构建。

`build.rs` 使用已有 Windows SDK 的资源编译器将 `resources/app.rc` 编译并链接到 EXE，不新增 Cargo 依赖或运行依赖。优先使用 `RC` 指定路径，其次 PATH、Windows SDK 环境变量和标准安装目录。非标准 SDK 安装可设置 `RC` 为 `rc.exe` 的完整路径。

平台图标模块从 EXE 资源加载系统尺寸图标，供托盘和窗口类使用。共享图标由 Windows 管理，不调用 `DestroyIcon`。分发程序无需携带外部 ICO。
