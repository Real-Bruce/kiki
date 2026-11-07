param(
    [Parameter(Mandatory)][int]$ProcessId,
    [string]$OutputDirectory = ('artifacts/performance/idle-' + (Get-Date -Format 'yyyyMMdd-HHmmss')),
    [string]$State = 'idle',
    [ValidateRange(60,600)][int]$Seconds = 60
)
$ErrorActionPreference = 'Stop'
if ((Test-Path -LiteralPath $OutputDirectory) -and (Get-ChildItem -LiteralPath $OutputDirectory -Force | Select-Object -First 1)) {throw '输出目录必须为空。'}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
if (-not ('KikiMetrics.Native' -as [type])) {
    # Read-only metrics helper in the sampling host, never loaded by or distributed with kiki.
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
namespace KikiMetrics {
    [StructLayout(LayoutKind.Sequential)] public struct Memory {
        public uint cb, PageFaultCount;
        public UIntPtr PeakWorkingSetSize, WorkingSetSize, QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage,
            QuotaPeakNonPagedPoolUsage, QuotaNonPagedPoolUsage, PagefileUsage, PeakPagefileUsage,
            PrivateUsage, PrivateWorkingSetSize;
        public ulong SharedCommitUsage;
    }
    public static class Native {
        [DllImport("psapi.dll", SetLastError=true)]
        public static extern bool GetProcessMemoryInfo(IntPtr process, ref Memory memory, uint size);
        [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr process, uint flags);
    }
}
"@
}
$targetProcess = Get-Process -Id $ProcessId
$targetPath = $targetProcess.Path
$startedAt = $targetProcess.StartTime
$logicalProcessors = (Get-CimInstance Win32_Processor | Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum
if ($logicalProcessors -le 0) {throw '无法读取处理器总量。'}
function Read-Metrics([double]$Elapsed) {
    $targetProcess.Refresh()
    if ($targetProcess.HasExited -or $targetProcess.StartTime -ne $startedAt) {throw '采样进程已退出或改变。'}
    $memory = [KikiMetrics.Memory]::new()
    $memory.cb = [Runtime.InteropServices.Marshal]::SizeOf($memory)
    if (-not [KikiMetrics.Native]::GetProcessMemoryInfo($targetProcess.Handle,[ref]$memory,$memory.cb)) {throw 'GetProcessMemoryInfo 失败。'}
    [pscustomobject]@{
        Seconds=$Elapsed; PrivateWorkingSet=$memory.PrivateWorkingSetSize.ToUInt64()
        PrivateBytes=$memory.PrivateUsage.ToUInt64(); TotalWorkingSet=$memory.WorkingSetSize.ToUInt64()
        Handles=$targetProcess.HandleCount; Gdi=[KikiMetrics.Native]::GetGuiResources($targetProcess.Handle,0)
        User=[KikiMetrics.Native]::GetGuiResources($targetProcess.Handle,1); Threads=$targetProcess.Threads.Count
    }
}
$records = [Collections.Generic.List[object]]::new()
$records.Add((Read-Metrics 0))
$cpuStart = $targetProcess.TotalProcessorTime.TotalSeconds
$watch = [Diagnostics.Stopwatch]::StartNew()
while ($watch.Elapsed.TotalSeconds -lt $Seconds) {
    Start-Sleep -Milliseconds 1000
    $records.Add((Read-Metrics $watch.Elapsed.TotalSeconds))
}
$targetProcess.Refresh()
$cpuSeconds = $targetProcess.TotalProcessorTime.TotalSeconds - $cpuStart
$elapsedSeconds = $watch.Elapsed.TotalSeconds
$records | Export-Csv -LiteralPath (Join-Path $OutputDirectory 'samples.csv') -NoTypeInformation -Encoding utf8
[pscustomobject]@{
    ProcessId=$ProcessId; Executable=$targetPath; State=$State; Seconds=$elapsedSeconds
    CpuSeconds=$cpuSeconds; LogicalProcessors=$logicalProcessors
    NormalizedCpuPercent=$cpuSeconds/$elapsedSeconds/$logicalProcessors*100
    PrivateWorkingSetMin=($records.PrivateWorkingSet | Measure-Object -Minimum).Minimum
    PrivateWorkingSetMax=($records.PrivateWorkingSet | Measure-Object -Maximum).Maximum
    Method='External read-only EX2 memory, GetGuiResources, process CPU time / wall time / total logical processors; no working-set trimming'
} | ConvertTo-Json | Tee-Object -FilePath (Join-Path $OutputDirectory 'summary.json')
