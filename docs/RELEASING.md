# 打包与发布

在 Windows x64 上使用 PowerShell 7，在仓库根目录运行 `./scripts/Package-Windows.ps1`。脚本按锁文件构建 Release，将 EXE、快速使用说明和许可证打成 ZIP，并在 `dist` 下生成 ZIP 及 SHA-256 校验文件。

根目录的 [VERSION](../VERSION) 是发布版本来源，格式为 `X.Y.Z`。发布新版本时，先确认要打标签的分支包含发布工作流。以 `v0.2.2` 为例，将 `VERSION` 改为 `0.2.2`，再运行以下命令同步 Cargo 版本、验证本地打包、提交并推送。

```powershell
./scripts/Sync-Version.ps1
./scripts/Package-Windows.ps1 -Tag v0.2.2
git add VERSION Cargo.toml Cargo.lock
git commit -m "chore(release): prepare v0.2.2"
git push origin HEAD
git tag -a v0.2.2 -m "v0.2.2"
git push origin v0.2.2
```

推送新建的 `vX.Y.Z` 标签后，[发布工作流](../.github/workflows/release.yml) 会核对标签与 `VERSION`，在 Windows 上运行同一打包脚本，并将 ZIP 和 `.zip.sha256` 附到同名 GitHub Release。仓库需要启用 GitHub Actions，并允许工作流使用自带的 `GITHUB_TOKEN` 写入 Release。

推送标签会公开发布产物；仅在维护者明确决定发布时执行。本地验证不推送。
