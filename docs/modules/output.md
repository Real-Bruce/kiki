# 输出与圆角

[模块索引](README.md) · [当前进度](../PROGRESS.md) · [使用说明](../USAGE.md)

复制、文件保存、生成贴图是独立动作，使用相同的干净成图语义。PNG 保留透明，JPEG 以白底合成透明区域并使用 90% 品质；圆角同时裁切底图与标注。取消或输出失败保留编辑内容。

## 设计与行为

产品规格第 6 节；功能方案第 2.5 节；F04、F05。

[产品规格](../PRODUCT_SPEC.md) · [视觉规范](../DESIGN_SPEC.md) · [功能方案](../FEATURE_PLAN.md)

- [next/s03-settings-light.png](../design/next/s03-settings-light.png)
- [next/s03-settings-dark.png](../design/next/s03-settings-dark.png)

## 开发入口

| 源码 | 职责 |
| --- | --- |
| [src/core/image.rs](../../src/core/image.rs) | 图像尺寸与像素 |
| [src/platform/windows/composition.rs](../../src/platform/windows/composition.rs) | 标注、马赛克与圆角合成 |
| [src/platform/windows/image_encode.rs](../../src/platform/windows/image_encode.rs) | WIC PNG/JPEG 编码 |
| [src/platform/windows/output.rs](../../src/platform/windows/output.rs) | 剪贴板、保存对话框与后台输出 |

共用构建、原生诊断和资源约束见 [开发说明](../DEVELOPMENT.md)。

## 验证与记录

检查实际文件签名、扩展名、尺寸、PNG alpha、JPEG 白底和剪贴板实际粘贴；覆盖文件占用、不可写目录、取消与重试。当前图库的截图设置上部不展示保存与圆角区域，不能用该图替代输出证据。

[F04 用例](../ACCEPTANCE_PLAN.md#f04) · [F05 用例](../ACCEPTANCE_PLAN.md#f05)

- [F04-v0.6.0.md](../validation/output/F04-v0.6.0.md)
- [F05-v0.7.0.md](../validation/output/F05-v0.7.0.md)
- [P5.md](../validation/output/P5.md)
- [P5_COMPLETION.md](../validation/output/P5_COMPLETION.md)

报告只代表各自记录的版本和环境，当前放行及缺项见 [PROGRESS.md](../PROGRESS.md)。

## 真实界面

本次附件未包含本模块独立操作或输出样张。历史证据见以上报告；不以设计稿替代实拍。[当前图库](../screenshots/README.md)

相关模块：[标注与参数](annotation.md) · [置顶贴图](pinned.md) · [长截图](long-capture.md)
