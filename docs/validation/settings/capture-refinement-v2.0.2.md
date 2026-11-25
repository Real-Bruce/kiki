# v2.0.2 截图设置修订验证

2026-11-25，基于 `bad0c32`，按用户四项截图设置反馈修订。保持 Rust 1.88.0／2024、Win32／Direct2D，不增加依赖。提交主题：`fix(settings): refine capture tool rows and parameter labels`。

## 变化

- 首个分组标题由「截图工具条」改为「工具条」。点击实时预览仍可展开和收起选项。
- 工具选项从两列复选框改为单列整行：左侧保留 A 版共享图标和名称，右侧使用与通用页自启一致的 42×22 DIP 开关，行高 42 DIP。整个原生按钮均可命中；悬停、按下和键盘焦点继续使用主题反馈。
- 删除样式调整和保存与圆角中的参数范围说明，保留右侧当前像素读数和原有合法边界、拖动／滚轮／键盘输入。
- 零圆角仅显示「无」，非零继续显示「数值 px」。

## 检查结果

| 检查 | 实测结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 通过，无新增规则豁免；F 盘增量缓存硬链接不支持时由编译器改为复制 |
| `cargo test --workspace --locked -- --test-threads=1` | 89 通过、0 失败、7 按现有标记跳过 |
| `cargo test --workspace --all-features --locked -- --test-threads=1` | 89 通过、0 失败、7 按现有标记跳过 |
| 布局与原生控件回归 | 六页、630／660／790／944／1280 DIP 布局和各工具组合通过；工具行单列、无重叠，展开只移动后续内容；参数显隐、滚动、保存／回滚、销毁通过 |
| 圆角值与输出 | 原生滑杆 End 到 256、Home 到 0，通过像素数／「无」读数检查；原有 PNG／JPEG、透明角／白底、贴图成图解码与像素检查随完整套件通过 |
| 正式便携包 | Release 构建、ZIP 解压清单、EXE 版本与哈希通过；解压版设置启动通过 |

## 真实界面

Windows 11 `10.0.26300.9974`，单屏 200%，MSVC x64 Release diagnostics，独立配置 `artifacts/capture-refinement-v2.0.2-config`。工具按逻辑坐标输出 JPEG，约 949×815，不是原始物理分辨率。

- 检查[浅色参数页](../../screenshots/capture-refinement-v2.0.2/capture-light.jpg)和[深色参数页](../../screenshots/capture-refinement-v2.0.2/capture-dark.jpg)：标题为「工具条」，六项标注参数的范围描述均已移除，当前数值可读。
- 检查[浅色工具列表](../../screenshots/capture-refinement-v2.0.2/tools-light.jpg)和[深色工具列表](../../screenshots/capture-refinement-v2.0.2/tools-dark.jpg)：八项工具顺序、行距、文字与开关对齐、原图标、悬停及开／关状态清楚。实际点击长截图开关，预览立即移除对应图标，独立配置持久化为关闭。
- 检查[浅色圆角](../../screenshots/capture-refinement-v2.0.2/corners-light.jpg)和[深色圆角](../../screenshots/capture-refinement-v2.0.2/corners-dark.jpg)：不再显示范围，零值仅为「无」。实际点击滑杆到 128 px，读数与圆角预览同步改变，按 Home 后回到「无」和直角预览。
- 已启动正式 ZIP 解压后的 `dist/kiki-v2.0.2-windows-x64/kiki.exe --settings`，通过窗口枚举核对路径，并查看截图页。正常读取原有用户参数。未操作用户自启、热键或参数；诊断操作只写独立配置。

本次不改变截图、合成或编码算法，未新增手动图像导出或性能采样。完整实机 DPI、混合显示器、HDR、外部粘贴及 Daily 长截图遗留见[进度页](../../PROGRESS.md)，不将历史性能结果写为本轮通过。仍为升级候选，不创建完成标签、合并 main、推送或发布。

## 交付

| 产物 | SHA-256 |
| --- | --- |
| Release diagnostics EXE | `44d6bf87834e318f934f4fcfe012cee1b258c5ee613f45042960707049dd6615` |
| 普通 Release EXE | `8d69a8111617c1fc5f3229cb5256dbec68de00bdcbec2173fc5d1749f48d58d5` |
| `kiki-v2.0.2-windows-x64.zip` | `f7b255812561db3fab4a01468c01d13a505103435a3f3a5e5f5843048b09d07a` |

ZIP 仅有 `kiki.exe`（1,010,688 字节）、`LICENSE.txt` 和 `README.txt`。EXE 文件／产品版本均为 `2.0.2`，解压路径为 `dist/kiki-v2.0.2-windows-x64`。普通发行版启动证据为 `artifacts/capture-refinement-v2.0.2-unpacked-start.jpg`。
