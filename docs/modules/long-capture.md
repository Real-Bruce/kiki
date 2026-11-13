# 长截图

[模块索引](README.md) · [当前进度](../PROGRESS.md) · [使用说明](../USAGE.md)

在单屏固定区域内手动纵向滚动，匹配并追加新内容，再进入可标注的静态长图编辑。进入时立即丢弃普通截图标注、草稿和撤销/重做记录，失败或取消也不恢复。

## 设计与行为

产品规格第 9a 节；视觉规范第 7a 节；功能方案第 2.4 节与 F11。

[产品规格](../PRODUCT_SPEC.md) · [视觉规范](../DESIGN_SPEC.md) · [功能方案](../FEATURE_PLAN.md)

- [next/s07-long-capture-light.png](../design/next/s07-long-capture-light.png)
- [next/s07-long-capture-dark.png](../design/next/s07-long-capture-dark.png)
- [next/s08-long-edit-light.png](../design/next/s08-long-edit-light.png)
- [next/s08-long-edit-dark.png](../design/next/s08-long-edit-dark.png)

## 开发入口

| 源码 | 职责 |
| --- | --- |
| [src/core/long_image.rs](../../src/core/long_image.rs) | 重叠匹配、追加与容量限制 |
| [src/platform/windows/long_capture.rs](../../src/platform/windows/long_capture.rs) | 采集、预览与静态编辑 |
| [src/platform/windows/capture.rs](../../src/platform/windows/capture.rs) | 捕获资源 |
| [src/platform/windows/capture_stream_tests.rs](../../src/platform/windows/capture_stream_tests.rs) | 原生滚动捕获回归 |
| [src/platform/windows/composition.rs](../../src/platform/windows/composition.rs) | 完整长图合成 |

共用构建、原生诊断和资源约束见 [开发说明](../DEVELOPMENT.md)。

## 验证与记录

检查快速滚动、回滚、重复帧、目标失效和取消后的资源释放；输出必须包含完整长图。高度 32768 px 或原始像素 256 MiB 先达到者停止增长。v1.0.3 报告保留内置浏览器漏段样本，v1.0.4 单独记录远程显示修复，不能合并成“全部已修复”。

[F11 用例](../ACCEPTANCE_PLAN.md#f11)

- [F11-v1.0.0.md](../validation/long-capture/F11-v1.0.0.md)
- [long-capture-v1.0.3.md](../validation/long-capture/long-capture-v1.0.3.md)
- [long-capture-v1.0.4.md](../validation/long-capture/long-capture-v1.0.4.md)

报告只代表各自记录的版本和环境，当前放行及缺项见 [PROGRESS.md](../PROGRESS.md)。

## 真实界面

本次附件未包含本模块独立操作或输出样张。历史证据见以上报告；不以设计稿替代实拍。[当前图库](../screenshots/README.md)

相关模块：[标注与参数](annotation.md) · [取色](color-picker.md) · [输出与圆角](output.md)
