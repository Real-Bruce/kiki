# 启动失败修复与迁移后回归

日期：2026-11-06。项目现位于 `F:/codespace/vibecode/kiki`，分支 `feat/windows-v1`。本次不代表 P2/P3 或首版全部验收完成。

## 修复

- 托盘创建失败时返回稳定的中文错误，避免销毁隐藏窗口后读取被覆盖的线程错误码，出现“操作成功完成”的误导信息。隐藏窗口由既有 RAII 释放。
- 顶层启动错误显示后返回退出码 1；正常退出和重复实例的受控退出仍返回 0。
- 单实例检查立即保存 CreateMutexW 的状态，避免后续操作改变线程错误状态。
- 无新增依赖；临时定位代码已移除，不向普通运行目录写调试日志。

Shell_NotifyIconW 的官方契约仅承诺 BOOL 成败，没有承诺扩展错误码。因此用户错误文本不依赖 GetLastError。参考：[微软 API 文档](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw)。

## 实际验证

- fmt 检查通过；默认和 all-features 的 all-targets 严格 Clippy 通过。
- 默认及 all-features 各 14 项测试通过，包括原生失败恢复和 WIC PNG 像素往返。
- 默认及 diagnostics Release 均构建成功。默认 EXE 为 477,184 字节，位置 `target/continuation-default/x86_64-pc-windows-msvc/release/kiki.exe`。
- 默认构建在受限环境启动，托盘不可用，进程实际退出码为 1。未取得该错误弹窗的视觉证据。
- 正常桌面环境中运行 `scripts/Measure-Startup.ps1`，30 次独立进程启动均正常退出并生成独立 CSV，全部 hotkey_registered=true。原始数据位于忽略目录 `artifacts/performance/startup-kiki-continuation`。
- 进程创建时刻至热键注册和托盘就绪：P50 33.224 ms，P95 37.861 ms，最大 48.294 ms，均值 34.132 ms。按正常系统缓存采样，不清空工作集，不混入同进程唤起样本。数据来自 diagnostics Release，不是截图端到端时延或最终产品性能。

## 本轮环境

Rust 1.88.0，x86_64-pc-windows-msvc，Release；Windows 11 专业版 10.0.26300；Intel Core i5-12400、12 逻辑处理器；系统可见内存 33,222,288 KiB；Intel UHD Graphics 730 驱动 32.0.101.7088。另有 GameViewer 虚拟显示适配器 15.6.5.199，未报告活动分辨率。活动显示 1920×1080、60 Hz；启动探针报告系统 DPI 120（125%），不作为混合 DPI 验收。HDR 本轮未独立复核，不作为 HDR 证据。

## GUI 验证边界和下一步

已实际启动 diagnostics 浅色截图窗口，窗口枚举返回“kiki 截图”。获取画布状态时电脑操作授权超时；刷新后遮罩已不在窗口列表中，测试进程仍驻留托盘。因此没有取得新的画布截图，也没有将边缘标签、默认 ToolWindow 输入或图标视觉标为通过。

下一步仍为 P3 最新边缘标签与工具条避让、默认窗口输入、拖动中 Esc 和高 DPI 实机验收；P4 标注随后接入。P2 多屏实机继续暂缓，HDR 与锁屏/热插拔遗留保留。输出污染检查随真实输出链路验证。

关联提交主题：fix(app): report tray startup failures and validate renamed build。
