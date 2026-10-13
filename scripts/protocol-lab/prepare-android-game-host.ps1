# audience: external
# # prepare-android-game-host
#
# 此脚本编译游戏宿主连接, 用 SDK dexlib2 合并单 DEX, 并用 AXMLWriter 转移二进制清单的桌面入口.
# AXML 依赖来自 Maven Central 的 de.upb.cs.swt:axml:2.0.0, 输出沿用原清单的资源引用.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InputApkPath,
    [string]$InputDexPath,
    [Parameter(Mandatory)][string]$OutputDexPath,
    [Parameter(Mandatory)][string]$OutputManifestPath,
    [Parameter(Mandatory)][string]$WorkingDirectory,
    [Parameter(Mandatory)][ValidateRange(1, [int]::MaxValue)][int]$GameVersionCode,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$ReleaseVersion,
    [string]$AndroidSdkRoot,
    [string]$JavaHome
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'android-packaging-paths.psm1') -Force
$RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$WorkspaceRoot = Resolve-AndroidPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
if ([string]::IsNullOrWhiteSpace($AndroidSdkRoot)) {
    $AndroidSdkRoot = Join-Path $WorkspaceRoot 'artifacts/android-sdk/sdk'
}
if ([string]::IsNullOrWhiteSpace($JavaHome)) {
    $JavaHome = Split-Path -Parent (Split-Path -Parent (Get-Command javac.exe).Source)
}
$Java = Join-Path $JavaHome 'bin/java.exe'
$Javac = Join-Path $JavaHome 'bin/javac.exe'
$WorkingDirectory = [IO.Path]::GetFullPath($WorkingDirectory)
$OutputDexPath = [IO.Path]::GetFullPath($OutputDexPath)
$OutputManifestPath = [IO.Path]::GetFullPath($OutputManifestPath)
$InputApkPath = [IO.Path]::GetFullPath($InputApkPath)
$BuildTools = Get-ChildItem -LiteralPath (Join-Path $AndroidSdkRoot 'build-tools') -Directory |
    Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } |
    Sort-Object { [Version]$_.Name } -Descending | Select-Object -First 1
$Platform = Get-ChildItem -LiteralPath (Join-Path $AndroidSdkRoot 'platforms') -Directory |
    Where-Object { $_.Name -match '^android-\d+$' } |
    Sort-Object { [int]($_.Name -replace '^android-', '') } -Descending | Select-Object -First 1
