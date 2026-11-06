# P6 精简交接

- 仓库：F:/codespace/vibecode/kiki，分支 feat/windows-v1；原 iCapture 目录为空。Rust 2024 / 1.88、windows-rs 0.61.3、Win32/D2D/DWrite/WIC。P3 暂缓，P2/P4 实机遗留保留。不要推送远端。
- P5 已完成单屏 SDR 验收，提交 a60840c。默认 EXE 664,576 字节，标准路径 target/x86_64-pc-windows-msvc/release/kiki.exe。详见 validation/P5_COMPLETION.md。
- 用户要求“压缩上下文后开始 P6”。当前工具没有会话压缩动作，已明确告知；真正的压缩需用户发送 /compact。P6 基础工作已保存，不能宣称系统已完成压缩。
- 本轮 P6 新文件：src/core/config.rs、src/platform/windows/config_store.rs、hotkey.rs。三者暂用 cfg(test) 门控。新增 6 项测试，全套 all-features 38 项通过；详见 validation/P6.md。
- 配置模型为有版本的 INI，无新 crate；Config::encode/decode/validate。主题、Hotkey、show_hints、first_tip_seen、默认样式、last_directory。Writer 合并为一个待写快照，用 revision + Outcome 通知。自动保存成功提示必须等最新结果成功。配置路径为 LocalAppData/kiki，原子替换、损坏备份、更高版本保护已有基础。
- Registration::replace 先申请新 ID，成功后再注销旧 ID；需接入当前硬编码 Ctrl+Alt+A 注册并让 WM_HOTKEY 校验活动 ID。原生 HOTKEY_CLASS 控件转换函数已实际测试。输入热键时不要让活动截图热键意外开启选区。

## 接下来按序实现

1. preferences 运行时管理器：加载 Config、持有 Writer 与 Registration，保存状态/错误，应用主题及工具默认值，持久化最近保存目录。移除对应测试门控。
2. 设置窗口六页：通用、快捷键、截图、贴图、外观、关于。左导航、卡片、真实键盘交互、有效保存状态，参考 03-settings.png；固定 PNG、启动驻留托盘等为静态说明。
3. 开机启动默认关闭，仅用户在应用内操作时写 HKCU Run；提供程序移动后的路径修复。测试用隔离目标，不能为验收直接开启用户实际自启。
4. 主题覆盖设置、截图工具条/参数、文字输入、贴图及菜单；系统主题变化走事件。提示开关控制非关键操作提示。工具默认值仅影响新的截图；保留现有标注。
5. 托盘补设置和全屏截图，首次使用提示、版本/帮助/第三方许可。关闭设置保持托盘后台。先创建可审阅结果，再做真实 GUI、重启持久化、热键冲突等验收，更新进度并本地提交。

## 工具与构建注意

- 旧 cwd 仍可能为 iCapture，所有命令显式 workdir 新仓库。写入/构建/Git 需 require_escalated，用户已授权此项目工作。
- 构建临时目录：TEMP/TMP=F:/codespace/vibecode/kiki/artifacts/rust-tmp。原始图像/日志/CSV 只放忽略的 artifacts。
- Windows PowerShell 不支持 Bash 花括号路径展开。多文件 rg 用明确路径；写文本统一 LF/UTF-8，并 git diff --check。
- 真实 UI 用 computer-use 技能与 node_repl 的 @oai/sky。先枚举返回窗口并唯一定位，每次动作后刷新；不要用其他输入渠道绕过工具错误或授权。用户停止电脑操作时停止输入。
- Windows 测试的图形工厂须使用 tests::graphics_runtime 保持进程生命周期，绘图设备在 COM 退出前显式清理。已有 P5 原生窗口回归可参考 preview.rs / pinned.rs 的 tests。
- 不主动生成子代理；当前没有用户授权委派。
