# 打包与发布

在 Windows x64 上使用 PowerShell 7，在仓库根目录运行 `./scripts/Package-Windows.ps1`。脚本按锁文件构建 Release，将 EXE、快速使用说明和许可证打成 ZIP，并在 `dist` 下生成 ZIP 及 SHA-256 校验文件。

应用版本只在根目录的 `VERSION` 中填写，格式为 `X.Y.Z`。构建会将它写入「关于」页、EXE 版本资源和 Windows 清单，打包脚本用它校验标签并命名 ZIP。Cargo 为本地应用包自动使用 `0.0.0`，锁文件照常提交。 [版本文件](../VERSION)

最小修订递增第三位，小优化或常规新增递增第二位，大优化递增第一位，递增较高位时较低位归零。仅规划文档不升级应用版本、不创建功能完成标签。 [开发说明](FEATURE_PLAN.md) · [验收计划](ACCEPTANCE_PLAN.md)

在 `VERSION` 写入候选版本后，构建并验收候选 EXE 和解压包。报告记录被测源码及 EXE、ZIP 的 SHA-256。验收后修改代码或资源时，重新构建并复测相关项。只补充报告的提交需明确被测二进制来源。

```powershell
$tag = 'v' + (Get-Content -LiteralPath VERSION -Raw).Trim()
./scripts/Package-Windows.ps1 -Tag $tag
git diff --check
```

验收通过后，暂存本功能的代码、`VERSION`、进度和报告，检查 `git diff --cached` 并提交。在创建带说明标签时填写面向用户的更新介绍。可以运行 `git tag -a $tag` 在编辑器中填写，也可以先把 Markdown 写到本地 `artifacts/release-notes.md`，再执行下面的命令。文件使用 UTF-8，`--cleanup=whitespace` 保留 Markdown 标题。

```powershell
git tag -a $tag --cleanup=whitespace -F ./artifacts/release-notes.md
git tag --list $tag --format='%(contents)'
git status --short --branch
```

确认公开发布后，推送包含发布工作流的提交及新建的 `vX.Y.Z` 标签。工作流从标签所指向的提交运行，核对标签与 `VERSION`，构建 Windows x64 便携包，并将标签说明写入同名 GitHub Release 正文。Release 同时附带 ZIP 和 `.zip.sha256`。仓库需要启用 GitHub Actions，并允许工作流使用 `GITHUB_TOKEN` 写入 Release。 [发布工作流](../.github/workflows/release.yml)

```powershell
git push origin "refs/tags/$tag"
```
