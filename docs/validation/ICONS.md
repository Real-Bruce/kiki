# 应用图标接入验证

日期：2026-11-06。范围仅为用户指定的应用图标，不代表 P2–P8 完成。

## 实现

- 原样保留用户 ICO 的九种尺寸，嵌入 EXE 的首个图标组（资源 ID 1）。
- 托盘由系统通用图标改为应用资源，按系统小图标尺寸加载；隐藏宿主及截图窗口类使用应用图标。
- 无新增依赖，现有 manifest 链接方式保留。默认 Release 为 476,672 字节，比上一检查点 368,640 字节增加 108,032 字节。

## 已执行

- 重新阅读项目规范并实际查看四张设计 PNG、用户包内 256 px 图标。
- `cargo fmt --all -- --check` 通过。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过。
- `cargo test --workspace --all-features`：14 项通过；其后仅将资源 ID 指针表达方式改为 `without_provenance`，重新通过严格 Clippy 和 Release 构建。
- 默认及 diagnostics Release 构建通过。沙箱临时目录曾造成 rustc 归档重命名拒绝访问；将本次命令的 TEMP/TMP 指向忽略的 `artifacts/rust-tmp` 后成功，未改变系统设置。
- 真实资源管理器中已见 EXE 的蓝底白猫爪图标；界面证据在忽略的 `artifacts/visual/icons/`。
- 用 System.Drawing 从最终 EXE 提取关联图标并实际查看，与用户图案一致，证据 `artifacts/icon-from-exe.png`。
- 读取 ICO 目录确认九种尺寸；原文件 SHA-256 记录在 `resources/icons/README.md`。

## 验证边界

- 已尝试正常启动及 diagnostics 启动探针，但本轮未取得 startup.csv；退出码 0 不能单独证明托盘就绪，未计为启动性能或托盘视觉验收。
- 托盘实际显示、窗口切换视图中的标识及跨 DPI 显示仍待实机复验。默认截图窗口原本为 ToolWindow，不为显示任务栏图标而改变窗口行为。
- 本次不更改截图工具条的操作图标，不推进其他未完成阶段。
