# audience: external
# # analyze-client
# 此脚本分析一个 APK 或包含 split APK 的导出目录, 并生成签名, manifest, DEX, ELF, SWF 和协议常量证据.
# 此脚本复用 Starview 发布包中的 Android build-tools 和 FFDec, 原始 APK 只读且结果写入 artifacts.
# 此脚本可以复用中断的分析目录, 并按类名限制 FFDec 导出范围.

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$InputPath,
    [string]$OutputDirectory,
    [string]$StarviewToolsRoot,
    [string]$JavaHome,
    [string]$PythonPath,
    [string[]]$SwfClassName = @(),
    [switch]$Resume,
    [switch]$SkipSwfExport
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force
Add-Type -AssemblyName System.IO.Compression.FileSystem

# //// 收集一个 APK 或导出目录中的全部 split APK [@x380kkm 2026-07-20] ////
function Get-ClientApkPaths {
    param([Parameter(Mandatory)][string]$Path)

    $ResolvedPath = [IO.Path]::GetFullPath($Path)
    if (Test-Path -LiteralPath $ResolvedPath -PathType Leaf) {
        if ([IO.Path]::GetExtension($ResolvedPath) -ne ".apk") {
            throw "客户端输入文件不是 APK: $ResolvedPath"
        }
        return ,$ResolvedPath
    }
    if (-not (Test-Path -LiteralPath $ResolvedPath -PathType Container)) {
        throw "客户端输入不存在: $ResolvedPath"
    }

    $ApkPaths = @(Get-ChildItem -LiteralPath $ResolvedPath -Filter "*.apk" -File | Sort-Object Name | ForEach-Object { $_.FullName })
    if ($ApkPaths.Count -eq 0) {
        throw "客户端目录中没有 APK: $ResolvedPath"
    }
    $ApkPaths
}
# //// /收集一个 APK 或导出目录中的全部 split APK ////

# //// 构造一个 APK 输入和解包目录的证据记录 [@x380kkm 2026-07-20] ////
function Get-ClientApkRecord {
    param(
        [Parameter(Mandatory)][string]$ApkPath,
        [Parameter(Mandatory)][string]$UnpackedPath
    )

    [pscustomobject][ordered]@{
        FileName = [IO.Path]::GetFileName($ApkPath)
        SourcePath = $ApkPath
        UnpackedPath = $UnpackedPath
        Bytes = (Get-Item -LiteralPath $ApkPath).Length
        Sha256 = (Get-FileHash -LiteralPath $ApkPath -Algorithm SHA256).Hash
    }
}
# //// /构造一个 APK 输入和解包目录的证据记录 ////

# //// 执行分析工具并把完整输出保存到文件 [@x380kkm 2026-07-20] ////
function Invoke-AnalysisTool {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$ArgumentList,
        [Parameter(Mandatory)][string]$OutputPath
    )

    $Output = @(& $FilePath @ArgumentList 2>&1 | ForEach-Object { $_.ToString() })
    $ExitCode = $LASTEXITCODE
    $Output | Set-Content -LiteralPath $OutputPath -Encoding utf8
    if ($ExitCode -ne 0) {
        throw "分析工具失败, exit=$ExitCode tool=$FilePath output=$OutputPath"
    }
}
# //// /执行分析工具并把完整输出保存到文件 ////

# //// 解包每个 APK 并保存原始文件哈希 [@x380kkm 2026-07-20] ////
function Expand-ClientApks {
    param(
        [Parameter(Mandatory)][string[]]$ApkPaths,
        [Parameter(Mandatory)][string]$UnpackedDirectory
    )

    $Records = foreach ($ApkPath in $ApkPaths) {
        $Name = [IO.Path]::GetFileNameWithoutExtension($ApkPath)
        $DestinationPath = Join-Path $UnpackedDirectory $Name
        New-Item -ItemType Directory -Force -Path $DestinationPath | Out-Null
        [IO.Compression.ZipFile]::ExtractToDirectory($ApkPath, $DestinationPath)
        Get-ClientApkRecord -ApkPath $ApkPath -UnpackedPath $DestinationPath
    }
    @($Records)
}
# //// /解包每个 APK 并保存原始文件哈希 ////

# //// 从已有解包目录恢复每个 APK 的证据记录 [@x380kkm 2026-07-20] ////
function Get-ExistingClientApkRecords {
    param(
        [Parameter(Mandatory)][string[]]$ApkPaths,
        [Parameter(Mandatory)][string]$UnpackedDirectory
    )

    $Records = foreach ($ApkPath in $ApkPaths) {
        $Name = [IO.Path]::GetFileNameWithoutExtension($ApkPath)
        $DestinationPath = Join-Path $UnpackedDirectory $Name
        if (-not (Test-Path -LiteralPath $DestinationPath -PathType Container)) {
            throw "恢复分析缺少 APK 解包目录: $DestinationPath"
        }
        Get-ClientApkRecord -ApkPath $ApkPath -UnpackedPath $DestinationPath
    }
    @($Records)
}
# //// /从已有解包目录恢复每个 APK 的证据记录 ////