$AndroidJar = Join-Path $Platform.FullName 'android.jar'
$D8Jar = Join-Path $BuildTools.FullName 'lib/d8.jar'
$SdkLibraries = Join-Path $AndroidSdkRoot 'cmdline-tools/latest/lib/external'
$BridgeSources = @(Get-ChildItem -LiteralPath (Join-Path $RepositoryRoot 'platforms/android/GameHostBridge/src') `
    -Recurse -File -Filter '*.java' | Select-Object -ExpandProperty FullName)
if ($BridgeSources.Count -eq 0) { throw '游戏宿主连接源码为空.' }
foreach ($RequiredFile in @($Java, $Javac, $AndroidJar, $D8Jar, $InputApkPath)) {
    if (-not (Test-Path -LiteralPath $RequiredFile -PathType Leaf)) {
        throw "游戏宿主连接工具输入不存在: $RequiredFile"
    }
}
New-Item -ItemType Directory -Force -Path $WorkingDirectory, (Split-Path -Parent $OutputDexPath), `
    (Split-Path -Parent $OutputManifestPath) | Out-Null
$CompilationDirectory = Join-Path $WorkingDirectory ([IO.Path]::GetRandomFileName())
$BridgeClasses = Join-Path $CompilationDirectory 'bridge-classes'
$HelperClasses = Join-Path $CompilationDirectory 'helper-classes'
$BridgeDexDirectory = Join-Path $CompilationDirectory 'bridge-dex'
$DependencyDirectory = Join-Path $WorkspaceRoot 'artifacts/android-game-host-tools'
New-Item -ItemType Directory -Force -Path $BridgeClasses, $HelperClasses, $BridgeDexDirectory, `
    $DependencyDirectory | Out-Null

# //// 下载局部 AXML 读写依赖 [@x380kkm 2026-09-10] ////
function Get-GameHostDependency {
    param([string]$MavenPath)
    $Destination = Join-Path $DependencyDirectory (Split-Path -Leaf $MavenPath)
    if (-not (Test-Path -LiteralPath $Destination -PathType Leaf)) {
        $DownloadPath = Join-Path $DependencyDirectory ([IO.Path]::GetRandomFileName())
        Invoke-WebRequest -Uri "https://repo.maven.apache.org/maven2/$MavenPath" -OutFile $DownloadPath
        Move-Item -LiteralPath $DownloadPath -Destination $Destination -Force
    }
    return $Destination
}

# //// 运行构建工具并保存诊断输出 [@x380kkm 2026-09-10] ////
function Invoke-GameHostTool {
    param([string]$FilePath, [string[]]$ArgumentList, [string]$LogName)
    $LogPath = Join-Path $CompilationDirectory $LogName
    $Output = @(& $FilePath @ArgumentList 2>&1)
    $ExitCode = $LASTEXITCODE
    $Output | Out-File -LiteralPath $LogPath -Encoding UTF8
    if ($ExitCode -ne 0) {
        throw "游戏宿主连接构建失败: exit=$ExitCode log=$LogPath`n$($Output -join [Environment]::NewLine)"
    }
}

# //// 从游戏 APK 导出 Manifest, 单 DEX 和实际运行 SWF [@x380kkm 2026-09-20] ////
function Export-GameHostInputs {
    $Archive = [IO.Compression.ZipFile]::OpenRead($InputApkPath)
    try {
        $DexEntries = @($Archive.Entries | Where-Object { $_.FullName -match '^classes(?:\d+)?\.dex$' })
        if ($DexEntries.Count -ne 1 -or $DexEntries[0].FullName -cne 'classes.dex') {
            throw '游戏宿主连接需要原始单 DEX APK.'
        }
        $ManifestEntries = @($Archive.Entries | Where-Object { $_.FullName -ceq 'AndroidManifest.xml' })
        if ($ManifestEntries.Count -ne 1) { throw '游戏 APK Manifest 数量错误.' }
        $ManifestPath = Join-Path $CompilationDirectory 'AndroidManifest-original.xml'
        [IO.Compression.ZipFileExtensions]::ExtractToFile($ManifestEntries[0], $ManifestPath)
        $SwfEntries = @($Archive.Entries | Where-Object {
            $_.FullName -ceq 'assets/worldflipper_android_release.swf'
        })
        if ($SwfEntries.Count -ne 1) { throw '游戏 APK SWF 数量错误.' }
        $SwfPath = Join-Path $CompilationDirectory 'worldflipper_android_release.swf'
        [IO.Compression.ZipFileExtensions]::ExtractToFile($SwfEntries[0], $SwfPath)
        $DexPath = $InputDexPath
        if ([string]::IsNullOrWhiteSpace($DexPath)) {
            $DexPath = Join-Path $CompilationDirectory 'classes-original.dex'
            [IO.Compression.ZipFileExtensions]::ExtractToFile($DexEntries[0], $DexPath)
        } else {
            $DexPath = (Resolve-Path -LiteralPath $DexPath).Path
        }
        return [pscustomobject]@{ ManifestPath = $ManifestPath; DexPath = $DexPath; SwfPath = $SwfPath }
    } finally { $Archive.Dispose() }
}

$AxmlJar = Get-GameHostDependency 'de/upb/cs/swt/axml/2.0.0/axml-2.0.0.jar'
$SdkDependencyPatterns = @('smali-dexlib2-*.jar', 'smali-util-*.jar', 'guava-*.jar',
    'failureaccess-*.jar', 'gson-*.jar', 'jsr305-*.jar')
