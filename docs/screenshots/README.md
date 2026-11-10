# 当前软件截图

采集日期：2026-11-10。版本 0.2.1，源码基线 c490e1d；本轮仅更新文档，没有修改应用代码。

- Windows 11 x64，单屏 1920×1080、125%。
- Rust 1.88.0 Release diagnostics 构建，独立配置位于忽略目录 artifacts/docs-refresh/config。
- 使用 computer-use 的窗口截图；浅深色设置为工具返回的 JPEG 原图，工具条图仅裁剪软件区域并存为 PNG。截图工具可能缩放或压缩画面，不作为原始像素精度证据。
- --accessible-capture 仅使诊断画布可枚举；实际控件、命中和渲染来自应用本身。
- 未使用设计 PNG、模拟 UI 或生成图片替代运行结果。

| 图片 | 内容 |
| --- | --- |
| [浅色设置](settings-light.jpg) | 截图提示、默认颜色、线宽、字号 |
| [深色设置](settings-dark.jpg) | 相同页面的深色主题 |
| [截图标注](capture-dark.png) | 856×366 物理像素选区、矩形标注、主工具条与参数条 |

设计参考仍保存在 [design](../design/README.md)，与产品介绍图分开。
