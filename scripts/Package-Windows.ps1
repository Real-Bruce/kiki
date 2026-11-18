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
    $rustCopyright = Get-Content -LiteralPath (Join-Path $rustDocs 'COPYRIGHT-library.html') -Raw -Encoding utf8
    # Rust 1.88.0 的清单将许可原文放在 HTML 正文的 pre 元素中。先去除标记，再解码转义字符。
    $rustBody = [regex]::Match($rustCopyright, '(?is)<body\b[^>]*>(.*?)</body>')
    if (-not $rustBody.Success) {
        throw '无法提取 Rust 标准库版权清单正文。'
    }
    $rustNotices = $rustBody.Groups[1].Value -replace '<li>', '- ' -replace '<[^>]+>', ''
    $rustNotices = [Net.WebUtility]::HtmlDecode($rustNotices) -replace '(?m)^[ \t]+(?=\r?$)', ''
    $rustNotices = $rustNotices -replace '(\r?\n){3,}', "`n`n"
    $rustLicenseFiles = Get-ChildItem -LiteralPath (Join-Path $rustDocs 'licenses') -File -Recurse | Sort-Object Name
    if (-not $rustLicenseFiles) {
        throw 'Rust 工具链的许可文本目录为空。'
    }
    $licenseSections = @(
        '=== kiki project license ==='
        (Get-Content -LiteralPath 'LICENSE' -Raw -Encoding utf8).TrimEnd()
        '=== kiki dependency notices ==='
        (Get-Content -LiteralPath 'resources/THIRD_PARTY_LICENSES.txt' -Raw -Encoding utf8).TrimEnd()
        '=== Rust standard library notices ==='
        $rustNotices.Trim()
        '=== Additional license texts from the Rust toolchain ==='
        foreach ($file in $rustLicenseFiles) {
            '--- ' + $file.Name + ' ---'
            (Get-Content -LiteralPath $file.FullName -Raw -Encoding utf8).TrimEnd()
        }
    )

    $null = New-Item -ItemType Directory -Path $dist -Force
    $stage = New-Item -ItemType Directory -Path (Join-Path $dist ('.package-' + [guid]::NewGuid()))
    try {
        $payload = Join-Path $stage.FullName 'payload'
        $null = New-Item -ItemType Directory -Path $payload
        Copy-Item -LiteralPath (Join-Path $targetDir "$target/release/kiki.exe") -Destination $payload
        $exeVersion = (Get-Item -LiteralPath (Join-Path $payload 'kiki.exe')).VersionInfo.ProductVersion
        if ($exeVersion -cne $version) {
            throw "打包的 EXE 版本 $exeVersion 与 VERSION 中的 $version 不一致。"
        }
        Copy-Item -LiteralPath 'docs/QUICK_START.txt' -Destination (Join-Path $payload 'README.txt')
        $licenseText = ($licenseSections -join "`n`n") -replace '\r?\n', "`r`n"
        [IO.File]::WriteAllText((Join-Path $payload 'LICENSE.txt'), "$licenseText`r`n")

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