$Dependencies = @($AxmlJar)
foreach ($Pattern in $SdkDependencyPatterns) {
    $Matches = @(Get-ChildItem -LiteralPath $SdkLibraries -Recurse -File -Filter $Pattern)
    if ($Matches.Count -ne 1) { throw "SDK 依赖数量错误: $Pattern count=$($Matches.Count)" }
    $Dependencies += $Matches[0].FullName
}
$HelperClasspath = $Dependencies -join [IO.Path]::PathSeparator
$Inputs = Export-GameHostInputs

# //// 编译 Java 连接并生成单 DEX [@x380kkm 2026-09-10] ////
Invoke-GameHostTool -FilePath $Javac -ArgumentList (@(
    '-encoding', 'UTF-8', '-source', '8', '-target', '8', '-cp', $AndroidJar,
    '-d', $BridgeClasses
) + $BridgeSources) -LogName 'bridge-javac.log'
$BridgeClassFiles = @(Get-ChildItem -LiteralPath $BridgeClasses -Recurse -File -Filter '*.class' |
    Select-Object -ExpandProperty FullName)
Invoke-GameHostTool -FilePath $Java -ArgumentList (@(
    '-cp', $D8Jar, 'com.android.tools.r8.D8', '--lib', $AndroidJar, '--min-api', '21',
    '--output', $BridgeDexDirectory
) + $BridgeClassFiles) -LogName 'bridge-d8.log'
$DexOutputs = @(Get-ChildItem -LiteralPath $BridgeDexDirectory -File -Filter '*.dex')
if ($DexOutputs.Count -ne 1 -or $DexOutputs[0].Name -cne 'classes.dex') {
    throw '游戏宿主连接需要单 DEX 输出.'
}
# //// /编译 Java 连接并生成单 DEX ////

# //// 改写游戏 DEX 和二进制清单 [@x380kkm 2026-09-10] ////
Invoke-GameHostTool -FilePath $Javac -ArgumentList @(
    '-encoding', 'UTF-8', '-cp', $HelperClasspath, '-d', $HelperClasses,
    (Join-Path $PSScriptRoot 'AndroidGameDexPatch.java'),
    (Join-Path $PSScriptRoot 'AndroidGameManifestPatch.java')
) -LogName 'helper-javac.log'
$RuntimeClasspath = $HelperClasses + [IO.Path]::PathSeparator + $HelperClasspath
$DexEvidencePath = Join-Path $WorkingDirectory 'game-host-dex.json'
$ManifestEvidencePath = Join-Path $WorkingDirectory 'game-host-manifest.json'
Invoke-GameHostTool -FilePath $Java -ArgumentList @(
    '-cp', $RuntimeClasspath, 'AndroidGameDexPatch', $Inputs.DexPath, $DexOutputs[0].FullName,
    $OutputDexPath, $DexEvidencePath
) -LogName 'dex-patch.log'
Invoke-GameHostTool -FilePath $Java -ArgumentList @(
    '-cp', $RuntimeClasspath, 'AndroidGameManifestPatch', $Inputs.ManifestPath,
    $OutputManifestPath, $GameVersionCode.ToString(), $ManifestEvidencePath, $Inputs.SwfPath, $ReleaseVersion
) -LogName 'manifest-patch.log'
# //// /改写游戏 DEX 和二进制清单 ////

[pscustomobject]@{
    DexPath = $OutputDexPath
    ManifestPath = $OutputManifestPath
    BridgeDexPath = $DexOutputs[0].FullName
    DexEvidencePath = $DexEvidencePath
    ManifestEvidencePath = $ManifestEvidencePath
    CompilationDirectory = $CompilationDirectory
    GameVersionCode = $GameVersionCode
    ReleaseVersion = $ReleaseVersion
}
