# P5 完成验收：输出与多张贴图

日期：2026-11-06。状态：**P5 已完成当前单屏 SDR 环境的实施与验收**。P3 按用户要求暂缓；P2/P4 的跨环境及 IME 实机遗留继续单列，不在本记录中标为通过。下一阶段为 P6，尚未开始。

## 计划验收对应

| P5 验收项 | 结果与证据 |
| --- | --- |
| Windows 应用粘贴、PNG 内容与透明区 | Word 2016 实际粘贴通过；可见中文、矩形、箭头、笔迹和透明间隙。原生测试验证 PNG/直通 alpha 和 DIB 布局。其他应用兼容性留给 P7 扩展矩阵，不推断全部应用通过。 |
| 保存取消及输出失败后继续编辑 | 原生对话框取消实测保留；原生窗口消息泵回归实际制造只读目标、独占文件锁和另一 HWND 的剪贴板锁，错误后窗口及全部对象保留，解锁后重试保存成功。 |
| 缩放后输出原始尺寸 | 640×360 图缩放到 110% 后保存与复制仍为 640×360；4K 图以约 28% 显示时复制仍为 3840×2160。 |
| 多张 4K CPU/GPU 资源记录、关闭后回落 | 十轮四张 4K 压测及四张可见 4K 采样均保留原始数据，关闭后 CPU/GPU 占用回落；窗口句柄未随轮数持续增长。详见下文。 |
| 真实贴图界面对照 04 | 最终设备绘制路径的浅色成图、悬停条、菜单、保存，以及深色 4K 悬停条、比例提示已实际查看。右键恢复比例、保存菜单实际点击；托盘关闭全部由用户操作确认，随后窗口枚举和计数均为零。 |

## 本轮修正

- 将贴图从旧式软件 HWND render target 改为共享 D3D11 / Direct2D device context 与每窗口 DXGI 交换链。仍是既定 Rust、Win32、Direct2D、DirectWrite 技术栈，未增加 crate 或框架；硬件设备创建失败时使用 WARP。
- 将可缓存纹理预算限制为 16 MiB。最后一张关闭时释放绘图绑定及无用缓存，并通过消息窗口在 200 ms 后再执行一次清理，覆盖尚未完成的 GPU 工作。空闲没有周期渲染或鼠标轮询；没有清空工作集、改工作集上下限或用采样过滤美化数字。
- 设备在应用生命周期内复用，避免反复创建设备导致驱动句柄增长；应用退出时先销毁贴图和设备，再结束 COM。诊断命令也使用相同释放顺序，已确认最终压测进程退出码为 0。
- 交换链重建前先释放旧表面，设备被移除时可重新创建设备；原生测试实际重建表面，并验证应用清理能关闭仍然存在的多张贴图。
- 主图像仍是独立成图的 Arc；显示缓存与原始输出分离，贴图不持有桌面快照或编辑历史。数量、原始图像字节及显示尺寸上限保留。
- 原生窗口集成测试走实际后台合成、文件写入、剪贴板锁、WM_TIMER、原生 EDIT 和窗口销毁。只在测试构建中替代文件选择结果、收集错误提示，未模拟 IO 成功/失败；这些入口不进入普通构建。
- 测试进程与实际 Runtime 一样保持 DXGI/COM 工厂的进程生命周期，修正跨 Rust 测试线程提前卸载驱动造成的访问冲突；全套测试在修正后通过。

