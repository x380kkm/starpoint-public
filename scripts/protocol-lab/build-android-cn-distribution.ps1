# audience: external
# # build-android-cn-distribution
#
# 该脚本构建包含游戏安装入口与个人服务的 Android 安装包.
# Embedded 把 CDN 附在 APK 尾部, External 生成标准安装 APK 和由系统文件选择器导入的资源文档.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InputApkPath,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$SourceCdnRoot,
    [string]$AndroidSdkRoot,
    [string]$StarviewToolsRoot,
    [string]$JavaHome,
    [string]$CargoTargetDirectory,
    [string]$KeystorePath,
    [string]$KeystorePasswordFile,
    [string]$CompanionKeystorePath,
    [string]$CompanionKeystorePassword,
    [ValidateSet('Embedded', 'External')][string]$ResourceDelivery = 'Embedded',
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ServiceBaselineRef,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$ReleaseVersion,
    [Parameter(Mandatory)][ValidateRange(1, [int]::MaxValue)][int]$VersionCode
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot "android-packaging-paths.psm1") -Force

$RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../.."))
& (Join-Path $PSScriptRoot 'check-personal-service-web.ps1') -RepositoryRoot $RepositoryRoot
$WorkspaceRoot = Resolve-AndroidPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
$ServiceBaseline = Assert-AndroidPersonalServiceBaseline `
    -RepositoryRoot $RepositoryRoot `
    -BaselineRef $ServiceBaselineRef
$SourceCdnRoot = Resolve-AndroidPackagingCdnRoot `
    -RepositoryRoot $RepositoryRoot `
    -WorkspaceRoot $WorkspaceRoot `
    -ExplicitPath $SourceCdnRoot
$InputApkPath = [IO.Path]::GetFullPath($InputApkPath)
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path -LiteralPath $InputApkPath -PathType Leaf)) {
    throw "CN Android APK 不存在: $InputApkPath"
}
if (-not (Test-Path -LiteralPath $SourceCdnRoot -PathType Container)) {
    throw "CN Android CDN 不存在: $SourceCdnRoot"
}
if (Test-Path -LiteralPath $OutputDirectory) {
    throw "Android 单 APK 输出目录已存在: $OutputDirectory"
}

$PrepareData = Join-Path $PSScriptRoot "prepare-android-cn-data.py"
$PackageGame = Join-Path $PSScriptRoot "package-android-cn-game.ps1"
$AppendBundle = Join-Path $PSScriptRoot "append-android-apk-bundle.py"
$InspectBundle = Join-Path $PSScriptRoot "inspect-android-apk-bundle.py"
$PackageResources = Join-Path $PSScriptRoot 'package-android-resource-document.py'
$BuildCompanion = Join-Path $RepositoryRoot "platforms/android/companion/build-companion-apk.ps1"
foreach ($RequiredFile in @($PrepareData, $PackageGame, $AppendBundle, $InspectBundle, $PackageResources, $BuildCompanion)) {
    if (-not (Test-Path -LiteralPath $RequiredFile -PathType Leaf)) {
        throw "Android 单 APK 构建依赖不存在: $RequiredFile"
    }
}

# //// 执行 Python 工具并解析 JSON 报告 [@x380kkm 2026-08-31] ////
function Invoke-AndroidBundlePython {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string[]]$ArgumentList,
        [Parameter(Mandatory)][string]$LogPath
    )

    $Output = @(
        & uv run --python 3.12 python $ScriptPath @ArgumentList 2>&1 |
            ForEach-Object { $_.ToString() }
    )
    $ExitCode = $LASTEXITCODE
    $Output | Set-Content -LiteralPath $LogPath -Encoding UTF8
    if ($ExitCode -ne 0) {
        throw "Android 单 APK Python 工具失败: exit=$ExitCode script=$ScriptPath log=$LogPath"
    }
    $JsonStart = -1
    for ($Index = 0; $Index -lt $Output.Count; $Index++) {
        if ($Output[$Index].TrimStart().StartsWith("{")) {
            $JsonStart = $Index
            break
        }
    }
    if ($JsonStart -lt 0) {
        throw "Android 单 APK Python 工具没有返回 JSON: $ScriptPath"
    }
    ($Output[$JsonStart..($Output.Count - 1)] -join "`n") | ConvertFrom-Json
}
# //// /执行 Python 工具并解析 JSON 报告 ////

