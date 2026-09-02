# audience: external
# # probe-provider-observer
# 此脚本在已启动的 Android 模拟器中安装观察器和 CN 客户端, 并只读取观察器输出的协议形状.
# 此脚本不启动服务器, 不记录请求体, 不生成 SDK 成功响应.

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ClientApkPath,
    [Parameter(Mandatory)]
    [string]$ObserverApkPath,
    [string]$SdkRoot,
    [string]$AvdHome,
    [ValidateRange(1, 65535)]
    [Nullable[int]]$AdbServerPort,
    [ValidateRange(1, 60)]
    [int]$WaitSeconds = 30,
    [ValidateRange(1, 30)]
    [int]$AdbCommandTimeoutSeconds = 10,
    [ValidateRange(10, 300)]
    [int]$ApkInstallTimeoutSeconds = 180
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'protocol-lab.psm1') -Force

$ClientPackage = 'com.leiting.wf'
$ClientActivity = 'air.com.leiting.wf.AppEntry'
$ObserverPackage = 'com.mtl.check'
$ObserverLogTag = 'StarpointProviderObserver'

# //// 验证探针 APK 与已启动模拟器 [@x380kkm 2026-07-28] ////
$ClientApkPath = [IO.Path]::GetFullPath($ClientApkPath)
$ObserverApkPath = [IO.Path]::GetFullPath($ObserverApkPath)
Assert-ProtocolLabFile -Path $ClientApkPath -Description 'CN 客户端 APK'
Assert-ProtocolLabFile -Path $ObserverApkPath -Description 'Provider 观察器 APK'
$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot -AvdHome $AvdHome
$State = Read-ProtocolLabState -Path $Paths.EmulatorStatePath -AllowMissing
if ($null -eq $State) {
    throw "模拟器未运行. 请先运行 start-emulator.ps1: $($Paths.EmulatorStatePath)"
}
foreach ($Property in @('AdbPath', 'Serial')) {
    if ($State.PSObject.Properties.Name -notcontains $Property -or [string]::IsNullOrWhiteSpace([string]$State.$Property)) {
        throw "模拟器状态缺少 ${Property}: $($Paths.EmulatorStatePath)"
    }
}
$ResolvedAdbServerPort = Resolve-ProtocolLabAdbServerPort -RequestedPort $AdbServerPort -State $State
# //// /验证探针 APK 与已启动模拟器 ////

# //// 安装观察器并从真实 AIR 入口采集安全日志 [@x380kkm 2026-07-28] ////
foreach ($ApkPath in @($ObserverApkPath, $ClientApkPath)) {
    $InstallResult = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @('install', '-r', $ApkPath) -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $ApkInstallTimeoutSeconds
    if ($InstallResult.ExitCode -ne 0) {
        throw "APK 安装失败, exit=$($InstallResult.ExitCode) apk=$ApkPath"
    }
}
$ObserverPackageResult = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @('shell', 'pm', 'path', $ObserverPackage) -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds
if ($ObserverPackageResult.ExitCode -ne 0 -or ($ObserverPackageResult.Output -join "`n") -notmatch '^package:') {
    throw "观察器包没有安装: package=$ObserverPackage exit=$($ObserverPackageResult.ExitCode)"
}
$ClientPackageResult = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @('shell', 'pm', 'path', $ClientPackage) -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds
if ($ClientPackageResult.ExitCode -ne 0 -or ($ClientPackageResult.Output -join "`n") -notmatch '^package:') {
    throw "客户端包没有安装: package=$ClientPackage exit=$($ClientPackageResult.ExitCode)"
}

$LogcatClearResult = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @('logcat', '-c') -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds
if ($LogcatClearResult.ExitCode -ne 0) {
    throw "logcat 清理失败, exit=$($LogcatClearResult.ExitCode)"
}

$ForceStopResult = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @('shell', 'am', 'force-stop', $ClientPackage) -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds
if ($ForceStopResult.ExitCode -ne 0) {
    throw "客户端停止失败, exit=$($ForceStopResult.ExitCode) package=$ClientPackage"
}

$StartResult = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @('shell', 'am', 'start', '-n', "$ClientPackage/$ClientActivity") -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds
if ($StartResult.ExitCode -ne 0) {
    throw "AIR 入口启动失败, exit=$($StartResult.ExitCode) component=$ClientPackage/$ClientActivity"
}

Start-Sleep -Seconds $WaitSeconds
$LogcatReadResult = Invoke-ProtocolLabAdb -AdbPath $State.AdbPath -Serial $State.Serial -CommandArguments @('logcat', '-d', '-v', 'brief', "$ObserverLogTag`:I", '*:S') -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds
if ($LogcatReadResult.ExitCode -ne 0) {
    throw "观察器日志读取失败, exit=$($LogcatReadResult.ExitCode)"
}

$ObserverLines = @($LogcatReadResult.Output | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$LegacyQueryPattern = 'query variant=legacy authority=(?<authority>[^\s]+) projectionCount=(?<projectionCount>\d+) selectionPresent=(?<selectionPresent>true|false) selectionArgumentCount=(?<selectionArgumentCount>\d+) sortPresent=(?<sortPresent>true|false)'
$ModernQueryPattern = 'query variant=modern authority=(?<authority>[^\s]+) projectionCount=(?<projectionCount>\d+) queryArgumentsPresent=(?<queryArgumentsPresent>true|false) cancellationPresent=(?<cancellationPresent>true|false)'
$Queries = foreach ($Line in $ObserverLines) {
    if ($Line -match $LegacyQueryPattern) {
        [pscustomobject][ordered]@{
            Variant = 'legacy'
            Authority = $Matches.authority
            ProjectionCount = [int]$Matches.projectionCount
            SelectionPresent = [bool]::Parse($Matches.selectionPresent)
            SelectionArgumentCount = [int]$Matches.selectionArgumentCount
            SortPresent = [bool]::Parse($Matches.sortPresent)
        }
        continue
    }
    if ($Line -match $ModernQueryPattern) {
        [pscustomobject][ordered]@{
            Variant = 'modern'
            Authority = $Matches.authority
            ProjectionCount = [int]$Matches.projectionCount
            QueryArgumentsPresent = [bool]::Parse($Matches.queryArgumentsPresent)
            CancellationPresent = [bool]::Parse($Matches.cancellationPresent)
        }
    }
}
[pscustomobject][ordered]@{
    ClientPackage = $ClientPackage
    ClientActivity = $ClientActivity
    ObserverPackage = $ObserverPackage
    AdbServerPort = $ResolvedAdbServerPort
    ObserverLogCount = $ObserverLines.Count
    QueryCount = @($Queries).Count
    Queries = @($Queries)
}
# //// /安装观察器并从真实 AIR 入口采集安全日志 ////
