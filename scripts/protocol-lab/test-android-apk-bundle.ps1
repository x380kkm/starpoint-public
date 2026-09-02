# audience: external
# # test-android-apk-bundle
#
# 该脚本检查单 APK bundle, 可在隔离的 Android Emulator 中安装验证.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BundleApkPath,
    [ValidatePattern('^emulator-[0-9]+$')][string]$Serial = "emulator-5554",
    [string]$AdbPath,
    [switch]$Install,
    [switch]$SkipPayloadDigests
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot "android-packaging-paths.psm1") -Force

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
    # //// 启动只暴露目标 Emulator 的 ADB 服务 [@x380kkm 2026-08-31] ////
    do {
        $Listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
        try {
            $Listener.Start()
            $AdbPort = ([Net.IPEndPoint]$Listener.LocalEndpoint).Port
        } finally {
            $Listener.Stop()
        }
    } while ($AdbPort -eq 5037)
    $StartOutput = @(
        & $AdbPath -P $AdbPort --one-device $Serial start-server 2>&1 |
            ForEach-Object { $_.ToString() }
    )
    if ($LASTEXITCODE -ne 0) {
        throw "隔离 ADB 启动失败: $($StartOutput -join "`n")"
    }
    try {
        $Devices = @(& $AdbPath -P $AdbPort devices -l 2>&1 | ForEach-Object { $_.ToString() })
        $Rows = @($Devices | Where-Object { $_ -match '\s+device\s+' })
        if ($Rows.Count -ne 1 -or $Rows[0] -notmatch "^$([regex]::Escape($Serial))\s") {
            throw "ADB 服务暴露了非目标设备: $($Rows -join '; ')"
        }
        $Qemu = @(& $AdbPath -P $AdbPort -s $Serial shell getprop ro.kernel.qemu 2>&1 | ForEach-Object { $_.ToString() }) | Select-Object -Last 1
        if ($null -eq $Qemu -or $Qemu.Trim() -cne "1") {
            throw "目标设备不是 Android Emulator: $Serial"
        }
        $Abis = @(& $AdbPath -P $AdbPort -s $Serial shell getprop ro.product.cpu.abilist 2>&1 | ForEach-Object { $_.ToString() }) | Select-Object -Last 1
        if ($Abis.Trim().Split(",", [StringSplitOptions]::RemoveEmptyEntries) -cnotcontains "arm64-v8a") {
            throw "目标 Emulator 没有 arm64-v8a: $Serial"
        }
        $InstallOutput = @(
            & $AdbPath -P $AdbPort -s $Serial install -r $BundleApkPath 2>&1 |
                ForEach-Object { $_.ToString() }
        )
        if ($LASTEXITCODE -ne 0) {
            throw "Android bundle 安装失败: $($InstallOutput -join "`n")"
        }
        [pscustomobject]@{
            Bundle = $InspectorReport
            Install = [ordered]@{
                serial = $Serial
                adb_port = $AdbPort
                output = $InstallOutput
            }
        } | ConvertTo-Json -Depth 12
    } finally {
        & $AdbPath -P $AdbPort kill-server 2>&1 | Out-Null
    }
} else {
    $InspectorReport | ConvertTo-Json -Depth 12
}