# //// 执行 PowerShell 工具并保存报告 [@x380kkm 2026-08-31] ////
function Invoke-AndroidBundlePowerShell {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][hashtable]$Parameters,
        [Parameter(Mandatory)][string]$LogPath
    )

    $Output = @(& $ScriptPath @Parameters 2>&1 | ForEach-Object { $_.ToString() })
    $ExitCode = $LASTEXITCODE
    $Output | Set-Content -LiteralPath $LogPath -Encoding UTF8
    if ($ExitCode -ne 0) {
        throw "Android 单 APK PowerShell 工具失败: exit=$ExitCode script=$ScriptPath log=$LogPath"
    }
    $Output
}
# //// /执行 PowerShell 工具并保存报告 ////

$OutputParent = Split-Path -Parent $OutputDirectory
New-Item -ItemType Directory -Force -Path $OutputParent | Out-Null
$BuildDirectory = "$OutputDirectory.build"
if (Test-Path -LiteralPath $BuildDirectory) {
    throw "Android 单 APK 工作目录已存在: $BuildDirectory"
}
New-Item -ItemType Directory -Path $BuildDirectory | Out-Null
$Logs = Join-Path $BuildDirectory "logs"
New-Item -ItemType Directory -Path $Logs | Out-Null

# //// 生成版本化 CDN 数据 [@x380kkm 2026-08-31] ////
$CdnOutput = Join-Path $BuildDirectory "cdn"
$CdnReport = Invoke-AndroidBundlePython `
        -ScriptPath $PrepareData `
        -ArgumentList @("--source", $SourceCdnRoot, "--output", $CdnOutput) `
        -LogPath (Join-Path $Logs "prepare-android-cn-data.log")
$CdnDataDirectory = [IO.Path]::GetFullPath([string]$CdnReport.data_directory)
$CdnManifestPath = [IO.Path]::GetFullPath([string]$CdnReport.manifest_file)
# //// /生成版本化 CDN 数据 ////

# //// 构建回环游戏 APK [@x380kkm 2026-08-31] ////
$GameBuildDirectory = Join-Path $BuildDirectory "game"
$GameApkPath = Join-Path $GameBuildDirectory "StarpointCN-Android.apk"
$GameParameters = @{
        InputApkPath = $InputApkPath
        OutputApkPath = $GameApkPath
        WorkingDirectory = $GameBuildDirectory
        GameVersionCode = 1008001 + $VersionCode
        ReleaseVersion = $ReleaseVersion
        CdnRoot = $SourceCdnRoot
    }
foreach ($Optional in @(
        @{ Name = "StarviewToolsRoot"; Value = $StarviewToolsRoot },
        @{ Name = "JavaHome"; Value = $JavaHome },
        @{ Name = "KeystorePath"; Value = $KeystorePath },
        @{ Name = "KeystorePasswordFile"; Value = $KeystorePasswordFile }
    )) {
        if (-not [string]::IsNullOrWhiteSpace($Optional.Value)) {
            $GameParameters[$Optional.Name] = $Optional.Value
        }
}
Invoke-AndroidBundlePowerShell `
        -ScriptPath $PackageGame `
        -Parameters $GameParameters `
        -LogPath (Join-Path $Logs "package-android-game.log") | Out-Null
$GameReport = Get-Content `
        -LiteralPath (Join-Path $GameBuildDirectory "package-report.json") `
        -Raw `
        -Encoding UTF8 | ConvertFrom-Json
# //// /构建回环游戏 APK ////

# //// 构建嵌入游戏的个人服务 APK [@x380kkm 2026-08-31] ////
$CompanionBuildDirectory = Join-Path $BuildDirectory "companion"
$CompanionParameters = @{
        CdnManifestPath = $CdnManifestPath
        GameApkPath = $GameApkPath
        OutputDirectory = $CompanionBuildDirectory
        ServiceBaselineRef = $ServiceBaselineRef
        VersionCode = $VersionCode
        VersionName = $ReleaseVersion
        ResourceDelivery = $ResourceDelivery
    }
foreach ($Optional in @(
        @{ Name = "AndroidSdkRoot"; Value = $AndroidSdkRoot },
        @{ Name = "CargoTargetDirectory"; Value = $CargoTargetDirectory },
        @{ Name = "KeystorePath"; Value = $CompanionKeystorePath },
        @{ Name = "KeystorePassword"; Value = $CompanionKeystorePassword }
    )) {
        if (-not [string]::IsNullOrWhiteSpace($Optional.Value)) {
            $CompanionParameters[$Optional.Name] = $Optional.Value
        }
}
Invoke-AndroidBundlePowerShell `
        -ScriptPath $BuildCompanion `
        -Parameters $CompanionParameters `
        -LogPath (Join-Path $Logs "build-android-companion.log") | Out-Null
