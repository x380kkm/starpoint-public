# audience: internal
# # package-android-cn-game
#
# 该脚本将 CN 客户端固定到回环个人服务, 跳过 delegated SDK 初始化, 保持单一主 DEX, 再对齐并签名输出.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InputApkPath,
    [string]$OutputApkPath,
    [string]$WorkingDirectory,
    [string]$StarviewToolsRoot,
    [string]$JavaHome,
    [string]$KeystorePath,
    [string]$KeystorePasswordFile
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem
Import-Module (Join-Path $PSScriptRoot "android-packaging-paths.psm1") -Force

$RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../.."))
$WorkspaceRoot = Resolve-AndroidPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
$InputApkPath = [IO.Path]::GetFullPath($InputApkPath)
if (-not (Test-Path -LiteralPath $InputApkPath -PathType Leaf)) {
    throw "CN APK 不存在: $InputApkPath"
}
if ([string]::IsNullOrWhiteSpace($StarviewToolsRoot)) {
    $StarviewToolsRoot = Join-Path $WorkspaceRoot "artifacts/starview-tools/starview-windows"
}
if ([string]::IsNullOrWhiteSpace($JavaHome)) {
    $JavaHome = Split-Path -Parent (Split-Path -Parent (Get-Command java -ErrorAction Stop).Source)
}
if ([string]::IsNullOrWhiteSpace($WorkingDirectory)) {
    $Timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
    $WorkingDirectory = Join-Path $WorkspaceRoot "artifacts/protocol-lab/android-game/$Timestamp"
}
$WorkingDirectory = [IO.Path]::GetFullPath($WorkingDirectory)
if (Test-Path -LiteralPath $WorkingDirectory) {
    throw "Android 游戏包工作目录已存在: $WorkingDirectory"
}
if ([string]::IsNullOrWhiteSpace($OutputApkPath)) {
    $OutputApkPath = Join-Path $WorkingDirectory "StarpointCN-Android.apk"
}
$OutputApkPath = [IO.Path]::GetFullPath($OutputApkPath)
if (Test-Path -LiteralPath $OutputApkPath) {
    throw "Android 游戏 APK 已存在: $OutputApkPath"
}
if ([string]::IsNullOrWhiteSpace($KeystorePath)) {
    $KeystorePath = Join-Path $WorkspaceRoot "artifacts/protocol-lab/signing/cn-client-runtime.p12"
}
if ([string]::IsNullOrWhiteSpace($KeystorePasswordFile)) {
    $KeystorePasswordFile = Join-Path $WorkspaceRoot "artifacts/protocol-lab/signing/cn-client-runtime.pass"
}
$StarviewToolsRoot = [IO.Path]::GetFullPath($StarviewToolsRoot)
$JavaHome = [IO.Path]::GetFullPath($JavaHome)
$KeystorePath = [IO.Path]::GetFullPath($KeystorePath)
$KeystorePasswordFile = [IO.Path]::GetFullPath($KeystorePasswordFile)

$Java = Join-Path $JavaHome "bin/java.exe"
$Aapt = Join-Path $StarviewToolsRoot "build-tools/aapt.exe"
$Zipalign = Join-Path $StarviewToolsRoot "build-tools/zipalign.exe"
$ApkSignerJar = Join-Path $StarviewToolsRoot "build-tools/lib/apksigner.jar"
$PatchClient = Join-Path $PSScriptRoot "patch-cn-client.ps1"
$PatchSdkBootstrap = Join-Path $PSScriptRoot "patch-android-sdk-bootstrap.py"
$Uv = (Get-Command uv -ErrorAction Stop).Source
foreach ($RequiredFile in @(
    $Java,
    $Aapt,
    $Zipalign,
    $ApkSignerJar,
    $PatchClient,
    $PatchSdkBootstrap
)) {
    if (-not (Test-Path -LiteralPath $RequiredFile -PathType Leaf)) {
        throw "Android 游戏包依赖不存在: $RequiredFile"
    }
}

# //// 执行外部工具并保存输出 [@x380kkm 2026-08-31] ////
function Invoke-AndroidGameTool {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$ArgumentList,
        [Parameter(Mandatory)][string]$LogPath
    )

    $Output = @(& $FilePath @ArgumentList 2>&1 | ForEach-Object { $_.ToString() })
    $ExitCode = $LASTEXITCODE
    $Output | Set-Content -LiteralPath $LogPath -Encoding UTF8
    if ($ExitCode -ne 0) {
        throw "Android 游戏包工具失败: exit=$ExitCode tool=$FilePath log=$LogPath"
    }
    $Output
}
# //// /执行外部工具并保存输出 ////