# //// 导出 APK manifest, package 信息和签名证书 [@x380kkm 2026-07-20] ////
function Export-ApkMetadata {
    param(
        [Parameter(Mandatory)][object[]]$ApkRecords,
        [Parameter(Mandatory)][string]$MetadataDirectory,
        [Parameter(Mandatory)][string]$AaptPath,
        [Parameter(Mandatory)][string]$ApkSignerPath
    )

    foreach ($Apk in $ApkRecords) {
        $Name = [IO.Path]::GetFileNameWithoutExtension([string]$Apk.FileName)
        Invoke-AnalysisTool -FilePath $AaptPath -ArgumentList @("dump", "badging", [string]$Apk.SourcePath) -OutputPath (Join-Path $MetadataDirectory "$Name.badging.txt")
        Invoke-AnalysisTool -FilePath $AaptPath -ArgumentList @("dump", "xmltree", [string]$Apk.SourcePath, "AndroidManifest.xml") -OutputPath (Join-Path $MetadataDirectory "$Name.manifest.txt")
        Invoke-AnalysisTool -FilePath $ApkSignerPath -ArgumentList @("verify", "--verbose", "--print-certs", [string]$Apk.SourcePath) -OutputPath (Join-Path $MetadataDirectory "$Name.signature.txt")
    }
}
# //// /导出 APK manifest, package 信息和签名证书 ////

# //// 导出全部 classes DEX 指令和结构 [@x380kkm 2026-07-20] ////
function Export-DexDumps {
    param(
        [Parameter(Mandatory)][string]$UnpackedDirectory,
        [Parameter(Mandatory)][string]$DexDumpDirectory,
        [Parameter(Mandatory)][string]$DexDumpPath
    )

    foreach ($DexPath in Get-ChildItem -LiteralPath $UnpackedDirectory -Filter "*.dex" -File -Recurse) {
        $RelativePath = [IO.Path]::GetRelativePath($UnpackedDirectory, $DexPath.FullName)
        $OutputName = ($RelativePath -replace "[\\/:]", "_") + ".txt"
        Invoke-AnalysisTool -FilePath $DexDumpPath -ArgumentList @("-d", "-f", $DexPath.FullName) -OutputPath (Join-Path $DexDumpDirectory $OutputName)
    }
}
# //// /导出全部 classes DEX 指令和结构 ////

# //// 用 FFDec 同时导出 ActionScript 源码和 P-code [@x380kkm 2026-07-20] ////
function Export-SwfScripts {
    param(
        [Parameter(Mandatory)][object[]]$InventoryFiles,
        [Parameter(Mandatory)][string]$UnpackedDirectory,
        [Parameter(Mandatory)][string]$SwfDirectory,
        [Parameter(Mandatory)][string]$FfdecPath,
        [string[]]$ClassNames = @()
    )

    $ExportPrefix = @("-onerror", "ignore")
    if ($ClassNames.Count -gt 0) {
        $ExportPrefix += @("-selectclass", ($ClassNames -join ","))
    }

    foreach ($Swf in $InventoryFiles | Where-Object { $_.kind -eq "swf" }) {
        $InputSwfPath = Join-Path $UnpackedDirectory ([string]$Swf.path)
        $SafeName = ([string]$Swf.path -replace "[\\/:]", "_")
        $SourceDirectory = Join-Path $SwfDirectory "$SafeName\source"
        $PcodeDirectory = Join-Path $SwfDirectory "$SafeName\pcode"
        New-Item -ItemType Directory -Force -Path $SourceDirectory, $PcodeDirectory | Out-Null
        $SourceArguments = $ExportPrefix + @("-export", "script", $SourceDirectory, $InputSwfPath)
        $PcodeArguments = $ExportPrefix + @("-format", "script:pcode", "-export", "script", $PcodeDirectory, $InputSwfPath)
        Invoke-AnalysisTool -FilePath $FfdecPath -ArgumentList $SourceArguments -OutputPath (Join-Path $SwfDirectory "$SafeName.source.log")
        Invoke-AnalysisTool -FilePath $FfdecPath -ArgumentList $PcodeArguments -OutputPath (Join-Path $SwfDirectory "$SafeName.pcode.log")
    }
}
# //// /用 FFDec 同时导出 ActionScript 源码和 P-code ////

