# 当前进度

更新时间：2026-11-13。当前源码 **v1.0.9**（[VERSION](../VERSION)）。[按模块阅读](README.md) · [实施历史](IMPLEMENTATION_HISTORY.md) · [验证记录](validation/README.md)

## 当前状态与下一步

F01–F12 已实现并按下表记录放行；已实现能力不因环境待验而重新搭建。最近应用代码提交为 `e9547be`，Windows x64 ZIP 及实际验证范围见 [v1.0.9 设置验收](validation/settings/settings-usability-v1.0.9.md)。最近维护为开源前检查、MIT 许可与打包说明，应用版本、既有二进制和版本标签保持不变。

1. 继续定位 Codex 内置浏览器 Daily 页面长截图漏段，保留 [v1.0.3 失败样本](validation/long-capture/long-capture-v1.0.3.md)；v1.0.4 的远程显示修复不代表漏段已修复。
2. 补验 v1.0.9 手动框选后的复制粘贴、保存及贴图链路；本次用户截图不替代这些验收。
3. 按下方环境遗留补齐实机矩阵；维护者指定任务优先于这些待验项。

## 最近维护：开源前检查与 MIT

- 检查当前受控文件、103 个历史提交及当前／历史图片，未发现入库密钥或明显工作资料；发现 Git 作者／标签邮箱、本机路径和测试环境信息。被忽略的 IDE、日志和浏览器测试资料含本地信息，不能直接随整个目录分发。详情与范围限制见 [检查报告](validation/engineering/open-source-audit-2026-11-13.md)，本次未改写历史或删除旧证据。
- 增加 MIT 协议及 Cargo 许可声明，更新双语 README、快速说明和打包脚本，未来 ZIP 将包含 `LICENSE`。增加 `.idea` 忽略规则，保留本地 IDE 文件。
- 已删除 14 个合入 `main` 且未被工作树占用的旧分支，保留 `main` 和当前文档工作分支；标签及远端引用未变。
- Cargo 元数据与隔离打包烟雾检查通过；打包检查复用已有 EXE，未重新构建、运行 GUI 或替换既有 ZIP。应用版本仍为 v1.0.9。本地提交主题：`chore: prepare MIT licensing and record open-source audit`。
- 开源前仍需维护者决定是否接受历史中的个人邮箱和路径公开；仅修改当前文件不能消除历史元数据。

## README 截图展示

