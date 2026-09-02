# audience: external
# # build-android-personal-service-companion
#
# 该脚本在 Windows 上构建 ARM64 游戏安装入口和个人服务前台宿主, 嵌入已签名游戏 APK 与 CDN 清单.
# targetSdkVersion 29 和 v1 签名允许分发包在 ZIP 末尾追加 CDN 数据.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$CdnManifestPath,
    [Parameter(Mandatory)][string]$GameApkPath,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$AndroidSdkRoot,
    [string]$CargoTargetDirectory,
    [string]$KeystorePath,
    [string]$KeystorePassword,
    [string]$KeyAlias = "starpoint-companion",
    [string]$KeyPassword,
    [string]$NdkVersion = "29.0.14206865",
    [string]$BuildToolsVersion = "35.0.0",
    [ValidateRange(26, 35)][int]$MinimumApi = 26,
    [ValidateRange(26, 35)][int]$TargetApi = 29,
    [ValidateRange(26, 35)][int]$PlatformApi = 35,
    [string]$RustToolchain = "1.78.0",
    [ValidateNotNullOrEmpty()][string]$ServiceBaselineRef = "v-beta-0.0.15",
    [ValidateRange(1, [int]::MaxValue)][int]$VersionCode = 1,
    [string]$VersionName = "1.0.0"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression.FileSystem
Import-Module (Join-Path $PSScriptRoot "../../../scripts/protocol-lab/android-packaging-paths.psm1") -Force

$AndroidRoot = Split-Path -Parent $PSScriptRoot
$RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $AndroidRoot "../.."))
$WorkspaceRoot = Resolve-AndroidPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
$ServiceBaseline = Assert-AndroidPersonalServiceBaseline `
    -RepositoryRoot $RepositoryRoot `
    -BaselineRef $ServiceBaselineRef
foreach ($RequiredBaselineField in @(
    "personal_service_tree",
    "personal_service_baseline_tree",
    "assets_tree",
    "assets_baseline_tree",
    "assets_consistent",
    "working_tree_clean"
)) {
    if ($null -eq $ServiceBaseline[$RequiredBaselineField]) {
        throw "Android 共享输入来源报告缺少字段: $RequiredBaselineField"
    }
}
if (-not $ServiceBaseline.assets_consistent -or -not $ServiceBaseline.working_tree_clean) {
    throw "Android 共享输入来源未通过 iOS tag 一致性核对."
}
$CompanionHostRoot = Join-Path $AndroidRoot "CompanionHost"
$CoreRoot = Join-Path $RepositoryRoot "core/personal-service"
$ResolvedManifestPath = [IO.Path]::GetFullPath($CdnManifestPath)
$ResolvedGameApkPath = [IO.Path]::GetFullPath($GameApkPath)
$ResolvedOutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)

if (-not (Test-Path -LiteralPath $ResolvedManifestPath -PathType Leaf)) {
    throw "CDN 清单不存在: $ResolvedManifestPath"
}
if ((Get-Item -LiteralPath $ResolvedManifestPath).Length -eq 0) {
    throw "CDN 清单为空: $ResolvedManifestPath"
}
if (-not (Test-Path -LiteralPath $ResolvedGameApkPath -PathType Leaf)) {
    throw "内嵌游戏 APK 不存在: $ResolvedGameApkPath"
}
if ((Get-Item -LiteralPath $ResolvedGameApkPath).Length -eq 0) {
    throw "内嵌游戏 APK 为空: $ResolvedGameApkPath"
}
if ($TargetApi -lt $MinimumApi -or $TargetApi -gt $PlatformApi) {
    throw "Android API 关系无效: minimum=$MinimumApi target=$TargetApi platform=$PlatformApi"
}
if (Test-Path -LiteralPath $ResolvedOutputDirectory) {
    throw "伴随 APK 输出目录已存在: $ResolvedOutputDirectory"
}
if ([string]::IsNullOrWhiteSpace($AndroidSdkRoot)) {
    $AndroidSdkRoot = Join-Path $WorkspaceRoot "artifacts/android-sdk/sdk"
}
if ([string]::IsNullOrWhiteSpace($CargoTargetDirectory)) {
    $WorktreeName = Split-Path -Leaf $RepositoryRoot
    $CargoTargetDirectory = Join-Path $WorkspaceRoot "artifacts/android-cargo-target/$WorktreeName"
}
if ([string]::IsNullOrWhiteSpace($KeystorePath)) {
    $KeystorePath = Join-Path $WorkspaceRoot "artifacts/android-signing/starpoint-companion.keystore"
}
if ([string]::IsNullOrWhiteSpace($KeystorePassword)) {
    $KeystorePassword = [Environment]::GetEnvironmentVariable(
        "STARPOINT_ANDROID_KEYSTORE_PASSWORD",
        "Process"
    )
}
if ([string]::IsNullOrWhiteSpace($KeystorePassword)) {
    $KeystorePassword = "android"
}
if ([string]::IsNullOrWhiteSpace($KeyPassword)) {
    $KeyPassword = $KeystorePassword
}

