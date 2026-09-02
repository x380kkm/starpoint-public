# audience: external
# # install-android-cn-distribution
#
# 该脚本在独立 ADB 服务中向项目 Android Emulator 安装单 APK 分发包或目录分发包.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^emulator-[0-9]+$')][string]$Serial,
    [string]$DistributionDirectory,
    [string]$BundleApkPath,
    [string]$AdbPath
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
if (-not (Test-Path -LiteralPath $ProjectAdbPath -PathType Leaf)) {
    throw "项目 Android SDK 中的 adb 不存在: $ProjectAdbPath"
}
if ([string]::IsNullOrWhiteSpace($AdbPath)) {
    $AdbPath = $ProjectAdbPath
} elseif ([IO.Path]::GetFullPath($AdbPath) -cne $ProjectAdbPath) {
    throw "Android 安装器只允许项目 SDK adb: $ProjectAdbPath"
}

$ProtocolLabPaths = Get-ProtocolLabPaths -RepositoryRoot $RepositoryRoot
$EmulatorState = Read-ProtocolLabState -Path $ProtocolLabPaths.EmulatorStatePath
Assert-ProtocolLabEmulatorState -State $EmulatorState
if ($EmulatorState.Serial -cne $Serial) {
    throw "模拟器状态 serial 与安装目标不一致: state=$($EmulatorState.Serial) target=$Serial"
}
if ([IO.Path]::GetFullPath([string]$EmulatorState.AdbPath) -cne $ProjectAdbPath) {
    throw "模拟器状态未使用项目 SDK adb: $($EmulatorState.AdbPath)"
}
$EmulatorProcess = Get-OwnedProtocolLabProcess `
    -ProcessId ([int]$EmulatorState.EmulatorProcessId) `
    -ExecutablePath ([string]$EmulatorState.EmulatorPath) `
    -StartTimeUtc ([datetime]$EmulatorState.EmulatorStartTimeUtc)
if ($null -eq $EmulatorProcess) {
    throw "模拟器状态对应的无头进程未运行: $($ProtocolLabPaths.EmulatorStatePath)"
}

# //// 调用当前分发进程的 ADB 服务 [@x380kkm 2026-08-31] ////
function Invoke-DistributionAdbRaw {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string[]]$ArgumentList)

    $CommandArguments = @("-P", $script:AdbServerPort) + $ArgumentList
    $Output = @(& $script:AdbPath @CommandArguments 2>&1 | ForEach-Object { $_.ToString() })
    if ($LASTEXITCODE -ne 0) {
        throw "ADB 执行失败: adb $($CommandArguments -join ' ')`n$($Output -join "`n")"
    }
    $Output
}
# //// /调用当前分发进程的 ADB 服务 ////

# //// 核对独立 ADB 服务中的唯一 Emulator [@x380kkm 2026-08-31] ////
function Assert-DistributionEmulator {
    [CmdletBinding()]
    param()

    $VisibleDevices = @(Invoke-DistributionAdbRaw -ArgumentList @("devices", "-l"))
    $DeviceRows = @($VisibleDevices | Where-Object { $_ -match '\s+device\s+' })
    if ($DeviceRows.Count -ne 1 -or $DeviceRows[0] -notmatch "^$([regex]::Escape($Serial))\s") {
        throw "独立 ADB 暴露了非目标设备: $($DeviceRows -join '; ')"
    }
    $QemuState = @(
        Invoke-DistributionAdbRaw -ArgumentList @("-s", $Serial, "shell", "getprop", "ro.kernel.qemu")
    ) | Select-Object -Last 1
    if ($null -eq $QemuState -or $QemuState.Trim() -cne "1") {
        throw "目标设备不是 Android Emulator: $Serial"
    }
    $AbiList = @(
        Invoke-DistributionAdbRaw -ArgumentList @(
            "-s", $Serial, "shell", "getprop", "ro.product.cpu.abilist"
        )
    ) | Select-Object -Last 1
    $SupportedAbis = @($AbiList.Trim().Split(",", [StringSplitOptions]::RemoveEmptyEntries))
    if ($SupportedAbis -cnotcontains "arm64-v8a") {
        throw "Android Emulator 没有提供 arm64-v8a 运行环境: $Serial"
    }
}
# //// /核对独立 ADB 服务中的唯一 Emulator ////

