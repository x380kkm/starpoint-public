# audience: external
# # stop-emulator
# 此脚本恢复 Android 全局 HTTP 代理, 停止状态文件记录的 Emulator 进程, 并保留 PCAP, 日志和 AVD 用户数据.
# PID 被其他进程复用时脚本拒绝强制终止该进程.

[CmdletBinding()]
param(
    [string]$SdkRoot,
    [string]$AvdHome,
    [ValidateRange(1, 65535)]
    [Nullable[int]]$AdbServerPort
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

# //// 停止本次启动的 Emulator 并保留捕获内容 [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot -AvdHome $AvdHome
$State = Read-ProtocolLabState -Path $Paths.EmulatorStatePath -AllowMissing
if ($null -eq $State) {
    [pscustomobject]@{ Stopped = $true; AlreadyStopped = $true }
    return
}
Assert-ProtocolLabEmulatorState -State $State
$ResolvedAdbServerPort = Resolve-ProtocolLabAdbServerPort -RequestedPort $AdbServerPort -State $State

$Process = Get-OwnedProtocolLabProcess -ProcessId ([int]$State.EmulatorProcessId) -ExecutablePath ([string]$State.EmulatorPath) -StartTimeUtc ([datetime]$State.EmulatorStartTimeUtc)
$ProxyRestored = $false
$ProxyRestoreError = $null
if ($null -ne $Process) {
    Start-ProtocolLabAdbServer -AdbPath $State.AdbPath -Serial $State.Serial -AdbServerPort $ResolvedAdbServerPort | Out-Null
}
if ($null -ne $Process -and $State.PSObject.Properties.Name -contains "OriginalProxy") {
    try {
        Restore-ProtocolLabProxy -AdbPath $State.AdbPath -Serial $State.Serial -ProxyValue ([string]$State.OriginalProxy) -AdbServerPort $ResolvedAdbServerPort
        $ProxyRestored = $true
    } catch {
        $ProxyRestoreError = $_.Exception.Message
        Write-Warning "Android 全局 HTTP 代理恢复失败: $ProxyRestoreError"
    }
}
if ($null -ne $Process) {
    try {
        Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @("emu", "kill") -AdbServerPort $ResolvedAdbServerPort -AllowFailure | Out-Null
    } catch {
        Write-Warning "Emulator 平滑停止失败: $($_.Exception.Message)"
    }
    $Deadline = (Get-Date).AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 500
        $Process.Refresh()
    } while (-not $Process.HasExited -and (Get-Date) -lt $Deadline)

    if (-not $Process.HasExited) {
        $Process | Stop-Process -Force
        $Process.WaitForExit(5000)
    }
}

Stop-ProtocolLabAdbServer -AdbPath $State.AdbPath -AdbServerPort $ResolvedAdbServerPort -AllowFailure | Out-Null
Remove-Item -LiteralPath $Paths.EmulatorStatePath -Force
[pscustomobject]@{
    Stopped = $true
    AlreadyStopped = $false
    Serial = $State.Serial
    AdbServerPort = $ResolvedAdbServerPort
    ProxyRestored = $ProxyRestored
    ProxyRestoreError = $ProxyRestoreError
    PcapPath = $State.PcapPath
    PcapBytes = if (Test-Path -LiteralPath $State.PcapPath) { (Get-Item -LiteralPath $State.PcapPath).Length } else { 0 }
}
# //// /停止本次启动的 Emulator 并保留捕获内容 ////