实现参考：[Direct2D device context](https://learn.microsoft.com/en-us/windows/win32/direct2d/devices-and-device-contexts)、[ClearResources](https://learn.microsoft.com/en-us/windows/win32/api/d2d1_1/nf-d2d1_1-id2d1device-clearresources)。

## 实际输出与界面

- Word 文档由用户打开，电脑操作在正文粘贴后实际看到了完整测试图；没有保存或关闭用户的 Word 文档。测试图为明确的诊断成图，不把测试底图当作桌面截图。证据：artifacts/visual/p5-completion/word-paste-full-view.jpg。
- 最终菜单“恢复原始比例”实际从 110% 返回 100%；“保存 PNG”打开原生对话框并实际写出 artifacts/p5-final-gui-save.png，大小 9,729 字节。菜单复制入口及 Ctrl+C 后检查有 PNG、DIBV5、传统 DIB。
- 保存文件和剪贴板 PNG 的 SHA-256 均为 7E85A57B6C06AC07908FE281A49F8860C62C2D00EA6D93668B2BABB9E8BB26D1，与第一批相同，证明替换显示实现后成图数据不变。元数据：artifacts/p5-final-clipboard/clipboard.txt。
- 四张 4K 测试图同时显示时，滚轮更新比例到 28%，深色悬停按钮和提示可见。Ctrl+C 产生原始 3840×2160 图像，见 artifacts/p5-final-4k-clipboard。
- 用户实际右键托盘选择关闭全部，回复“四张已全部关闭”；随后电脑操作枚举不再有 kiki 贴图窗口，诊断 count 从 4 到 0。托盘进程保留，符合关闭贴图后仍可截图的行为。
- 此前独立记录的真实 WGC 桌面 Ctrl+P、单张关闭、拖动、保存取消、工具条不污染输出等检查继续有效，见 P5.md / P5_PINNED.md；本轮没有重新认定 P3 的任意矩形框选验收。

## 资源结果

环境：Windows 11 专业版 10.0.26300，Intel i5-12400，枚举到 GameViewer Virtual Display Adapter、Intel UHD Graphics 730；Rust 1.88 / MSVC x64 Release diagnostics。GPU 数据来自绘图设备所属适配器 node 0 的 QueryVideoMemoryInfo，记录本进程 local / nonlocal 用量，不是整机显存。设备尚未创建的基线标为 NA，不能推断成零；UMA 的 GPU 数值也不与进程 private bytes 相加。

1. **十轮四张独立 4K**：命令 `--pin-stress artifacts/p5-final-stress`，创建隐藏原生窗口、实际绘制并关闭，最后泵消息处理延后清理。每轮源图共 126.56 MiB。
   - 打开时 private bytes 约 403.4–403.9 MiB，GPU local 约 304.7–305.1 MiB。
   - 部分立即关闭样本仍有异步回收中的分配；最终两秒后 private bytes **53.28 MiB**、工作集 **57.26 MiB**、GPU local **13.93 MiB**、nonlocal 0。
   - 打开时句柄均 294，关闭后大多 270、个别即时样本 271；最终 270。原始 CPU CSV 保留全部 22 行，GPU CSV 亦全部保留。
2. **四张可见 4K**：`--pin-4k-demo --theme=dark --pin-sample artifacts/p5-visible-final-4k`，每图初始按 25% 摆成四格；采样期间由用户通过托盘关闭全部。
   - 关闭前最后一行 count=4，private bytes **313.50 MiB**、GPU local **155.32 MiB**。
   - 关闭后 count=0，private bytes **58.24 MiB**、工作集 **68.86 MiB**、GPU local **17.07 MiB**，nonlocal 0。
   - 该次包含托盘菜单与交互引入的系统资源，不与纯隐藏窗口的句柄/GDI/USER 基线混算。全量 visible.csv 保留 67 行（含表头）。

较上一批关闭后约 245 MiB 的私有内存有明显改善。这里采的是 private bytes 与 total working set，**不是私有工作集**，没有据此判定闲置私有工作集 ≤20 MiB 达标；对应指标与更多机器、真实 4K 内容下的响应时延在 P7 验证。

## 原生窗口回归覆盖

- 只读目标或文件被独占时，原目标字节不变，本次临时文件清理，窗口和标注完整保留；解锁后相同窗口成功保存并结束截图。
- 剪贴板由不同 HWND 持有时，有限重试后提示占用，编辑窗口保留，既有剪贴板序列不变。
- 文件保存与贴图都不改变剪贴板序列；贴图不会额外创建文件，创建成功才关闭截图窗口。
- 未提交的原生中文多行草稿先提交再进入贴图，原桌面 Arc 的弱引用随后失效；真实 IME 候选交互仍归 P4 实机项。
- 输出受阻时销毁截图窗口，任务取消，源图引用在限时内释放；随后输出仍可完成。
- 窗口独立关闭、关闭全部、退出清理、图像引用归还、表面重建、无多余 WM_QUIT 均有真实原生 HWND 回归。

## 自动检查与边界

- 32 项测试通过，包含本轮新增的原生输出失败/重试/取消集成回归；fmt、默认及 all-features 严格 Clippy、默认与 diagnostics Release 构建通过。
- 默认 EXE 为 664,576 字节，检查点：target/p5-pin-checkpoint/x86_64-pc-windows-msvc/release/kiki.exe。原始图片、CSV、环境资料均在忽略的 artifacts 中，不提交用户屏幕内容。
- 多屏/混合 DPI、HDR、真实 IME 候选、更多应用粘贴及 P7 私有工作集指标仍按各阶段记录推进。ZIP 发布属于 P8；本次仅本地源码提交，没有推送或发布。
