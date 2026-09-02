# audience: external
# # stop-isolated-cn-service
# 此脚本只停止状态文件确认属于本实验的 CN 服务, 并删除其运行状态文件.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$StatePath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$StatePath = [IO.Path]::GetFullPath($StatePath)
if (-not (Test-Path -LiteralPath $StatePath -PathType Leaf)) {
    [pscustomobject]@{ Stopped = $true; AlreadyStopped = $true; StatePath = $StatePath }
    return
}

$state = Get-Content -LiteralPath $StatePath -Raw -Encoding UTF8 | ConvertFrom-Json
foreach ($property in @("ProcessId", "ProcessStartTimeUtc", "ExecutablePath", "RepositoryRoot", "RunDirectory")) {
    if ($state.PSObject.Properties.Name -notcontains $property) { throw "服务状态缺少 $property." }
}
$processId = [int]$state.ProcessId
$process = Get-Process -Id $processId -ErrorAction SilentlyContinue
if ($null -ne $process) {
    $processInfo = Get-CimInstance Win32_Process -Filter "ProcessId = $processId"
    $executablePath = [IO.Path]::GetFullPath([string]$state.ExecutablePath)
    $repositoryRoot = [IO.Path]::GetFullPath([string]$state.RepositoryRoot)
    $runDirectory = [IO.Path]::GetFullPath([string]$state.RunDirectory)
    $entryPath = if ($state.PSObject.Properties.Name -contains "EntryPath") {
        [IO.Path]::GetFullPath([string]$state.EntryPath)
    } else {
        $null
    }
    $commandLine = [string]$processInfo.CommandLine
    $sameExecutable = [IO.Path]::GetFullPath([string]$processInfo.ExecutablePath) -eq $executablePath
    $sameRoot = $commandLine.IndexOf($repositoryRoot, [StringComparison]::OrdinalIgnoreCase) -ge 0
    $sameRunDirectory = [IO.Path]::GetFullPath((Split-Path -Parent $StatePath)) -eq $runDirectory
    $sameEntry = if ($null -ne $entryPath) {
        $commandLine.IndexOf($entryPath, [StringComparison]::OrdinalIgnoreCase) -ge 0
    } else {
        $commandLine -eq "`"$executablePath`" --env-file=.env.cn out/start.js"
    }
    if ($null -eq $entryPath -and $sameEntry) { $sameRoot = $true }
    $expectedStart = if ($state.ProcessStartTimeUtc -is [DateTime]) {
        $state.ProcessStartTimeUtc.ToUniversalTime()
    } else {
        [DateTime]::Parse(
            [string]$state.ProcessStartTimeUtc,
            [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::AssumeUniversal -bor [Globalization.DateTimeStyles]::AdjustToUniversal
        )
    }
    $actualStart = $process.StartTime.ToUniversalTime()
    $sameStart = [Math]::Abs(($actualStart - $expectedStart).TotalSeconds) -lt 5
    if (-not ($sameExecutable -and $sameRoot -and $sameRunDirectory -and $sameEntry -and $sameStart)) {
        throw "拒绝停止不属于本实验的进程: $processId"
    }
    Stop-Process -Id $processId -Force
    $process.WaitForExit(5000)
}
Remove-Item -LiteralPath $StatePath -Force
[pscustomobject]@{ Stopped = $true; AlreadyStopped = $false; ProcessId = $processId; StatePath = $StatePath }
