# 文档索引

使用者从[中文介绍](README.zh-CN.md)或[使用说明](USAGE.md)开始；维护者从对应功能模块进入设计、源码与验证记录。当前版本、放行情况和下一步统一见 [PROGRESS.md](PROGRESS.md)，旧阶段记录见[实施历史](IMPLEMENTATION_HISTORY.md)。

## 按功能模块

| 模块 | 内容与入口 | 对应阶段 |
| --- | --- | --- |
| [普通截图与工具条](modules/capture.md) | 冻结捕获、窗口/屏幕选区、跨屏坐标、工具显示与布局 | P2、P3、F01、F07、F12 |
| [标注与参数](modules/annotation.md) | 矩形、箭头、画笔、文字、马赛克、编号与撤销 | P4、F01–F03、F08、F09 |
| [取色](modules/color-picker.md) | 底图采样、HEX/RGB、颜色编码复制 | F10 |
| [长截图](modules/long-capture.md) | 单屏纵向采集、拼接、预览、长图编辑及远程显示 | F11、v1.0.3–v1.0.4 |
| [输出与圆角](modules/output.md) | 合成、剪贴板、PNG/JPEG、圆角与失败恢复 | P5、F04、F05 |
| [置顶贴图](modules/pinned.md) | 多图、移动缩放、复制保存与生命周期 | P5 |
| [设置与运行入口](modules/settings.md) | 六页设置、热键、托盘、主题、持久化及恢复默认 | P6、F06、v1.0.5–v1.0.9 |
| [视觉资源](modules/visuals.md) | 松绿主题、线性图标、设计源与真实图库 | 视觉基线与图标维护 |
| [工程、性能与交付](modules/engineering.md) | 构建、诊断、性能口径、版本及 ZIP 发布 | P1、P7、P8 |

各模块只整理阅读顺序、职责和证据入口，不复制整份规格，也不另建当前完成状态表。[全部验证报告](validation/README.md)按相同模块归档；跨模块报告只保留一份原件，通过链接共享。

## 规格与计划

| 文档 | 用途 |
| --- | --- |
| [产品规格](PRODUCT_SPEC.md) | 产品边界、输出语义与性能目标 |
| [视觉与交互规范](DESIGN_SPEC.md) | 已批准视觉方向及最新交互约束 |
| [功能开发方案](FEATURE_PLAN.md) | F01–F12 的行为、默认值、接入和配置迁移 |
| [实施计划](IMPLEMENTATION_PLAN.md) | P0–P8 原始实施基线与工程原则 |
| [验收计划](ACCEPTANCE_PLAN.md) | 用例定义、环境矩阵和放行条件 |
| [验收模板](validation/FEATURE_ACCEPTANCE_TEMPLATE.md) | 新报告的环境、步骤、证据与失败记录格式 |

规划时的版本和“待执行”初值不代表当前状态。报告保留当时条件和失败样本，当前结论以进度页及相关版本报告为准。

## 使用、开发与资料

- [使用说明](USAGE.md) · [包内快速说明](QUICK_START.txt) · [版本变更](CHANGELOG.md)
- [开发与诊断](DEVELOPMENT.md) · [打包与发布](RELEASING.md) · [代理约束](../AGENTS.md)
- [已批准设计](design/README.md) · [F01–F12 草图](design/next/README.md) · [当前真实截图](screenshots/README.md)
- [图标来源](ICONS.md) · [第三方许可](THIRD_PARTY_LICENSES.md)

## 目录维护

- `modules/`：按功能串联设计、开发和验证的入口。
- `design/`：批准的原始设计与可编辑草图；01、02、04、05 和 `next/` 保持稳定路径，03 为历史稿。
- `screenshots/capture/`、`screenshots/settings/`：当前公开真实界面；`screenshots/archive/` 保存被替换的历史原件。
- `validation/<模块>/`：版本报告与同版截图。新增报告写明实际版本、构建、环境及证据；模板留在 `validation/`。
- 根级规格、计划、进度和通用指南保持稳定路径，便于 AGENTS.md、打包脚本与外部引用继续使用。

新资料归入对应模块，并同时更新模块入口和相应索引；移动文件后检查相对链接和图片引用。生成物、日志及个人配置继续放在忽略目录，不纳入公开文档。
