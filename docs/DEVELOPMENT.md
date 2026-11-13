# 开发与验证

## 环境与构建

Windows 11 x64、MSVC Build Tools、Windows SDK、PowerShell 7。Rust 1.88.0 和 x86_64-pc-windows-msvc 目标由仓库配置固定；使用 Cargo.lock，不随意升级依赖。

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo build --release --locked
```

程序位于 target/x86_64-pc-windows-msvc/release/kiki.exe。打包见 [发布说明](RELEASING.md)，交互说明见 [使用说明](USAGE.md)。

原生集成测试访问热键、注册表、窗口和原子文件操作，受限沙箱可能拒绝这些调用。区分环境失败与产品失败，在有权限的桌面会话复测并保留原结果。若受限 TEMP 导致 Rust 归档重命名失败，可将当前构建进程的 TEMP/TMP 指向仓库内忽略的 artifacts/build-tmp。

按功能查找设计、源码和验证入口见 [模块索引](modules/README.md)。

## 模块与约束

| 路径 | 职责 |
| --- | --- |
| src/core | 平台无关的坐标、图像、选区、标注、配置及会话模型 |
| src/platform/windows | 捕获、窗口、绘制、输入、输出、设置与资源管理 |
| resources | 编译和分发使用的图标、清单、许可原文 |
| scripts | 打包、区域验证和性能采样；应用版本统一来自根目录 VERSION |
| docs | 使用、开发、规格、进度及验证说明 |

代码与 Git 规则见 [AGENTS.md](../AGENTS.md)。界面变化必须运行实际程序检查，并核对最终成图，不能只跑单元测试。

## 普通启动参数

- --settings：打开设置；已有实例时转发请求。
- --capture：开始截图。
- --capture-screen：选择鼠标所在屏幕。
- --theme=light / --theme=dark：指定启动主题。

应用保持单实例；测试前确认运行进程的实际路径，避免把旧版窗口误判为新构建。

## 诊断构建

建议使用独立输出目录，避免覆盖普通发行程序：

```powershell
cargo build --release --locked --features diagnostics --target-dir target/diagnostics
./target/diagnostics/x86_64-pc-windows-msvc/release/kiki.exe --settings --config-dir artifacts/dev-config --accessible-capture
```

--config-dir 隔离应用配置，但不隔离单实例锁、系统热键或系统开机启动项。不要在测试时修改用户实际自启设置。--accessible-capture 使诊断画布可被自动化枚举，普通构建保留 ToolWindow 样式。

| 诊断参数 | 用途 |
| --- | --- |
| --probe <目录> | 真实桌面和中心裁剪 PNG、捕获耗时 |
| --stress <目录> | 100 次捕获释放与资源采样 |
| --cancel-stress <目录> | 100 次取消及同进程恢复 |
| --annotation-probe <目录> | 测试底图及桌面裁剪的标注合成 |
| --trace-window-targets <文件> | 窗口类别、样式和边界；不记录标题或输入 |
| --pin-demo / --pin-4k-demo | 两张测试图 / 四张可见 4K 测试图 |
| --pin-stress <目录> | 100 轮四张独立 4K 图资源采样 |
| --pin-sample <目录> | 可见贴图有界资源采样 |
| --tray-menu | 托盘菜单预览，选择后退出 |

仅显式启用探针。真实桌面图可能包含个人信息，原始结果放忽略的 artifacts，不能直接作为公开测试数据。诊断结果不等同于完整用户流程通过。

## 性能与验收

scripts/Measure-Startup.ps1 采集 30 次进程启动到热键/托盘就绪；Measure-Performance.ps1 采集首次截图、重复生命周期与闲置 CPU；Measure-Idle.ps1 读取指定进程的外部闲置指标。

每次报告说明源码版本、构建模式、机器、显示器和采样方法。应用内时延不能混称物理键盘全链路时延，不通过清空工作集优化指标。

当前待验项见 [PROGRESS.md](PROGRESS.md)，环境矩阵见 [实施清单](IMPLEMENTATION_PLAN.md)，历史性能证据见 [P7](validation/engineering/P7.md)，历史文档与构建复核见 [DOCS_REFRESH](validation/engineering/DOCS_REFRESH.md)。
