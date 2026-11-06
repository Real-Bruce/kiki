# P4 第二批：文字标注与输入

日期：2026-11-06。P3 按用户要求继续暂缓；本轮推进 P4，尚不代表完整阶段验收。

## 实现

- 文字是平台无关标注对象，保存 UTF-8 内容、全局物理范围、物理字号与换行宽度；通过 DirectWrite 测量与绘制。整块命中和移动不改变换行宽度。
- 文字工具单击创建或重新打开顶层命中的文字；选择工具双击文字再次编辑。一次输入确认形成一条历史；取消保留原文；清空并确认会删除旧对象，删除可撤销。失败恢复输入框，保留草稿和原对象。
- 原生多行 EDIT 接收中文输入，Enter 换行，Ctrl+Enter 提交，Esc 取消编辑；Ctrl+A、Ctrl+Z、Delete 和 F 首先由文本控件处理。组合输入期间不将确认候选/取消候选按键当成截图命令。点击画布或工具前尝试完成 IME 组合；无法完成时提示先确认候选，保留草稿。
- 文字参数为颜色与 18/24/36 DIP 字号；工具样式与对象物理字号分开，按屏幕缩放换算。颜色、线宽、字号参数可用 Tab/Shift+Tab 导航、Enter/空格激活，有焦点绘制。
- Native EDIT 的 HWND、HFONT、背景 HBRUSH 和子类状态独立管理。父窗口先销毁子控件时会清除共享存活标记，防止旧句柄被重复销毁；COM/Windows 回调均保留 panic 边界。
- 每个文字对象最多 16384 个 UTF-16 单元（包含 CRLF 的存储开销），同时校验测量范围、溢出与对象索引。参考：[EDIT 长度限制](https://learn.microsoft.com/en-us/windows/win32/controls/em-setlimittext)、[输入法组合字符串](https://learn.microsoft.com/en-us/windows/win32/intl/composition-string)。
- 仅开启现有 windows 0.61.3 的 Win32_UI_Controls 与 Win32_UI_Input_Ime 功能；没有新 crate、工具链升级或非 Rust 应用运行时。IME 使用 Windows 自带能力。默认 Release 526,336 字节，比几何批次增加 26,624 字节。

## 实际自动检查

- fmt 检查通过；默认及 all-features all-targets 严格 Clippy 通过。
- all-features 24 项测试通过：原 20 项，加上中文多行重编/删除历史往返、文本失败保留对象与 redo、移动不重排/顶层遮挡命中，以及隐藏原生 EDIT 的 Unicode 往返、模拟 IME 取消路由、Ctrl+Enter LF 抑制和父子窗口回收。
- 模拟 IME 开始/结束消息仅验证消息路由，不等于真实输入法实测。
- 曾在加入 GetWindowSubclass 测试/查询后出现 STATUS_ENTRYPOINT_NOT_FOUND，测试尚未进入运行即退出。改为控件持有 Rc/Cell 状态，不依赖该导出查询，后续测试恢复通过。未把本机结果扩展为其他 Windows 环境兼容性结论。
- 最新默认 Release 构建成功：`target/p4-text-checkpoint/x86_64-pc-windows-msvc/release/kiki.exe`。最新 diagnostics Release 构建成功：`target/p4-text-final/x86_64-pc-windows-msvc/release/kiki.exe`。
- 最终诊断构建运行一次启动探针，退出码 0，hotkey_registered=true，系统 DPI 120，就绪 36.675 ms，证据 `artifacts/p4-text-startup/startup.csv`。这是加载和就绪烟雾检查，不能作为 P95 或端到端截图时延。

## 实际界面与修正边界

使用同机 Windows 11、1920×1080、125% 的 diagnostics 可访问窗口模式：

- 框选 1087×525，浅色工具条中“文字”可启用，显示 8 色与 18/24/36 字号参数。
- 单击创建原生输入框，辅助功能树确认焦点在 EDIT；输入“中文标注测试 ABC 123”，Enter 后输入“第二行 F 保持为字符”。真实截图显示两行，未完成截图，F 是文本字符。
- Ctrl+Enter 后输入框消失，文字出现在冻结画布并有对象选择框；再次单击原文字，输入框重新打开并选中全部内容。
- 这些中文字符通过 Unicode 文本注入输入，未观察到真实拼音候选框，不计为中文 IME 验收。
- 发现 Ctrl+Enter 附带的 WM_CHAR LF 被当成第三行空行。已增加抑制标记，在收到 CR/LF 时消费；原生回归覆盖按键抬起后到达的 LF，不会用抬键提前清除标记。
- 重编后尝试继续修改时，电脑操作返回 foreground window did not report a process id；刷新窗口列表后截图窗口已不在列表中。随后最终深色窗口也在读取前消失，未完成后续 GUI 输入。
- 实际证据在忽略目录 `artifacts/visual/p4-text/`：multiline-input.jpg、committed-text.jpg、reedit-selection.jpg。它们来自较早诊断构建；LF 修正、换行宽度统一、参数键盘导航、IME 完成处理和深色背景的最新代码尚待实机复验。

## 尚待验收

- 真实拼音输入、候选框定位、候选确认/取消、多行与中英切换；重编草稿取消恢复和文本内撤销优先级的完整 GUI 矩阵。
- 最新 Ctrl+Enter 修正、长行自动换行、字号修改、参数键盘访问、深浅色文字输入框及 150%/200%/混合 DPI。
- 没有为绕过窗口关闭或授权状态改用其他 UI 输入通道。
- P5 的剪贴板、PNG、贴图尚未接通；保存/贴图前提交可见编辑及预览/成图一致性随 P5 联验。P4 尚不标为完整验收。

关联提交主题：feat(annotation): add native multiline text editing and IME routing。