$AndroidSdkRoot = [IO.Path]::GetFullPath($AndroidSdkRoot)
$CargoTargetDirectory = [IO.Path]::GetFullPath($CargoTargetDirectory)
$KeystorePath = [IO.Path]::GetFullPath($KeystorePath)

$BuildToolsRoot = Join-Path $AndroidSdkRoot "build-tools/$BuildToolsVersion"
$AndroidJar = Join-Path $AndroidSdkRoot "platforms/android-$PlatformApi/android.jar"
$Aapt2 = Join-Path $BuildToolsRoot "aapt2.exe"
$Zipalign = Join-Path $BuildToolsRoot "zipalign.exe"
$D8Jar = Join-Path $BuildToolsRoot "lib/d8.jar"
$ApkSignerJar = Join-Path $BuildToolsRoot "lib/apksigner.jar"
$NdkToolchain = Join-Path $AndroidSdkRoot "ndk/$NdkVersion/toolchains/llvm/prebuilt/windows-x86_64/bin"
$Clang = Join-Path $NdkToolchain "clang.exe"
$LlvmAr = Join-Path $NdkToolchain "llvm-ar.exe"
$LlvmNm = Join-Path $NdkToolchain "llvm-nm.exe"
$LlvmReadelf = Join-Path $NdkToolchain "llvm-readelf.exe"
$Java = (Get-Command java.exe -ErrorAction Stop).Source
$Javac = (Get-Command javac.exe -ErrorAction Stop).Source
$Jar = (Get-Command jar.exe -ErrorAction Stop).Source
$Keytool = (Get-Command keytool.exe -ErrorAction Stop).Source
$Cargo = (Get-Command cargo.exe -ErrorAction Stop).Source
$Rustup = (Get-Command rustup.exe -ErrorAction Stop).Source
$JniSource = Join-Path $CompanionHostRoot "native/starpoint_android_bridge.c"

foreach ($RequiredFile in @(
    $AndroidJar,
    $Aapt2,
    $Zipalign,
    $D8Jar,
    $ApkSignerJar,
    $Clang,
    $LlvmAr,
    $LlvmNm,
    $LlvmReadelf,
    $JniSource
)) {
    if (-not (Test-Path -LiteralPath $RequiredFile -PathType Leaf)) {
        throw "伴随 APK 构建依赖不存在: $RequiredFile"
    }
}

# //// 执行伴随 APK 构建工具并保存输出 [@x380kkm 2026-08-31] ////
function Invoke-CompanionTool {
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
        throw "伴随 APK 构建工具失败: exit=$ExitCode tool=$FilePath log=$LogPath"
    }
    $Output
}
# //// /执行伴随 APK 构建工具并保存输出 ////

