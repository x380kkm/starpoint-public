# audience: external
# # test-android-embedded-personal-service
#
# 该脚本检查 Android 伴随宿主, JNI 入口和单 APK 打包链的静态契约.

[CmdletBinding()]
param(
    [string]$BundleApkPath,
    [ValidateNotNullOrEmpty()][string]$ServiceBaselineRef = "v-beta-0.0.15",
    [switch]$SkipCargoMetadata
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../.."))
$PackagingPaths = Join-Path $PSScriptRoot "android-packaging-paths.psm1"
Import-Module $PackagingPaths -Force
$PackagingPathsText = Get-Content -LiteralPath $PackagingPaths -Raw -Encoding UTF8
$WorkspaceRoot = Resolve-AndroidPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
$ServiceBaseline = Assert-AndroidPersonalServiceBaseline `
    -RepositoryRoot $RepositoryRoot `
    -BaselineRef $ServiceBaselineRef

function Assert-AndroidContract {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][bool]$Condition,
        [Parameter(Mandatory)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Read-AndroidContractText {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    Assert-AndroidContract -Condition (Test-Path -LiteralPath $Path -PathType Leaf) -Message "Android 契约文件不存在: $Path"
    Get-Content -LiteralPath $Path -Raw -Encoding UTF8
}

$CompanionRoot = Join-Path $RepositoryRoot "platforms/android/CompanionHost"
$CompanionScript = Join-Path $RepositoryRoot "platforms/android/companion/build-companion-apk.ps1"
$DistributionScript = Join-Path $PSScriptRoot "build-android-cn-distribution.ps1"
$GameScript = Join-Path $PSScriptRoot "package-android-cn-game.ps1"
$AppendScript = Join-Path $PSScriptRoot "append-android-apk-bundle.py"
$InspectScript = Join-Path $PSScriptRoot "inspect-android-apk-bundle.py"
$PrepareScript = Join-Path $PSScriptRoot "prepare-android-cn-data.py"
$ManifestPath = Join-Path $CompanionRoot "AndroidManifest.xml"
$JniPath = Join-Path $CompanionRoot "native/starpoint_android_bridge.c"
$HostPath = Join-Path $CompanionRoot "src/dev/starpoint/personalservice/CompanionServiceHost.java"
$ForegroundPath = Join-Path $CompanionRoot "src/dev/starpoint/personalservice/PersonalServiceForegroundService.java"
$CdnInstallerPath = Join-Path $CompanionRoot "src/dev/starpoint/personalservice/CdnAssetInstaller.java"
$GameInstallerPath = Join-Path $CompanionRoot "src/dev/starpoint/personalservice/GameInstaller.java"

foreach ($RequiredPath in @(
    $CompanionScript,
    $DistributionScript,
    $GameScript,
    $AppendScript,
    $InspectScript,
    $PrepareScript,
    $ManifestPath,
    $JniPath,
    $HostPath,
    $ForegroundPath,
    $CdnInstallerPath,
    $GameInstallerPath
)) {
    Assert-AndroidContract -Condition (Test-Path -LiteralPath $RequiredPath -PathType Leaf) -Message "Android 打包链文件不存在: $RequiredPath"
}

$ManifestText = Read-AndroidContractText -Path $ManifestPath
$HostText = Read-AndroidContractText -Path $HostPath
$ForegroundText = Read-AndroidContractText -Path $ForegroundPath
$CdnInstallerText = Read-AndroidContractText -Path $CdnInstallerPath
$GameInstallerText = Read-AndroidContractText -Path $GameInstallerPath
$JniText = Read-AndroidContractText -Path $JniPath
$CompanionScriptText = Read-AndroidContractText -Path $CompanionScript
$DistributionScriptText = Read-AndroidContractText -Path $DistributionScript
$GameScriptText = Read-AndroidContractText -Path $GameScript
$InstallerText = Read-AndroidContractText -Path (Join-Path $PSScriptRoot "install-android-cn-distribution.ps1")

foreach ($Marker in @(
    "dev.starpoint.personalservice",
    ".ManagementActivity",
    ".GameInstallReceiver",
    ".PersonalServiceForegroundService",
    "android.intent.action.MAIN",
    "android.permission.REQUEST_INSTALL_PACKAGES"
)) {
    Assert-AndroidContract -Condition $ManifestText.Contains($Marker) -Message "Android 清单缺少契约标记: $Marker"
}
foreach ($Marker in @(
    "nativeStart",
    "nativeGetPort",
    "nativeIsRunning",
    "nativeFlush",
    "nativeStop",
    "17171"
)) {
    Assert-AndroidContract -Condition ($HostText + $ForegroundText + $CdnInstallerText + $JniText).Contains($Marker) -Message "Android 宿主缺少契约标记: $Marker"
}
Assert-AndroidContract -Condition $JniText.Contains("starpoint_personal_service_start_with_cdn_root") -Message "JNI 未使用当前 personal-service CDN 启动入口."
foreach ($Marker in @(
    "starpoint-game.apk",
    "com.leiting.wf",
    "PackageInstaller"
)) {
    Assert-AndroidContract -Condition $GameInstallerText.Contains($Marker) -Message "Android 游戏安装器缺少契约标记: $Marker"
}

foreach ($PathText in @($CompanionScriptText, $DistributionScriptText, $GameScriptText)) {
    Assert-AndroidContract -Condition (-not $PathText.Contains("starpoint-ios-live")) -Message "Android 打包脚本引用了 iOS worktree."
    Assert-AndroidContract -Condition (-not $PathText.Contains("ios-cn-device")) -Message "Android 打包脚本引用了 iOS 产物目录."
    Assert-AndroidContract -Condition (-not $PathText.Contains("DiagnosticHarness")) -Message "Android 打包脚本引用了已移除的 DiagnosticHarness."
}
Assert-AndroidContract -Condition $CompanionScriptText.Contains("Resolve-AndroidPackagingWorkspaceRoot") -Message "伴随 APK 构建脚本未解析 Git 公共工作区."
Assert-AndroidContract -Condition $CompanionScriptText.Contains("starpoint-build-source.json") -Message "伴随 APK 未嵌入共享服务来源记录."
Assert-AndroidContract -Condition $CompanionScriptText.Contains("assets_tree") -Message "伴随 APK 来源报告未记录顶层 assets tree."
Assert-AndroidContract -Condition $CompanionScriptText.Contains("assets_consistent") -Message "伴随 APK 来源报告未记录顶层 assets 一致性."
Assert-AndroidContract -Condition $PackagingPathsText.Contains('-RelativePath "assets"') -Message "Android 服务一致性门禁未覆盖顶层 assets."
Assert-AndroidContract -Condition $PackagingPathsText.Contains("Get-AndroidWorkingTreePathTree") -Message "Android 服务一致性门禁未核对实际工作树."
Assert-AndroidContract -Condition $ServiceBaseline.personal_service_consistent -Message "Android personal-service 与 iOS tag 不一致."
Assert-AndroidContract -Condition $ServiceBaseline.assets_consistent -Message "Android 顶层 assets 与 iOS tag 不一致."
Assert-AndroidContract -Condition $ServiceBaseline.working_tree_clean -Message "Android 共享服务输入包含未提交改动."
Assert-AndroidContract -Condition ($ServiceBaseline.personal_service_tree -ceq $ServiceBaseline.personal_service_working_tree) -Message "Android personal-service 工作树来源记录不一致."
Assert-AndroidContract -Condition ($ServiceBaseline.assets_tree -ceq $ServiceBaseline.assets_working_tree) -Message "Android 顶层 assets 工作树来源记录不一致."
Assert-AndroidContract -Condition ($ServiceBaseline.assets_tree -ceq $ServiceBaseline.assets_baseline_tree) -Message "Android 顶层 assets 基线来源记录不一致."
Assert-AndroidContract -Condition $DistributionScriptText.Contains("Resolve-AndroidPackagingCdnRoot") -Message "Android 分发脚本未解析可用 CN CDN 根目录."
Assert-AndroidContract -Condition $DistributionScriptText.Contains("source.assets_tree") -Message "Android 分发脚本未核对伴随 APK 的 assets 来源."
Assert-AndroidContract -Condition $GameScriptText.Contains("Resolve-AndroidPackagingWorkspaceRoot") -Message "Android 游戏打包脚本未解析 Git 公共工作区."
Assert-AndroidContract -Condition ($DistributionScriptText -match 'schema_version\s*=\s*3') -Message "Android 单 APK 分发清单版本不是 3."
Assert-AndroidContract -Condition ($InstallerText -match 'schema_version\s*-ne\s*3') -Message "Android 安装器未接受单 APK 分发清单版本 3."
Assert-AndroidContract -Condition $InstallerText.Contains("--one-device") -Message "Android 安装器未使用独立 one-device ADB server."
Assert-AndroidContract -Condition (-not $InstallerText.Contains("Get-Command adb")) -Message "Android 安装器仍会从 PATH 解析 adb."

if (-not $SkipCargoMetadata) {
    $Cargo = Get-Command cargo.exe -ErrorAction Stop
    $CargoOutput = @(
        & $Cargo.Source metadata --no-deps --format-version 1 --manifest-path (Join-Path $RepositoryRoot "core/personal-service/Cargo.toml") 2>&1 |
            ForEach-Object { $_.ToString() }
    )
    Assert-AndroidContract -Condition ($LASTEXITCODE -eq 0) -Message "personal-service Cargo metadata 读取失败: $($CargoOutput -join "`n")"
    $Metadata = ($CargoOutput -join "`n") | ConvertFrom-Json
    $Package = @($Metadata.packages | Where-Object name -CEQ "starpoint-personal-service") | Select-Object -First 1
    Assert-AndroidContract -Condition ($null -ne $Package) -Message "Cargo metadata 缺少 starpoint-personal-service."
    $StaticLibrary = @($Package.targets | Where-Object kind -contains "staticlib") | Select-Object -First 1
    Assert-AndroidContract -Condition ($null -ne $StaticLibrary) -Message "personal-service 没有 staticlib target."
}

$Report = [ordered]@{
    schema_version = 2
    platform = "android"
    repository_root = $RepositoryRoot
    workspace_root = $WorkspaceRoot
    personal_service = $ServiceBaseline
    assets_consistent = $ServiceBaseline.assets_consistent
    companion_manifest = $ManifestPath
    cargo_metadata_checked = -not $SkipCargoMetadata
    bundle_checked = $false
}

if (-not [string]::IsNullOrWhiteSpace($BundleApkPath)) {
    $Inspector = Join-Path $PSScriptRoot "test-android-apk-bundle.ps1"
    Assert-AndroidContract -Condition (Test-Path -LiteralPath $Inspector -PathType Leaf) -Message "Android bundle 检查脚本不存在: $Inspector"
    $BundlePath = [IO.Path]::GetFullPath($BundleApkPath)
    Assert-AndroidContract -Condition (Test-Path -LiteralPath $BundlePath -PathType Leaf) -Message "Android bundle 不存在: $BundlePath"
    $Inspection = @(
        & $Inspector -BundleApkPath $BundlePath -SkipPayloadDigests 2>&1 |
            ForEach-Object { $_.ToString() }
    )
    Assert-AndroidContract -Condition ($LASTEXITCODE -eq 0) -Message "Android bundle 检查失败: $($Inspection -join "`n")"
    $Report.bundle_checked = $true
    $Report.bundle = ($Inspection -join "`n") | ConvertFrom-Json
}

$Report | ConvertTo-Json -Depth 12