# //// 向已核对的 Emulator 发送一条命令 [@x380kkm 2026-08-31] ////
function Invoke-DistributionEmulatorAdb {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string[]]$ArgumentList)

    Assert-DistributionEmulator
    Invoke-DistributionAdbRaw -ArgumentList (@("-s", $Serial) + $ArgumentList)
}
# //// /向已核对的 Emulator 发送一条命令 ////

# //// 启动只允许目标 Emulator 的 ADB server [@x380kkm 2026-09-01] ////
function Start-DistributionAdbServer {
    [CmdletBinding()]
    param()

    $StartOutput = @(
        & $script:AdbPath -P $script:AdbServerPort --one-device $Serial start-server 2>&1 |
            ForEach-Object { $_.ToString() }
    )
    if ($LASTEXITCODE -ne 0) {
        throw "独立 ADB 启动失败: $($StartOutput -join "`n")"
    }
}
# //// /启动只允许目标 Emulator 的 ADB server ////

$script:AdbServerPort = [int]$EmulatorState.AdbServerPort

# //// 可续传地提交分发目录条目 [@x380kkm 2026-08-31] ////
function Push-DistributionEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$RemoteDirectory
    )

    $Attempt = 0
    while ($true) {
        try {
            Invoke-DistributionEmulatorAdb -ArgumentList @(
                "push", "--sync", "-Z", $SourcePath, $RemoteDirectory
            ) | Out-Null
            return
        } catch {
            $Attempt++
            if ($Attempt -ge 3) {
                throw
            }
            Start-Sleep -Seconds 1
        }
    }
}
# //// /可续传地提交分发目录条目 ////

# //// 核对远端 CDN 完成标记字节 [@x380kkm 2026-08-31] ////
function Test-RemoteCompleteMarker {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RemotePath,
        [Parameter(Mandatory)][string]$ExpectedDigest
    )

    $ExpectedByteCount = [Text.Encoding]::UTF8.GetByteCount("$ExpectedDigest`n")
    $RemoteCommand = 'if [ -f ''{0}'' ] && [ "$(wc -c < ''{0}'')" -eq {1} ] && [ "$(cat ''{0}'')" = ''{2}'' ]; then printf ''valid''; else printf ''invalid''; fi' -f `
        $RemotePath, $ExpectedByteCount, $ExpectedDigest
    $Result = @(Invoke-DistributionEmulatorAdb -ArgumentList @("shell", $RemoteCommand))
    $Status = $Result | Select-Object -Last 1
    $null -ne $Status -and $Status.Trim() -ceq "valid"
}
# //// /核对远端 CDN 完成标记字节 ////

# //// 解析并核对分发文件 [@x380kkm 2026-09-01] ////
$HasDistributionDirectory = -not [string]::IsNullOrWhiteSpace($DistributionDirectory)
$DistributionDirectory = if (-not $HasDistributionDirectory) {
    $null
} else {
    [IO.Path]::GetFullPath($DistributionDirectory)
}
$DistributionManifestPath = if ($HasDistributionDirectory) {
    Join-Path $DistributionDirectory "distribution-manifest.json"
} else {
    $null
}
$Distribution = if ($null -ne $DistributionManifestPath -and
    (Test-Path -LiteralPath $DistributionManifestPath -PathType Leaf)) {
    Get-Content -LiteralPath $DistributionManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
} else {
    $null
}
$SingleApkMode = $null -ne $BundleApkPath -or
    ($null -ne $Distribution -and [int]$Distribution.schema_version -eq 3)
if ($null -eq $Distribution -and [string]::IsNullOrWhiteSpace($BundleApkPath) -and
    $HasDistributionDirectory) {
    $Candidates = @(Get-ChildItem -LiteralPath $DistributionDirectory -File -Filter "*.apk")
    if ($Candidates.Count -eq 1) {
        $BundleApkPath = $Candidates[0].FullName
        $SingleApkMode = $true
    }
}
if ($SingleApkMode) {
    if ($null -ne $Distribution -and [int]$Distribution.schema_version -ne 3) {
        throw "单 APK 分发清单版本无效: $($Distribution.schema_version)"
    }
    if ([string]::IsNullOrWhiteSpace($BundleApkPath)) {
        $BundleApkPath = [string](Join-Path $DistributionDirectory $Distribution.apk.path)
    }
    $BundleApkPath = [IO.Path]::GetFullPath($BundleApkPath)
    if (-not (Test-Path -LiteralPath $BundleApkPath -PathType Leaf)) {
        throw "Android 单 APK 不存在: $BundleApkPath"
    }
    $Inspector = Join-Path $PSScriptRoot "inspect-android-apk-bundle.py"
    if (-not (Test-Path -LiteralPath $Inspector -PathType Leaf)) {
        throw "Android bundle 检查器不存在: $Inspector"
    }
    $InspectionOutput = @(
        & uv run --python 3.12 python $Inspector --bundle $BundleApkPath --skip-digests 2>&1 |
            ForEach-Object { $_.ToString() }
    )
    if ($LASTEXITCODE -ne 0) {
        throw "Android 单 APK bundle 检查失败: $($InspectionOutput -join "`n")"
    }
    $Inspection = ($InspectionOutput -join "`n") | ConvertFrom-Json
    $script:AdbPath = [IO.Path]::GetFullPath($AdbPath)
    Start-DistributionAdbServer
    Assert-DistributionEmulator
    $InstallOutput = @(Invoke-DistributionEmulatorAdb -ArgumentList @(
        "install", "-r", "-t", $BundleApkPath
    ))
    $PackageCheck = @(Invoke-DistributionEmulatorAdb -ArgumentList @(
        "shell", "pm", "path", "dev.starpoint.personalservice"
    ))
    if (-not ($PackageCheck -join "`n" -match "package:/")) {
        throw "Android 单 APK 安装后未找到伴随服务包."
    }
    $LaunchOutput = @(Invoke-DistributionEmulatorAdb -ArgumentList @(
        "shell", "am", "start", "-W", "-a", "android.intent.action.MAIN",
        "-c", "android.intent.category.LAUNCHER", "-n", "dev.starpoint.personalservice/.ManagementActivity"
    ))
    [pscustomobject]@{
        Mode = "single-apk-tail-cdn"
        Serial = $Serial
        AdbServerPort = $script:AdbServerPort
        BundleApk = $BundleApkPath
        Bundle = $Inspection
        Install = $InstallOutput
        Launch = $LaunchOutput
    }
    return
}
if ($null -eq $Distribution -or [int]$Distribution.schema_version -ne 2 -or
    $Distribution.platform -cne "android") {
    $ExpectedPath = if ($null -ne $DistributionManifestPath) {
        $DistributionManifestPath
    } else {
        "distribution-manifest.json or BundleApkPath"
    }
    throw "Android 分发清单格式无效或未提供单 APK: $ExpectedPath"
}
if ($Distribution.game.package_id -cne "com.leiting.wf") {
    throw "Android 游戏包名无效: $($Distribution.game.package_id)"
}
if ($Distribution.companion.package_id -cne "dev.starpoint.personalservice") {
    throw "Android 伴随服务包名无效: $($Distribution.companion.package_id)"
}
if ($Distribution.cdn.manifest_prefix -notmatch '^[0-9a-f]{16}$' `
    -or $Distribution.cdn.manifest_sha256 -notmatch '^[0-9a-f]{64}$') {
    throw "Android CDN 标识无效."
}
$CompanionFilesRoot = "/sdcard/Android/data/dev.starpoint.personalservice/files"
$RemoteServiceRoot = "$CompanionFilesRoot/starpoint-personal-service"
$RemoteCdnRoot = "$RemoteServiceRoot/cdn"
$ExpectedRemoteData = "$RemoteCdnRoot/$($Distribution.cdn.manifest_prefix)"
if ($Distribution.cdn.remote_root -cne $ExpectedRemoteData) {
    throw "Android CDN 远端目录无效: $($Distribution.cdn.remote_root)"
}

$GameApkPath = [IO.Path]::GetFullPath((Join-Path $DistributionDirectory $Distribution.game.apk.path))
$CompanionApkPath = [IO.Path]::GetFullPath((Join-Path $DistributionDirectory $Distribution.companion.apk.path))
$DataDirectory = [IO.Path]::GetFullPath((Join-Path $DistributionDirectory $Distribution.cdn.path))
$ManifestPath = Join-Path $DataDirectory "manifest.sha256"
$CompleteMarker = Join-Path $DataDirectory ".complete"
foreach ($RequiredPath in @($GameApkPath, $CompanionApkPath, $ManifestPath, $CompleteMarker)) {
    if (-not (Test-Path -LiteralPath $RequiredPath -PathType Leaf)) {
        throw "Android 分发文件不存在: $RequiredPath"
    }
}
if ((Split-Path -Leaf $DataDirectory) -cne $Distribution.cdn.manifest_prefix) {
    throw "Android CDN 目录名和清单前缀不一致."
}
$ManifestDigest = (Get-FileHash -LiteralPath $ManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ManifestDigest -cne $Distribution.cdn.manifest_sha256) {
    throw "Android CDN 清单摘要不一致."
}
foreach ($Package in @(
    @{ Path = $GameApkPath; Sha256 = $Distribution.game.apk.sha256 },
    @{ Path = $CompanionApkPath; Sha256 = $Distribution.companion.apk.sha256 }
)) {
    $PackageDigest = (Get-FileHash -LiteralPath $Package.Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($PackageDigest -cne $Package.Sha256) {
        throw "Android APK 摘要不一致: $($Package.Path)"
    }
}
$ExpectedMarker = "$ManifestDigest`n"
$ActualMarker = [IO.File]::ReadAllText($CompleteMarker, [Text.Encoding]::UTF8)
if ($ActualMarker -cne $ExpectedMarker) {
    throw "Android CDN 完成标记无效."
}
# //// /解析并核对分发文件 ////

# //// 启动当前进程专用的 ADB 服务 [@x380kkm 2026-08-31] ////
$script:AdbPath = [IO.Path]::GetFullPath($AdbPath)
# //// /启动当前进程专用的 ADB 服务 ////

# //// 安装双 APK 并提交伴随服务 CDN [@x380kkm 2026-08-31] ////
Start-DistributionAdbServer | Out-Null
Assert-DistributionEmulator
Invoke-DistributionEmulatorAdb -ArgumentList @("install", "-r", $CompanionApkPath) | Out-Null
Invoke-DistributionEmulatorAdb -ArgumentList @("install", "-r", $GameApkPath) | Out-Null

$RemoteData = $ExpectedRemoteData
$RemoteStaging = "$RemoteCdnRoot/.$($Distribution.cdn.manifest_prefix).staging"
$RemoteMarkerPath = "$RemoteData/.complete"
if (-not (Test-RemoteCompleteMarker -RemotePath $RemoteMarkerPath -ExpectedDigest $ManifestDigest)) {
    Invoke-DistributionEmulatorAdb -ArgumentList @(
        "shell", "rm -rf '$RemoteStaging' && mkdir -p '$RemoteStaging'"
    ) | Out-Null
    foreach ($Entry in Get-ChildItem -LiteralPath $DataDirectory -Force | Where-Object Name -CNE ".complete") {
        Push-DistributionEntry -SourcePath $Entry.FullName -RemoteDirectory "$RemoteStaging/"
    }
    Invoke-DistributionEmulatorAdb -ArgumentList @(
        "shell",
        "printf '%s\n' '$ManifestDigest' > '$RemoteStaging/.complete' && rm -rf '$RemoteData' && mv '$RemoteStaging' '$RemoteData'"
    ) | Out-Null
}
$OwnershipCommand = 'if [ "$(id -u)" -eq 0 ]; then owner="$(stat -c ''%u:%g'' ''{0}'')"; chown -R "$owner" ''{1}''; fi' -f `
    $CompanionFilesRoot, $RemoteServiceRoot
Invoke-DistributionEmulatorAdb -ArgumentList @("shell", $OwnershipCommand) | Out-Null
if (-not (Test-RemoteCompleteMarker -RemotePath $RemoteMarkerPath -ExpectedDigest $ManifestDigest)) {
    throw "Android Emulator CDN 完成标记写入失败."
}
# //// /安装双 APK 并提交伴随服务 CDN ////

# //// 启动伴随服务管理页和游戏 [@x380kkm 2026-08-31] ////
Invoke-DistributionEmulatorAdb -ArgumentList @(
    "shell", "am", "start", "-W", "-a", "android.intent.action.MAIN",
    "-c", "android.intent.category.LAUNCHER", "-n", $Distribution.companion.launch_component
) | Out-Null
Invoke-DistributionEmulatorAdb -ArgumentList @(
    "shell", "am", "start", "-W", "-n", $Distribution.game.launch_component
) | Out-Null
# //// /启动伴随服务管理页和游戏 ////

[pscustomobject]@{
    Serial = $Serial
    AdbServerPort = $script:AdbServerPort
    GamePackageId = $Distribution.game.package_id
    CompanionPackageId = $Distribution.companion.package_id
    GameApk = $GameApkPath
    CompanionApk = $CompanionApkPath
    Cdn = $RemoteData
    ManifestSha256 = $ManifestDigest
}