$CompanionReport = Get-Content `
        -LiteralPath (Join-Path $CompanionBuildDirectory "companion-distribution.json") `
        -Raw `
        -Encoding UTF8 | ConvertFrom-Json
    $CompanionApkPath = if ([IO.Path]::IsPathRooted([string]$CompanionReport.apk.path)) {
        [IO.Path]::GetFullPath([string]$CompanionReport.apk.path)
    } else {
        [IO.Path]::GetFullPath((Join-Path $CompanionBuildDirectory $CompanionReport.apk.path))
    }
$ExpectedSignature = if ($ResourceDelivery -eq 'External') { 'v1+v2' } else { 'v1' }
$ExpectedMinimumApi = if ($ResourceDelivery -eq 'Embedded') { 31 } else { 26 }
if ($CompanionReport.package_id -cne "dev.starpoint.personalservice" `
    -or $CompanionReport.endpoint -cne "http://127.0.0.1:17171" `
    -or $CompanionReport.lifecycle.target_sdk -ne 29 `
    -or $CompanionReport.lifecycle.minimum_sdk -ne $ExpectedMinimumApi `
    -or $CompanionReport.lifecycle.signature_scheme -cne $ExpectedSignature `
    -or $CompanionReport.lifecycle.resource_delivery -cne $ResourceDelivery.ToLowerInvariant() `
    -or $CompanionReport.game.sha256 -cne $GameReport.Output.Sha256 `
    -or $CompanionReport.cdn.manifest_sha256 -cne $CdnReport.manifest_sha256 `
    -or $CompanionReport.source.personal_service_tree -cne $ServiceBaseline.personal_service_tree `
    -or $CompanionReport.source.personal_service_baseline_tree -cne $ServiceBaseline.personal_service_baseline_tree `
    -or $CompanionReport.source.assets_tree -cne $ServiceBaseline.assets_tree `
    -or $CompanionReport.source.assets_baseline_tree -cne $ServiceBaseline.assets_baseline_tree `
    -or $CompanionReport.source.service_baseline_ref -cne $ServiceBaseline.service_baseline_ref `
    -or $CompanionReport.source.service_baseline_commit -cne $ServiceBaseline.service_baseline_commit `
    -or -not $CompanionReport.source.personal_service_consistent `
    -or -not $CompanionReport.source.assets_consistent `
    -or -not $CompanionReport.source.working_tree_clean) {
    throw "Android 单 APK 伴随服务契约无效."
}
if (-not (Test-Path -LiteralPath $CompanionApkPath -PathType Leaf)) {
    throw "Android 单 APK 伴随服务输出不存在: $CompanionApkPath"
}
# //// /构建嵌入游戏的个人服务 APK ////

# //// 按资源分发模式组装安装文件 [@x380kkm 2026-09-26] ////
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$ArtifactName = "sp-$ReleaseVersion.apk"
$CompleteApkPath = Join-Path $OutputDirectory $ArtifactName
$CdnDelivery = [ordered]@{
    manifest_sha256 = $CdnReport.manifest_sha256
    files = $CdnReport.files
    bytes = $CdnReport.bytes
}
$ExpectedFiles = @($CompleteApkPath)
if ($ResourceDelivery -eq 'External') {
    Copy-Item -LiteralPath $CompanionApkPath -Destination $CompleteApkPath
    $ResourceArtifactName = "sp-$ReleaseVersion.starpoint-resources"
    $ResourcePath = Join-Path $OutputDirectory $ResourceArtifactName
    $ResourceReport = Invoke-AndroidBundlePython `
        -ScriptPath $PackageResources `
        -ArgumentList @('--base-apk', $CompanionApkPath, '--cdn-root', $CdnDataDirectory, '--output-resources', $ResourcePath) `
        -LogPath (Join-Path $Logs 'package-android-resource-document.log')
    if ($ResourceReport.payload.files -ne $CdnReport.files `
        -or $ResourceReport.payload.bytes -ne $CdnReport.bytes `
        -or $ResourceReport.manifest_sha256 -cne $CdnReport.manifest_sha256) {
        throw 'Android 独立资源报告与签名清单不一致.'
    }
    $ApkReport = $CompanionReport.apk
    $DistributionMode = 'apk-and-resource-document'
    $CdnDelivery.payload_bytes = $ResourceReport.payload.bytes
    $CdnDelivery.verified = $ResourceReport.digests_verified
    $CdnDelivery.resource_document = [ordered]@{
        path = $ResourceArtifactName
        bytes = $ResourceReport.resources.bytes
        sha256 = $ResourceReport.resources.sha256
    }
    $ExpectedFiles += $ResourcePath
} else {
    $BundleReport = Invoke-AndroidBundlePython `
        -ScriptPath $AppendBundle `
        -ArgumentList @(
            "--base-apk", $CompanionApkPath,
            "--cdn-root", $CdnDataDirectory,
            "--output-apk", $CompleteApkPath
        ) `
        -LogPath (Join-Path $Logs "append-android-apk-bundle.log")
    $InspectReport = Invoke-AndroidBundlePython `
        -ScriptPath $InspectBundle `
        -ArgumentList @("--bundle", $CompleteApkPath, "--skip-digests") `
        -LogPath (Join-Path $Logs "inspect-android-apk-bundle.log")
    if ($InspectReport.payload.files -ne $CdnReport.files `
        -or $InspectReport.payload.bytes -ne $CdnReport.bytes `
        -or $InspectReport.manifest_sha256 -cne $CdnReport.manifest_sha256) {
        throw "Android 单 APK CDN 尾随 payload 报告不一致."
    }
    $ApkReport = $BundleReport.apk
    $DistributionMode = 'single-apk-tail-cdn'
    $CdnDelivery.payload_bytes = $BundleReport.payload.bytes
    $CdnDelivery.footer = $BundleReport.footer
    $CdnDelivery.verified = $InspectReport.digests_verified
}
# //// /按资源分发模式组装安装文件 ////

