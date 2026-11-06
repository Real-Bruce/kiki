# P6 交接 / 下一阶段入口

- 仓库 F:/codespace/vibecode/kiki，分支 feat/windows-v1。P6 核心功能与当前单屏 SDR / 125% 验收完成，详情见 validation/P6.md。P5 已完成，P3 暂缓，P2/P4 原环境遗留保留。
- 普通程序已启用配置、后台原子写入、热键事务、六页设置、主题/提示、默认样式、最近目录、显式开机启动、托盘入口、帮助和嵌入许可。默认 Ctrl+Alt+A，可从托盘或 --settings 修改。
- 当前默认及 all-features 各 41 项测试，双配置严格 Clippy、fmt 和 Release 构建通过。诊断 GUI 用 --config-dir artifacts/p6-ui，未覆盖用户真实配置，未修改实际 Run:kiki。
- 下一阶段从 IMPLEMENTATION_PLAN.md 的 P7 继续：完整回归、CPU/私有工作集/GPU/句柄指标、视觉修正。150%/200%、混合 DPI、真实 IME、HDR 以及登录自启需要实机环境；不提前标为通过。
- 不推送远端。构建 TEMP/TMP 使用仓库 artifacts/rust-tmp。截图、诊断产物和个人配置均不提交。
- 真实 UI 使用 computer-use 技能的 node_repl / @oai/sky；恢复时重新枚举并唯一定位窗口，动作后刷新。不要绕过工具的权限或失败，也不要创建额外代理。
- 本轮最终验证程序为 artifacts/p6-final/kiki-reviewed.exe；清理测试进程前核对路径及 --config-dir，不能终止用户应用或其他阶段遗留进程。