# //// 提取 APK 中的唯一条目 [@x380kkm 2026-08-31] ////
function Export-AndroidGameEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$PackagePath,
        [Parameter(Mandatory)][string]$EntryName,
        [Parameter(Mandatory)][string]$OutputPath
    )

    $Archive = [IO.Compression.ZipFile]::OpenRead($PackagePath)
    try {
        $Entries = @($Archive.Entries | Where-Object FullName -CEQ $EntryName)
        if ($Entries.Count -ne 1) {
            throw "APK 条目数量不正确: entry=$EntryName count=$($Entries.Count)"
        }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($Entries[0], $OutputPath, $false)
    } finally {
        $Archive.Dispose()
    }
}
# //// /提取 APK 中的唯一条目 ////

# //// 判断 ZIP 条目是否属于 APK v1 签名 [@x380kkm 2026-08-31] ////
function Test-AndroidGameSignatureEntry {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$EntryName)

    if (-not $EntryName.StartsWith("META-INF/", [StringComparison]::OrdinalIgnoreCase)) {
        return $false
    }
    $LeafName = $EntryName.Substring($EntryName.LastIndexOf("/") + 1)
    $LeafName -match "(?i)^(MANIFEST\.MF|SIG-.*|.*\.(SF|RSA|DSA|EC))$"
}
# //// /判断 ZIP 条目是否属于 APK v1 签名 ////

# //// 重建 APK 并替换主 DEX [@x380kkm 2026-08-31] ////
function New-AndroidGameUnsignedApk {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$InputApkPath,
        [Parameter(Mandatory)][string]$PatchedDexPath,
        [Parameter(Mandatory)][string]$OutputApkPath
    )

    Copy-Item -LiteralPath $InputApkPath -Destination $OutputApkPath
    $Archive = [IO.Compression.ZipFile]::Open(
        $OutputApkPath,
        [IO.Compression.ZipArchiveMode]::Update
    )
    $ReplacedDexEntries = 0
    $RemovedSignatureEntries = 0
    try {
        foreach ($Entry in @($Archive.Entries)) {
            if (Test-AndroidGameSignatureEntry -EntryName $Entry.FullName) {
                $Entry.Delete()
                $RemovedSignatureEntries++
                continue
            }
            if ($Entry.FullName -ceq "classes.dex") {
                $Entry.Delete()
                $ReplacedDexEntries++
            }
        }
        $PatchedDexEntry = [IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $Archive,
            $PatchedDexPath,
            "classes.dex",
            [IO.Compression.CompressionLevel]::NoCompression
        )
        $PatchedDexEntry.LastWriteTime = (Get-Item -LiteralPath $PatchedDexPath).LastWriteTime
    } finally {
        $Archive.Dispose()
    }

    if ($ReplacedDexEntries -ne 1) {
        throw "主 DEX 替换数量不正确: expected=1 actual=$ReplacedDexEntries"
    }
    [ordered]@{
        ReplacedDexEntries = $ReplacedDexEntries
        RemovedSignatureEntries = $RemovedSignatureEntries
    }
}
# //// /重建 APK 并替换主 DEX ////

# //// 验证游戏 APK 的包名和主 Activity [@x380kkm 2026-08-31] ////
function Assert-AndroidGameManifest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$PackagePath,
        [Parameter(Mandatory)][string]$LogPath
    )

    $Dump = @(Invoke-AndroidGameTool -FilePath $Aapt -ArgumentList @(
        "dump", "xmltree", $PackagePath, "AndroidManifest.xml"
    ) -LogPath $LogPath)
    if (@($Dump | Where-Object { $_ -match '^\s+A: package="com\.leiting\.wf"' }).Count -ne 1) {
        throw "Android 游戏包名不是 com.leiting.wf."
    }

    $ActivityNameIndex = -1
    for ($Index = 0; $Index -lt $Dump.Count; $Index++) {
        if ($Dump[$Index] -match 'android:name.*"air\.com\.leiting\.wf\.AppEntry"') {
            $ActivityNameIndex = $Index
            break
        }
    }
    if ($ActivityNameIndex -lt 0) {
        throw "Android 游戏包缺少 AppEntry."
    }
    $ActivityStart = $ActivityNameIndex
    while ($ActivityStart -ge 0 -and $Dump[$ActivityStart] -notmatch '^\s{6}E: activity') {
        $ActivityStart--
    }
    if ($ActivityStart -lt 0) {
        throw "Android 游戏包 AppEntry 边界无效."
    }
    $ActivityEnd = $Dump.Count
    for ($Index = $ActivityNameIndex + 1; $Index -lt $Dump.Count; $Index++) {
        if ($Dump[$Index] -match '^\s{6}E: ') {
            $ActivityEnd = $Index
            break
        }
    }
    $ActivityBlock = @($Dump[$ActivityStart..($ActivityEnd - 1)])
    if (@($ActivityBlock | Where-Object { $_ -match 'android:screenOrientation.*\(type 0x10\)0x1$' }).Count -ne 1) {
        throw "Android 游戏包 AppEntry 没有保持 portrait."
    }
    if (@($ActivityBlock | Where-Object { $_ -match 'android:configChanges.*\(type 0x11\)0x40000da0$' }).Count -ne 1) {
        throw "Android 游戏包 AppEntry configChanges 不正确."
    }

    [ordered]@{
        PackageId = "com.leiting.wf"
        MainActivity = "air.com.leiting.wf.AppEntry"
        ScreenOrientation = "portrait"
        ConfigChanges = "0x40000da0"
    }
}
# //// /验证游戏 APK 的包名和主 Activity ////