$Distribution = [ordered]@{
        schema_version = 3
        platform = "android"
        mode = $DistributionMode
        endpoint = "http://127.0.0.1:17171"
        source = [ordered]@{
            repository_root = $RepositoryRoot
            workspace_root = $WorkspaceRoot
            personal_service = $ServiceBaseline
            assets_consistent = $ServiceBaseline.assets_consistent
        }
        apk = [ordered]@{
            path = $ArtifactName
            bytes = $ApkReport.bytes
            sha256 = $ApkReport.sha256
            version_name = $ReleaseVersion
            version_code = $VersionCode
        }
        companion = [ordered]@{
            package_id = "dev.starpoint.personalservice"
            minimum_sdk = $CompanionReport.lifecycle.minimum_sdk
            target_sdk = $CompanionReport.lifecycle.target_sdk
            embedded_game_asset = $CompanionReport.game.asset
        }
        game = [ordered]@{
            package_id = "com.leiting.wf"
            source_sha256 = $GameReport.Output.Sha256
            embedded_sha256 = $CompanionReport.game.sha256
        }
        cdn = $CdnDelivery
    }
$OutputFiles = @(Get-ChildItem -LiteralPath $OutputDirectory -File)
if ($OutputFiles.Count -ne $ExpectedFiles.Count -or @($OutputFiles | Where-Object { $_.FullName -cnotin $ExpectedFiles }).Count -gt 0) {
    throw "Android 分发输出目录包含意外文件: $($OutputFiles.Name -join ', ')"
}
$ResolvedBuildDirectory = [IO.Path]::GetFullPath($BuildDirectory)
$ExpectedBuildDirectory = [IO.Path]::GetFullPath("$OutputDirectory.build")
if ($ResolvedBuildDirectory -cne $ExpectedBuildDirectory -or -not $ResolvedBuildDirectory.EndsWith(".build", [StringComparison]::OrdinalIgnoreCase)) {
    throw "Android 单 APK 工作目录边界无效: $ResolvedBuildDirectory"
}
[IO.Directory]::Delete($ResolvedBuildDirectory, $true)
$Distribution | ConvertTo-Json -Depth 12
