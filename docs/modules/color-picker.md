# 取色

[模块索引](README.md) · [当前进度](../PROGRESS.md) · [使用说明](../USAGE.md)

从冻结底图或已拼接长图采样颜色。每次进入默认 HEX，按一次 Shift 持续切换 HEX/RGB；单击复制编码后继续取色。透明空隙显示“无像素”，标注、马赛克和界面不参与采样。

## 设计与行为

产品规格第 5 节；视觉规范第 7a 节；F10。

[产品规格](../PRODUCT_SPEC.md) · [视觉规范](../DESIGN_SPEC.md) · [功能方案](../FEATURE_PLAN.md)

- [next/s06-picker-light.png](../design/next/s06-picker-light.png)
- [next/s06-picker-dark.png](../design/next/s06-picker-dark.png)

## 开发入口

| 源码 | 职责 |
| --- | --- |
| [src/platform/windows/color_picker.rs](../../src/platform/windows/color_picker.rs) | 格式、取色状态与浮层 |
| [src/core/image.rs](../../src/core/image.rs) | 底图像素 |
| [src/platform/windows/preview.rs](../../src/platform/windows/preview.rs) | 普通截图采样路由 |
| [src/platform/windows/long_capture.rs](../../src/platform/windows/long_capture.rs) | 长图视口与采样 |
| [src/platform/windows/output.rs](../../src/platform/windows/output.rs) | 剪贴板输出 |

共用构建、原生诊断和资源约束见 [开发说明](../DEVELOPMENT.md)。

## 验证与记录

检查已知 RGB 像素、BGRA 通道、Shift 长按不重复切换、复制失败重试及长图坐标。真实多屏边界和 IME 组合仍按报告分别记录；不将设计图中颜色当作被测输出。

[F10 用例](../ACCEPTANCE_PLAN.md#f10)

- [F10-v0.11.0.md](../validation/color-picker/F10-v0.11.0.md)

报告只代表各自记录的版本和环境，当前放行及缺项见 [PROGRESS.md](../PROGRESS.md)。

## 真实界面

本次附件未包含本模块独立操作或输出样张。历史证据见以上报告；不以设计稿替代实拍。[当前图库](../screenshots/README.md)

相关模块：[普通截图与工具条](capture.md) · [长截图](long-capture.md) · [输出与圆角](output.md)
