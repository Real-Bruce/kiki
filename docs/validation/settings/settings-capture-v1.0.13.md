# v1.0.13 设置页截图残影复测

2026-11-17，Windows 11 x64 build 26300，单屏 1920 × 1080 SDR，Rust 1.88.0，MSVC x64。正式包使用默认特性的 Release 构建，预览和 PNG 输出使用同一修复的 Release diagnostics 构建，启用可访问截图窗口与隔离配置。

- 用户提供的 6.35 秒录屏中，约 1.41 秒已出现设置内容叠在桌面上的画面，约 2.82 秒时文字残影仍在框选区域内，约 3.53 秒退出截图后恢复正常。抽帧留在本机忽略目录。
- Win32 隐藏窗口后原有的 `DwmFlush` 同步当前提交，退出动画仍可能跨帧。修复临时关闭设置窗口的 DWM 过渡，隐藏并同步合成后恢复其过渡，再提交桌面捕获。相关系统调用失败时给出提示并保留或恢复设置窗口。[DWM 窗口属性](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute) · [DwmFlush](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmflush)
- 正式 ZIP 解压 17 个文件，核对 1.0.13 EXE 产品版本及哈希。启动解压后的 EXE，实际点击「通用 → 开始截图」，确认设置窗口隐藏、进程主窗口进入「kiki 截图」。
- 浅色诊断版从同一按钮进入截图，框选设置原先覆盖的区域，检查底图与工具条，通过原生保存对话框输出 750 × 417 PNG。单独打开成图，确认有原桌面内容，未见设置文字、半透明窗体残影或截图工具条。成图留在 `artifacts/settings-capture-v1.0.13/`。
- rustfmt、全特性全目标严格 Clippy、默认 Release 构建、全特性 Release 构建、打包和 `git diff --check` 通过。全特性串行测试在正常桌面权限下为 82 通过、6 跳过。首次沙箱运行有 4 项热键或注册表权限失败，与 [v1.0.12 记录](../engineering/windows-modules-2026-11-17.md)相同，正常权限复测全部通过。
- 用户随后确认人工验证通过。16:24:24 深色录屏约 2.07 秒可见框选及深色工具条，16:24:45 浅色录屏约 2.13 秒可见框选，两段录屏中选区都仅显示底层窗口内容，未见设置退出残影。录屏抽帧留在本机忽略目录。
- 未测多屏、HDR、独立干净系统或截图时延样本。

正式包为 `dist/kiki-v1.0.13-windows-x64.zip`，附 `.sha256`。

- ZIP SHA-256 `eecfc0eb12d9ec1e8b02be9b2e168f48e179e9dd2f107e5a071cd1395363bb42`
- 正式 Release EXE SHA-256 `fa6e630232fd025c2b79d44295817df138cb4f30a04191a510258120e81f1e01`