# //// 向伴随 APK 写入唯一生成项 [@x380kkm 2026-08-31] ////
function Add-ApkEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][IO.Compression.ZipArchive]$Archive,
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$EntryName,
        [switch]$WithoutCompression
    )

    if ($null -ne $Archive.GetEntry($EntryName)) {
        throw "基础 APK 已包含生成项: $EntryName"
    }
    $CompressionLevel = if ($WithoutCompression) {
        [IO.Compression.CompressionLevel]::NoCompression
    } else {
        [IO.Compression.CompressionLevel]::Optimal
    }
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
        $Archive,
        $SourcePath,
        $EntryName,
        $CompressionLevel
    ) | Out-Null
}
# //// /向伴随 APK 写入唯一生成项 ////

$Logs = Join-Path $ResolvedOutputDirectory "logs"
$NativeDirectory = Join-Path $ResolvedOutputDirectory "native"
$JavaClassesDirectory = Join-Path $ResolvedOutputDirectory "java-classes"
$DexDirectory = Join-Path $ResolvedOutputDirectory "dex"
$AssetsDirectory = Join-Path $ResolvedOutputDirectory "assets/starpoint-personal-service-cdn"
$SourceReportPath = Join-Path $ResolvedOutputDirectory "starpoint-build-source.json"
New-Item -ItemType Directory -Force -Path @(
    $Logs,
    $NativeDirectory,
    $JavaClassesDirectory,
    $DexDirectory,
    $AssetsDirectory,
    $CargoTargetDirectory,
    (Split-Path -Parent $KeystorePath)
) | Out-Null
$ServiceBaseline | ConvertTo-Json -Depth 8 | Set-Content `
    -LiteralPath $SourceReportPath `
    -Encoding UTF8

$ManifestDigest = (Get-FileHash -LiteralPath $ResolvedManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$ManifestPrefix = $ManifestDigest.Substring(0, 16)
$GameDigest = (Get-FileHash -LiteralPath $ResolvedGameApkPath -Algorithm SHA256).Hash.ToLowerInvariant()
$GameVerification = Invoke-CompanionTool -FilePath $Java -ArgumentList @(
    "-jar",
    $ApkSignerJar,
    "verify",
    "--verbose",
    "--print-certs",
    $ResolvedGameApkPath
) -LogPath (Join-Path $Logs "game-apksigner-verify.log")
$GameSignerDigests = @(
    $GameVerification | ForEach-Object {
        if ($_ -match '^Signer #[0-9]+ certificate SHA-256 digest: ([0-9a-fA-F]{64})$') {
            $Matches[1].ToLowerInvariant()
        }
    } | Sort-Object -Unique
)
if ($GameSignerDigests.Count -eq 0) {
    throw "内嵌游戏 APK 没有可用的签名证书摘要: $ResolvedGameApkPath"
}
$GameSignersPath = Join-Path $ResolvedOutputDirectory "starpoint-game-signers.sha256"
(($GameSignerDigests -join "`n") + "`n") | Set-Content `
    -LiteralPath $GameSignersPath `
    -Encoding UTF8 `
    -NoNewline
$GameBadging = Invoke-CompanionTool -FilePath $Aapt2 -ArgumentList @(
    "dump",
    "badging",
    $ResolvedGameApkPath
) -LogPath (Join-Path $Logs "game-aapt2-badging.log")
if (($GameBadging -join "`n") -notmatch "package: name='com\.leiting\.wf'") {
    throw "内嵌游戏 APK 包名不是 com.leiting.wf: $ResolvedGameApkPath"
}
Copy-Item -LiteralPath $ResolvedManifestPath -Destination (Join-Path $AssetsDirectory "manifest.sha256")

