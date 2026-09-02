# audience: external
# # provider-observer-builder
# 此脚本构建只记录 ContentProvider 调用形状的 Android 调试 APK.
# 它把签名和构建产物写入 artifacts, 不读取客户端、存档或账号凭据.

[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [string]$AndroidJarPath,
    [string]$SdkRoot,
    [ValidateRange(21, 35)]
    [int]$MinSdkVersion = 23,
    [ValidateRange(21, 35)]
    [int]$TargetSdkVersion = 35
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'protocol-lab.psm1') -Force
Add-Type -AssemblyName System.IO.Compression.FileSystem

$SourceDirectory = Join-Path $PSScriptRoot '..\..\tools\protocol-lab\android-provider-observer'
$SourceDirectory = [IO.Path]::GetFullPath($SourceDirectory)
$ManifestPath = Join-Path $SourceDirectory 'AndroidManifest.xml'
$JavaSourcePath = Join-Path $SourceDirectory 'src\com\mtl\check\ProviderObserver.java'

# //// 验证观察器源文件 [@x380kkm 2026-07-28] ////
foreach ($Path in @($ManifestPath, $JavaSourcePath)) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "观察器源文件不存在: $Path"
    }
}
# //// /验证观察器源文件 ////

# //// 解析 Android SDK 与输出目录 [@x380kkm 2026-07-28] ////
$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot
if ([string]::IsNullOrWhiteSpace($AndroidJarPath)) {
    $AndroidJarPath = Join-Path $Paths.WorkspaceRoot 'artifacts\android-sdk\platforms\android-35\android.jar'
}
$AndroidJarPath = [IO.Path]::GetFullPath($AndroidJarPath)
if (-not (Test-Path -LiteralPath $AndroidJarPath -PathType Leaf)) {
    throw "Android 编译桩不存在: $AndroidJarPath"
}

$BuildToolsDirectory = Get-ChildItem -LiteralPath (Join-Path $Paths.SdkRoot 'build-tools') -Directory |
    Sort-Object { [version]$_.Name } -Descending |
    Select-Object -First 1
if ($null -eq $BuildToolsDirectory) {
    throw "Android SDK 中没有 build-tools: $($Paths.SdkRoot)"
}
$D8Path = Join-Path $BuildToolsDirectory.FullName 'd8.bat'
$Aapt2Path = Join-Path $BuildToolsDirectory.FullName 'aapt2.exe'
$ZipalignPath = Join-Path $BuildToolsDirectory.FullName 'zipalign.exe'
$ApkSignerJarPath = Join-Path $BuildToolsDirectory.FullName 'lib\apksigner.jar'
$JavaPath = (Get-Command java -ErrorAction Stop).Source
$JavaCompilerPath = (Get-Command javac -ErrorAction Stop).Source
$KeytoolPath = (Get-Command keytool -ErrorAction Stop).Source
foreach ($Tool in @($D8Path, $Aapt2Path, $ZipalignPath, $ApkSignerJarPath, $JavaPath, $JavaCompilerPath, $KeytoolPath)) {
    if (-not (Test-Path -LiteralPath $Tool -PathType Leaf)) {
        throw "Android 构建工具不存在: $Tool"
    }
}

$Timestamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ')
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $Paths.ArtifactsRoot "provider-observer\$Timestamp"
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) {
    throw "观察器输出目录已存在: $OutputDirectory"
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
# //// /解析 Android SDK 与输出目录 ////

# //// 编译 Java Provider 并生成 classes.dex [@x380kkm 2026-07-28] ////
$ClassesDirectory = Join-Path $OutputDirectory 'classes'
$DexDirectory = Join-Path $OutputDirectory 'dex'
$UnsignedApkPath = Join-Path $OutputDirectory 'unsigned.apk'
$AlignedApkPath = Join-Path $OutputDirectory 'aligned.apk'
$OutputApkPath = Join-Path $OutputDirectory 'starpoint-provider-observer.apk'
New-Item -ItemType Directory -Force -Path $ClassesDirectory, $DexDirectory | Out-Null

& $JavaCompilerPath -encoding UTF-8 -source 8 -target 8 -bootclasspath $AndroidJarPath -d $ClassesDirectory $JavaSourcePath
if ($LASTEXITCODE -ne 0) {
    throw "Provider Java 编译失败, 退出码 $LASTEXITCODE"
}
$ClassFiles = @(Get-ChildItem -LiteralPath $ClassesDirectory -Recurse -File -Filter '*.class')
if ($ClassFiles.Count -eq 0) {
    throw "Provider Java 编译没有生成 class 文件: $ClassesDirectory"
}
& $D8Path --min-api $MinSdkVersion --lib $AndroidJarPath --output $DexDirectory $ClassFiles.FullName
if ($LASTEXITCODE -ne 0) {
    throw "Provider D8 转换失败, 退出码 $LASTEXITCODE"
}
$DexPath = Join-Path $DexDirectory 'classes.dex'
if (-not (Test-Path -LiteralPath $DexPath -PathType Leaf)) {
    throw "D8 没有生成 classes.dex: $DexPath"
}
& $Aapt2Path link --manifest $ManifestPath -I $AndroidJarPath --min-sdk-version $MinSdkVersion --target-sdk-version $TargetSdkVersion --version-code 1 --version-name 1.0 -o $UnsignedApkPath
if ($LASTEXITCODE -ne 0) {
    throw "Provider manifest 打包失败, 退出码 $LASTEXITCODE"
}
$Archive = [IO.Compression.ZipFile]::Open($UnsignedApkPath, [IO.Compression.ZipArchiveMode]::Update)
try {
    if ($null -ne $Archive.GetEntry('classes.dex')) {
        throw "Provider APK 已包含 classes.dex: $UnsignedApkPath"
    }
    $Entry = $Archive.CreateEntry('classes.dex', [IO.Compression.CompressionLevel]::NoCompression)
    $EntryStream = $Entry.Open()
    $DexStream = [IO.File]::OpenRead($DexPath)
    try {
        $DexStream.CopyTo($EntryStream)
    } finally {
        $DexStream.Dispose()
        $EntryStream.Dispose()
    }
} finally {
    $Archive.Dispose()
}
& $ZipalignPath -f -p 4 $UnsignedApkPath $AlignedApkPath
if ($LASTEXITCODE -ne 0) {
    throw "Provider APK 对齐失败, 退出码 $LASTEXITCODE"
}
# //// /编译 Java Provider 并生成 classes.dex ////

# //// 生成 artifacts 中的观察器测试签名 [@x380kkm 2026-07-28] ////
$SigningDirectory = Join-Path $Paths.ArtifactsRoot 'signing'
$KeystorePath = Join-Path $SigningDirectory 'provider-observer.p12'
$PasswordPath = Join-Path $SigningDirectory 'provider-observer.pass'
$PasswordEnvironmentVariable = 'STARPOINT_PROVIDER_OBSERVER_PASSWORD'
$PreviousPassword = [Environment]::GetEnvironmentVariable($PasswordEnvironmentVariable, 'Process')
New-Item -ItemType Directory -Force -Path $SigningDirectory | Out-Null
if (-not (Test-Path -LiteralPath $PasswordPath -PathType Leaf)) {
    $Bytes = [byte[]]::new(32)
    [Security.Cryptography.RandomNumberGenerator]::Fill($Bytes)
    $Password = [Convert]::ToBase64String($Bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
    [IO.File]::WriteAllText($PasswordPath, $Password, [Text.UTF8Encoding]::new($false))
} else {
    $Password = [IO.File]::ReadAllText($PasswordPath, [Text.Encoding]::UTF8).TrimEnd("`r", "`n")
}
if ($Password.Length -lt 6) {
    throw "观察器 keystore 口令长度不足: $PasswordPath"
}
[Environment]::SetEnvironmentVariable($PasswordEnvironmentVariable, $Password, 'Process')
try {
    if (-not (Test-Path -LiteralPath $KeystorePath -PathType Leaf)) {
        & $KeytoolPath -genkeypair -noprompt -alias starpoint-provider-observer -keyalg RSA -keysize 2048 -sigalg SHA256withRSA -validity 3650 -dname 'CN=Starpoint Provider Observer,O=Starpoint,C=XX' -keystore $KeystorePath -storetype PKCS12 '-storepass:env' $PasswordEnvironmentVariable '-keypass:env' $PasswordEnvironmentVariable
        if ($LASTEXITCODE -ne 0) {
            throw "观察器 keystore 生成失败, 退出码 $LASTEXITCODE"
        }
    }
    & $JavaPath -jar $ApkSignerJarPath sign --ks $KeystorePath --ks-type PKCS12 --ks-key-alias starpoint-provider-observer "--ks-pass=env:$PasswordEnvironmentVariable" "--key-pass=env:$PasswordEnvironmentVariable" --v4-signing-enabled false --out $OutputApkPath $AlignedApkPath
    if ($LASTEXITCODE -ne 0) {
        throw "观察器 APK 签名失败, 退出码 $LASTEXITCODE"
    }
} finally {
    if ($null -eq $PreviousPassword) {
        [Environment]::SetEnvironmentVariable($PasswordEnvironmentVariable, $null, 'Process')
    } else {
        [Environment]::SetEnvironmentVariable($PasswordEnvironmentVariable, $PreviousPassword, 'Process')
    }
}
# //// /生成 artifacts 中的观察器测试签名 ////

# //// 验证 authority 与签名后的 APK 内容 [@x380kkm 2026-07-28] ////
& $JavaPath -jar $ApkSignerJarPath verify --verbose $OutputApkPath
if ($LASTEXITCODE -ne 0) {
    throw "观察器 APK 签名验证失败, 退出码 $LASTEXITCODE"
}
& $ZipalignPath -c -p -v 4 $OutputApkPath
if ($LASTEXITCODE -ne 0) {
    throw "观察器 APK 对齐验证失败, 退出码 $LASTEXITCODE"
}
$ManifestDump = @(& $Aapt2Path dump xmltree --file AndroidManifest.xml $OutputApkPath)
if ($LASTEXITCODE -ne 0 -or ($ManifestDump -join "`n") -notmatch 'com\.mtl\.check\.DataContentProvider') {
    throw "观察器 APK 没有目标 Provider authority"
}
$FinalArchive = [IO.Compression.ZipFile]::OpenRead($OutputApkPath)
try {
    if ($null -eq $FinalArchive.GetEntry('classes.dex')) {
        throw "观察器 APK 不包含 classes.dex"
    }
} finally {
    $FinalArchive.Dispose()
}
[pscustomobject][ordered]@{
    OutputApkPath = $OutputApkPath
    OutputApkSha256 = (Get-FileHash -LiteralPath $OutputApkPath -Algorithm SHA256).Hash.ToLowerInvariant()
    ProviderAuthority = 'com.mtl.check.DataContentProvider'
    SourcePackage = 'com.mtl.check'
    MinSdkVersion = $MinSdkVersion
    TargetSdkVersion = $TargetSdkVersion
}
# //// /验证 authority 与签名后的 APK 内容 ////
