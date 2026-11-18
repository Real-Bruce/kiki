# v1.0.17 便携包许可合并验证

2026-11-18，Windows x64（10.0.26300.0），PowerShell 7.6.9，Rust 1.88.0 MSVC。被测源码以 `c5ebdb2` 为基线，应用改动为 `VERSION` 递增，打包改动见本报告对应提交。

- 执行 `./scripts/Package-Windows.ps1 -Tag v1.0.17`，按 `Cargo.lock` 构建 `x86_64-pc-windows-msvc` Release。构建成功，报告了一项已有的 `pinned::open` 未使用警告。
- ZIP 恰含三个根目录文件 `kiki.exe`、`README.txt`、`LICENSE.txt`。解压 EXE 与刚构建的二进制逐字节一致，产品版本为 1.0.17。解压的 README 与快速说明源文件一致，并指向 `LICENSE.txt`。
- 合并许可可按 UTF-8 读取。核对了项目 MIT、仓库第三方许可汇总及工具链 12 份许可原文，除行尾格式外均保留。Rust 标准库版权清单转为纯文本后，核对了 62 处原文块及清单中的版权和许可字段。
- 沙箱启动未观察到托盘窗口。随后在正常桌面检查时发现已有 kiki 实例，解压包独立启动待补验。本次未采集性能数据。

`dist/kiki-v1.0.17-windows-x64.zip` 为 566,566 字节。ZIP 的 SHA-256 为 `f290ee930fb3688a533256b052829ef32d6c9ac826571c1dc861dc85f11dac8e`，与外部校验文件一致。EXE 的 SHA-256 为 `853fc9aa492cb1e8ab2cedf469079488ef859ed95fcc897a6b6a8f56bd9854c5`。
