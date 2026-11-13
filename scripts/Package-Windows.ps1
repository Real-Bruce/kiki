#Requires -Version 7.0

param([string]$Tag)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
$releaseTag = "v$version"
if ($PSBoundParameters.ContainsKey('Tag') -and $Tag -cne $releaseTag) {
    throw "标签 $Tag 与 VERSION 对应的 $releaseTag 不一致。"
}

$target = 'x86_64-pc-windows-msvc'
$targetDir = Join-Path $repo 'target'
$dist = Join-Path $repo 'dist'
$zipName = "kiki-$releaseTag-windows-x64.zip"

Push-Location -LiteralPath $repo
try {
    cargo build --locked --release --no-default-features --target $target --target-dir $targetDir --bin kiki
    if ($LASTEXITCODE -ne 0) {
        throw 'Windows x64 Release 构建失败。'
    }
    $sysroot = rustc --print sysroot
    if ($LASTEXITCODE -ne 0) {
        throw '无法定位当前 Rust 工具链的许可文件。'
    }

    $rustDocs = Join-Path $sysroot 'share/doc/rust'
    $null = New-Item -ItemType Directory -Path $dist -Force
    $stage = New-Item -ItemType Directory -Path (Join-Path $dist ('.package-' + [guid]::NewGuid()))
    try {
        $payload = Join-Path $stage.FullName 'payload'
        $licenseDir = Join-Path $payload 'licenses/rust'
        $null = New-Item -ItemType Directory -Path $licenseDir
        Copy-Item -LiteralPath (Join-Path $targetDir "$target/release/kiki.exe") -Destination $payload
        $exeVersion = (Get-Item -LiteralPath (Join-Path $payload 'kiki.exe')).VersionInfo.ProductVersion
        if ($exeVersion -cne $version) {
            throw "打包的 EXE 版本 $exeVersion 与 VERSION 中的 $version 不一致。"
        }
        Copy-Item -LiteralPath 'docs/QUICK_START.txt' -Destination (Join-Path $payload 'README.txt')
        Copy-Item -LiteralPath 'LICENSE' -Destination $payload
        Copy-Item -LiteralPath 'resources/THIRD_PARTY_LICENSES.txt' -Destination $payload
        Copy-Item -LiteralPath (Join-Path $rustDocs 'COPYRIGHT-library.html') -Destination $licenseDir
        Copy-Item -LiteralPath (Join-Path $rustDocs 'licenses') -Destination $licenseDir -Recurse

        $zip = Join-Path $stage.FullName $zipName
        [IO.Compression.ZipFile]::CreateFromDirectory($payload, $zip, 'Optimal', $false)
        $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        Set-Content -LiteralPath "$zip.sha256" -Value "$hash  $zipName" -Encoding ascii

        Move-Item -LiteralPath $zip, "$zip.sha256" -Destination $dist -Force
        Write-Output (Join-Path $dist $zipName)
        Write-Output (Join-Path $dist "$zipName.sha256")
    } finally {
        $stagePath = (Resolve-Path -LiteralPath $stage.FullName).ProviderPath
        $distPath = (Resolve-Path -LiteralPath $dist).ProviderPath
        if ((Split-Path -Parent $stagePath) -eq $distPath) {
            Remove-Item -LiteralPath $stagePath -Recurse -Force
        }
    }
} finally {
    Pop-Location
}