# //// 构建 ARM64 Rust 核心和伴随服务 JNI [@x380kkm 2026-08-31] ////
$EnvironmentNames = @(
    "CARGO_TARGET_DIR",
    "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER",
    "CARGO_TARGET_AARCH64_LINUX_ANDROID_AR",
    "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
    "CC_aarch64_linux_android",
    "AR_aarch64_linux_android"
)
$OriginalEnvironment = @{}
foreach ($Name in $EnvironmentNames) {
    $OriginalEnvironment[$Name] = [Environment]::GetEnvironmentVariable($Name, "Process")
}
try {
    $env:CARGO_TARGET_DIR = $CargoTargetDirectory
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = $Clang
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_AR = $LlvmAr
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS =
        "-Clink-arg=--target=aarch64-linux-android$MinimumApi"
    $env:CC_aarch64_linux_android = "$Clang --target=aarch64-linux-android$MinimumApi"
    $env:AR_aarch64_linux_android = $LlvmAr
    Invoke-CompanionTool -FilePath $Rustup -ArgumentList @(
        "target", "add", "aarch64-linux-android", "--toolchain", $RustToolchain
    ) -LogPath (Join-Path $Logs "rustup-target.log") | Out-Null
    Invoke-CompanionTool -FilePath $Cargo -ArgumentList @(
        "+$RustToolchain",
        "build",
        "--locked",
        "--release",
        "--lib",
        "--manifest-path",
        (Join-Path $CoreRoot "Cargo.toml"),
        "--target",
        "aarch64-linux-android"
    ) -LogPath (Join-Path $Logs "cargo-build.log") | Out-Null
} finally {
    foreach ($Name in $EnvironmentNames) {
        [Environment]::SetEnvironmentVariable($Name, $OriginalEnvironment[$Name], "Process")
    }
}

$RustLibrary = Join-Path $CargoTargetDirectory "aarch64-linux-android/release/libstarpoint_personal_service.a"
$JniLibrary = Join-Path $NativeDirectory "libstarpoint_android_bridge.so"
if (-not (Test-Path -LiteralPath $RustLibrary -PathType Leaf)) {
    throw "Android Rust 静态库没有生成: $RustLibrary"
}
Invoke-CompanionTool -FilePath $Clang -ArgumentList @(
    "--target=aarch64-linux-android$MinimumApi",
    "-fPIC",
    "-shared",
    "-Wall",
    "-Wextra",
    "-Werror",
    "-Wl,--build-id=sha1",
    "-Wl,--gc-sections",
    "-Wl,--exclude-libs,ALL",
    "-Wl,-z,noexecstack",
    "-Wl,-z,relro",
    "-Wl,-z,now",
    "-I",
    (Join-Path $CoreRoot "include"),
    $JniSource,
    $RustLibrary,
    "-landroid",
    "-latomic",
    "-ldl",
    "-llog",
    "-lm",
    "-lunwind",
    "-o",
    $JniLibrary
) -LogPath (Join-Path $Logs "clang-link.log") | Out-Null
$ReadelfOutput = Invoke-CompanionTool -FilePath $LlvmReadelf -ArgumentList @(
    "-h",
    $JniLibrary
) -LogPath (Join-Path $Logs "readelf.log")
if (($ReadelfOutput -join "`n") -notmatch "AArch64") {
    throw "JNI 库不是 AArch64: $JniLibrary"
}
$Symbols = Invoke-CompanionTool -FilePath $LlvmNm -ArgumentList @(
    "-D",
    "--defined-only",
    $JniLibrary
) -LogPath (Join-Path $Logs "jni-symbols.log")
foreach ($RequiredSymbol in @(
    "JNI_OnLoad",
    "Java_dev_starpoint_personalservice_CompanionServiceHost_nativeStart",
    "Java_dev_starpoint_personalservice_CompanionServiceHost_nativeGetPort",
    "Java_dev_starpoint_personalservice_CompanionServiceHost_nativeIsRunning",
    "Java_dev_starpoint_personalservice_CompanionServiceHost_nativeFlush",
    "Java_dev_starpoint_personalservice_CompanionServiceHost_nativeStop"
)) {
    if (-not ($Symbols | Where-Object { $_ -match "\s$([regex]::Escape($RequiredSymbol))$" })) {
        throw "JNI 库缺少伴随宿主符号: $RequiredSymbol"
    }
}
# //// /构建 ARM64 Rust 核心和伴随服务 JNI ////

