# v2.0.3 滑杆端点及圆角排版验证

2026-11-25，基于 `55b5bb4` 按用户标记截图修订。Rust 1.88.0／2024、Win32／Direct2D；不增加依赖。提交主题：`fix(settings): prevent clipped slider rings and align corner preview`。

## 修改与缺陷原因

- 去除「用于新标注。尺寸按截图的实际像素计算。」、可见的「圆角半径」及预览旁的格式／短边一半说明。保留原生圆角控件的无障碍名称和实际参数边界。
- 原滑杆轨道端点距离控件边缘仅 6 DIP，而圆环半径为 7 DIP，最小／最大值会越界裁切。全部七个设置滑杆现在共用绘制方法，端点留白为 9 DIP；鼠标映射复用同一端点，避免显示位置和点击值偏移。
- 圆角预览 108×72 DIP 在左，间隔 24 DIP 后为右侧滑杆，滑杆中心与预览中心相同；当前读数靠右。窄布局仍保持并排。圆角为零继续仅显示「无」。

## 自动验证

| 检查 | 最终结果 |
| --- | --- |
| 格式及锁定依赖、全目标／全特性的严格 Clippy | 通过；无新增规则豁免，F 盘增量缓存硬链接不支持时编译器改为复制 |
| 默认特性串行测试 | 90 通过、0 失败、7 按现有标记跳过 |
| 全特性串行测试 | 90 通过、0 失败、7 按现有标记跳过 |
| 新增原生像素回归 | 实际 Direct2D 绘制到原生 DIB；100%／125%／150%／200%，浅深两色、100／280 DIP 宽、最小／中间／最大位置，共 48 组；两侧边缘留白且圆环两侧像素存在 |
| 布局及真实控件回归 | 630／660／790／944／1280 DIP 六页布局通过；补验圆角预览在滑杆左侧且垂直中心相等，展开工具只移动后续内容；滚动、键盘调参、保存回滚及销毁通过 |
| 既有成图检查 | PNG／JPEG、透明圆角／白底、贴图成图的解码与像素检查随完整套件通过 |
| Release／便携包 | 正常及 diagnostics Release 构建、ZIP 解压清单、版本和哈希通过 |

缺陷复现：新增像素用例先沿用旧 6 DIP 边距运行，端点边缘检查失败；改为 9 DIP 后通过。测试夹具初次编译存在浮点类型未明确的问题，补充 `f32` 后完成上述旧边距失败／新边距通过对照。对圆角预览右侧留白作最后调整后，已重新运行全部格式、Clippy、两套测试及 Release 构建。

## 真实界面与操作

Windows 11 `10.0.26300.9974`，单屏 200%，MSVC x64 Release diagnostics，独立配置位于 `artifacts/slider-corner-v2.0.3-config`。JPEG 是工具按逻辑坐标输出的约 949×815 图像。真实 GUI 仅本机 200%；其余比例为原生绘制／布局测试，不冒充整窗实机验证。

- [浅色参数](../../screenshots/slider-corner-v2.0.3/capture-light.jpg)和[深色参数](../../screenshots/slider-corner-v2.0.3/capture-dark.jpg)：六项标注尺寸设在各自合法最小值，圆环均完整。不是把字号、编号等非法改为零。
- [浅色圆角](../../screenshots/slider-corner-v2.0.3/corners-light.jpg)和[深色圆角](../../screenshots/slider-corner-v2.0.3/corners-dark.jpg)：标记文字均不存在；预览和滑杆并排居中，零读数为「无」。
- 实际点击新滑杆到 140 px，预览同步更新；按 End 到 256 px，检查[最大值圆环](../../screenshots/slider-corner-v2.0.3/corners-max-light.jpg)完整，再按 Home 回到「无」与直角预览。只修改独立配置，未改用户参数或启动项。
- 正式 ZIP 解压路径 `dist/kiki-v2.0.3-windows-x64`；已停止诊断实例，启动普通发行版并通过窗口路径确认，查看新版截图页。启动证据保存在 `artifacts/slider-corner-v2.0.3-unpacked-start.jpg`。

本次只调整设置绘制和布局，未改变截图及圆角成图算法，未新增手动图像导出或性能采样。多屏／混合 DPI、HDR、外部粘贴和 Daily 长截图等遗留继续见[进度页](../../PROGRESS.md)。保持候选状态，未合并 main、创建完成标签、推送或发布。

## 最终产物

| 产物 | SHA-256 |
| --- | --- |
| diagnostics Release EXE | `72b500daf6598bbea5c6b5b14d5b2c5fcf96174fca91d9af15412866e9d251b5` |
| 普通 Release EXE | `9732bd9a43f6d75b4f168dfb1dbb4ab9e93be0faf3d38b494dca74d4dc7c0cf6` |
| `kiki-v2.0.3-windows-x64.zip` | `035ba05f2351259175da04d802304fa9ee6b182548333fbc60218ecc086eb5c9` |

ZIP 仅包含 `kiki.exe`、`LICENSE.txt`、`README.txt`；EXE 1,010,688 字节，文件／产品版本均为 `2.0.3`。说明沿用现有滑杆操作，源码、使用与设计说明、实拍及本报告一并提交。
