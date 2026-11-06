param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/release/icapture.exe',
    [string]$OutputDirectory = ('artifacts/performance/startup-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
)
$ErrorActionPreference = 'Stop'
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
if ((Test-Path -LiteralPath $OutputDirectory) -and (Get-ChildItem -LiteralPath $OutputDirectory -Force | Select-Object -First 1)) {
    throw '采样输出目录必须为空，避免读取上一轮的遗留结果。请使用新目录。'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$resolvedOutput = (Resolve-Path -LiteralPath $OutputDirectory).Path
$samples = @()
for ($index = 1; $index -le 30; $index++) {
    $sampleDirectory = Join-Path $resolvedOutput ('sample-{0:D2}' -f $index)
    # A new process for every sample. No cache flush, forced WS trimming, or excluded preheat run.
    $trial = Start-Process -FilePath $resolvedExecutable -ArgumentList '--startup-probe',('"' + $sampleDirectory + '"') -WindowStyle Hidden -PassThru
    if (-not $trial.WaitForExit(5000)) {
        if ($trial.Path -eq $resolvedExecutable) { Stop-Process -Id $trial.Id }
        throw "第 $index 次启动超时；保留失败目录，不将其计为有效样本。请确认构建启用了 diagnostics 且没有另一 iCapture 实例。"
    }
    $sampleFile = Join-Path $sampleDirectory 'startup.csv'
    if ($trial.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $sampleFile)) {
        throw "第 $index 次启动失败，退出码 $($trial.ExitCode)。"
    }
    $sample = Import-Csv -LiteralPath $sampleFile
    $record = [pscustomobject]@{ Sample=$index; ReadyMs=[double]$sample.ready_ms; HotkeyRegistered=$sample.hotkey_registered; SystemDpi=$sample.system_dpi }
    $samples += $record
    $samples | Export-Csv -NoTypeInformation -Encoding utf8 -LiteralPath (Join-Path $resolvedOutput 'samples.csv')
    if ($sample.hotkey_registered -ne 'true') { throw "第 $index 次注册热键失败，已保留该失败样本。" }
}
$ordered = @($samples.ReadyMs | Sort-Object)
[pscustomobject]@{
    Count=$ordered.Count
    P50Ms=$ordered[14]
    P95Ms=$ordered[28]
    MaxMs=$ordered[-1]
    MeanMs=($ordered | Measure-Object -Average).Average
    Method='30 independent process starts; OS creation time to registered hotkey and tray readiness; normal OS cache; diagnostics Release'
} | ConvertTo-Json | Tee-Object -FilePath (Join-Path $resolvedOutput 'summary.json')