# //// 完成客户端解包和二进制分析 [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths
$ApkPaths = @(Get-ClientApkPaths -Path $InputPath)
if ([string]::IsNullOrWhiteSpace($StarviewToolsRoot)) {
    $StarviewToolsRoot = Join-Path $Paths.WorkspaceRoot "artifacts\starview-tools\starview-windows"
}
$StarviewToolsRoot = [IO.Path]::GetFullPath($StarviewToolsRoot)
if ($Resume -and [string]::IsNullOrWhiteSpace($OutputDirectory)) {
    throw "恢复分析必须指定 OutputDirectory."
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $Timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    $OutputDirectory = Join-Path $Paths.ArtifactsRoot "client-analysis\$Timestamp"
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if ($Resume -and -not (Test-Path -LiteralPath $OutputDirectory -PathType Container)) {
    throw "恢复分析目录不存在: $OutputDirectory"
}
if (-not $Resume -and (Test-Path -LiteralPath $OutputDirectory)) {
    throw "分析输出目录已存在: $OutputDirectory"
}

$UnpackedDirectory = Join-Path $OutputDirectory "unpacked"
$MetadataDirectory = Join-Path $OutputDirectory "metadata"
$DexDumpDirectory = Join-Path $OutputDirectory "dexdump"
$StringsDirectory = Join-Path $OutputDirectory "strings"
$SwfDirectory = Join-Path $OutputDirectory "swf"
New-Item -ItemType Directory -Force -Path $UnpackedDirectory, $MetadataDirectory, $DexDumpDirectory, $StringsDirectory, $SwfDirectory | Out-Null

$AaptPath = Join-Path $StarviewToolsRoot "build-tools\aapt.exe"
$ApkSignerPath = Join-Path $StarviewToolsRoot "build-tools\apksigner.bat"
$DexDumpPath = Join-Path $StarviewToolsRoot "build-tools\dexdump.exe"
$FfdecPath = Join-Path $StarviewToolsRoot "ffdec\ffdec.bat"
Assert-ProtocolLabFile -Path $AaptPath -Description "aapt"
Assert-ProtocolLabFile -Path $ApkSignerPath -Description "apksigner"
Assert-ProtocolLabFile -Path $DexDumpPath -Description "dexdump"
if (-not $SkipSwfExport) {
    Assert-ProtocolLabFile -Path $FfdecPath -Description "FFDec"
}

$ResolvedJavaHome = Resolve-ProtocolLabJavaHome -JavaHome $JavaHome
$env:JAVA_HOME = $ResolvedJavaHome
$env:PATH = (Join-Path $ResolvedJavaHome "bin") + ";" + $env:PATH
$PythonPath = Resolve-ProtocolLabPythonPath -PythonPath $PythonPath
$InventoryPath = Join-Path $OutputDirectory "binary-inventory.json"
if ($Resume) {
    $ApkRecords = @(Get-ExistingClientApkRecords -ApkPaths $ApkPaths -UnpackedDirectory $UnpackedDirectory)
    Assert-ProtocolLabFile -Path $InventoryPath -Description "已有二进制清单"
}
else {
    $ApkRecords = @(Expand-ClientApks -ApkPaths $ApkPaths -UnpackedDirectory $UnpackedDirectory)
    Export-ApkMetadata -ApkRecords $ApkRecords -MetadataDirectory $MetadataDirectory -AaptPath $AaptPath -ApkSignerPath $ApkSignerPath
    Export-DexDumps -UnpackedDirectory $UnpackedDirectory -DexDumpDirectory $DexDumpDirectory -DexDumpPath $DexDumpPath

    $InventoryScriptPath = Join-Path $PSScriptRoot "binary_inventory.py"
    & $PythonPath $InventoryScriptPath --root $UnpackedDirectory --output $InventoryPath --strings $StringsDirectory
    if ($LASTEXITCODE -ne 0) {
        throw "客户端二进制清单生成失败, exit=$LASTEXITCODE"
    }
}
$Inventory = Get-Content -LiteralPath $InventoryPath -Raw | ConvertFrom-Json
if (-not $SkipSwfExport) {
    Export-SwfScripts -InventoryFiles @($Inventory.files) -UnpackedDirectory $UnpackedDirectory -SwfDirectory $SwfDirectory -FfdecPath $FfdecPath -ClassNames $SwfClassName
}

$Manifest = [ordered]@{
    SchemaVersion = 2
    AnalyzedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
    InputPath = [IO.Path]::GetFullPath($InputPath)
    ApkFiles = $ApkRecords
    InventoryPath = $InventoryPath
    Summary = $Inventory.summary
    SwfExport = [ordered]@{
        Skipped = [bool]$SkipSwfExport
        ClassNames = @($SwfClassName)
    }
    Tools = [ordered]@{
        Aapt = $AaptPath
        ApkSigner = $ApkSignerPath
        DexDump = $DexDumpPath
        Ffdec = $FfdecPath
        JavaHome = $ResolvedJavaHome
        Python = $PythonPath
    }
}
$ManifestPath = Join-Path $OutputDirectory "analysis.json"
$Manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $ManifestPath -Encoding utf8
[pscustomobject]@{
    OutputDirectory = $OutputDirectory
    ManifestPath = $ManifestPath
    ApkCount = $ApkRecords.Count
    DexCount = $Inventory.summary.dex_count
    ElfCount = $Inventory.summary.elf_count
    SwfCount = $Inventory.summary.swf_count
    FilesWithProtocolPorts = $Inventory.summary.files_with_protocol_ports
    FilesWith18888 = $Inventory.summary.files_with_18888
    FilesWithIndicators = $Inventory.summary.files_with_indicators
}
# //// /完成客户端解包和二进制分析 ////