# //// 编译伴随 Activity, 前台服务和独立 DEX [@x380kkm 2026-08-31] ////
$JavaSources = @(
    (Join-Path $CompanionHostRoot "src/dev/starpoint/personalservice/CdnAssetInstaller.java"),
    (Join-Path $CompanionHostRoot "src/dev/starpoint/personalservice/CompanionServiceHost.java"),
    (Join-Path $CompanionHostRoot "src/dev/starpoint/personalservice/GameInstaller.java"),
    (Join-Path $CompanionHostRoot "src/dev/starpoint/personalservice/GameInstallReceiver.java"),
    (Join-Path $CompanionHostRoot "src/dev/starpoint/personalservice/ManagementActivity.java"),
    (Join-Path $CompanionHostRoot "src/dev/starpoint/personalservice/PersonalServiceForegroundService.java")
)
foreach ($JavaSource in $JavaSources) {
    if (-not (Test-Path -LiteralPath $JavaSource -PathType Leaf)) {
        throw "伴随 APK Java 源码不存在: $JavaSource"
    }
}
$JavaCompilationArguments = @(
    "--release",
    "8",
    "-encoding",
    "UTF-8",
    "-classpath",
    $AndroidJar,
    "-d",
    $JavaClassesDirectory
) + $JavaSources
Invoke-CompanionTool -FilePath $Javac -ArgumentList $JavaCompilationArguments `
    -LogPath (Join-Path $Logs "javac.log") | Out-Null
$HostJar = Join-Path $ResolvedOutputDirectory "starpoint-companion-host.jar"
Invoke-CompanionTool -FilePath $Jar -ArgumentList @(
    "--create",
    "--file",
    $HostJar,
    "--date=2000-01-01T00:00:00Z",
    "-C",
    $JavaClassesDirectory,
    "dev"
) -LogPath (Join-Path $Logs "jar.log") | Out-Null
Invoke-CompanionTool -FilePath $Java -ArgumentList @(
    "-cp",
    $D8Jar,
    "com.android.tools.r8.D8",
    "--release",
    "--min-api",
    $MinimumApi.ToString(),
    "--lib",
    $AndroidJar,
    "--output",
    $DexDirectory,
    $HostJar
) -LogPath (Join-Path $Logs "d8.log") | Out-Null
$DexPath = Join-Path $DexDirectory "classes.dex"
if (-not (Test-Path -LiteralPath $DexPath -PathType Leaf)) {
    throw "伴随 APK DEX 没有生成: $DexPath"
}
# //// /编译伴随 Activity, 前台服务和独立 DEX ////

# //// 组装, 对齐并签名伴随 APK [@x380kkm 2026-08-31] ////
$CompiledResources = Join-Path $ResolvedOutputDirectory "compiled-resources.zip"
$BaseApk = Join-Path $ResolvedOutputDirectory "companion-base.apk"
$UnalignedApk = Join-Path $ResolvedOutputDirectory "companion-unsigned-unaligned.apk"
$UnsignedApk = Join-Path $ResolvedOutputDirectory "StarpointPersonalService-unsigned.apk"
$SignedApk = Join-Path $ResolvedOutputDirectory "StarpointPersonalService.apk"
Invoke-CompanionTool -FilePath $Aapt2 -ArgumentList @(
    "compile",
    "--dir",
    (Join-Path $CompanionHostRoot "res"),
    "-o",
    $CompiledResources
) -LogPath (Join-Path $Logs "aapt2-compile.log") | Out-Null
Invoke-CompanionTool -FilePath $Aapt2 -ArgumentList @(
    "link",
    "-I",
    $AndroidJar,
    "-R",
    $CompiledResources,
    "--manifest",
    (Join-Path $CompanionHostRoot "AndroidManifest.xml"),
    "--min-sdk-version",
    $MinimumApi.ToString(),
    "--target-sdk-version",
    $TargetApi.ToString(),
    "--version-code",
    $VersionCode.ToString(),
    "--version-name",
    $VersionName,
    "-o",
    $BaseApk
) -LogPath (Join-Path $Logs "aapt2-link.log") | Out-Null
Copy-Item -LiteralPath $BaseApk -Destination $UnalignedApk
$Archive = [IO.Compression.ZipFile]::Open(
    $UnalignedApk,
    [IO.Compression.ZipArchiveMode]::Update
)
try {
    Add-ApkEntry -Archive $Archive -SourcePath $DexPath -EntryName "classes.dex"
    Add-ApkEntry `
        -Archive $Archive `
        -SourcePath (Join-Path $AssetsDirectory "manifest.sha256") `
        -EntryName "assets/starpoint-personal-service-cdn/manifest.sha256"
    Add-ApkEntry `
        -Archive $Archive `
        -SourcePath $ResolvedGameApkPath `
        -EntryName "assets/starpoint-game.apk" `
        -WithoutCompression
    Add-ApkEntry `
        -Archive $Archive `
        -SourcePath $GameSignersPath `
        -EntryName "assets/starpoint-game-signers.sha256"
    Add-ApkEntry `
        -Archive $Archive `
        -SourcePath $SourceReportPath `
        -EntryName "assets/starpoint-build-source.json"
    Add-ApkEntry `
        -Archive $Archive `
        -SourcePath $JniLibrary `
        -EntryName "lib/arm64-v8a/libstarpoint_android_bridge.so"
} finally {
    $Archive.Dispose()
}
Invoke-CompanionTool -FilePath $Zipalign -ArgumentList @(
    "-f",
    "-p",
    "4",
    $UnalignedApk,
    $UnsignedApk
) -LogPath (Join-Path $Logs "zipalign.log") | Out-Null

if (-not (Test-Path -LiteralPath $KeystorePath -PathType Leaf)) {
    Invoke-CompanionTool -FilePath $Keytool -ArgumentList @(
        "-genkeypair",
        "-noprompt",
        "-keystore",
        $KeystorePath,
        "-storepass",
        $KeystorePassword,
        "-keypass",
        $KeyPassword,
        "-alias",
        $KeyAlias,
        "-keyalg",
        "RSA",
        "-keysize",
        "4096",
        "-validity",
        "10000",
        "-dname",
        "CN=Starpoint Android Companion,O=Starpoint,C=CN"
    ) -LogPath (Join-Path $Logs "keytool.log") | Out-Null
}
Invoke-CompanionTool -FilePath $Java -ArgumentList @(
    "-jar",
    $ApkSignerJar,
    "sign",
    "--ks",
    $KeystorePath,
    "--ks-pass",
    "pass:$KeystorePassword",
    "--ks-key-alias",
    $KeyAlias,
    "--key-pass",
    "pass:$KeyPassword",
    "--v1-signing-enabled",
    "true",
    "--v2-signing-enabled",
    "false",
    "--v3-signing-enabled",
    "false",
    "--v4-signing-enabled",
    "false",
    "--out",
    $SignedApk,
    $UnsignedApk
) -LogPath (Join-Path $Logs "apksigner-sign.log") | Out-Null
Invoke-CompanionTool -FilePath $Java -ArgumentList @(
    "-jar",
    $ApkSignerJar,
    "verify",
    "--verbose",
    "--print-certs",
    $SignedApk
) -LogPath (Join-Path $Logs "apksigner-verify.log") | Out-Null
Invoke-CompanionTool -FilePath $Zipalign -ArgumentList @(
    "-c",
    "-p",
    "4",
    $SignedApk
) -LogPath (Join-Path $Logs "zipalign-verify.log") | Out-Null
# //// /组装, 对齐并签名伴随 APK ////

# //// 核对伴随包名, 生命周期清单和分发路径 [@x380kkm 2026-08-31] ////
$SignedArchive = [IO.Compression.ZipFile]::OpenRead($SignedApk)
try {
    $SignedEntryNames = @($SignedArchive.Entries | ForEach-Object FullName)
    foreach ($RequiredEntry in @(
        "AndroidManifest.xml",
        "classes.dex",
        "assets/starpoint-personal-service-cdn/manifest.sha256",
        "assets/starpoint-game.apk",
        "assets/starpoint-game-signers.sha256",
        "assets/starpoint-build-source.json",
        "lib/arm64-v8a/libstarpoint_android_bridge.so"
    )) {
        if ($SignedEntryNames -cnotcontains $RequiredEntry) {
            throw "伴随 APK 缺少生成项: $RequiredEntry"
        }
    }
    if ($SignedEntryNames | Where-Object { $_.Contains("\") }) {
        throw "伴随 APK 包含 Windows 路径分隔符."
    }
} finally {
    $SignedArchive.Dispose()
}
$Badging = Invoke-CompanionTool -FilePath $Aapt2 -ArgumentList @(
    "dump",
    "badging",
    $SignedApk
) -LogPath (Join-Path $Logs "aapt2-badging.log")
if (($Badging -join "`n") -notmatch "package: name='dev\.starpoint\.personalservice'") {
    throw "伴随 APK 包名无效."
}
if (($Badging -join "`n") -notmatch "targetSdkVersion:'$TargetApi'") {
    throw "伴随 APK targetSdkVersion 无效."
}
$ManifestTree = Invoke-CompanionTool -FilePath $Aapt2 -ArgumentList @(
    "dump",
    "xmltree",
    "--file",
    "AndroidManifest.xml",
    $SignedApk
) -LogPath (Join-Path $Logs "aapt2-manifest.log")
$ManifestText = $ManifestTree -join "`n"
foreach ($RequiredManifestMarker in @(
    ".ManagementActivity",
    ".GameInstallReceiver",
    ".PersonalServiceForegroundService",
    "android.intent.action.MAIN",
    "android.intent.category.LAUNCHER",
    "0x40000ff0",
    "android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE",
    "android.permission.REQUEST_INSTALL_PACKAGES"
)) {
    if (-not $ManifestText.Contains($RequiredManifestMarker)) {
        throw "伴随 APK 清单缺少标记: $RequiredManifestMarker"
    }
}
$ApkDigest = (Get-FileHash -LiteralPath $SignedApk -Algorithm SHA256).Hash.ToLowerInvariant()
"$ApkDigest`n" | Set-Content -LiteralPath "$SignedApk.sha256" -Encoding UTF8 -NoNewline
$Distribution = [ordered]@{
    schema_version = 1
    package_id = "dev.starpoint.personalservice"
    endpoint = "http://127.0.0.1:17171"
    source = $ServiceBaseline
    launch_component = "dev.starpoint.personalservice/.ManagementActivity"
    service_component = "dev.starpoint.personalservice/.PersonalServiceForegroundService"
    apk = [ordered]@{
        path = (Split-Path -Leaf $SignedApk)
        bytes = (Get-Item -LiteralPath $SignedApk).Length
        sha256 = $ApkDigest
    }
    cdn = [ordered]@{
        manifest_sha256 = $ManifestDigest
        manifest_prefix = $ManifestPrefix
        app_relative_root = "starpoint-personal-service/cdn/$ManifestPrefix"
        remote_root = "/sdcard/Android/data/dev.starpoint.personalservice/files/starpoint-personal-service/cdn/$ManifestPrefix"
    }
    lifecycle = [ordered]@{
        foreground_service = "START_STICKY"
        fixed_orientation = "portrait"
        config_changes = "0x40000ff0"
        minimum_sdk = $MinimumApi
        target_sdk = $TargetApi
        compile_sdk = $PlatformApi
        signature_scheme = "v1"
    }
    game = [ordered]@{
        package_id = "com.leiting.wf"
        asset = "assets/starpoint-game.apk"
        bytes = (Get-Item -LiteralPath $ResolvedGameApkPath).Length
        sha256 = $GameDigest
        signer_sha256 = $GameSignerDigests
    }
}
$Distribution | ConvertTo-Json -Depth 6 | Set-Content `
    -LiteralPath (Join-Path $ResolvedOutputDirectory "companion-distribution.json") `
    -Encoding UTF8
$Distribution
# //// /核对伴随包名, 生命周期清单和分发路径 ////
