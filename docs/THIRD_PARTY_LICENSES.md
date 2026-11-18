# 第三方许可来源

项目自身采用 [MIT 开源协议](../LICENSE)，使用 [OSI 的 MIT 标准文本](https://opensource.org/license/mit)。第三方组件仍适用各自的许可，项目的 MIT 声明不替换以下许可原文。

[许可原文](../resources/THIRD_PARTY_LICENSES.txt) 汇总 Cargo.lock 中所有第三方包发行内容的 LICENSE / COPYING 文件（含小写名称），保留原文。范围包含 windows 系列、windows-numerics，以及构建时使用的 proc-macro2、quote、syn、unicode-ident；对应版本列在正文分隔标题中。

便携 ZIP 根目录的 `LICENSE.txt` 合并了项目 MIT 原文、第三方依赖许可原文、从 Rust 工具链 `COPYRIGHT-library.html` 转为纯文本的标准库版权声明，以及工具链附带的许可文本。更新 `Cargo.lock` 时须同步核对依赖许可文件。升级工具链时须核对标准库版权清单转换结果。