# //// 验证伴随服务游戏包的 DEX 和条目结构 [@x380kkm 2026-08-31] ////
function Assert-AndroidGamePackage {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$PackagePath)

    $Archive = [IO.Compression.ZipFile]::OpenRead($PackagePath)
    try {
        $Names = @($Archive.Entries | ForEach-Object FullName)
        foreach ($RequiredEntry in @("classes.dex", "assets/worldflipper_android_release.swf")) {
            if ($Names -cnotcontains $RequiredEntry) {
                throw "Android 游戏包缺少条目: $RequiredEntry"
            }
        }
        $TopLevelDexEntries = @($Names | Where-Object { $_ -match '^classes(?:[0-9]+)?\.dex$' })
        if ($TopLevelDexEntries.Count -ne 1 -or $TopLevelDexEntries[0] -cne "classes.dex") {
            throw "Android 游戏包顶层 DEX 结构无效: $($TopLevelDexEntries -join ', ')"
        }
        $ProcessRuntimeEntries = @($Names | Where-Object {
            $_ -ceq "lib/arm64-v8a/libstarpoint_android_bridge.so" -or
            $_.StartsWith("assets/META-INF/AIR/extensions/dev.starpoint.personalservice/", [StringComparison]::Ordinal)
        })
        if ($ProcessRuntimeEntries.Count -ne 0) {
            throw "Android 游戏包包含进程内个人服务条目: $($ProcessRuntimeEntries -join ', ')"
        }
        [ordered]@{
            TopLevelDexEntries = $TopLevelDexEntries
            RequiredEntries = @("classes.dex", "assets/worldflipper_android_release.swf")
        }
    } finally {
        $Archive.Dispose()
    }
}
# //// /验证伴随服务游戏包的 DEX 和条目结构 ////

New-Item -ItemType Directory -Force -Path $WorkingDirectory, (Split-Path -Parent $OutputApkPath) | Out-Null
$Logs = Join-Path $WorkingDirectory "logs"
New-Item -ItemType Directory -Force -Path $Logs | Out-Null
$EndpointPatchDirectory = Join-Path $WorkingDirectory "endpoint-patch"
$EndpointPatchedApk = Join-Path $EndpointPatchDirectory "worldflipper-cn-loopback.apk"
$OriginalPrimaryDex = Join-Path $WorkingDirectory "classes-original.dex"
$PatchedPrimaryDex = Join-Path $WorkingDirectory "classes-sdk-silent.dex"
$SdkBootstrapEvidence = Join-Path $WorkingDirectory "sdk-bootstrap-evidence.json"
$UnsignedApk = Join-Path $WorkingDirectory "game-unsigned.apk"
$AlignedApk = Join-Path $WorkingDirectory "game-aligned.apk"

# //// 生成回环端点客户端并应用 delegated SDK 补丁 [@x380kkm 2026-08-31] ////
& $PatchClient `
    -InputApkPath $InputApkPath `
    -OutputApkPath $EndpointPatchedApk `
    -WorkingDirectory $EndpointPatchDirectory `
    -StarviewToolsRoot $StarviewToolsRoot `
    -JavaHome $JavaHome `
    -ServerHost "127.0.0.1" `
    -Port 17171 `
    -EmbeddedPersonalService `
    -KeystorePath $KeystorePath `
    -KeystorePasswordFile $KeystorePasswordFile | Out-Null
$InputManifestContract = Assert-AndroidGameManifest `
    -PackagePath $EndpointPatchedApk `
    -LogPath (Join-Path $Logs "aapt-manifest-input.log")
