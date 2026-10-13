# audience: external
# # test-android-apk-bundle
#
# 该脚本检查单 APK bundle, 可在隔离的 Android Emulator 中安装验证.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BundleApkPath,
    [ValidatePattern('^emulator-[0-9]+$')][string]$Serial,
    [string]$AdbPath,
    [switch]$Install,
    [switch]$SkipPayloadDigests
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot "android-packaging-paths.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

$RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../.."))
$WorkspaceRoot = Resolve-AndroidPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
$ProjectAdbPath = [IO.Path]::GetFullPath(
    (Join-Path $WorkspaceRoot "artifacts/android-sdk/sdk/platform-tools/adb.exe")
)
$BundleApkPath = [IO.Path]::GetFullPath($BundleApkPath)
$Inspector = Join-Path $PSScriptRoot "inspect-android-apk-bundle.py"
if (-not (Test-Path -LiteralPath $BundleApkPath -PathType Leaf)) {
    throw "Android bundle 不存在: $BundleApkPath"
}
if (-not (Test-Path -LiteralPath $Inspector -PathType Leaf)) {
    throw "Android bundle 检查器不存在: $Inspector"
}
if ([string]::IsNullOrWhiteSpace($AdbPath)) {
    $AdbPath = $ProjectAdbPath
} elseif ([IO.Path]::GetFullPath($AdbPath) -cne $ProjectAdbPath) {
    throw "Android bundle 检查只允许项目 SDK adb: $ProjectAdbPath"
}
if (-not (Test-Path -LiteralPath $ProjectAdbPath -PathType Leaf)) {
    throw "项目 Android SDK 中的 adb 不存在: $ProjectAdbPath"
}
$AdbPath = [IO.Path]::GetFullPath($AdbPath)

# //// 运行 bundle 检查器 [@x380kkm 2026-08-31] ////
$InspectorArguments = @("--bundle", $BundleApkPath)
if ($SkipPayloadDigests) {
    $InspectorArguments += "--skip-digests"
}
$InspectorOutput = @(
    & uv run --python 3.12 python $Inspector @InspectorArguments 2>&1 |
        ForEach-Object { $_.ToString() }
)
if ($LASTEXITCODE -ne 0) {
    throw "Android bundle 检查失败: $($InspectorOutput -join "`n")"
}
$InspectorReport = ($InspectorOutput -join "`n") | ConvertFrom-Json
# //// /运行 bundle 检查器 ////

if ($Install) {
    # //// 复用状态所属的隔离 Emulator 服务 [@x380kkm 2026-09-20] ////
    $ProtocolLabPaths = Get-ProtocolLabPaths -RepositoryRoot $RepositoryRoot
    $EmulatorState = Read-ProtocolLabState -Path $ProtocolLabPaths.EmulatorStatePath
    Assert-ProtocolLabEmulatorState -State $EmulatorState
    if ([string]::IsNullOrWhiteSpace($Serial)) {
        $Serial = [string]$EmulatorState.Serial
    }
    if ($EmulatorState.Serial -cne $Serial) {
        throw "模拟器状态 serial 与安装目标不一致: state=$($EmulatorState.Serial) target=$Serial"
    }
    if ([IO.Path]::GetFullPath([string]$EmulatorState.AdbPath) -cne $ProjectAdbPath) {
        throw "模拟器状态未使用项目 SDK adb: $($EmulatorState.AdbPath)"
    }
    $EmulatorProcess = Get-OwnedProtocolLabProcess -ProcessId ([int]$EmulatorState.EmulatorProcessId) -ExecutablePath ([string]$EmulatorState.EmulatorPath) -StartTimeUtc ([datetime]$EmulatorState.EmulatorStartTimeUtc)
    if ($null -eq $EmulatorProcess) {
        throw "模拟器状态对应的无头进程未运行: $($ProtocolLabPaths.EmulatorStatePath)"
    }
    $AdbPort = Resolve-ProtocolLabAdbServerPort -State $EmulatorState
    Start-ProtocolLabAdbServer -AdbPath $AdbPath -Serial $Serial -AdbServerPort $AdbPort | Out-Null
    $AbiResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -AdbServerPort $AdbPort -CommandArguments @('shell', 'getprop', 'ro.product.cpu.abilist')
    $Abis = ($AbiResult.Output -join '').Trim()
    if ($Abis.Split(',', [StringSplitOptions]::RemoveEmptyEntries) -cnotcontains 'arm64-v8a') {
        throw "目标 Emulator 没有 arm64-v8a: $Serial"
    }
    $InstallResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -AdbServerPort $AdbPort -CommandArguments @('install', '-r', $BundleApkPath)
    [pscustomobject]@{
        Bundle = $InspectorReport
        Install = [ordered]@{
            serial = $Serial
            adb_port = $AdbPort
            output = $InstallResult.Output
        }
    } | ConvertTo-Json -Depth 12
    # //// /复用状态所属的隔离 Emulator 服务 ////
} else {
    $InspectorReport | ConvertTo-Json -Depth 12
}
