param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/release/kiki.exe',
    [string]$OutputDirectory = ('artifacts/performance/p7-' + (Get-Date -Format 'yyyyMMdd-HHmmss')),
    [switch]$FirstOnly,
    [string]$Hotkey = 'Ctrl+Alt+A'
)
$ErrorActionPreference = 'Stop'
$executablePath = (Resolve-Path -LiteralPath $Executable).Path
# This literal also occurs in a diagnostics-only error message, so Release LTO cannot remove it.
# Check before launch: ordinary builds ignore --config-dir and must never be used for isolated probes.
$diagnosticBytes = [IO.File]::ReadAllBytes($executablePath)
if (-not [Text.Encoding]::UTF8.GetString($diagnosticBytes).Contains('--startup-probe')) {
    throw '需要 diagnostics 构建：请先 cargo build --release --features diagnostics。程序尚未启动。'
}
if ((Test-Path -LiteralPath $OutputDirectory) -and (Get-ChildItem -LiteralPath $OutputDirectory -Force | Select-Object -First 1)) {
    throw '输出目录必须为空，所有失败样本保留。'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$outputPath = (Resolve-Path -LiteralPath $OutputDirectory).Path
$environmentInfo = [ordered]@{
    CapturedAt = (Get-Date -Format o)
    Executable = $executablePath
    Sha256 = (Get-FileHash -LiteralPath $executablePath -Algorithm SHA256).Hash
    Hotkey = $Hotkey
    Cpu = @(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors)
    Computer = (Get-CimInstance Win32_ComputerSystem | Select-Object TotalPhysicalMemory)
    Windows = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber)
    Gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,AdapterRAM,CurrentHorizontalResolution,CurrentVerticalResolution,CurrentRefreshRate)
}
$environmentInfo | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $outputPath 'environment.json') -Encoding utf8
$count = if ($FirstOnly) {30} else {1}
for ($index = 1; $index -le $count; $index++) {
    $samplePath = Join-Path $outputPath ('sample-{0:D2}' -f $index)
    $configPath = Join-Path $samplePath 'config'
    New-Item -ItemType Directory -Force -Path $configPath | Out-Null
    # Isolate actual user configuration. No startup-registry writes; no excluded warm-up samples.
    "version=1`nfirst_tip_seen=true`nhotkey=$Hotkey`n" | Set-Content -LiteralPath (Join-Path $configPath 'settings.ini') -Encoding utf8NoBOM
    $arguments = @('--performance-probe',('"' + $samplePath + '"'),'--config-dir',('"' + $configPath + '"'))
    if ($FirstOnly) {$arguments += '--first-only'}
    $trial = Start-Process -FilePath $executablePath -ArgumentList $arguments -WindowStyle Hidden -PassThru
    $timeoutMs = if ($FirstOnly) {15000} else {360000}
    if (-not $trial.WaitForExit($timeoutMs)) {
        if ($trial.Path -eq $executablePath) {Stop-Process -Id $trial.Id; $trial.WaitForExit(5000) | Out-Null}
        throw "第 $index 个采样进程超时，失败目录已保留。"
    }
    if ($trial.ExitCode -ne 0 -or (Test-Path -LiteralPath (Join-Path $samplePath 'error.txt')) -or -not (Test-Path -LiteralPath (Join-Path $samplePath 'complete.txt'))) {
        throw "第 $index 个采样失败，退出码 $($trial.ExitCode)；检查目录中的 error.txt。"
    }
}
$latencies = @(Get-ChildItem -LiteralPath $outputPath -Filter 'sample-*' -Directory | ForEach-Object {
    $sampleIndex = $_.Name
    Import-Csv -LiteralPath (Join-Path $_.FullName 'latency.csv') | Select-Object @{n='process';e={$sampleIndex}},*
})
$latencies | Export-Csv -LiteralPath (Join-Path $outputPath 'latencies.csv') -NoTypeInformation -Encoding utf8
$summary = @($latencies | Group-Object kind | ForEach-Object {
    $ordered = @($_.Group.ready_ms | ForEach-Object {[double]$_} | Sort-Object)
    [pscustomobject]@{
        Kind=$_.Name; Count=$ordered.Count
        P50Ms=$ordered[[Math]::Ceiling($ordered.Count*0.50)-1]
        P95Ms=$ordered[[Math]::Ceiling($ordered.Count*0.95)-1]
        MaxMs=$ordered[-1]; MeanMs=($ordered | Measure-Object -Average).Average
        Method='Posted registered WM_HOTKEY path to visible Preview first paint + DwmFlush; real desktop; diagnostics Release; physical keyboard delivery excluded'
    }
})
$summary | ConvertTo-Json | Tee-Object -FilePath (Join-Path $outputPath 'summary.json')
