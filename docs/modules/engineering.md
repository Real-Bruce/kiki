# 工程、性能与交付

[模块索引](README.md) · [当前进度](../PROGRESS.md) · [使用说明](../USAGE.md)

Rust 2024 / Rust 1.88.0 / Windows MSVC x64。核心接收通用数据，Win32/COM/GPU 及 unsafe 限于平台层。版本统一来自 VERSION，发布包为 Windows x64 ZIP；构建、诊断及性能结果分别记录。

## 设计与行为

实施计划 B 节定义模块、资源与线程边界；产品规格第 10 节定义性能目标；验收计划 common 组定义测量与放行口径。

[产品规格](../PRODUCT_SPEC.md) · [视觉规范](../DESIGN_SPEC.md) · [功能方案](../FEATURE_PLAN.md)

## 开发入口

| 源码 | 职责 |
| --- | --- |
| [Cargo.toml](../../Cargo.toml) | 依赖功能和 Release 配置 |
| [rust-toolchain.toml](../../rust-toolchain.toml) | 已验证工具链 |
| [build.rs](../../build.rs) | 构建元数据 |
| [VERSION](../../VERSION) | 应用版本 |
| [src/platform/windows/diagnostics.rs](../../src/platform/windows/diagnostics.rs) | 显式诊断入口 |
| [src/platform/windows/performance.rs](../../src/platform/windows/performance.rs) | 性能采样 |
| [scripts/Package-Windows.ps1](../../scripts/Package-Windows.ps1) | ZIP 与校验值 |
| [scripts/Measure-Performance.ps1](../../scripts/Measure-Performance.ps1) | 性能测量 |
| [scripts/Measure-Startup.ps1](../../scripts/Measure-Startup.ps1) | 启动时延 |
| [scripts/Measure-Idle.ps1](../../scripts/Measure-Idle.ps1) | 闲置采样 |

共用构建、原生诊断和资源约束见 [开发说明](../DEVELOPMENT.md)。

## 验证与记录

命令见开发说明；本地原生集成测试可能受热键、桌面权限和沙箱影响，记录原始失败。性能注明 Release/diagnostics、机器、显示器、样本与计时边界，不清空工作集。文档整理不重建 ZIP、不提高应用版本、不代替实机验收。

[COMMON 用例](../ACCEPTANCE_PLAN.md#common)

- [DOCS_REFRESH.md](../validation/engineering/DOCS_REFRESH.md)
- [FEATURE_BASELINE.md](../validation/engineering/FEATURE_BASELINE.md)
- [ICONS.md](../validation/engineering/ICONS.md)
- [P1.md](../validation/engineering/P1.md)
- [P7.md](../validation/engineering/P7.md)
- [P8.md](../validation/engineering/P8.md)
- [STARTUP.md](../validation/engineering/STARTUP.md)
- [docs-organization-2026-11-13.md](../validation/engineering/docs-organization-2026-11-13.md)
- [open-source-audit-2026-11-13.md](../validation/engineering/open-source-audit-2026-11-13.md)
- [release-v1.0.10.md](../validation/engineering/release-v1.0.10.md)

报告只代表各自记录的版本和环境，当前放行及缺项见 [PROGRESS.md](../PROGRESS.md)。

## 真实界面

本次附件未包含本模块独立操作或输出样张。历史证据见以上报告；不以设计稿替代实拍。[当前图库](../screenshots/README.md)

交付流程见 [打包与发布](../RELEASING.md)，依赖原文来源见 [第三方许可](../THIRD_PARTY_LICENSES.md)。

相关模块：[普通截图与工具条](capture.md) · [长截图](long-capture.md) · [输出与圆角](output.md)
