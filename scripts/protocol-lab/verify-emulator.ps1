# audience: external
# # verify-emulator
# 此脚本验证记录的 Emulator 进程, Android 启动状态, root 身份, eth0 默认路由和实时 PCAP 增长.
# 验证过程只发送到模拟器宿主网关的 ICMP 流量, 不修改游戏数据.

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

# //// 验证持续运行的 Android 协议实验环境 [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot -AvdHome $AvdHome
$State = Read-ProtocolLabState -Path $Paths.EmulatorStatePath
$ResolvedAdbServerPort = Resolve-ProtocolLabAdbServerPort -RequestedPort $AdbServerPort -State $State
$Process = Get-OwnedProtocolLabProcess -ProcessId ([int]$State.EmulatorProcessId) -ExecutablePath ([string]$State.EmulatorPath) -StartTimeUtc ([datetime]$State.EmulatorStartTimeUtc
)
if ($null -eq $Process) {
    throw "记录的 Emulator 进程已退出: $($State.EmulatorProcessId)"
}

$DeviceState = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @("get-state") -AdbServerPort $ResolvedAdbServerPort
$BootCompleted = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @("shell", "getprop", "sys.boot_completed") -AdbServerPort $ResolvedAdbServerPort
$Identity = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @("shell", "id") -AdbServerPort $ResolvedAdbServerPort
$Routes = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @("shell", "ip", "route") -AdbServerPort $ResolvedAdbServerPort
if (($DeviceState.Output -join "").Trim() -ne "device") {
    throw "ADB 设备状态异常: $($DeviceState.Output -join ' ')"
}
if (($BootCompleted.Output -join "").Trim() -ne "1") {
    throw "Android framework 尚未完成启动."
}
if (($Identity.Output -join " ") -notmatch "uid=0\(root\)") {
    throw "adbd 当前不是 root: $($Identity.Output -join ' ')"
}
$RouteText = $Routes.Output -join "`n"
if ($RouteText -notmatch "(?m)^default via $([regex]::Escape([string]$State.HostAddress)) dev eth0 onlink\s*$") {
    throw "eth0 默认路由缺失: $RouteText"
}

$CaptureCheck = Test-ProtocolLabPcapGrowth -AdbPath $State.AdbPath -Serial $State.Serial -PcapPath $State.PcapPath -HostAddress $State.HostAddress -AdbServerPort $ResolvedAdbServerPort
[pscustomobject]@{
    AvdName = $State.AvdName
    Serial = $State.Serial
    AdbServerPort = $ResolvedAdbServerPort
    ProcessId = $Process.Id
    AdbState = "device"
    BootCompleted = 1
    AdbUser = "root"
    Route = "default via $($State.HostAddress) dev eth0 onlink"
    PcapPath = $State.PcapPath
    PcapBytes = $CaptureCheck.AfterBytes
    AddedBytes = $CaptureCheck.AddedBytes
}
# //// /验证持续运行的 Android 协议实验环境 ////
