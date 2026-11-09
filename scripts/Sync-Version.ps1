#Requires -Version 7.0

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
if ($version -cnotmatch '\A(0|[1-9][0-9]{0,4})(\.(0|[1-9][0-9]{0,4})){2}\z' -or
    ($version.Split('.') | Where-Object { [int]$_ -gt 65535 })) {
    throw 'VERSION 必须是 X.Y.Z 格式，各段取值为 0 至 65535，且没有多余的前导零。'
}

$manifestPath = Join-Path $repo 'Cargo.toml'
$manifest = Get-Content -LiteralPath $manifestPath -Raw
$packageVersion = '(?m)(^\[package\]\r?\nname = "kiki"\r?\nversion = ")[^"\r\n]+(?=")'
if ($manifest -cnotmatch $packageVersion) {
    throw 'Cargo.toml 中找不到 kiki 的包版本。'
}
$updatedManifest = $manifest -creplace $packageVersion, ('${1}' + $version)

Push-Location -LiteralPath $repo
try {
    if ($updatedManifest -cne $manifest) {
        [IO.File]::WriteAllText($manifestPath, $updatedManifest)
    }
    cargo update --workspace
    if ($LASTEXITCODE -ne 0) {
        throw 'Cargo.lock 版本同步失败。'
    }
} catch {
    if ($updatedManifest -cne $manifest) {
        [IO.File]::WriteAllText($manifestPath, $manifest)
    }
    throw
} finally {
    Pop-Location
}