- 中英文 README 均直接展示全部六张界面图：顶部截图工具条，以及通用、快捷键、截图、贴图、外观五个面板。移除“查看”链接表格和截图来源、日期、配置解释等过程性介绍，使用功能标题与简短说明。
- 图片引用与本地链接检查见 [文档检查补充](validation/engineering/docs-organization-2026-11-13.md#readme-截图展示修订)。仅更新 Markdown，版本保持 v1.0.9；本地提交主题：`docs: display all screenshots in bilingual readmes`。

## 文档模块整理与双语介绍

- 设计、开发和验证资料通过九个功能/工程模块关联；验收报告及其图片移入模块子目录，历史测试结论保留。阶段流水移入实施历史，当前页保留状态与缺项。
- README 改为英文主版，中文介绍位于 `../README.zh-CN.md`；两版均链接最新界面和五个设置面板。收录 2026-11-13 用户提供的六张原始 PNG，旧三张公开截图归档并注明历史来源。
- 修正文档中把规划时版本写成当前版本、遗漏撤销按钮、已删除版本同步脚本等过时表述。默认热键仍是 Ctrl+Alt+A，新四工具默认关闭。
- 验证与来源见 [本次文档检查](validation/engineering/docs-organization-2026-11-13.md)。本地提交主题：`docs: organize module guides and refresh bilingual readme`。未运行新一轮应用或性能验收。

## v1.0.9 设置界面整理

- 通用页补齐从框选到复制粘贴、保存和贴图的操作提示。截图页先显示工具开关和预览，数值范围紧贴对应参数，恢复截图默认设置移至页尾。快捷键框用边框和焦点状态提示可编辑。
- 单屏 125% 下查看了浅深设置窗口，实测工具开关、圆角滑杆、取消恢复及配置重启读取。默认与全特性测试各通过 82 项、跳过 6 项，Windows x64 ZIP 已解压核验。桌面操作中止后，手动截图输出链路待补。 [验收记录](validation/settings/settings-usability-v1.0.9.md)
- 本地提交主题为 `fix(settings): simplify layout and first-use guidance`。其他 DPI、多屏和 HDR 继续按既有记录补验。

## 已确认功能状态

用户于 2026-11-10 确认功能方案及验收文档。此表为唯一当前功能状态入口；各版本报告保存当时证据，不建立第二份竞争状态表。

[开发说明](FEATURE_PLAN.md) · [验收计划](ACCEPTANCE_PLAN.md) · [模板](validation/FEATURE_ACCEPTANCE_TEMPLATE.md)

| ID | 功能／目标版本 | 前置阶段 | 草图 | 状态 | 验收报告 | 实现提交／标签 |
| --- | --- | --- | --- | --- | --- | --- |
| F01 | 直接编辑与截图边缘 / v0.3.0 | v0.2.2 | S01 | 已按用户确认本地放行，环境回归待补 | [F01](validation/capture/F01-v0.3.0.md) | `840fdd4`／`v0.3.0` |
| F02 | 独立线宽与滚轮 / v0.4.0 | F01 | S02、S03 | 随 F03 合入主分支，滚轮和双向同步人工通过，环境回归待补 | [F02](validation/annotation/F02-v0.4.0.md) | `6fe4e7c`／独立标签待验收 |
| F03 | 连续字号 / v0.5.0 | F02 | S01–S03 | 已按用户确认合入并本地放行，实机矩阵待补 | [F03](validation/annotation/F03-v0.5.0.md) | `8773b38`／`v0.5.0` |
| F04 | 截图圆角 / v0.6.1 | F03 | S03 | 按用户要求本地放行，首次进入布局原生回归通过，实机矩阵待补 | [F04](validation/output/F04-v0.6.0.md) | `32aa569`／`v0.6.1` |
| F05 | PNG/JPEG / v0.7.0 | F04 | S03 | 按用户要求本地放行，浅深模式保存人工通过，贴图与环境矩阵待补 | [F05](validation/output/F05-v0.7.0.md) | `c0d87c9`／`v0.7.0` |
| F06 | 恢复截图默认 / v0.7.1 | F05 | S03 | 已完成，浅深界面与原生回归通过，补验项见报告 | [F06](validation/settings/F06-v0.7.1.md) | `900c4ba`／`v0.7.1` |
| F07 | 工具显示开关 / v0.8.0 | F06 | S03、S09 | 已完成，浅深界面、重启、输出及原生回归通过，补验项见报告 | [F07](validation/capture/F07-v0.8.0.md) | `2ffef72`／`v0.8.0` |
| F07 修订 | 工具条实时预览 / v0.8.1 | F07 | S03、S09 | 按用户要求本地放行，浅深主题与窄设置窗口实测通过 | [修订验收](validation/capture/F07-v0.8.1.md) | `c9fc767`／`v0.8.1` |
| F08 | 马赛克 / v0.9.0 | F07 | S02–S04 | 按用户要求合入主分支并标记本地版本，跨环境及百轮资源待验 | [F08](validation/annotation/F08-v0.9.0.md) | `faff207`、`989e9cd`／`v0.9.0` |
| F09 | 顺序标识 / v0.10.0 | F08 | S05 | 按用户要求本地放行。编号历史、参数和输出自动通过，实机多 DPI、整进程重启及外部应用粘贴显示待补 | [F09](validation/annotation/F09-v0.10.0.md) | `0300e86`／`v0.10.0` |
| F10 | 取色 / v0.11.0 | F09 | S06 | 按用户要求本地放行。Shift、剪贴板往返及模拟采样边界自动通过，真实双屏和中文 IME 候选待补 | [F10](validation/color-picker/F10-v0.11.0.md) | `ae75019`／`v0.11.0` |
| F11 | 长截图及长图编辑 / v1.0.0 | F10 | S07、S08 | 已按用户要求合入主分支并标记本地版本，实体双屏和混合 DPI 后续补验 | [F11](validation/long-capture/F11-v1.0.0.md) | `31a748e`／`v1.0.0` |
| F12 | 最终工具条精简 / v1.0.2 | F11 | S09 | 已按用户要求本地放行，实机遗留项见报告 | [F12](validation/capture/F12-v1.0.2.md) | `a5fdced`／`v1.0.2` |

## 既有阶段状态

| 阶段 | 当前情况 | 证据 |
| --- | --- | --- |
| P0/P1 规划与工程 | 完成，Rust 1.88.0 / 2024 / MSVC x64 | [P1](validation/engineering/P1.md) |
| P2 捕获 | 单屏 SDR 通过；多屏实机待验，HDR 映射未完成 | [P2](validation/capture/P2.md) |
| P3 选区与工具条 | 已实现，后续主题与图标修订已实测；完整边界/DPI 矩阵待验 | [界面记录](validation/capture/UI_POLISH.md) |
| P4 标注 | 矩形、箭头、画笔、透明文字、旋转和撤销已实现；完整 IME 矩阵待验 | [文字记录](validation/annotation/P4_TEXT.md)、[界面记录](validation/capture/UI_POLISH.md) |
| P5 输出与贴图 | 单屏 SDR 验收完成，失败恢复与 Word 粘贴有记录 | [P5 完成记录](validation/output/P5_COMPLETION.md) |
| P6 设置与主题 | 六页设置、暖灰松绿主题、托盘与首次使用已实现 | [P6](validation/settings/P6.md) |
| P7 性能与完整验收 | 本机回归完成，200% 时延波动及跨环境缺项保留 | [P7](validation/engineering/P7.md) |
| P8 交付 | 本地便携包通过既有验收；独立干净系统待验 | [P8](validation/engineering/P8.md) |

## 既有环境与功能遗留

1. 真实双屏、混合 DPI、负坐标/竖屏、显示器间隙、热插拔及锁屏仍需补测；多屏暂缓不等于删除支持。
2. HDR → SDR 色彩路径尚未完成，现有 SDR 结果不代表 HDR 支持。
3. 不同环境下的中文 IME、重编取消、系统主题广播和登录自启仍待验证；各功能特有缺项保留在对应报告。
4. 200% 后续截图时延波动仍待复核；性能口径与未达标样本保留在 [P7](validation/engineering/P7.md)。
5. 独立无开发工具的 Windows 11 x64 系统仍需验收 ZIP。
6. 最新用户截图仅含浅色工具条和五个设置页；最新版深色截图、关于页及截图页下方“保存与圆角”区域尚未纳入公开图库。

历史修改与旧版验证见 [实施历史](IMPLEMENTATION_HISTORY.md)，用户可见变化见 [版本变更](CHANGELOG.md)。
