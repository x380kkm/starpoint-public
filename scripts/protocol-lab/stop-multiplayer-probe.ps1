# audience: external
# # stop-multiplayer-probe
# 此脚本只停止状态文件记录的多人协议探针, 并保留 JSONL 事件和所有原始数据块.
# PID 被其他进程复用时脚本拒绝终止该进程.

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

# //// 停止本次启动的多人协议探针 [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths
$State = Read-ProtocolLabState -Path $Paths.ProbeStatePath -AllowMissing
if ($null -eq $State) {
    [pscustomobject]@{ Stopped = $true; AlreadyStopped = $true }
    return
}

$Process = Get-OwnedProtocolLabProcess -ProcessId ([int]$State.ProcessId) -ExecutablePath ([string]$State.PythonPath) -StartTimeUtc ([datetime]$State.ProcessStartTimeUtc)
if ($null -ne $Process) {
    $Process | Stop-Process
    if (-not $Process.WaitForExit(5000)) {
        $Process | Stop-Process -Force
        $Process.WaitForExit(5000)
    }
}
Remove-Item -LiteralPath $Paths.ProbeStatePath -Force
[pscustomobject]@{
    Stopped = $true
    AlreadyStopped = $false
    CaptureDirectory = $State.CaptureDirectory
    EventPath = $State.EventPath
}
# //// /停止本次启动的多人协议探针 ////
