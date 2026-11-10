# 打包与发布

在 Windows x64 上使用 PowerShell 7，在仓库根目录运行 `./scripts/Package-Windows.ps1`。脚本按锁文件构建 Release，将 EXE、快速使用说明和许可证打成 ZIP，并在 `dist` 下生成 ZIP 及 SHA-256 校验文件。

根目录的 [VERSION](../VERSION) 是发布版本来源，格式为 `X.Y.Z`。最小修订递增第三位，小优化或常规新增递增第二位，大优化递增第一位，递增较高位时较低位归零。F01–F12 目标见 [开发说明](FEATURE_PLAN.md)，完成门槛见 [验收计划](ACCEPTANCE_PLAN.md)。仅规划文档不升级应用版本、不创建功能完成标签。

每项功能先实现并同步候选目标版本，再构建、验收和检查解压包，报告记录被测源码、EXE/ZIP SHA-256。代码或资源在验收后又变化时，重新构建并复测相关项；不能验收旧版本后二次改版本就给新包打标签。只补充报告的提交需明确被测二进制来源。

以下以未来 F01 目标 `v0.3.0` 演示本地流程，不表示 F01 已完成，也不要求在文档阶段执行。先将 VERSION 改为目标，再同步并打包：

```powershell
./scripts/Sync-Version.ps1
./scripts/Package-Windows.ps1 -Tag v0.3.0
git diff --check
```

确认验收和解压包检查通过后，显式暂存本功能对应代码、版本、进度和报告，检查 `git diff --cached`，再形成有意义的本地提交和带说明标签。不要顺带暂存用户修改，不覆盖已有标签，不在文档中预填未知的提交 SHA。

```powershell
git diff --cached
git commit -m "feat(annotation): support direct tool editing"
git tag -a v0.3.0 -m "F01: direct editing; acceptance docs/validation/F01-v0.3.0.md"
git show --no-patch v0.3.0
git status --short --branch
```

本地打标签不会上传。只有获得明确公开发布授权后才推送分支及标签。推送新建的 `vX.Y.Z` 标签后，[发布工作流](../.github/workflows/release.yml) 会核对标签与 `VERSION`，在 Windows 上运行同一打包脚本，并将 ZIP 和 `.zip.sha256` 附到同名 GitHub Release。仓库需要启用 GitHub Actions，并允许工作流使用自带的 `GITHUB_TOKEN` 写入 Release。

推送标签会公开发布产物；仅在维护者明确决定发布时执行。本地验证不推送。
