# 文档整理与构建复核

日期：2026-11-10。环境：Windows 11 专业版 10.0.26300，单屏 1920×1080、125%。源码基线 c490e1d，版本 0.2.1。文档维护分支 codex/docs-refresh；未修改应用代码、依赖或版本。

## 文档处理

README 精简为常见项目入口结构，详细使用、开发、发布说明迁入 docs。CHANGELOG、图标来源及第三方许可来源迁入 docs；运行时嵌入的许可原文仍保留 resources。删除已被当前状态取代的 P6_HANDOFF.md。进度首页改为现状与待办，阶段历史保存在 IMPLEMENTATION_HISTORY.md。历史验证报告与设计图保留。

## 实际执行

- cargo fmt --all -- --check：通过。
- cargo test --workspace --locked：44 项通过。
- cargo test --workspace --all-features --locked：44 项通过。
- 默认配置严格 Clippy：通过。
- cargo clippy --workspace --all-targets --all-features --locked -- -D warnings：通过；Cargo 报增量缓存硬链接回退为复制的环境警告。
- Package-Windows.ps1：普通 Release 构建与打包通过。
- 独立 target/docs-diagnostics 的 Release diagnostics 构建通过。
- 初次沙箱运行两套测试均为 37 通过、7 失败，涉及原生热键、注册表与原子文件操作权限；正常桌面权限下复测两套均 44 通过。未修改测试或屏蔽失败。
- 初次 Release 在沙箱 TEMP 归档重命名时拒绝访问；将 TEMP/TMP 指向仓库内 artifacts/build-tmp 后构建成功。

## 便携包

- ZIP：dist/kiki-v0.2.1-windows-x64.zip，526,923 字节。
- SHA-256：df620d811ac8f5b4c69fc6cb30dc49693ba6cc25c16c20bb2e49e518a9355718。
- EXE：1,022,976 字节；SHA-256：62e66a72229a445e7733634aada326d88ee77dab12bc04c55361f0d807c7b4bf。
- 解压到 artifacts/docs-refresh/便携包 测试，16 个文件，EXE 与默认 Release 哈希一致。
- 首次启动转发到原驻留旧版，未误记为新包通过。随后暂时结束旧进程，独立启动新包，枚举确认进程路径与设置窗口。普通版最终可见画面受到其他截图遮罩干扰，不据此宣称完整 GUI 验收通过。
- 测试后结束本轮程序，恢复原路径的旧版；用户日常配置未改主题，诊断版主题变更使用独立配置。

## 界面与输出

当时实际检查诊断版浅色/深色截图设置、主题切换、冻结截图、856×366 选区、红色矩形和深色参数条，并裁掉聊天及其他桌面内容后纳入项目介绍。后因图片清晰度不足，介绍图已替换为用户提供的 PNG 原图，来源见 [当前界面](../../screenshots/archive/2026-11-10/README.md)；新图片不作为本轮诊断检查的证据。

Ctrl+S 打开原生保存对话框并成功设置目标文件名；自动化保存按钮命中被工具拒绝，重试键盘路径受到其他截图软件影响，未生成目标 PNG。因此本轮 GUI 保存不标通过。

另运行现有 --annotation-probe，生成 fixture-annotated.png 和 desktop-annotated.png。实际打开测试底图成图，确认红色矩形、实心箭头、笔画和旋转多行中文可见。该证据是共享成图探针，不冒充 GUI 保存通过。输出保留 artifacts/docs-refresh/composition，不入库。

## 边界

本轮未重新采集性能，没有新性能达标结论。多屏、混合 DPI、HDR、完整 IME、独立干净系统及云端发布继续按 [当前进度](../../PROGRESS.md) 跟踪。
