# 应用图标

- 来源为用户于 2026-11-06 提供的 `iCapture-icons.zip` 中的 `iCapture-icons/iCapture.ico`。原版 SHA-256 为 `E5AC712D38BB829F79CA60A37CD2B69E8DB8E8CD11422A9C2A89BFB9F8149FED`。
- 应用图标采用用户确认的松绿底色 #365F56 和暖白猫爪 #FAF9F6；从原图高分辨率版本重着色并缩放，保留透明边缘和过渡色。
- 内含 16、20、24、32、40、48、64、128、256 px 九种尺寸。
- 当前 SHA-256 为 `2E98E6E042DD597BD6706B09C96C8C26AA6849DDE6C225DA0088376912DCF67E`。
- Windows 版只引入 ICO；macOS、网页及原始设计图片不参与本次构建。

`build.rs` 使用已有 Windows SDK 的资源编译器将 `resources/app.rc` 编译并链接到 EXE，不新增 Cargo 依赖或运行依赖。优先使用 `RC` 指定路径，其次 PATH、Windows SDK 环境变量和标准安装目录。非标准 SDK 安装可设置 `RC` 为 `rc.exe` 的完整路径。

平台图标模块从 EXE 资源加载系统尺寸图标，供托盘和窗口类使用。共享图标由 Windows 管理，不调用 `DestroyIcon`。分发程序无需携带外部 ICO。

仓库内图标为 `resources/icons/kiki.ico`。