Export-AndroidGameEntry `
    -PackagePath $EndpointPatchedApk `
    -EntryName "classes.dex" `
    -OutputPath $OriginalPrimaryDex
Invoke-AndroidGameTool -FilePath $Uv -ArgumentList @(
    "run", "--python", "3.12", "python", $PatchSdkBootstrap,
    "--mode", "skip-delegated-sdk-oncreate",
    "--input", $OriginalPrimaryDex,
    "--output", $PatchedPrimaryDex,
    "--report", $SdkBootstrapEvidence
) -LogPath (Join-Path $Logs "patch-sdk-bootstrap.log") | Out-Null
# //// /生成回环端点客户端并应用 delegated SDK 补丁 ////

# //// 重建, 对齐并签名游戏 APK [@x380kkm 2026-08-31] ////
$Repack = New-AndroidGameUnsignedApk `
    -InputApkPath $EndpointPatchedApk `
    -PatchedDexPath $PatchedPrimaryDex `
    -OutputApkPath $UnsignedApk
Invoke-AndroidGameTool -FilePath $Zipalign -ArgumentList @(
    "-f", "-p", "4", $UnsignedApk, $AlignedApk
) -LogPath (Join-Path $Logs "zipalign.log") | Out-Null
if (-not (Test-Path -LiteralPath $KeystorePath -PathType Leaf)) {
    throw "端点补丁没有生成测试 keystore: $KeystorePath"
}
if (-not (Test-Path -LiteralPath $KeystorePasswordFile -PathType Leaf)) {
    throw "端点补丁没有生成测试 keystore 口令文件: $KeystorePasswordFile"
}
$Password = [IO.File]::ReadAllText($KeystorePasswordFile, [Text.Encoding]::UTF8).Trim()
$PasswordEnvironmentVariable = "STARPOINT_ANDROID_GAME_KEYSTORE_PASSWORD"
$PreviousPassword = [Environment]::GetEnvironmentVariable($PasswordEnvironmentVariable, "Process")
try {
    [Environment]::SetEnvironmentVariable($PasswordEnvironmentVariable, $Password, "Process")
    Invoke-AndroidGameTool -FilePath $Java -ArgumentList @(
        "-jar", $ApkSignerJar, "sign",
        "--ks", $KeystorePath,
        "--ks-type", "PKCS12",
        "--ks-key-alias", "starpoint-cn-runtime",
        "--ks-pass", "env:$PasswordEnvironmentVariable",
        "--key-pass", "env:$PasswordEnvironmentVariable",
        "--v4-signing-enabled", "false",
        "--out", $OutputApkPath,
        $AlignedApk
    ) -LogPath (Join-Path $Logs "apksigner-sign.log") | Out-Null
} finally {
    [Environment]::SetEnvironmentVariable($PasswordEnvironmentVariable, $PreviousPassword, "Process")
}
Invoke-AndroidGameTool -FilePath $Java -ArgumentList @(
    "-jar", $ApkSignerJar, "verify", "--verbose", "--print-certs", $OutputApkPath
) -LogPath (Join-Path $Logs "apksigner-verify.log") | Out-Null
Invoke-AndroidGameTool -FilePath $Zipalign -ArgumentList @(
    "-c", "-p", "-v", "4", $OutputApkPath
) -LogPath (Join-Path $Logs "zipalign-verify.log") | Out-Null
# //// /重建, 对齐并签名游戏 APK ////

# //// 写入游戏包构建报告 [@x380kkm 2026-08-31] ////
$OutputManifestContract = Assert-AndroidGameManifest `
    -PackagePath $OutputApkPath `
    -LogPath (Join-Path $Logs "aapt-manifest-output.log")
$PackageContract = Assert-AndroidGamePackage -PackagePath $OutputApkPath
$PackageReport = [ordered]@{
    SchemaVersion = 1
    Mode = "companion-personal-service"
    Endpoint = "http://127.0.0.1:17171"
    Input = [ordered]@{
        Path = $InputApkPath
        Sha256 = (Get-FileHash -LiteralPath $InputApkPath -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    Repack = $Repack
    SdkBootstrapEvidence = $SdkBootstrapEvidence
    AndroidManifest = [ordered]@{
        Input = $InputManifestContract
        Output = $OutputManifestContract
    }
    Package = $PackageContract
    Output = [ordered]@{
        Path = $OutputApkPath
        Bytes = (Get-Item -LiteralPath $OutputApkPath).Length
        Sha256 = (Get-FileHash -LiteralPath $OutputApkPath -Algorithm SHA256).Hash.ToLowerInvariant()
        SignatureEvidence = Join-Path $Logs "apksigner-verify.log"
        AlignmentEvidence = Join-Path $Logs "zipalign-verify.log"
    }
}
$PackageReportPath = Join-Path $WorkingDirectory "package-report.json"
$PackageReport | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $PackageReportPath -Encoding UTF8
$PackageReport
# //// /写入游戏包构建报告 ////
