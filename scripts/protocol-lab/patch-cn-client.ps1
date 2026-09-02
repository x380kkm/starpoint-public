# audience: internal
# # patch-cn-client
# 此脚本把 CN APK 中的版本地址和 API 端点改为指定服务器, 再按需使用方法级 ABC 补丁探测登录, 版本查询, 响应解码和预展开资源, 再重建, 对齐和签名 APK.
# 此脚本只接受一个 APK, 只替换已知地址, 并把运行时产物和证据写入 workspace artifacts.
# 此脚本在 artifacts 中生成或复用测试签名, 不使用发行签名处理客户端.

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [Alias("InputPath")]
    [Alias("ApkPath")]
    [string]$InputApkPath,
    [Alias("OutputPath")]
    [string]$OutputApkPath,
    [Alias("WorkDirectory")]
    [string]$WorkingDirectory,
    [string]$StarviewToolsRoot,
    [Alias("Host")]
    [ValidateNotNullOrEmpty()]
    [string]$ServerHost = "10.0.2.2",
    [Alias("ServerPort")]
    [ValidateRange(1, 65535)]
    [int]$Port = 8001,
    [string]$JavaHome,
    [string]$KeystorePath,
    [string]$KeystorePasswordFile,
    [ValidatePattern("^[A-Za-z0-9._-]+$")]
    [string]$KeyAlias = "starpoint-cn-runtime",
    [ValidateSet("PKCS12", "JKS")]
    [string]$KeystoreType = "PKCS12",
    [ValidateRange(1, 36500)]
    [int]$CertificateValidityDays = 3650,
    [ValidateNotNullOrEmpty()]
    [string]$CertificateDistinguishedName = "CN=Starpoint Protocol Lab,O=Starpoint,C=XX",
    [switch]$ProbeSdkDummyLoginRequest,
    [switch]$ProbeVersionQuery,
    [switch]$ProbeMessagePackDecodePosition,
    [switch]$UsePreextractedBundle
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force
Add-Type -AssemblyName System.IO.Compression.FileSystem

$SwfEntryName = "assets/worldflipper_android_release.swf"
$VersionClassName = "pinball.gbits.logic.GbitsVersionLogic"
$ApiConfigClassName = "pinball.config.gbits.DevConfig_gf_android"
$ApiConfigScriptName = "DevConfig_gf_android.as"
$ChannelMainClassName = "pinball.channels.ChannelSDKMain"
$ChannelMainScriptName = "ChannelSDKMain.as"
$ChannelDummyClassName = "pinball.channels.dummy.ChannelSDKDummy"
$ChannelDummyScriptName = "ChannelSDKDummy.as"
$TitleSceneClassName = "pinball.scene.title.TitleScene"
$TitleSceneScriptName = "TitleScene.as"
$RemoteUtilClassName = "pinball.context.remote.RemoteUtil"
$DevConfigClassName = "pinball.config.core.DevConfig"
$DevConfigScriptName = "DevConfig.as"
$DefaultApiExpressions = @(
    'ApiServerKind.Custom("http","10.0.2.2:8001")',
    'ApiServerKind.Custom("https","shijtswygamegf.leiting.com")'
)
$VersionUrlPatchEnabled = $true

# //// 判断 Android 模拟器是否可以访问补丁目标地址 [@x380kkm 2026-08-13] ////
function Test-ClientPatchAndroidServerHost {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ServerHost
    )

    if ($ServerHost -ieq "localhost") {
        return $false
    }

    $Address = $null
    if ([System.Net.IPAddress]::TryParse($ServerHost, [ref]$Address)) {
        return -not [System.Net.IPAddress]::IsLoopback($Address)
    }

    return $true
}
# //// /判断 Android 模拟器是否可以访问补丁目标地址 ////

# //// 计算文件的稳定 SHA-256 证据 [@x380kkm 2026-07-21] ////
function Get-ClientPatchFileEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    $Item = Get-Item -LiteralPath $Path
    [pscustomobject][ordered]@{
        Path = $Item.FullName
        Bytes = $Item.Length
        Sha256 = (Get-FileHash -LiteralPath $Item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
# //// /计算文件的稳定 SHA-256 证据 ////

# //// 计算 APK 内唯一条目的 SHA-256 证据 [@x380kkm 2026-07-21] ////
function Get-ClientPatchZipEntryEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ApkPath,
        [Parameter(Mandatory)][string]$EntryName
    )

    $Archive = [IO.Compression.ZipFile]::OpenRead($ApkPath)
    try {
        $Entries = @($Archive.Entries | Where-Object { $_.FullName -ceq $EntryName })
        if ($Entries.Count -ne 1) {
            throw "APK 条目数量不正确: entry=$EntryName count=$($Entries.Count) apk=$ApkPath"
        }

        $Entry = $Entries[0]
        $Stream = $Entry.Open()
        $HashAlgorithm = [Security.Cryptography.SHA256]::Create()
        try {
            $Hash = [BitConverter]::ToString($HashAlgorithm.ComputeHash($Stream)).Replace("-", "").ToLowerInvariant()
        } finally {
            $HashAlgorithm.Dispose()
            $Stream.Dispose()
        }

        [pscustomobject][ordered]@{
            Entry = $EntryName
            Bytes = $Entry.Length
            CompressedBytes = $Entry.CompressedLength
            Sha256 = $Hash
        }
    } finally {
        $Archive.Dispose()
    }
}
# //// /计算 APK 内唯一条目的 SHA-256 证据 ////

# //// 从 APK 提取唯一 SWF 条目 [@x380kkm 2026-07-21] ////
function Export-ClientPatchZipEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ApkPath,
        [Parameter(Mandatory)][string]$EntryName,
        [Parameter(Mandatory)][string]$OutputPath
    )

    $Archive = [IO.Compression.ZipFile]::OpenRead($ApkPath)
    try {
        $Entries = @($Archive.Entries | Where-Object { $_.FullName -ceq $EntryName })
        if ($Entries.Count -ne 1) {
            throw "APK 条目数量不正确: entry=$EntryName count=$($Entries.Count) apk=$ApkPath"
        }

        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutputPath) | Out-Null
        $InputStream = $Entries[0].Open()
        $OutputStream = [IO.File]::Open($OutputPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $InputStream.CopyTo($OutputStream)
        } finally {
            $OutputStream.Dispose()
            $InputStream.Dispose()
        }
    } finally {
        $Archive.Dispose()
    }
}
# //// /从 APK 提取唯一 SWF 条目 ////

# //// 判断 ZIP 条目是否属于旧 APK v1 签名 [@x380kkm 2026-07-21] ////
function Test-ClientPatchSignatureEntry {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$EntryName)

    if (-not $EntryName.StartsWith("META-INF/", [StringComparison]::OrdinalIgnoreCase)) {
        return $false
    }

    $LeafName = $EntryName.Substring($EntryName.LastIndexOf("/") + 1)
    $LeafName -match "(?i)^(MANIFEST\.MF|SIG-.*|.*\.(SF|RSA|DSA|EC))$"
}
# //// /判断 ZIP 条目是否属于旧 APK v1 签名 ////

# //// 重建无旧签名 APK 并替换唯一 SWF 条目 [@x380kkm 2026-07-21] ////
function New-ClientPatchUnsignedApk {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$InputApkPath,
        [Parameter(Mandatory)][string]$PatchedSwfPath,
        [Parameter(Mandatory)][string]$OutputApkPath,
        [Parameter(Mandatory)][string]$SwfEntryName
    )

    $SourceArchive = [IO.Compression.ZipFile]::OpenRead($InputApkPath)
    $OutputStream = [IO.File]::Open($OutputApkPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    $OutputArchive = [IO.Compression.ZipArchive]::new($OutputStream, [IO.Compression.ZipArchiveMode]::Create, $false)
    $RemovedSignatureEntries = 0
    $ReplacedSwfEntries = 0
    try {
        foreach ($Entry in $SourceArchive.Entries) {
            if (Test-ClientPatchSignatureEntry -EntryName $Entry.FullName) {
                $RemovedSignatureEntries++
                continue
            }

            $CompressionLevel = if ($Entry.Length -eq 0 -or $Entry.CompressedLength -eq $Entry.Length) {
                [IO.Compression.CompressionLevel]::NoCompression
            } else {
                [IO.Compression.CompressionLevel]::Optimal
            }
            $NewEntry = $OutputArchive.CreateEntry($Entry.FullName, $CompressionLevel)
            $NewEntry.LastWriteTime = $Entry.LastWriteTime
            $NewEntry.ExternalAttributes = $Entry.ExternalAttributes
            if ($Entry.FullName.EndsWith("/", [StringComparison]::Ordinal)) {
                continue
            }

            $NewEntryStream = $NewEntry.Open()
            if ($Entry.FullName -ceq $SwfEntryName) {
                $PatchedSwfStream = [IO.File]::OpenRead($PatchedSwfPath)
                try {
                    $PatchedSwfStream.CopyTo($NewEntryStream)
                    $ReplacedSwfEntries++
                } finally {
                    $PatchedSwfStream.Dispose()
                    $NewEntryStream.Dispose()
                }
                continue
            }

            $SourceEntryStream = $Entry.Open()
            try {
                $SourceEntryStream.CopyTo($NewEntryStream)
            } finally {
                $SourceEntryStream.Dispose()
                $NewEntryStream.Dispose()
            }
        }
    } finally {
        $OutputArchive.Dispose()
        $OutputStream.Dispose()
        $SourceArchive.Dispose()
    }

    if ($ReplacedSwfEntries -ne 1) {
        throw "重建 APK 时 SWF 替换数量不正确: expected=1 actual=$ReplacedSwfEntries"
    }

    [pscustomobject]@{
        RemovedSignatureEntries = $RemovedSignatureEntries
        ReplacedSwfEntries = $ReplacedSwfEntries
    }
}
# //// /重建无旧签名 APK 并替换唯一 SWF 条目 ////

# //// 执行外部工具并保存完整输出 [@x380kkm 2026-07-21] ////
function Invoke-ClientPatchTool {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$ArgumentList,
        [Parameter(Mandatory)][string]$LogPath,
        [int[]]$AllowedExitCodes = @(0)
    )

    $Output = @(& $FilePath @ArgumentList 2>&1 | ForEach-Object { $_.ToString() })
    $ExitCode = $LASTEXITCODE
    $Output | Set-Content -LiteralPath $LogPath -Encoding utf8
    if ($AllowedExitCodes -notcontains $ExitCode) {
        throw "客户端补丁工具失败, exit=$ExitCode tool=$FilePath log=$LogPath"
    }

    [pscustomobject]@{
        ExitCode = $ExitCode
        Output = $Output
    }
}
# //// /执行外部工具并保存完整输出 ////

# //// 验证 RemoteUtil 方法级 ABC 补丁证据 [@x380kkm 2026-08-03] ////
function Assert-ClientPatchRemoteUtilAbcEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory)][psobject]$Evidence)

    $ChangedMethods = @($Evidence.changedMethods)
    $DigestIncludes = @($Evidence.digestIncludes)
    $ExpectedDigestIncludes = @("method-info", "method-body", "constant-semantics", "exceptions", "traits")
    $Response = $Evidence.requestCompleteHandler
    $Request = $Evidence.getURLRequest
    if ($Evidence.className -cne "pinball.context.remote.RemoteUtil" `
        -or $Evidence.patchMethod -cne "getURLRequest" `
        -or $Evidence.digestVersion -ne 2 `
        -or ($DigestIncludes -join "|") -cne ($ExpectedDigestIncludes -join "|") `
        -or -not $Evidence.methodOnly `
        -or -not $Evidence.unchangedMethodsVerified `
        -or $ChangedMethods.Count -ne 1 `
        -or $ChangedMethods[0] -cne "getURLRequest" `
        -or $Evidence.nativeExtensionSequenceCountBefore -ne 1 `
        -or $Evidence.nativeExtensionSequenceCountAfter -ne 0) {
        throw "RemoteUtil 方法级 ABC 补丁证据不完整"
    }
    if ([string]::IsNullOrWhiteSpace($Response.referenceSha256) `
        -or $Response.referenceSha256 -cne $Response.inputSha256 `
        -or $Response.referenceSha256 -cne $Response.outputSha256) {
        throw "RemoteUtil 响应完成方法没有保持原始语义摘要"
    }
    if ([string]::IsNullOrWhiteSpace($Request.inputSha256) `
        -or [string]::IsNullOrWhiteSpace($Request.outputSha256) `
        -or $Request.inputSha256 -ceq $Request.outputSha256) {
        throw "RemoteUtil 请求方法没有产生唯一的语义摘要变化"
    }

    [pscustomobject][ordered]@{
        ClassName = $Evidence.className
        DigestVersion = $Evidence.digestVersion
        DigestIncludes = $DigestIncludes
        ChangedMethods = $ChangedMethods
        ResponseMethodSha256 = $Response.outputSha256
        Verified = $true
    }
}
# //// /验证 RemoteUtil 方法级 ABC 补丁证据 ////

# //// 验证 MessagePack 响应和解码状态诊断证据 [@x380kkm 2026-08-11] ////
function Assert-ClientPatchRemoteUtilDecodeDiagnosticEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory)][psobject]$Evidence)

    $InputChanges = @($Evidence.inputChangesFromReference)
    $ChangedMethods = @($Evidence.changedMethods)
    $Fields = @($Evidence.diagnosticFields)
    $ExpectedFields = @(
        "responseTextLength",
        "responseTextSha256",
        "responseTextPrefix4096Sha256Prefix",
        "responseTextPrefix8192Sha256Prefix",
        "responseTextPrefix12288Sha256Prefix",
        "decodedBytesLength",
        "decodedBytesSha256",
        "decoderPosition",
        "decoderBytesAvailable"
    )
    $Response = $Evidence.requestCompleteHandler
    $Request = $Evidence.getURLRequest
    $Display = $Evidence.displayableError
    if ($Evidence.className -cne "pinball.context.remote.RemoteUtil" `
        -or $Evidence.patchMethod -cne "requestCompleteHandler" `
        -or $Evidence.digestVersion -ne 2 `
        -or -not $Evidence.methodOnly `
        -or -not $Evidence.changesErrorTextOnly `
        -or -not $Evidence.forcesInternalErrorDisplay `
        -or $Evidence.displayConditionIndex -lt 0 `
        -or ($Fields -join "|") -cne ($ExpectedFields -join "|") `
        -or $ChangedMethods.Count -ne 1 `
        -or $ChangedMethods[0] -cne "requestCompleteHandler" `
        -or @($InputChanges | Where-Object { $_ -cne "getURLRequest" }).Count -ne 0) {
        throw "RemoteUtil 响应和解码状态诊断证据不完整"
    }
    if ([string]::IsNullOrWhiteSpace($Response.referenceSha256) `
        -or $Response.referenceSha256 -cne $Response.inputSha256 `
        -or $Response.inputSha256 -ceq $Response.outputSha256) {
        throw "响应和解码状态诊断没有保持输入响应方法或没有产生唯一错误文本变化"
    }
    if ([string]::IsNullOrWhiteSpace($Request.inputSha256) `
        -or $Request.inputSha256 -cne $Request.outputSha256) {
        throw "响应和解码状态诊断修改了 getURLRequest"
    }
    if ($Display.className -cne "pinball.common.error.DisplayableError" `
        -or $Display.patchMethod -cne "getDisplayMessage" `
        -or [string]::IsNullOrWhiteSpace($Display.referenceSha256) `
        -or $Display.referenceSha256 -cne $Display.inputSha256 `
        -or $Display.inputSha256 -ceq $Display.outputSha256) {
        throw "响应和解码状态诊断没有产生唯一的内部错误显示方法变化"
    }

    [pscustomobject][ordered]@{
        ClassName = $Evidence.className
        DigestVersion = $Evidence.digestVersion
        DiagnosticFields = $Fields
        InputChanges = $InputChanges
        ChangedMethods = $ChangedMethods
        DisplayClassName = $Display.className
        DisplayMethod = $Display.patchMethod
        Verified = $true
    }
}
# //// /验证 MessagePack 响应和解码状态诊断证据 ////

# //// 验证预展开内置资源补丁证据 [@x380kkm 2026-08-11] ////
function Assert-ClientPatchAssetExtractorPreextractedEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory)][psobject]$Evidence)

    $ChangedMethods = @($Evidence.changedMethods)
    $Start = $Evidence.start
    if ($Evidence.className -cne "pinball.loading.initial.AssetExtractor" `
        -or $Evidence.patchMethod -cne "start" `
        -or $Evidence.digestVersion -ne 2 `
        -or -not $Evidence.methodOnly `
        -or -not $Evidence.requiresPreextractedBundle `
        -or $Evidence.conditionIndex -lt 0 `
        -or $ChangedMethods.Count -ne 1 `
        -or $ChangedMethods[0] -cne "start") {
        throw "AssetExtractor 预展开资源补丁证据不完整"
    }
    if ([string]::IsNullOrWhiteSpace($Start.referenceSha256) `
        -or $Start.referenceSha256 -cne $Start.inputSha256 `
        -or $Start.inputSha256 -ceq $Start.outputSha256) {
        throw "AssetExtractor 预展开资源补丁没有保持输入方法或没有产生唯一变化"
    }

    [pscustomobject][ordered]@{
        ClassName = $Evidence.className
        DigestVersion = $Evidence.digestVersion
        ChangedMethods = $ChangedMethods
        RequiresPreextractedBundle = [bool]$Evidence.requiresPreextractedBundle
        Verified = $true
    }
}
# //// /验证预展开内置资源补丁证据 ////

# //// 验证版本查询补丁证据 [@x380kkm 2026-08-12] ////
function Assert-ClientPatchVersionQueryProbeEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory)][psobject]$Evidence)

    $ChangedMethods = @($Evidence.changedMethods)
    $QuerySuccess = $Evidence.isQuerySuccess
    if ($Evidence.className -cne "pinball.gbits.logic.GbitsVersionLogic" `
        -or $Evidence.patchMethod -cne "isQuerySuccess" `
        -or $Evidence.digestVersion -ne 2 `
        -or -not $Evidence.methodOnly `
        -or -not $Evidence.removesSdkDummyEarlySuccess `
        -or -not $Evidence.preservesPublishTargetCheck `
        -or -not $Evidence.preservesVersionsCheck `
        -or $Evidence.instructionRange.sdkDummyClassIndex -lt 0 `
        -or $Evidence.instructionRange.sdkDummyPropertyIndex -lt 0 `
        -or $Evidence.instructionRange.branchIndex -lt 0 `
        -or $ChangedMethods.Count -ne 1 `
        -or $ChangedMethods[0] -cne "isQuerySuccess") {
        throw "GbitsVersionLogic 版本查询补丁证据不完整"
    }
    if ([string]::IsNullOrWhiteSpace($QuerySuccess.inputSha256) `
        -or [string]::IsNullOrWhiteSpace($QuerySuccess.referenceSha256) `
        -or $QuerySuccess.referenceSha256 -cne $QuerySuccess.inputSha256 `
        -or $QuerySuccess.inputSha256 -ceq $QuerySuccess.outputSha256) {
        throw "GbitsVersionLogic 版本查询补丁没有产生唯一方法变化"
    }

    [pscustomobject][ordered]@{
        ClassName = $Evidence.className
        DigestVersion = $Evidence.digestVersion
        ChangedMethods = $ChangedMethods
        RemovesSdkDummyEarlySuccess = [bool]$Evidence.removesSdkDummyEarlySuccess
        Verified = $true
    }
}
# //// /验证版本查询补丁证据 ////

# //// 验证版本地址方法级补丁证据 [@x380kkm 2026-08-13] ////
function Assert-ClientPatchVersionUrlAbcEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][psobject]$Evidence,
        [Parameter(Mandatory)][string]$TargetUrl
    )

    $ChangedMethods = @($Evidence.changedMethods)
    $QueryVersion = $Evidence.queryVersion
    $Backup = $Evidence.onQueryErrorDefault
    $ExpectedMethods = @("queryVersion", "onQueryErrorDefault")
    $InputChanges = @($Evidence.inputChangesFromReference)
    $ChangedMethodSet = [Collections.Generic.HashSet[string]]::new([string[]]$ChangedMethods)
    $ExpectedMethodSet = [Collections.Generic.HashSet[string]]::new([string[]]$ExpectedMethods)
    if ($Evidence.className -cne "pinball.gbits.logic.GbitsVersionLogic" `
        -or $Evidence.digestVersion -ne 2 `
        -or -not $Evidence.methodOnly `
        -or -not $ChangedMethodSet.SetEquals($ExpectedMethodSet) `
        -or @($InputChanges | Where-Object { $_ -cne "isQuerySuccess" }).Count -ne 0 `
        -or $QueryVersion.sourceUrl -cne "https://update.leiting.com/shijtswy/version/" `
        -or $Backup.sourceUrl -cne "https://update.roguelike.com/shijtswy/version/" `
        -or $QueryVersion.targetUrl -cne $TargetUrl `
        -or $Backup.targetUrl -cne $TargetUrl) {
        throw "GbitsVersionLogic 版本地址方法级补丁证据不完整"
    }
    foreach ($Method in @($QueryVersion, $Backup)) {
        if ([string]::IsNullOrWhiteSpace($Method.referenceSha256) `
            -or $Method.referenceSha256 -cne $Method.inputSha256 `
            -or $Method.inputSha256 -ceq $Method.outputSha256) {
            throw "GbitsVersionLogic 版本地址补丁没有保持输入基线或产生唯一变化"
        }
    }

    [pscustomobject][ordered]@{
        ClassName = $Evidence.className
        DigestVersion = $Evidence.digestVersion
        ChangedMethods = $ChangedMethods
        TargetUrl = $TargetUrl
        Verified = $true
    }
}
# //// /验证版本地址方法级补丁证据 ////

# //// 查找 FFDec 导出的 CN API 配置类脚本 [@x380kkm 2026-07-21] ////
function Get-ClientPatchApiScript {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ExportDirectory,
        [Parameter(Mandatory)][string]$ScriptName
    )

    $Scripts = @(Get-ChildItem -LiteralPath $ExportDirectory -Recurse -File -Filter $ScriptName)
    if ($Scripts.Count -ne 1) {
        throw "FFDec CN API 配置类脚本数量不正确: expected=1 actual=$($Scripts.Count) directory=$ExportDirectory"
    }

    $Content = [IO.File]::ReadAllText($Scripts[0].FullName)
    if ($Content -notmatch "(?m)^package pinball\.config\.gbits\s*$" -or $Content -notmatch "public class DevConfig_gf_android") {
        throw "FFDec 导出文件不是 CN API 配置类: $($Scripts[0].FullName)"
    }
    $Scripts[0]
}
# //// /查找 FFDec 导出的 CN API 配置类脚本 ////

# //// 查找 FFDec 导出的渠道入口类脚本 [@x380kkm 2026-07-28] ////
function Get-ClientPatchChannelMainScript {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ExportDirectory,
        [Parameter(Mandatory)][string]$ScriptName
    )

    $Scripts = @(Get-ChildItem -LiteralPath $ExportDirectory -Recurse -File -Filter $ScriptName)
    if ($Scripts.Count -ne 1) {
        throw "FFDec 渠道入口类脚本数量不正确: expected=1 actual=$($Scripts.Count) directory=$ExportDirectory"
    }
    $Content = [IO.File]::ReadAllText($Scripts[0].FullName)
    if ($Content -notmatch "(?m)^package pinball\.channels\s*$" -or $Content -notmatch "public class ChannelSDKMain") {
        throw "FFDec 导出文件不是目标渠道入口类: $($Scripts[0].FullName)"
    }
    $Scripts[0]
}
# //// /查找 FFDec 导出的渠道入口类脚本 ////

# //// 查找 FFDec 导出的渠道 dummy 类脚本 [@x380kkm 2026-07-28] ////
function Get-ClientPatchChannelDummyScript {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ExportDirectory,
        [Parameter(Mandatory)][string]$ScriptName
    )

    $Scripts = @(Get-ChildItem -LiteralPath $ExportDirectory -Recurse -File -Filter $ScriptName)
    if ($Scripts.Count -ne 1) {
        throw "FFDec 渠道 dummy 类脚本数量不正确: expected=1 actual=$($Scripts.Count) directory=$ExportDirectory"
    }

    $Content = [IO.File]::ReadAllText($Scripts[0].FullName)
    if ($Content -notmatch "(?m)^package pinball\.channels\.dummy\s*$" -or $Content -notmatch "public class ChannelSDKDummy") {
        throw "FFDec 导出文件不是目标渠道 dummy 类: $($Scripts[0].FullName)"
    }

    $Scripts[0]
}
# //// /查找 FFDec 导出的渠道 dummy 类脚本 ////

# //// 查找 FFDec 导出的标题场景类脚本 [@x380kkm 2026-07-29] ////
function Get-ClientPatchTitleSceneScript {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ExportDirectory,
        [Parameter(Mandatory)][string]$ScriptName
    )

    $Scripts = @(Get-ChildItem -LiteralPath $ExportDirectory -Recurse -File -Filter $ScriptName)
    if ($Scripts.Count -ne 1) {
        throw "FFDec 标题场景类脚本数量不正确: expected=1 actual=$($Scripts.Count) directory=$ExportDirectory"
    }

    $Content = [IO.File]::ReadAllText($Scripts[0].FullName)
    if ($Content -notmatch "(?m)^package pinball\.scene\.title\s*$" -or $Content -notmatch "public class TitleScene") {
        throw "FFDec 导出文件不是目标标题场景类: $($Scripts[0].FullName)"
    }

    $Scripts[0]
}
# //// /查找 FFDec 导出的标题场景类脚本 ////

# //// 查找 FFDec 导出的全局开发配置类脚本 [@x380kkm 2026-07-22] ////
function Get-ClientPatchDevConfigScript {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ExportDirectory,
        [Parameter(Mandatory)][string]$ScriptName
    )

    $Scripts = @(Get-ChildItem -LiteralPath $ExportDirectory -Recurse -File -Filter $ScriptName)
    if ($Scripts.Count -ne 1) {
        throw "FFDec 全局开发配置类脚本数量不正确: expected=1 actual=$($Scripts.Count) directory=$ExportDirectory"
    }

    $Content = [IO.File]::ReadAllText($Scripts[0].FullName)
    if ($Content -notmatch "(?m)^package pinball\.config\.core\s*$" -or $Content -notmatch "public class DevConfig") {
        throw "FFDec 导出文件不是全局开发配置类: $($Scripts[0].FullName)"
    }
    $Scripts[0]
}
# //// /查找 FFDec 导出的全局开发配置类脚本 ////

# //// 只把唯一 sdkDummy 声明设为 true [@x380kkm 2026-07-22] ////
function Set-ClientPatchSdkDummy {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $Pattern = '(?m)^[\t ]*public static var sdkDummy:Boolean = (false|true);'
    $Matches = [regex]::Matches($Content, $Pattern)
    if ($Matches.Count -ne 1) {
        throw "sdkDummy 声明数量不正确: expected=1 actual=$($Matches.Count) script=$ScriptPath"
    }

    $Match = $Matches[0]
    $OriginalValue = $Match.Groups[1].Value
    $Changed = $OriginalValue -ceq "false"
    if ($Changed) {
        $Replacement = $Match.Value.Replace("false", "true", [StringComparison]::Ordinal)
        $Content = $Content.Remove($Match.Index, $Match.Length).Insert($Match.Index, $Replacement)
        [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
    }

    [pscustomobject][ordered]@{
        Declaration = "sdkDummy"
        From = $OriginalValue
        To = "true"
        Count = 1
        Changed = $Changed
    }
}
# //// /只把唯一 sdkDummy 声明设为 true ////

# //// 验证唯一 sdkDummy 声明为 true [@x380kkm 2026-07-22] ////
function Assert-ClientPatchSdkDummy {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $Pattern = '(?m)^[\t ]*public static var sdkDummy:Boolean = (false|true);'
    $Matches = [regex]::Matches($Content, $Pattern)
    if ($Matches.Count -ne 1) {
        throw "FFDec 导入验证的 sdkDummy 声明数量不正确: expected=1 actual=$($Matches.Count) script=$ScriptPath"
    }
    $Value = $Matches[0].Groups[1].Value
    if ($Value -cne "true") {
        throw "FFDec 导入验证的 sdkDummy 不是 true: actual=$Value script=$ScriptPath"
    }

    [pscustomobject][ordered]@{
        Declaration = "sdkDummy"
        Value = $Value
        Count = 1
    }
}
# //// /验证唯一 sdkDummy 声明为 true ////

# //// 读取 sdkDummy 下的渠道和媒体方法内容 [@x380kkm 2026-07-28] ////
function Get-ClientPatchSdkDummyNativeExtensionMethods {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Content)

    $Patterns = [ordered]@{
        Channel = '(?s)public static function getRealChannel\(\) : String.*?(?=\r?\n\s*public static function)'
        Media = '(?s)public static function getRealMedia\(\) : String.*?(?=\r?\n\s*public function)'
    }
    $Methods = [ordered]@{}
    foreach ($MethodName in $Patterns.Keys) {
        $Matches = [regex]::Matches($Content, $Patterns[$MethodName])
        if ($Matches.Count -ne 1) {
            throw "sdkDummy 原生扩展方法结构不匹配: method=$MethodName expected=1 actual=$($Matches.Count)"
        }
        $Methods[$MethodName] = $Matches[0].Value
    }

    [pscustomobject]$Methods
}
# //// /读取 sdkDummy 下的渠道和媒体方法内容 ////

# //// 在 sdkDummy 下跳过渠道和媒体原生扩展调用 [@x380kkm 2026-07-28] ////
function Set-ClientPatchSdkDummyNativeExtensionGuard {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $Methods = [ordered]@{
        Channel = [pscustomobject]@{ Name = 'getRealChannel'; DummyValue = 'dummyChannel' }
        Media = [pscustomobject]@{ Name = 'getRealMedia'; DummyValue = 'dummyMedia' }
    }
    $Replacements = @()
    foreach ($MethodName in $Methods.Keys) {
        $Method = $Methods[$MethodName]
        $Pattern = "(?s)(public static function $($Method.Name)\(\) : String\s*\{)"
        $Matches = [regex]::Matches($Content, $Pattern)
        if ($Matches.Count -ne 1) {
            throw "sdkDummy 原生扩展守卫入口结构不匹配: method=$($Method.Name) expected=1 actual=$($Matches.Count) script=$ScriptPath"
        }

        $Match = $Matches[0]
        $Replacement = $Match.Groups[1].Value + @"
         if(DevConfig.sdkDummy)
         {
            return DevConfig.$($Method.DummyValue);
         }
"@
        $Content = $Content.Remove($Match.Index, $Match.Length).Insert($Match.Index, $Replacement)
        $Replacements += [pscustomobject][ordered]@{
            Method = $Method.Name
            Count = $Matches.Count
            DummyValue = $Method.DummyValue
        }
    }

    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
    [pscustomobject][ordered]@{
        MethodReplacements = $Replacements
        NativeExtensionCallsGuarded = $true
    }
}
# //// /在 sdkDummy 下跳过渠道和媒体原生扩展调用 ////

# //// 验证 sdkDummy 下不创建渠道和媒体原生扩展 [@x380kkm 2026-07-28] ////
function Assert-ClientPatchSdkDummyNativeExtensionGuard {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Methods = Get-ClientPatchSdkDummyNativeExtensionMethods -Content ([IO.File]::ReadAllText($ScriptPath))
    foreach ($MethodName in @('Channel', 'Media')) {
        $MethodContent = $Methods.$MethodName
        $DummyValue = if ($MethodName -ceq 'Channel') { 'dummyChannel' } else { 'dummyMedia' }
        $GuardIndex = $MethodContent.IndexOf('if(DevConfig.sdkDummy)', [StringComparison]::Ordinal)
        $NativeIndex = $MethodContent.IndexOf('LeitingSDKExtension.getInstance()', [StringComparison]::Ordinal)
        if ($GuardIndex -lt 0 -or $NativeIndex -lt 0 -or $GuardIndex -gt $NativeIndex) {
            throw "FFDec 导入验证的 sdkDummy 原生扩展守卫无效: method=$MethodName script=$ScriptPath"
        }
        $GuardPrefix = $MethodContent.Substring(0, $NativeIndex)
        if ($GuardPrefix.IndexOf("return DevConfig.$DummyValue;", [StringComparison]::Ordinal) -lt 0) {
            throw "FFDec 导入验证的 sdkDummy 原生扩展守卫缺少返回值: method=$MethodName script=$ScriptPath"
        }
    }
}
# //// /验证 sdkDummy 下不创建渠道和媒体原生扩展 ////

# //// 读取标题场景中的微社区和初始化方法 [@x380kkm 2026-07-29] ////
function Get-ClientPatchSdkDummyTitleSceneMethods {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Content)

    $Patterns = [ordered]@{
        OpenMicroCommunity = '(?ms)^(?<indent>[\t ]*)public function openMicroCommunity\(param1:String\) : void\s*\{.*?^\k<indent>}'
        InitGbits = '(?ms)^(?<indent>[\t ]*)public function initGbits\(\) : void\s*\{.*?^\k<indent>}'
    }
    $Methods = [ordered]@{}
    foreach ($MethodName in $Patterns.Keys) {
        $Matches = [regex]::Matches($Content, $Patterns[$MethodName])
        if ($Matches.Count -ne 1) {
            throw "sdkDummy 标题场景方法结构不匹配: method=$MethodName expected=1 actual=$($Matches.Count)"
        }
        $Methods[$MethodName] = $Matches[0].Value
    }

    [pscustomobject]$Methods
}
# //// /读取标题场景中的微社区和初始化方法 ////

# //// 在 sdkDummy 下跳过标题场景的原生微社区调用 [@x380kkm 2026-07-29] ////
function Set-ClientPatchSdkDummyTitleSceneGuard {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $Methods = Get-ClientPatchSdkDummyTitleSceneMethods -Content $Content
    $OpenMicroCommunity = $Methods.OpenMicroCommunity
    $InitGbits = $Methods.InitGbits
    $OpenGuardPattern = 'if\(DevConfig\.sdkDummy\)\s*\{\s*return;\s*\}'
    $OpenGuardMatches = [regex]::Matches($OpenMicroCommunity, $OpenGuardPattern)
    if ($OpenGuardMatches.Count -ne 0) {
        throw "标题场景微社区方法已包含 dummy 守卫: expected=0 actual=$($OpenGuardMatches.Count) script=$ScriptPath"
    }
    $OpenJsonMatches = [regex]::Matches($OpenMicroCommunity, 'JSON\.stringify\(')
    if ($OpenJsonMatches.Count -ne 1) {
        throw "标题场景微社区 JSON 结构不匹配: expected=1 actual=$($OpenJsonMatches.Count) script=$ScriptPath"
    }
    $OpenShowMicroCommunityMatches = [regex]::Matches($OpenMicroCommunity, 'LeitingSDKExtension\.getInstance\(\)\.showMicroCommunity\(_loc2_\);')
    if ($OpenShowMicroCommunityMatches.Count -ne 1) {
        throw "标题场景微社区原生调用结构不匹配: expected=1 actual=$($OpenShowMicroCommunityMatches.Count) script=$ScriptPath"
    }
    $OpenNativeExtensionMatches = [regex]::Matches($OpenMicroCommunity, 'LeitingSDKExtension\.getInstance\(\)')
    if ($OpenNativeExtensionMatches.Count -ne 1) {
        throw "标题场景微社区原生扩展调用数量不正确: expected=1 actual=$($OpenNativeExtensionMatches.Count) script=$ScriptPath"
    }

    $InitNativeExtensionPattern = '(?m)(?<indent>^[\t ]*)var _loc1_:LeitingSDKExtension = LeitingSDKExtension\.getInstance\(\);\r?\n\k<indent>_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);'
    $InitNativeExtensionMatches = [regex]::Matches($InitGbits, $InitNativeExtensionPattern)
    if ($InitNativeExtensionMatches.Count -ne 1) {
        throw "标题场景初始化原生扩展结构不匹配: expected=1 actual=$($InitNativeExtensionMatches.Count) script=$ScriptPath"
    }
    $InitNativeExtensionCallMatches = [regex]::Matches($InitGbits, 'LeitingSDKExtension\.getInstance\(\)')
    if ($InitNativeExtensionCallMatches.Count -ne 1) {
        throw "标题场景初始化原生扩展调用数量不正确: expected=1 actual=$($InitNativeExtensionCallMatches.Count) script=$ScriptPath"
    }
    $InitListenerMatches = [regex]::Matches($InitGbits, '_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);')
    if ($InitListenerMatches.Count -ne 1) {
        throw "标题场景初始化微社区监听器数量不正确: expected=1 actual=$($InitListenerMatches.Count) script=$ScriptPath"
    }
    $InitGuardPattern = '(?s)if\(!DevConfig\.sdkDummy\)\s*\{\s*var _loc1_:LeitingSDKExtension = LeitingSDKExtension\.getInstance\(\);\s*_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);\s*\}'
    $InitGuardMatches = [regex]::Matches($InitGbits, $InitGuardPattern)
    if ($InitGuardMatches.Count -ne 0) {
        throw "标题场景初始化已包含 dummy 原生扩展守卫: expected=0 actual=$($InitGuardMatches.Count) script=$ScriptPath"
    }

    $NewLine = if ($Content.Contains("`r`n", [StringComparison]::Ordinal)) { "`r`n" } else { "`n" }
    $InitNativeExtensionMatch = $InitNativeExtensionMatches[0]
    $InitIndent = $InitNativeExtensionMatch.Groups['indent'].Value
    $IndentedNativeExtension = [regex]::Replace($InitNativeExtensionMatch.Value, '(?m)^', "$InitIndent   ")
    $InitReplacement = $InitIndent + 'if(!DevConfig.sdkDummy)' + $NewLine +
        $InitIndent + '{' + $NewLine +
        $IndentedNativeExtension + $NewLine +
        $InitIndent + '}'
    $PatchedInitGbits = $InitGbits.Remove($InitNativeExtensionMatch.Index, $InitNativeExtensionMatch.Length).Insert($InitNativeExtensionMatch.Index, $InitReplacement)
    $InitGbitsCount = [regex]::Matches($Content, [regex]::Escape($InitGbits)).Count
    if ($InitGbitsCount -ne 1) {
        throw "标题场景初始化方法文本数量不正确: expected=1 actual=$InitGbitsCount script=$ScriptPath"
    }
    $Content = $Content.Replace($InitGbits, $PatchedInitGbits, [StringComparison]::Ordinal)

    $OpenEntryPattern = '(?m)(?<indent>^[\t ]*)public function openMicroCommunity\(param1:String\) : void\s*\{'
    $OpenEntryMatches = [regex]::Matches($Content, $OpenEntryPattern)
    if ($OpenEntryMatches.Count -ne 1) {
        throw "标题场景微社区入口结构不匹配: expected=1 actual=$($OpenEntryMatches.Count) script=$ScriptPath"
    }
    $OpenEntryMatch = $OpenEntryMatches[0]
    $OpenIndent = $OpenEntryMatch.Groups['indent'].Value
    $OpenGuard = $NewLine +
        $OpenIndent + '   if(DevConfig.sdkDummy)' + $NewLine +
        $OpenIndent + '   {' + $NewLine +
        $OpenIndent + '      return;' + $NewLine +
        $OpenIndent + '   }'
    $OpenReplacement = $OpenEntryMatch.Value + $OpenGuard
    $Content = $Content.Remove($OpenEntryMatch.Index, $OpenEntryMatch.Length).Insert($OpenEntryMatch.Index, $OpenReplacement)

    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
    Assert-ClientPatchSdkDummyTitleSceneGuard -ScriptPath $ScriptPath | Out-Null
    [pscustomobject][ordered]@{
        OpenMicroCommunityMethodCount = 1
        OpenMicroCommunityShowCallCount = $OpenShowMicroCommunityMatches.Count
        InitGbitsMethodCount = 1
        InitGbitsNativeExtensionCallCount = $InitNativeExtensionCallMatches.Count
        InitGbitsListenerCount = $InitListenerMatches.Count
        OpenMicroCommunityGuarded = $true
        InitGbitsNativeExtensionGuarded = $true
    }
}
# //// /在 sdkDummy 下跳过标题场景的原生微社区调用 ////

# //// 验证 sdkDummy 下的标题场景原生微社区守卫 [@x380kkm 2026-07-29] ////
function Assert-ClientPatchSdkDummyTitleSceneGuard {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Methods = Get-ClientPatchSdkDummyTitleSceneMethods -Content ([IO.File]::ReadAllText($ScriptPath))
    $OpenMicroCommunity = $Methods.OpenMicroCommunity
    $OpenEntryGuardPattern = '(?s)public function openMicroCommunity\(param1:String\) : void\s*\{\s*if\(DevConfig\.sdkDummy\)\s*\{\s*return;\s*\}'
    $OpenEntryGuardMatches = [regex]::Matches($OpenMicroCommunity, $OpenEntryGuardPattern)
    if ($OpenEntryGuardMatches.Count -ne 1) {
        throw "FFDec 导入验证的标题场景微社区入口守卫数量不正确: expected=1 actual=$($OpenEntryGuardMatches.Count) script=$ScriptPath"
    }
    $OpenJsonMatches = [regex]::Matches($OpenMicroCommunity, 'JSON\.stringify\(')
    $OpenShowMicroCommunityMatches = [regex]::Matches($OpenMicroCommunity, 'LeitingSDKExtension\.getInstance\(\)\.showMicroCommunity\(_loc2_\);')
    $OpenNativeExtensionMatches = [regex]::Matches($OpenMicroCommunity, 'LeitingSDKExtension\.getInstance\(\)')
    if ($OpenJsonMatches.Count -ne 1 -or $OpenShowMicroCommunityMatches.Count -ne 1 -or $OpenNativeExtensionMatches.Count -ne 1) {
        throw "FFDec 导入验证的标题场景微社区原生调用数量不正确: json=$($OpenJsonMatches.Count) show=$($OpenShowMicroCommunityMatches.Count) native=$($OpenNativeExtensionMatches.Count) script=$ScriptPath"
    }
    if ($OpenJsonMatches[0].Index -lt $OpenEntryGuardMatches[0].Length) {
        throw "FFDec 导入验证的标题场景微社区守卫没有位于 JSON 构造前: script=$ScriptPath"
    }

    $InitGbits = $Methods.InitGbits
    $InitNativeExtensionPattern = '(?m)(?<indent>^[\t ]*)var _loc1_:LeitingSDKExtension = LeitingSDKExtension\.getInstance\(\);\r?\n\k<indent>_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);'
    $InitNativeExtensionMatches = [regex]::Matches($InitGbits, $InitNativeExtensionPattern)
    $InitNativeExtensionCallMatches = [regex]::Matches($InitGbits, 'LeitingSDKExtension\.getInstance\(\)')
    $InitListenerMatches = [regex]::Matches($InitGbits, '_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);')
    if ($InitNativeExtensionMatches.Count -ne 1 -or $InitNativeExtensionCallMatches.Count -ne 1 -or $InitListenerMatches.Count -ne 1) {
        throw "FFDec 导入验证的标题场景初始化原生扩展数量不正确: block=$($InitNativeExtensionMatches.Count) native=$($InitNativeExtensionCallMatches.Count) listener=$($InitListenerMatches.Count) script=$ScriptPath"
    }
    $InitGuardPattern = '(?s)if\(!DevConfig\.sdkDummy\)\s*\{\s*var _loc1_:LeitingSDKExtension = LeitingSDKExtension\.getInstance\(\);\s*_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);\s*\}'
    $InitGuardMatches = [regex]::Matches($InitGbits, $InitGuardPattern)
    if ($InitGuardMatches.Count -ne 1) {
        throw "FFDec 导入验证的标题场景初始化原生扩展守卫数量不正确: expected=1 actual=$($InitGuardMatches.Count) script=$ScriptPath"
    }
    $InitGuard = $InitGuardMatches[0]
    $InitGuardEnd = $InitGuard.Index + $InitGuard.Length
    foreach ($Match in @($InitNativeExtensionMatches[0], $InitListenerMatches[0])) {
        if ($Match.Index -lt $InitGuard.Index -or $Match.Index -ge $InitGuardEnd) {
            throw "FFDec 导入验证的标题场景初始化原生扩展仍位于 dummy 守卫外: script=$ScriptPath"
        }
    }

    [pscustomobject][ordered]@{
        OpenMicroCommunityGuarded = $true
        InitGbitsNativeExtensionGuarded = $true
        Verified = $true
    }
}
# //// /验证 sdkDummy 下的标题场景原生微社区守卫 ////

# //// 提取标题场景登录按钮的状态门控证据 [@x380kkm 2026-08-14] ////
function Get-ClientPatchTitleSceneLoginGateEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Content)

    $MethodPattern = '(?ms)^(?<indent>[\t ]*)public function buttonClicked\(param1:int\) : void\s*\{.*?^\k<indent>}'
    $MethodMatches = [regex]::Matches($Content, $MethodPattern)
    if ($MethodMatches.Count -ne 1) {
        throw "标题场景登录按钮方法数量不正确: expected=1 actual=$($MethodMatches.Count)"
    }
    $Method = $MethodMatches[0].Value
    $CaseZeroMatches = [regex]::Matches($Method, '(?s)case\s+0:.*?(?=\r?\n\s*case\s+1:)')
    if ($CaseZeroMatches.Count -eq 0) {
        $CaseZeroMatches = [regex]::Matches($Method, '(?s)case\s+0:.*')
    }
    if ($CaseZeroMatches.Count -ne 1) {
        throw "标题场景登录 case 0 结构数量不正确: expected=1 actual=$($CaseZeroMatches.Count)"
    }
    $CaseZero = $CaseZeroMatches[0].Value
    $Patterns = [ordered]@{
        CaseZero = 'case\s+0:'
        VersionQuery = 'versionLogic\.isQuerySuccess\(\)'
        SdkInitialized = 'channelSDK\.sdkIsInited\(\)'
        SdkLogging = 'channelSDK\.isSDKLogining\(\)'
        SdkLoggedIn = 'channelSDK\.isSDKLoginOk\(\)'
        StartLoginServer = 'channelSDK\.startLoginServer\(onLoginServerSuccess\);'
        ManualLogin = 'channelSDK\.sdkLoginManual\(onSDKLoginCompleteHander\);'
    }
    $MatchesByName = [ordered]@{}
    foreach ($Name in $Patterns.Keys) {
        $Matches = [regex]::Matches($CaseZero, $Patterns[$Name])
        if ($Matches.Count -ne 1) {
            throw "标题场景登录门控结构数量不正确: marker=$Name expected=1 actual=$($Matches.Count)"
        }
        $MatchesByName[$Name] = $Matches[0]
    }

    $OrderedNames = @(
        'CaseZero',
        'VersionQuery',
        'SdkInitialized',
        'SdkLogging',
        'SdkLoggedIn'
    )
    for ($Index = 1; $Index -lt $OrderedNames.Count; $Index++) {
        $Previous = $MatchesByName[$OrderedNames[$Index - 1]]
        $Current = $MatchesByName[$OrderedNames[$Index]]
        if ($Current.Index -le $Previous.Index) {
            throw "标题场景登录门控顺序不正确: previous=$($OrderedNames[$Index - 1]) current=$($OrderedNames[$Index])"
        }
    }
    $LoginBranchIndex = $MatchesByName.SdkLoggedIn.Index
    if ($MatchesByName.StartLoginServer.Index -le $LoginBranchIndex -or $MatchesByName.ManualLogin.Index -le $LoginBranchIndex) {
        throw "标题场景登录分支没有位于 SDK 登录状态判断之后"
    }

    $ButtonConstructionMatches = [regex]::Matches($Content, 'new ButtonGroupLogic\(buttonClicked,_loc1_\)')
    $ButtonDisableMatches = [regex]::Matches($Content, 'buttonGroup\.setEnabled\(0,false\);')
    $ButtonEnableMatches = [regex]::Matches($Content, 'buttonGroup\.setEnabled\(0,true\);')
    if ($ButtonConstructionMatches.Count -ne 1 -or $ButtonDisableMatches.Count -ne 1 -or $ButtonEnableMatches.Count -ne 1) {
        throw "标题场景登录按钮启用结构数量不正确: construction=$($ButtonConstructionMatches.Count) disable=$($ButtonDisableMatches.Count) enable=$($ButtonEnableMatches.Count)"
    }

    [pscustomobject][ordered]@{
        MethodName = 'buttonClicked'
        CaseZero = [ordered]@{ Index = $Method.IndexOf($CaseZero, [StringComparison]::Ordinal) }
        Guards = [ordered]@{
            VersionQuery = [ordered]@{ Index = $MatchesByName.VersionQuery.Index }
            SdkInitialized = [ordered]@{ Index = $MatchesByName.SdkInitialized.Index }
            SdkLogging = [ordered]@{ Index = $MatchesByName.SdkLogging.Index }
            SdkLoggedIn = [ordered]@{ Index = $MatchesByName.SdkLoggedIn.Index }
        }
        LoginBranches = [ordered]@{
            StartLoginServer = [ordered]@{ Index = $MatchesByName.StartLoginServer.Index }
            ManualLogin = [ordered]@{ Index = $MatchesByName.ManualLogin.Index }
        }
        ButtonWiring = [ordered]@{
            ConstructionCount = $ButtonConstructionMatches.Count
            DisabledInitially = $ButtonDisableMatches.Count -eq 1
            EnabledAfterSequence = $ButtonEnableMatches.Count -eq 1
        }
        Verified = $true
    }
}
# //// /提取标题场景登录按钮的状态门控证据 ////

# //// 验证标题场景登录按钮的状态门控证据 [@x380kkm 2026-08-14] ////
function Assert-ClientPatchTitleSceneLoginGateEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Evidence = Get-ClientPatchTitleSceneLoginGateEvidence -Content ([IO.File]::ReadAllText($ScriptPath))
    if (-not $Evidence.Verified -or -not $Evidence.ButtonWiring.DisabledInitially -or -not $Evidence.ButtonWiring.EnabledAfterSequence) {
        throw "标题场景登录按钮门控证据不完整: script=$ScriptPath"
    }
    $Evidence
}
# //// /验证标题场景登录按钮的状态门控证据 ////

# //// 统计一个文本中精确字符串的出现次数 [@x380kkm 2026-07-21] ////
function Get-ClientPatchOrdinalCount {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Content,
        [Parameter(Mandatory)][string]$Value
    )

    $Count = 0
    $Offset = 0
    while (($Index = $Content.IndexOf($Value, $Offset, [StringComparison]::Ordinal)) -ge 0) {
        $Count++
        $Offset = $Index + $Value.Length
    }
    $Count
}
# //// /统计一个文本中精确字符串的出现次数 ////

# //// 只替换 CN API 配置中的固定主机和端口 [@x380kkm 2026-07-21] ////
function Set-ClientPatchApiEndpoint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string[]]$OriginalExpressions,
        [Parameter(Mandatory)][string]$TargetExpression
    )

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $Matches = @(
        foreach ($Expression in $OriginalExpressions) {
            $Count = Get-ClientPatchOrdinalCount -Content $Content -Value $Expression
            if ($Count -gt 1) {
                throw "CN API 配置表达式重复: expression=$Expression count=$Count"
            }
            if ($Count -eq 1) {
                [pscustomobject]@{ Expression = $Expression; Count = $Count }
            }
        }
    )
    if ($Matches.Count -ne 1) {
        throw "CN API 配置未匹配唯一已知表达式: expected=1 actual=$($Matches.Count)"
    }
    $Content = $Content.Replace($Matches[0].Expression, $TargetExpression, [StringComparison]::Ordinal)
    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
    [pscustomobject][ordered]@{
        From = $Matches[0].Expression
        To = $TargetExpression
        Count = $Matches[0].Count
    }
}
# //// /只替换 CN API 配置中的固定主机和端口 ////

# //// 验证 CN API 配置只保留目标端点 [@x380kkm 2026-07-21] ////
function Assert-ClientPatchApiEndpoint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string[]]$OriginalExpressions,
        [Parameter(Mandatory)][string]$TargetExpression
    )

    $Content = [IO.File]::ReadAllText($ScriptPath)
    foreach ($Expression in $OriginalExpressions) {
        if ($Expression -ceq $TargetExpression) {
            continue
        }
        $OriginalCount = Get-ClientPatchOrdinalCount -Content $Content -Value $Expression
        if ($OriginalCount -ne 0) {
            throw "FFDec 导入验证仍包含旧 CN API 表达式: expression=$Expression count=$OriginalCount"
        }
    }
    $TargetCount = Get-ClientPatchOrdinalCount -Content $Content -Value $TargetExpression
    if ($TargetCount -ne 1) {
        throw "FFDec 导入验证的 CN API 表达式数量不正确: expression=$TargetExpression expected=1 actual=$TargetCount"
    }
}
# //// /验证 CN API 配置只保留目标端点 ////

# //// 读取 dummy SDK 登录请求探测模式 [@x380kkm 2026-07-29] ////
function Get-ClientPatchSdkDummyLoginRequestProbeMode {
    [CmdletBinding()]
    param([Parameter(Mandatory)][bool]$ProbeSdkDummyLoginRequest)

    if ($ProbeSdkDummyLoginRequest) {
        return [pscustomobject][ordered]@{
            Name = "dummy-login-request-probe"
            PatchSdkDummy = $true
            PatchChannelMainForSdkDummy = $true
            PatchDummyLoginRequestProbe = $true
            PatchSdkDummyNativeExtensionGuard = $true
            PatchRemoteUtilRequestGuard = $true
            PatchSdkDummyTitleSceneGuard = $true
        }
    }

    [pscustomobject][ordered]@{
        Name = "none"
        PatchSdkDummy = $false
        PatchChannelMainForSdkDummy = $false
        PatchDummyLoginRequestProbe = $false
        PatchSdkDummyNativeExtensionGuard = $false
        PatchRemoteUtilRequestGuard = $false
        PatchSdkDummyTitleSceneGuard = $false
    }
}
# //// /读取 dummy SDK 登录请求探测模式 ////

# //// 读取 MessagePack 解码位置诊断模式 [@x380kkm 2026-08-10] ////
function Get-ClientPatchMessagePackDecodeDiagnosticMode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][bool]$ProbeMessagePackDecodePosition,
        [Parameter(Mandatory)][bool]$ProbeSdkDummyLoginRequest
    )

    if ($ProbeMessagePackDecodePosition -and -not $ProbeSdkDummyLoginRequest) {
        throw "MessagePack 解码位置诊断需要同时启用 ProbeSdkDummyLoginRequest"
    }

    [pscustomobject][ordered]@{
        Name = if ($ProbeMessagePackDecodePosition) { "messagepack-decode-position" } else { "none" }
        Enabled = $ProbeMessagePackDecodePosition
        PatchRemoteUtilResponseError = $ProbeMessagePackDecodePosition
        PatchDisplayableErrorMessage = $ProbeMessagePackDecodePosition
    }
}
# //// /读取 MessagePack 解码位置诊断模式 ////

# //// 读取预展开内置资源模式 [@x380kkm 2026-08-11] ////
function Get-ClientPatchPreextractedBundleMode {
    [CmdletBinding()]
    param([Parameter(Mandatory)][bool]$UsePreextractedBundle)

    [pscustomobject][ordered]@{
        Name = if ($UsePreextractedBundle) { "preextracted-bundle" } else { "none" }
        Enabled = $UsePreextractedBundle
        PatchAssetExtractorStart = $UsePreextractedBundle
        RequiresPreextractedBundle = $UsePreextractedBundle
    }
}
# //// /读取预展开内置资源模式 ////

# //// 读取版本查询诊断模式 [@x380kkm 2026-08-12] ////
function Get-ClientPatchVersionQueryProbeMode {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][bool]$ProbeVersionQuery,
        [Parameter(Mandatory)][bool]$ProbeSdkDummyLoginRequest
    )

    if ($ProbeVersionQuery -and -not $ProbeSdkDummyLoginRequest) {
        throw "版本查询诊断需要同时启用 ProbeSdkDummyLoginRequest"
    }

    [pscustomobject][ordered]@{
        Name = if ($ProbeVersionQuery) { "sdk-dummy-version-query" } else { "none" }
        Enabled = $ProbeVersionQuery
        PatchVersionQuerySuccess = $ProbeVersionQuery
    }
}
# //// /读取版本查询诊断模式 ////

# //// 根据补丁模式选择导出的 ActionScript 类 [@x380kkm 2026-08-03] ////
function Get-ClientPatchSelectedClassNames {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][psobject]$PatchMode,
        [Parameter(Mandatory)][string]$VersionClassName,
        [Parameter(Mandatory)][string]$ApiConfigClassName,
        [Parameter(Mandatory)][string]$DevConfigClassName,
        [Parameter(Mandatory)][string]$TitleSceneClassName,
        [Parameter(Mandatory)][string]$ChannelMainClassName,
        [Parameter(Mandatory)][string]$ChannelDummyClassName,
        [switch]$SkipVersionClass
    )

    $SelectedClassNames = [System.Collections.Generic.List[string]]::new()
    if (-not $SkipVersionClass) {
        $null = $SelectedClassNames.Add($VersionClassName)
    }
    $null = $SelectedClassNames.Add($ApiConfigClassName)
    if ($PatchMode.PatchSdkDummy) {
        $null = $SelectedClassNames.Add($DevConfigClassName)
    }
    if ($PatchMode.PatchSdkDummyTitleSceneGuard) {
        $null = $SelectedClassNames.Add($TitleSceneClassName)
    }
    if ($PatchMode.PatchChannelMainForSdkDummy) {
        $null = $SelectedClassNames.Add($ChannelMainClassName)
    }
    if ($PatchMode.PatchDummyLoginRequestProbe) {
        $null = $SelectedClassNames.Add($ChannelDummyClassName)
    }

    $DuplicateClassNames = @($SelectedClassNames | Group-Object | Where-Object Count -gt 1)
    if ($DuplicateClassNames.Count -ne 0) {
        throw "补丁类选择包含重复类: $($DuplicateClassNames.Name -join ',')"
    }

    $SelectedClassNames.ToArray()
}
# //// /根据补丁模式选择导出的 ActionScript 类 ////

# //// 保留 dummy 标志并让渠道入口保留真实远端对象 [@x380kkm 2026-07-28] ////
function Set-ClientPatchSdkDummyRealRemoteBridge {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $RealRemoteNullPattern = '(?s)\r?\n\s*if\(DevConfig\.sdkDummy\)\s*\{\s*realRemote = null;\s*\}'
    $RealRemoteNullMatches = [regex]::Matches($Content, $RealRemoteNullPattern)
    if ($RealRemoteNullMatches.Count -ne 1) {
        throw "渠道入口 dummy 远端清空结构不匹配: expected=1 actual=$($RealRemoteNullMatches.Count) script=$ScriptPath"
    }
    $Content = [regex]::Replace($Content, $RealRemoteNullPattern, "", 1)

    $ChannelSelectionPattern = '(?s)if\(null == realRemote\)\s*\{\s*sdk = new ChannelSDKDummy\(realRemote,logic\);\s*\}\s*else\s*\{\s*sdk = new ChannelLeitingSDKAndroid\(realRemote,logic\);\s*\}'
    $ChannelSelectionReplacement = @'
                if(DevConfig.sdkDummy || null == realRemote)
                {
                   sdk = new ChannelSDKDummy(realRemote,logic);
                }
                else
                {
                   sdk = new ChannelLeitingSDKAndroid(realRemote,logic);
                }
'@
    $ChannelSelectionMatches = [regex]::Matches($Content, $ChannelSelectionPattern)
    if ($ChannelSelectionMatches.Count -ne 1) {
        throw "渠道入口 SDK 实例选择结构不匹配: expected=1 actual=$($ChannelSelectionMatches.Count) script=$ScriptPath"
    }
    $Content = [regex]::Replace($Content, $ChannelSelectionPattern, $ChannelSelectionReplacement, 1)
    if ($Content -match $RealRemoteNullPattern) {
        throw "渠道入口仍会在 dummy 标志下清空真实远端对象: script=$ScriptPath"
    }
    if ($Content -notmatch [regex]::Escape("if(DevConfig.sdkDummy || null == realRemote)")) {
        throw "渠道入口没有在 dummy 标志下保留真实远端的 dummy 实例: script=$ScriptPath"
    }

    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
    [pscustomobject][ordered]@{
        RealRemoteNullAssignmentCount = $RealRemoteNullMatches.Count
        ChannelSelectionCount = $ChannelSelectionMatches.Count
        SdkDummyUsesDummySdk = $true
    }
}
# //// /保留 dummy 标志并让渠道入口保留真实远端对象 ////

# //// 验证 dummy 标志下的渠道入口远端桥接 [@x380kkm 2026-07-28] ////
function Assert-ClientPatchSdkDummyRealRemoteBridge {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $RealRemoteNullPattern = '(?s)\r?\n\s*if\(DevConfig\.sdkDummy\)\s*\{\s*realRemote = null;\s*\}'
    if ($Content -match $RealRemoteNullPattern) {
        throw "FFDec 导入验证发现 dummy 标志清空真实远端对象: script=$ScriptPath"
    }
    if ($Content -notmatch [regex]::Escape("if(DevConfig.sdkDummy || null == realRemote)")) {
        throw "FFDec 导入验证缺少 dummy 标志下的真实远端 dummy 实例选择: script=$ScriptPath"
    }
}
# //// /验证 dummy 标志下的渠道入口远端桥接 ////

# //// 提取渠道 dummy 内置登录请求方法 [@x380kkm 2026-07-28] ////
function Get-ClientPatchSdkDummyTestLoginMethodContent {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Content)

    $Pattern = '(?s)public function testLogin\(\) : void.*?(?=\r?\n\s*public function|\r?\n\s*}\s*\r?\n})'
    $Matches = [regex]::Matches($Content, $Pattern)
    if ($Matches.Count -ne 1) {
        throw "渠道 dummy 内置登录请求方法结构不匹配: expected=1 actual=$($Matches.Count)"
    }

    $Matches[0].Value
}
# //// /提取渠道 dummy 内置登录请求方法 ////

# //// 让渠道 dummy 使用客户端原始登录请求进行探测 [@x380kkm 2026-07-29] ////
function Set-ClientPatchSdkDummyLoginRequestProbe {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $OriginalTestLogin = Get-ClientPatchSdkDummyTestLoginMethodContent -Content $Content
    $StartLoginPattern = '(?s)public function startLoginServer\(param1:Function\) : void.*?(?=\r?\n\s*public function|\r?\n\s*}\s*\r?\n})'
    $StartLoginReplacement = @'
      public function startLoginServer(param1:Function) : void
      {
         testConnect = true;
         testLogin();
         completeHandler = param1;
         lastOperationTime = getTimer() / 1000;
      }
'@
    $StartLoginMatches = [regex]::Matches($Content, $StartLoginPattern)
    if ($StartLoginMatches.Count -ne 1) {
        throw "渠道 dummy 登录入口结构不匹配: expected=1 actual=$($StartLoginMatches.Count) script=$ScriptPath"
    }

    $Content = [regex]::Replace($Content, $StartLoginPattern, $StartLoginReplacement, 1)
    $PatchedTestLogin = Get-ClientPatchSdkDummyTestLoginMethodContent -Content $Content
    if ($PatchedTestLogin -cne $OriginalTestLogin) {
        throw "渠道 dummy 桥接修改了内置登录请求方法: script=$ScriptPath"
    }

    $LoginSuccessPattern = '(?s)public function loginSuccessHandler\(param1:ResponseData\) : void\s*\{'
    $LoginSuccessReplacement = @'
      public function loginSuccessHandler(param1:ResponseData) : void
      {
         titleScene.showInstantMessage("协议响应已到达",InstantMessagePosition.Center);
'@
    $LoginSuccessMatches = [regex]::Matches($Content, $LoginSuccessPattern)
    if ($LoginSuccessMatches.Count -ne 1) {
        throw "渠道 dummy 登录响应入口结构不匹配: expected=1 actual=$($LoginSuccessMatches.Count) script=$ScriptPath"
    }
    $Content = [regex]::Replace($Content, $LoginSuccessPattern, $LoginSuccessReplacement, 1)

    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
    [pscustomobject][ordered]@{
        StartLoginMethodCount = $StartLoginMatches.Count
        TestLoginMethodPreserved = $true
        UsesClientTestLogin = $true
        LoginResponseMarkerCount = $LoginSuccessMatches.Count
    }
}
# //// /让渠道 dummy 使用客户端原始登录请求进行探测 ////

# //// 验证渠道 dummy 保留客户端原始登录请求 [@x380kkm 2026-07-29] ////
function Assert-ClientPatchSdkDummyLoginRequestProbe {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = [IO.File]::ReadAllText($ScriptPath)
    $StartLoginPattern = '(?s)public function startLoginServer\(param1:Function\) : void.*?(?=\r?\n\s*public function|\r?\n\s*}\s*\r?\n})'
    $StartLoginMatches = [regex]::Matches($Content, $StartLoginPattern)
    if ($StartLoginMatches.Count -ne 1) {
        throw "FFDec 导入验证的渠道 dummy 登录入口数量不正确: expected=1 actual=$($StartLoginMatches.Count) script=$ScriptPath"
    }

    $StartLogin = $StartLoginMatches[0].Value
    foreach ($Value in @('testConnect = true;', 'testLogin();', 'completeHandler = param1;', 'lastOperationTime = getTimer() / 1000;')) {
        if ($StartLogin.IndexOf($Value, [StringComparison]::Ordinal) -lt 0) {
            throw "FFDec 导入验证的渠道 dummy 登录入口缺少桥接调用: value=$Value script=$ScriptPath"
        }
    }

    $TestLogin = Get-ClientPatchSdkDummyTestLoginMethodContent -Content $Content
    foreach ($Value in @('"channels/channel_leiting/leiting_login"', 'remote.requestQueue.addRequest')) {
        if ($TestLogin.IndexOf($Value, [StringComparison]::Ordinal) -lt 0) {
            throw "FFDec 导入验证的渠道 dummy 内置登录请求缺少字段: value=$Value script=$ScriptPath"
        }
    }
    if ($Content.IndexOf('titleScene.showInstantMessage("协议响应已到达",InstantMessagePosition.Center);', [StringComparison]::Ordinal) -lt 0) {
        throw "FFDec 导入验证缺少登录响应到达标记: script=$ScriptPath"
    }
}
# //// /验证渠道 dummy 保留客户端原始登录请求 ////

# //// 判断一个路径是否位于指定目录内 [@x380kkm 2026-07-21] ////
function Test-ClientPatchPathWithinDirectory {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Directory
    )

    $FullPath = [IO.Path]::GetFullPath($Path)
    $FullDirectory = [IO.Path]::GetFullPath($Directory)
    $RelativePath = [IO.Path]::GetRelativePath($FullDirectory, $FullPath)
    if ($RelativePath -eq ".") {
        return $true
    }
    if ([IO.Path]::IsPathRooted($RelativePath) -or $RelativePath -eq "..") {
        return $false
    }
    -not $RelativePath.StartsWith("..$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::Ordinal)
}
# //// /判断一个路径是否位于指定目录内 ////

# //// 读取或创建 artifacts 中的运行时签名口令 [@x380kkm 2026-07-21] ////
function Get-ClientPatchKeystorePassword {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$PasswordFile,
        [Parameter(Mandatory)][bool]$KeystoreExists
    )

    $Created = $false
    if (Test-Path -LiteralPath $PasswordFile -PathType Leaf) {
        $Password = [IO.File]::ReadAllText($PasswordFile, [Text.Encoding]::UTF8).TrimEnd("`r", "`n")
    } else {
        if ($KeystoreExists) {
            throw "已有 keystore 缺少口令文件: $PasswordFile"
        }

        $Bytes = [byte[]]::new(32)
        $Generator = [Security.Cryptography.RandomNumberGenerator]::Create()
        try {
            $Generator.GetBytes($Bytes)
        } finally {
            $Generator.Dispose()
        }
        $Password = [Convert]::ToBase64String($Bytes).TrimEnd("=").Replace("+", "-").Replace("/", "_")
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $PasswordFile) | Out-Null
        $Stream = [IO.File]::Open($PasswordFile, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $Writer = [IO.StreamWriter]::new($Stream, [Text.UTF8Encoding]::new($false))
        try {
            $Writer.Write($Password)
        } finally {
            $Writer.Dispose()
            $Stream.Dispose()
        }
        $Created = $true
    }

    if ($Password.Length -lt 6 -or $Password.Contains("`r") -or $Password.Contains("`n")) {
        throw "keystore 口令文件必须只包含一行且长度至少为 6: $PasswordFile"
    }

    [pscustomobject]@{
        Password = $Password
        Created = $Created
    }
}
# //// /读取或创建 artifacts 中的运行时签名口令 ////

# //// 完成 CN 客户端版本地址补丁和运行时签名 [@x380kkm 2026-07-21] ////
$Paths = Get-ProtocolLabPaths
$ResolvedInputApkPath = [IO.Path]::GetFullPath($InputApkPath)
Assert-ProtocolLabFile -Path $ResolvedInputApkPath -Description "CN 客户端 APK"
if ([IO.Path]::GetExtension($ResolvedInputApkPath) -ine ".apk") {
    throw "客户端输入文件不是 APK: $ResolvedInputApkPath"
}

$HostKind = [Uri]::CheckHostName($ServerHost)
if ($HostKind -eq [UriHostNameType]::Unknown) {
    throw "服务器 Host 不是有效的 DNS 名称或 IP 地址: $ServerHost"
}
if (-not (Test-ClientPatchAndroidServerHost -ServerHost $ServerHost)) {
    throw "Android 模拟器不能访问宿主回环地址, 请使用 10.0.2.2 或可达的局域网地址: $ServerHost"
}
$TargetVersionUrl = ([UriBuilder]::new("http", $ServerHost, $Port, "shijtswy/version/")).Uri.AbsoluteUri
$TargetApiEndpoint = "$ServerHost`:$Port"
$TargetApiExpression = 'ApiServerKind.Custom("http","' + $TargetApiEndpoint + '")'

$Timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
if ([string]::IsNullOrWhiteSpace($WorkingDirectory)) {
    $WorkingDirectory = Join-Path $Paths.ArtifactsRoot "client-patching\$Timestamp"
}
$WorkingDirectory = [IO.Path]::GetFullPath($WorkingDirectory)
if (Test-Path -LiteralPath $WorkingDirectory) {
    throw "客户端补丁工作目录已存在: $WorkingDirectory"
}
if ([string]::IsNullOrWhiteSpace($OutputApkPath)) {
    $OutputApkPath = Join-Path $WorkingDirectory "worldflipper-cn-patched.apk"
}
$ResolvedOutputApkPath = [IO.Path]::GetFullPath($OutputApkPath)
if ([IO.Path]::GetExtension($ResolvedOutputApkPath) -ine ".apk") {
    throw "客户端输出文件不是 APK: $ResolvedOutputApkPath"
}
if ($ResolvedOutputApkPath -eq $ResolvedInputApkPath) {
    throw "客户端输入和输出 APK 不能是同一个文件: $ResolvedInputApkPath"
}
if (Test-Path -LiteralPath $ResolvedOutputApkPath) {
    throw "客户端输出 APK 已存在: $ResolvedOutputApkPath"
}

if ([string]::IsNullOrWhiteSpace($StarviewToolsRoot)) {
    $StarviewToolsRoot = Join-Path $Paths.WorkspaceRoot "artifacts\starview-tools\starview-windows"
}
$StarviewToolsRoot = [IO.Path]::GetFullPath($StarviewToolsRoot)
$ResolvedJavaHome = Resolve-ProtocolLabJavaHome -JavaHome $JavaHome
$FfdecJarPath = Join-Path $StarviewToolsRoot "ffdec\ffdec.jar"
$FfdecLibraryJarPath = Join-Path $StarviewToolsRoot "ffdec\lib\ffdec_lib.jar"
$ZipalignPath = Join-Path $StarviewToolsRoot "build-tools\zipalign.exe"
$ApkSignerJarPath = Join-Path $StarviewToolsRoot "build-tools\lib\apksigner.jar"
$BuildToolsPropertiesPath = Join-Path $StarviewToolsRoot "build-tools\source.properties"
$JavaPath = Join-Path $ResolvedJavaHome "bin\java.exe"
$JavacPath = Join-Path $ResolvedJavaHome "bin\javac.exe"
$KeytoolPath = Join-Path $ResolvedJavaHome "bin\keytool.exe"
$RemoteUtilAbcPatchSourcePath = Join-Path $PSScriptRoot "RemoteUtilAbcPatch.java"
$AbcMethodDigestSourcePath = Join-Path $PSScriptRoot "AbcMethodDigest.java"
$RemoteUtilMethodDigestSourcePath = Join-Path $PSScriptRoot "RemoteUtilMethodDigest.java"
$RemoteUtilDecodeDiagnosticPatchSourcePath = Join-Path $PSScriptRoot "RemoteUtilDecodeDiagnosticPatch.java"
$AssetExtractorPreextractedBundlePatchSourcePath = Join-Path $PSScriptRoot "AssetExtractorPreextractedBundlePatch.java"
$GbitsVersionQueryProbePatchSourcePath = Join-Path $PSScriptRoot "GbitsVersionQueryProbePatch.java"
$GbitsVersionUrlAbcPatchSourcePath = Join-Path $PSScriptRoot "GbitsVersionUrlAbcPatch.java"
foreach ($Tool in @(
    @($FfdecJarPath, "FFDec jar"),
    @($ZipalignPath, "zipalign"),
    @($ApkSignerJarPath, "apksigner jar"),
    @($BuildToolsPropertiesPath, "Android build-tools properties"),
    @($JavaPath, "Java"),
    @($KeytoolPath, "keytool")
)) {
    Assert-ProtocolLabFile -Path $Tool[0] -Description $Tool[1]
}
if ([bool]$ProbeSdkDummyLoginRequest) {
    foreach ($Tool in @(
        @($FfdecLibraryJarPath, "FFDec library jar"),
        @($JavacPath, "Java compiler"),
        @($RemoteUtilAbcPatchSourcePath, "RemoteUtil ABC patch source"),
        @($AbcMethodDigestSourcePath, "ABC method digest source"),
        @($RemoteUtilMethodDigestSourcePath, "RemoteUtil method digest source")
    )) {
        Assert-ProtocolLabFile -Path $Tool[0] -Description $Tool[1]
    }
}
if ($VersionUrlPatchEnabled) {
    foreach ($Tool in @(
        @($FfdecLibraryJarPath, "FFDec library jar"),
        @($JavacPath, "Java compiler"),
        @($AbcMethodDigestSourcePath, "ABC method digest source"),
        @($GbitsVersionUrlAbcPatchSourcePath, "GbitsVersionLogic URL patch source")
    )) {
        Assert-ProtocolLabFile -Path $Tool[0] -Description $Tool[1]
    }
}
if ([bool]$ProbeMessagePackDecodePosition) {
    Assert-ProtocolLabFile -Path $RemoteUtilDecodeDiagnosticPatchSourcePath -Description "RemoteUtil decode diagnostic patch source"
}
if ([bool]$UsePreextractedBundle) {
    foreach ($Tool in @(
        @($FfdecLibraryJarPath, "FFDec library jar"),
        @($JavacPath, "Java compiler"),
        @($AbcMethodDigestSourcePath, "ABC method digest source"),
        @($AssetExtractorPreextractedBundlePatchSourcePath, "AssetExtractor preextracted bundle patch source")
    )) {
        Assert-ProtocolLabFile -Path $Tool[0] -Description $Tool[1]
    }
}

$DefaultSigningDirectory = Join-Path $Paths.ArtifactsRoot "signing"
if ([string]::IsNullOrWhiteSpace($KeystorePath)) {
    $KeystorePath = Join-Path $DefaultSigningDirectory "cn-client-runtime.p12"
}
if ([string]::IsNullOrWhiteSpace($KeystorePasswordFile)) {
    $KeystorePasswordFile = Join-Path $DefaultSigningDirectory "cn-client-runtime.pass"
}
$KeystorePath = [IO.Path]::GetFullPath($KeystorePath)
$KeystorePasswordFile = [IO.Path]::GetFullPath($KeystorePasswordFile)
if (Test-ClientPatchPathWithinDirectory -Path $KeystorePath -Directory $Paths.RepositoryRoot) {
    throw "keystore 必须位于仓库外: $KeystorePath"
}
if (Test-ClientPatchPathWithinDirectory -Path $KeystorePasswordFile -Directory $Paths.RepositoryRoot) {
    throw "keystore 口令文件必须位于仓库外: $KeystorePasswordFile"
}
if ($KeystorePath -eq $KeystorePasswordFile) {
    throw "keystore 和口令文件不能使用同一路径: $KeystorePath"
}

$EvidenceDirectory = Join-Path $WorkingDirectory "evidence"
$OriginalSwfPath = Join-Path $WorkingDirectory "original\worldflipper_android_release.swf"
$ExportDirectory = Join-Path $WorkingDirectory "ffdec-export"
$IntermediateSwfPath = Join-Path $WorkingDirectory "patched\worldflipper_android_intermediate.swf"
$PatchedSwfPath = Join-Path $WorkingDirectory "patched\worldflipper_android_release.swf"
$RemoteUtilPatchedSwfPath = Join-Path $WorkingDirectory "patched\worldflipper_android_remoteutil.swf"
$VersionUrlPatchedSwfPath = Join-Path $WorkingDirectory "patched\worldflipper_android_version_url.swf"
$VersionQueryPatchedSwfPath = Join-Path $WorkingDirectory "patched\worldflipper_android_version_query.swf"
$DecodeDiagnosticPatchedSwfPath = Join-Path $WorkingDirectory "patched\worldflipper_android_decode_diagnostic.swf"
$VerificationDirectory = Join-Path $WorkingDirectory "ffdec-verify"
$RemoteUtilAbcPatchClassesDirectory = Join-Path $WorkingDirectory "remoteutil-abc-patch-classes"
$RemoteUtilAbcPatchEvidencePath = Join-Path $EvidenceDirectory "remoteutil-abc-patch.json"
$RemoteUtilDecodeDiagnosticClassesDirectory = Join-Path $WorkingDirectory "remoteutil-decode-diagnostic-classes"
$RemoteUtilDecodeDiagnosticEvidencePath = Join-Path $EvidenceDirectory "remoteutil-decode-diagnostic.json"
$AssetExtractorPreextractedBundleClassesDirectory = Join-Path $WorkingDirectory "asset-extractor-preextracted-bundle-classes"
$AssetExtractorPreextractedBundleEvidencePath = Join-Path $EvidenceDirectory "asset-extractor-preextracted-bundle.json"
$GbitsVersionQueryProbeClassesDirectory = Join-Path $WorkingDirectory "gbits-version-query-probe-classes"
$GbitsVersionQueryProbeEvidencePath = Join-Path $EvidenceDirectory "gbits-version-query-probe.json"
$GbitsVersionUrlAbcPatchClassesDirectory = Join-Path $WorkingDirectory "gbits-version-url-abc-patch-classes"
$GbitsVersionUrlAbcPatchEvidencePath = Join-Path $EvidenceDirectory "gbits-version-url-abc-patch.json"
$UnsignedApkPath = Join-Path $WorkingDirectory "unsigned.apk"
$AlignedApkPath = Join-Path $WorkingDirectory "aligned.apk"
$SdkDummyLoginRequestProbeMode = Get-ClientPatchSdkDummyLoginRequestProbeMode -ProbeSdkDummyLoginRequest ([bool]$ProbeSdkDummyLoginRequest)
$VersionQueryProbeMode = Get-ClientPatchVersionQueryProbeMode `
    -ProbeVersionQuery ([bool]$ProbeVersionQuery) `
    -ProbeSdkDummyLoginRequest ([bool]$ProbeSdkDummyLoginRequest)
$MessagePackDecodeDiagnosticMode = Get-ClientPatchMessagePackDecodeDiagnosticMode `
    -ProbeMessagePackDecodePosition ([bool]$ProbeMessagePackDecodePosition) `
    -ProbeSdkDummyLoginRequest ([bool]$ProbeSdkDummyLoginRequest)
$PreextractedBundleMode = Get-ClientPatchPreextractedBundleMode -UsePreextractedBundle ([bool]$UsePreextractedBundle)
$PatchedClassNames = Get-ClientPatchSelectedClassNames `
    -PatchMode $SdkDummyLoginRequestProbeMode `
    -VersionClassName $VersionClassName `
    -ApiConfigClassName $ApiConfigClassName `
    -DevConfigClassName $DevConfigClassName `
    -TitleSceneClassName $TitleSceneClassName `
    -ChannelMainClassName $ChannelMainClassName `
    -ChannelDummyClassName $ChannelDummyClassName `
    -SkipVersionClass
$SelectedClasses = $PatchedClassNames -join ","
New-Item -ItemType Directory -Force -Path $EvidenceDirectory, (Split-Path -Parent $ResolvedOutputApkPath) | Out-Null

$OriginalJavaHome = $env:JAVA_HOME
$OriginalPath = $env:PATH
$PasswordEnvironmentVariable = "STARPOINT_CN_PATCH_KEYSTORE_PASSWORD"
$OriginalPasswordEnvironmentValue = [Environment]::GetEnvironmentVariable($PasswordEnvironmentVariable, "Process")
try {
    $env:JAVA_HOME = $ResolvedJavaHome
    $env:PATH = (Join-Path $ResolvedJavaHome "bin") + ";" + $OriginalPath

    $FfdecVersionResult = Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @("-jar", $FfdecJarPath, "-help") -LogPath (Join-Path $EvidenceDirectory "ffdec-version.log")
    $FfdecVersion = @($FfdecVersionResult.Output | Where-Object { $_ -match "^JPEXS Free Flash Decompiler v\." } | Select-Object -First 1)
    if ($FfdecVersion.Count -ne 1) {
        throw "无法从 FFDec 输出读取版本: $FfdecJarPath"
    }
    $ApkSignerVersionResult = Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @("-jar", $ApkSignerJarPath, "version") -LogPath (Join-Path $EvidenceDirectory "apksigner-version.log")
    $JavaVersionResult = Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @("-version") -LogPath (Join-Path $EvidenceDirectory "java-version.log")
    $JavacVersionResult = if ($VersionUrlPatchEnabled -or $SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard `
        -or $VersionQueryProbeMode.PatchVersionQuerySuccess `
        -or $PreextractedBundleMode.PatchAssetExtractorStart) {
        Invoke-ClientPatchTool -FilePath $JavacPath -ArgumentList @("-version") -LogPath (Join-Path $EvidenceDirectory "javac-version.log")
    } else {
        $null
    }
    $KeytoolVersionResult = Invoke-ClientPatchTool -FilePath $KeytoolPath -ArgumentList @("-J-version") -LogPath (Join-Path $EvidenceDirectory "keytool-version.log")
    $BuildToolsRevisionLine = Get-Content -LiteralPath $BuildToolsPropertiesPath | Where-Object { $_ -match "^Pkg\.Revision=" } | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($BuildToolsRevisionLine)) {
        throw "Android build-tools properties 缺少 Pkg.Revision: $BuildToolsPropertiesPath"
    }
    $BuildToolsRevision = $BuildToolsRevisionLine.Substring($BuildToolsRevisionLine.IndexOf("=") + 1).Trim()

    $InputEvidence = Get-ClientPatchFileEvidence -Path $ResolvedInputApkPath
    $InputSwfEvidence = Get-ClientPatchZipEntryEvidence -ApkPath $ResolvedInputApkPath -EntryName $SwfEntryName
    $InputAndroidManifestEvidence = Get-ClientPatchZipEntryEvidence -ApkPath $ResolvedInputApkPath -EntryName "AndroidManifest.xml"
    Export-ClientPatchZipEntry -ApkPath $ResolvedInputApkPath -EntryName $SwfEntryName -OutputPath $OriginalSwfPath
    if ($VersionUrlPatchEnabled) {
        New-Item -ItemType Directory -Force -Path $GbitsVersionUrlAbcPatchClassesDirectory | Out-Null
        Invoke-ClientPatchTool -FilePath $JavacPath -ArgumentList @(
            "-encoding", "UTF-8",
            "-cp", $FfdecLibraryJarPath,
            "-d", $GbitsVersionUrlAbcPatchClassesDirectory,
            $GbitsVersionUrlAbcPatchSourcePath,
            $AbcMethodDigestSourcePath
        ) -LogPath (Join-Path $EvidenceDirectory "gbits-version-url-abc-patch-compile.log") | Out-Null
    }
    if ($VersionQueryProbeMode.PatchVersionQuerySuccess) {
        New-Item -ItemType Directory -Force -Path $GbitsVersionQueryProbeClassesDirectory | Out-Null
        Invoke-ClientPatchTool -FilePath $JavacPath -ArgumentList @(
            "-encoding", "UTF-8",
            "-cp", $FfdecLibraryJarPath,
            "-d", $GbitsVersionQueryProbeClassesDirectory,
            $GbitsVersionQueryProbePatchSourcePath,
            $AbcMethodDigestSourcePath
        ) -LogPath (Join-Path $EvidenceDirectory "gbits-version-query-probe-compile.log") | Out-Null
    }

    $VersionUrlAbcEvidence = if ($VersionUrlPatchEnabled) {
        $FfdecLibraryDirectory = Split-Path -Parent $FfdecLibraryJarPath
        $GbitsVersionUrlAbcClassPath = "$GbitsVersionUrlAbcPatchClassesDirectory;$FfdecLibraryDirectory\*"
        Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
            "-cp", $GbitsVersionUrlAbcClassPath,
            "GbitsVersionUrlAbcPatch",
            $OriginalSwfPath,
            $OriginalSwfPath,
            $VersionUrlPatchedSwfPath,
            $GbitsVersionUrlAbcPatchEvidencePath,
            $TargetVersionUrl,
            $TargetVersionUrl
        ) -LogPath (Join-Path $EvidenceDirectory "gbits-version-url-abc-patch.log") | Out-Null
        Assert-ProtocolLabFile -Path $GbitsVersionUrlAbcPatchEvidencePath -Description "GbitsVersionLogic 版本地址补丁证据"
        $Evidence = Get-Content -LiteralPath $GbitsVersionUrlAbcPatchEvidencePath -Raw -Encoding UTF8 | ConvertFrom-Json
        Assert-ClientPatchVersionUrlAbcEvidence -Evidence $Evidence -TargetUrl $TargetVersionUrl | Out-Null
        $Evidence
    }
    Assert-ProtocolLabFile -Path $VersionUrlPatchedSwfPath -Description "版本地址补丁输出 SWF"

    New-Item -ItemType Directory -Force -Path $ExportDirectory | Out-Null
    Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
        "-jar", $FfdecJarPath,
        "-onerror", "abort",
        "-selectclass", $SelectedClasses,
        "-export", "script", $ExportDirectory, $VersionUrlPatchedSwfPath
    ) -LogPath (Join-Path $EvidenceDirectory "ffdec-export.log") | Out-Null
    $ApiConfigScript = Get-ClientPatchApiScript -ExportDirectory $ExportDirectory -ScriptName $ApiConfigScriptName
    $DevConfigScript = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummy) {
        Get-ClientPatchDevConfigScript -ExportDirectory $ExportDirectory -ScriptName $DevConfigScriptName
    } else {
        $null
    }
    $TitleSceneScript = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard) {
        Get-ClientPatchTitleSceneScript -ExportDirectory $ExportDirectory -ScriptName $TitleSceneScriptName
    } else {
        $null
    }
    $ChannelMainScript = if ($SdkDummyLoginRequestProbeMode.PatchChannelMainForSdkDummy) {
        Get-ClientPatchChannelMainScript -ExportDirectory $ExportDirectory -ScriptName $ChannelMainScriptName
    } else {
        $null
    }
    $ChannelDummyScript = if ($SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe) {
        Get-ClientPatchChannelDummyScript -ExportDirectory $ExportDirectory -ScriptName $ChannelDummyScriptName
    } else {
        $null
    }
    $ApiConfigBeforeEvidence = Get-ClientPatchFileEvidence -Path $ApiConfigScript.FullName
    $ApiReplacements = @(Set-ClientPatchApiEndpoint -ScriptPath $ApiConfigScript.FullName -OriginalExpressions $DefaultApiExpressions -TargetExpression $TargetApiExpression)
    Assert-ClientPatchApiEndpoint -ScriptPath $ApiConfigScript.FullName -OriginalExpressions $DefaultApiExpressions -TargetExpression $TargetApiExpression
    $ApiConfigAfterEvidence = Get-ClientPatchFileEvidence -Path $ApiConfigScript.FullName
    $SdkDummyBeforeEvidence = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummy) {
        Get-ClientPatchFileEvidence -Path $DevConfigScript.FullName
    } else {
        $null
    }
    $SdkDummyReplacement = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummy) {
        Set-ClientPatchSdkDummy -ScriptPath $DevConfigScript.FullName
    } else {
        $null
    }
    $SdkDummyNativeExtensionGuardBeforeEvidence = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyNativeExtensionGuard) {
        Get-ClientPatchFileEvidence -Path $DevConfigScript.FullName
    } else {
        $null
    }
    $SdkDummyNativeExtensionGuardReplacement = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyNativeExtensionGuard) {
        Set-ClientPatchSdkDummyNativeExtensionGuard -ScriptPath $DevConfigScript.FullName
    } else {
        $null
    }
    $SdkDummyAfterEvidence = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummy) {
        Assert-ClientPatchSdkDummy -ScriptPath $DevConfigScript.FullName | Out-Null
        if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyNativeExtensionGuard) {
            Assert-ClientPatchSdkDummyNativeExtensionGuard -ScriptPath $DevConfigScript.FullName
        }
        Get-ClientPatchFileEvidence -Path $DevConfigScript.FullName
    } else {
        $null
    }
    $TitleSceneBeforeEvidence = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard) {
        Get-ClientPatchFileEvidence -Path $TitleSceneScript.FullName
    } else {
        $null
    }
    $SdkDummyTitleSceneGuardReplacement = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard) {
        Set-ClientPatchSdkDummyTitleSceneGuard -ScriptPath $TitleSceneScript.FullName
    } else {
        $null
    }
    $TitleSceneAfterEvidence = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard) {
        Assert-ClientPatchSdkDummyTitleSceneGuard -ScriptPath $TitleSceneScript.FullName | Out-Null
        Get-ClientPatchFileEvidence -Path $TitleSceneScript.FullName
    } else {
        $null
    }
    $ChannelMainBeforeEvidence = if ($SdkDummyLoginRequestProbeMode.PatchChannelMainForSdkDummy) {
        Get-ClientPatchFileEvidence -Path $ChannelMainScript.FullName
    } else {
        $null
    }
    $SdkDummyRealRemoteBridgeReplacement = if ($SdkDummyLoginRequestProbeMode.PatchChannelMainForSdkDummy) {
        Set-ClientPatchSdkDummyRealRemoteBridge -ScriptPath $ChannelMainScript.FullName
    } else {
        $null
    }
    $ChannelMainAfterEvidence = if ($SdkDummyLoginRequestProbeMode.PatchChannelMainForSdkDummy) {
        Assert-ClientPatchSdkDummyRealRemoteBridge -ScriptPath $ChannelMainScript.FullName
        Get-ClientPatchFileEvidence -Path $ChannelMainScript.FullName
    } else {
        $null
    }
    $ChannelDummyBeforeEvidence = if ($SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe) {
        Get-ClientPatchFileEvidence -Path $ChannelDummyScript.FullName
    } else {
        $null
    }
    $SdkDummyLoginRequestProbeReplacement = if ($SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe) {
        Set-ClientPatchSdkDummyLoginRequestProbe -ScriptPath $ChannelDummyScript.FullName
    } else {
        $null
    }
    $ChannelDummyAfterEvidence = if ($SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe) {
        Assert-ClientPatchSdkDummyLoginRequestProbe -ScriptPath $ChannelDummyScript.FullName
        Get-ClientPatchFileEvidence -Path $ChannelDummyScript.FullName
    } else {
        $null
    }

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $IntermediateSwfPath) | Out-Null
    Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
        "-jar", $FfdecJarPath,
        "-onerror", "abort",
        "-importScript", $VersionUrlPatchedSwfPath, $IntermediateSwfPath, $ExportDirectory
    ) -LogPath (Join-Path $EvidenceDirectory "ffdec-import.log") | Out-Null
    Assert-ProtocolLabFile -Path $IntermediateSwfPath -Description "FFDec 中间 SWF"
    $IntermediateSwfEvidence = Get-ClientPatchFileEvidence -Path $IntermediateSwfPath

    $RemoteUtilAbcPatchEvidence = if ($SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard) {
        New-Item -ItemType Directory -Force -Path $RemoteUtilAbcPatchClassesDirectory | Out-Null
        Invoke-ClientPatchTool -FilePath $JavacPath -ArgumentList @(
            "-encoding", "UTF-8",
            "-cp", $FfdecLibraryJarPath,
            "-d", $RemoteUtilAbcPatchClassesDirectory,
            $RemoteUtilAbcPatchSourcePath,
            $AbcMethodDigestSourcePath,
            $RemoteUtilMethodDigestSourcePath
        ) -LogPath (Join-Path $EvidenceDirectory "remoteutil-abc-patch-compile.log") | Out-Null
        $FfdecLibraryDirectory = Split-Path -Parent $FfdecLibraryJarPath
        $RemoteUtilAbcPatchClassPath = "$RemoteUtilAbcPatchClassesDirectory;$FfdecLibraryDirectory\*"
        Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
            "-cp", $RemoteUtilAbcPatchClassPath,
            "RemoteUtilAbcPatch",
            $OriginalSwfPath,
            $IntermediateSwfPath,
            $RemoteUtilPatchedSwfPath,
            $RemoteUtilAbcPatchEvidencePath
        ) -LogPath (Join-Path $EvidenceDirectory "remoteutil-abc-patch.log") | Out-Null
        Assert-ProtocolLabFile -Path $RemoteUtilAbcPatchEvidencePath -Description "RemoteUtil ABC 补丁证据"
        Get-Content -LiteralPath $RemoteUtilAbcPatchEvidencePath -Raw -Encoding UTF8 | ConvertFrom-Json
    } else {
        Copy-Item -LiteralPath $IntermediateSwfPath -Destination $RemoteUtilPatchedSwfPath
        $null
    }
    Assert-ProtocolLabFile -Path $RemoteUtilPatchedSwfPath -Description "RemoteUtil 补丁输出 SWF"
    $RemoteUtilAbcPatchVerification = if ($SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard) {
        Assert-ClientPatchRemoteUtilAbcEvidence -Evidence $RemoteUtilAbcPatchEvidence
    } else {
        $null
    }
    $RemoteUtilAbcPatchEvidenceFile = if ($SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard) {
        Get-ClientPatchFileEvidence -Path $RemoteUtilAbcPatchEvidencePath
    } else {
        $null
    }

    $GbitsVersionQueryProbeEvidence = if ($VersionQueryProbeMode.PatchVersionQuerySuccess) {
        $FfdecLibraryDirectory = Split-Path -Parent $FfdecLibraryJarPath
        $GbitsVersionQueryProbeClassPath = "$GbitsVersionQueryProbeClassesDirectory;$FfdecLibraryDirectory\*"
        Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
            "-cp", $GbitsVersionQueryProbeClassPath,
            "GbitsVersionQueryProbePatch",
            $VersionUrlPatchedSwfPath,
            $RemoteUtilPatchedSwfPath,
            $VersionQueryPatchedSwfPath,
            $GbitsVersionQueryProbeEvidencePath
        ) -LogPath (Join-Path $EvidenceDirectory "gbits-version-query-probe.log") | Out-Null
        Assert-ProtocolLabFile -Path $GbitsVersionQueryProbeEvidencePath -Description "GbitsVersionLogic 版本查询补丁证据"
        $Evidence = Get-Content -LiteralPath $GbitsVersionQueryProbeEvidencePath -Raw -Encoding UTF8 | ConvertFrom-Json
        Assert-ClientPatchVersionQueryProbeEvidence -Evidence $Evidence | Out-Null
        $Evidence
    } else {
        Copy-Item -LiteralPath $RemoteUtilPatchedSwfPath -Destination $VersionQueryPatchedSwfPath
        $null
    }
    Assert-ProtocolLabFile -Path $VersionQueryPatchedSwfPath -Description "版本查询补丁输出 SWF"
    $GbitsVersionQueryProbeEvidenceFile = if ($VersionQueryProbeMode.PatchVersionQuerySuccess) {
        Get-ClientPatchFileEvidence -Path $GbitsVersionQueryProbeEvidencePath
    } else {
        $null
    }

    $RemoteUtilDecodeDiagnosticEvidence = if ($MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError) {
        New-Item -ItemType Directory -Force -Path $RemoteUtilDecodeDiagnosticClassesDirectory | Out-Null
        Invoke-ClientPatchTool -FilePath $JavacPath -ArgumentList @(
            "-encoding", "UTF-8",
            "-cp", $FfdecLibraryJarPath,
            "-d", $RemoteUtilDecodeDiagnosticClassesDirectory,
            $RemoteUtilDecodeDiagnosticPatchSourcePath,
            $AbcMethodDigestSourcePath,
            $RemoteUtilMethodDigestSourcePath
        ) -LogPath (Join-Path $EvidenceDirectory "remoteutil-decode-diagnostic-compile.log") | Out-Null
        $FfdecLibraryDirectory = Split-Path -Parent $FfdecLibraryJarPath
        $RemoteUtilDecodeDiagnosticClassPath = "$RemoteUtilDecodeDiagnosticClassesDirectory;$FfdecLibraryDirectory\*"
        Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
            "-cp", $RemoteUtilDecodeDiagnosticClassPath,
            "RemoteUtilDecodeDiagnosticPatch",
            $OriginalSwfPath,
            $VersionQueryPatchedSwfPath,
            $DecodeDiagnosticPatchedSwfPath,
            $RemoteUtilDecodeDiagnosticEvidencePath
        ) -LogPath (Join-Path $EvidenceDirectory "remoteutil-decode-diagnostic.log") | Out-Null
        Assert-ProtocolLabFile -Path $RemoteUtilDecodeDiagnosticEvidencePath -Description "RemoteUtil 解码位置诊断证据"
        $Evidence = Get-Content -LiteralPath $RemoteUtilDecodeDiagnosticEvidencePath -Raw -Encoding UTF8 | ConvertFrom-Json
        Assert-ClientPatchRemoteUtilDecodeDiagnosticEvidence -Evidence $Evidence | Out-Null
        $Evidence
    } else {
        Copy-Item -LiteralPath $VersionQueryPatchedSwfPath -Destination $DecodeDiagnosticPatchedSwfPath
        $null
    }
    Assert-ProtocolLabFile -Path $DecodeDiagnosticPatchedSwfPath -Description "响应诊断补丁输出 SWF"
    $RemoteUtilDecodeDiagnosticEvidenceFile = if ($MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError) {
        Get-ClientPatchFileEvidence -Path $RemoteUtilDecodeDiagnosticEvidencePath
    } else {
        $null
    }

    $AssetExtractorPreextractedBundleEvidence = if ($PreextractedBundleMode.PatchAssetExtractorStart) {
        New-Item -ItemType Directory -Force -Path $AssetExtractorPreextractedBundleClassesDirectory | Out-Null
        Invoke-ClientPatchTool -FilePath $JavacPath -ArgumentList @(
            "-encoding", "UTF-8",
            "-cp", $FfdecLibraryJarPath,
            "-d", $AssetExtractorPreextractedBundleClassesDirectory,
            $AssetExtractorPreextractedBundlePatchSourcePath,
            $AbcMethodDigestSourcePath
        ) -LogPath (Join-Path $EvidenceDirectory "asset-extractor-preextracted-bundle-compile.log") | Out-Null
        $FfdecLibraryDirectory = Split-Path -Parent $FfdecLibraryJarPath
        $AssetExtractorPreextractedBundleClassPath = "$AssetExtractorPreextractedBundleClassesDirectory;$FfdecLibraryDirectory\*"
        Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
            "-cp", $AssetExtractorPreextractedBundleClassPath,
            "AssetExtractorPreextractedBundlePatch",
            $OriginalSwfPath,
            $DecodeDiagnosticPatchedSwfPath,
            $PatchedSwfPath,
            $AssetExtractorPreextractedBundleEvidencePath
        ) -LogPath (Join-Path $EvidenceDirectory "asset-extractor-preextracted-bundle.log") | Out-Null
        Assert-ProtocolLabFile -Path $AssetExtractorPreextractedBundleEvidencePath -Description "AssetExtractor 预展开资源补丁证据"
        $Evidence = Get-Content -LiteralPath $AssetExtractorPreextractedBundleEvidencePath -Raw -Encoding UTF8 | ConvertFrom-Json
        Assert-ClientPatchAssetExtractorPreextractedEvidence -Evidence $Evidence | Out-Null
        $Evidence
    } else {
        Copy-Item -LiteralPath $DecodeDiagnosticPatchedSwfPath -Destination $PatchedSwfPath
        $null
    }
    Assert-ProtocolLabFile -Path $PatchedSwfPath -Description "最终补丁输出 SWF"
    $AssetExtractorPreextractedBundleEvidenceFile = if ($PreextractedBundleMode.PatchAssetExtractorStart) {
        Get-ClientPatchFileEvidence -Path $AssetExtractorPreextractedBundleEvidencePath
    } else {
        $null
    }

    New-Item -ItemType Directory -Force -Path $VerificationDirectory | Out-Null
    Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
        "-jar", $FfdecJarPath,
        "-onerror", "abort",
        "-selectclass", $SelectedClasses,
        "-export", "script", $VerificationDirectory, $PatchedSwfPath
    ) -LogPath (Join-Path $EvidenceDirectory "ffdec-verify-export.log") | Out-Null
    $VerificationApiConfigScript = Get-ClientPatchApiScript -ExportDirectory $VerificationDirectory -ScriptName $ApiConfigScriptName
    $VerificationDevConfigScript = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummy) {
        Get-ClientPatchDevConfigScript -ExportDirectory $VerificationDirectory -ScriptName $DevConfigScriptName
    } else {
        $null
    }
    $VerificationTitleSceneScript = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard) {
        Get-ClientPatchTitleSceneScript -ExportDirectory $VerificationDirectory -ScriptName $TitleSceneScriptName
    } else {
        $null
    }
    $VerificationChannelMainScript = if ($SdkDummyLoginRequestProbeMode.PatchChannelMainForSdkDummy) {
        Get-ClientPatchChannelMainScript -ExportDirectory $VerificationDirectory -ScriptName $ChannelMainScriptName
    } else {
        $null
    }
    $VerificationChannelDummyScript = if ($SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe) {
        Get-ClientPatchChannelDummyScript -ExportDirectory $VerificationDirectory -ScriptName $ChannelDummyScriptName
    } else {
        $null
    }
    Assert-ClientPatchApiEndpoint -ScriptPath $VerificationApiConfigScript.FullName -OriginalExpressions $DefaultApiExpressions -TargetExpression $TargetApiExpression
    $SdkDummyVerification = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummy) {
        Assert-ClientPatchSdkDummy -ScriptPath $VerificationDevConfigScript.FullName
    } else {
        $null
    }
    $SdkDummyNativeExtensionGuardVerification = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyNativeExtensionGuard) {
        Assert-ClientPatchSdkDummyNativeExtensionGuard -ScriptPath $VerificationDevConfigScript.FullName
        [pscustomobject][ordered]@{
            DevConfigClassName = $DevConfigClassName
            Verified = $true
        }
    } else {
        $null
    }
    $SdkDummyTitleSceneGuardVerification = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard) {
        Assert-ClientPatchSdkDummyTitleSceneGuard -ScriptPath $VerificationTitleSceneScript.FullName
        [pscustomobject][ordered]@{
            ClassName = $TitleSceneClassName
            ScriptName = $TitleSceneScriptName
            Verified = $true
        }
    } else {
        $null
    }
    $SdkDummyTitleSceneLoginGateVerification = if ($SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard) {
        Assert-ClientPatchTitleSceneLoginGateEvidence -ScriptPath $VerificationTitleSceneScript.FullName
    } else {
        $null
    }
    $SdkDummyRealRemoteBridgeVerification = if ($SdkDummyLoginRequestProbeMode.PatchChannelMainForSdkDummy) {
        Assert-ClientPatchSdkDummyRealRemoteBridge -ScriptPath $VerificationChannelMainScript.FullName
        [pscustomobject][ordered]@{
            ClassName = $ChannelMainClassName
            ScriptName = $ChannelMainScriptName
            Verified = $true
        }
    } else {
        $null
    }
    $SdkDummyLoginRequestProbeVerification = if ($SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe) {
        Assert-ClientPatchSdkDummyLoginRequestProbe -ScriptPath $VerificationChannelDummyScript.FullName
        [pscustomobject][ordered]@{
            ClassName = $ChannelDummyClassName
            ScriptName = $ChannelDummyScriptName
            Verified = $true
        }
    } else {
        $null
    }
    $PatchedSwfEvidence = Get-ClientPatchFileEvidence -Path $PatchedSwfPath

    $RepackResult = New-ClientPatchUnsignedApk -InputApkPath $ResolvedInputApkPath -PatchedSwfPath $PatchedSwfPath -OutputApkPath $UnsignedApkPath -SwfEntryName $SwfEntryName
    Invoke-ClientPatchTool -FilePath $ZipalignPath -ArgumentList @("-f", "-p", "4", $UnsignedApkPath, $AlignedApkPath) -LogPath (Join-Path $EvidenceDirectory "zipalign.log") | Out-Null
    Invoke-ClientPatchTool -FilePath $ZipalignPath -ArgumentList @("-c", "-p", "-v", "4", $AlignedApkPath) -LogPath (Join-Path $EvidenceDirectory "zipalign-aligned-verify.log") | Out-Null
    $AlignedApkEvidence = Get-ClientPatchFileEvidence -Path $AlignedApkPath

    $KeystoreExisted = Test-Path -LiteralPath $KeystorePath -PathType Leaf
    $PasswordState = Get-ClientPatchKeystorePassword -PasswordFile $KeystorePasswordFile -KeystoreExists $KeystoreExisted
    [Environment]::SetEnvironmentVariable($PasswordEnvironmentVariable, $PasswordState.Password, "Process")
    if (-not $KeystoreExisted) {
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $KeystorePath) | Out-Null
        Invoke-ClientPatchTool -FilePath $KeytoolPath -ArgumentList @(
            "-genkeypair", "-noprompt",
            "-alias", $KeyAlias,
            "-keyalg", "RSA",
            "-keysize", "2048",
            "-sigalg", "SHA256withRSA",
            "-validity", $CertificateValidityDays.ToString(),
            "-dname", $CertificateDistinguishedName,
            "-keystore", $KeystorePath,
            "-storetype", $KeystoreType,
            "-storepass:env", $PasswordEnvironmentVariable,
            "-keypass:env", $PasswordEnvironmentVariable
        ) -LogPath (Join-Path $EvidenceDirectory "keytool-generate.log") | Out-Null
    }
    Assert-ProtocolLabFile -Path $KeystorePath -Description "运行时签名 keystore"
    Invoke-ClientPatchTool -FilePath $KeytoolPath -ArgumentList @(
        "-list", "-v",
        "-alias", $KeyAlias,
        "-keystore", $KeystorePath,
        "-storetype", $KeystoreType,
        "-storepass:env", $PasswordEnvironmentVariable
    ) -LogPath (Join-Path $EvidenceDirectory "keytool-certificate.log") | Out-Null

    Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
        "-jar", $ApkSignerJarPath,
        "sign",
        "--ks", $KeystorePath,
        "--ks-type", $KeystoreType,
        "--ks-key-alias", $KeyAlias,
        "--ks-pass", "env:$PasswordEnvironmentVariable",
        "--key-pass", "env:$PasswordEnvironmentVariable",
        "--v4-signing-enabled", "false",
        "--out", $ResolvedOutputApkPath,
        $AlignedApkPath
    ) -LogPath (Join-Path $EvidenceDirectory "apksigner-sign.log") | Out-Null
    Invoke-ClientPatchTool -FilePath $JavaPath -ArgumentList @(
        "-jar", $ApkSignerJarPath,
        "verify", "--verbose", "--print-certs", $ResolvedOutputApkPath
    ) -LogPath (Join-Path $EvidenceDirectory "apksigner-verify.log") | Out-Null
    Invoke-ClientPatchTool -FilePath $ZipalignPath -ArgumentList @(
        "-c", "-p", "-v", "4", $ResolvedOutputApkPath
    ) -LogPath (Join-Path $EvidenceDirectory "zipalign-output-verify.log") | Out-Null

    $OutputEvidence = Get-ClientPatchFileEvidence -Path $ResolvedOutputApkPath
    $OutputSwfEvidence = Get-ClientPatchZipEntryEvidence -ApkPath $ResolvedOutputApkPath -EntryName $SwfEntryName
    $OutputAndroidManifestEvidence = Get-ClientPatchZipEntryEvidence -ApkPath $ResolvedOutputApkPath -EntryName "AndroidManifest.xml"
    if ($OutputSwfEvidence.Sha256 -ne $PatchedSwfEvidence.Sha256) {
        throw "输出 APK 中的 SWF 哈希不匹配: expected=$($PatchedSwfEvidence.Sha256) actual=$($OutputSwfEvidence.Sha256)"
    }
    if ($OutputAndroidManifestEvidence.Sha256 -ne $InputAndroidManifestEvidence.Sha256) {
        throw "输出 APK 的 AndroidManifest.xml 已改变: input=$($InputAndroidManifestEvidence.Sha256) output=$($OutputAndroidManifestEvidence.Sha256)"
    }

    $Manifest = [ordered]@{
        SchemaVersion = 1
        PatchedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
        WorkingDirectory = $WorkingDirectory
        Target = [ordered]@{
            Host = $ServerHost
            Port = $Port
            ApiEndpoint = $TargetApiEndpoint
            VersionBaseUrl = $TargetVersionUrl
        }
        Input = [ordered]@{
            Apk = $InputEvidence
            SwfEntry = $InputSwfEvidence
            AndroidManifestEntry = $InputAndroidManifestEvidence
        }
        Patch = [ordered]@{
            ClassNames = $PatchedClassNames
            VersionUrlAbc = [ordered]@{
                Enabled = [bool]$VersionUrlPatchEnabled
                EvidenceFile = if ($VersionUrlPatchEnabled) { Get-ClientPatchFileEvidence -Path $GbitsVersionUrlAbcPatchEvidencePath } else { $null }
                Evidence = $VersionUrlAbcEvidence
                Verification = if ($VersionUrlPatchEnabled) { Assert-ClientPatchVersionUrlAbcEvidence -Evidence $VersionUrlAbcEvidence -TargetUrl $TargetVersionUrl } else { $null }
            }
            ApiConfigSourceBefore = $ApiConfigBeforeEvidence
            ApiConfigSourceAfter = $ApiConfigAfterEvidence
            ApiReplacements = $ApiReplacements
            SdkLoginMode = $SdkDummyLoginRequestProbeMode.Name
            SdkDummyLoginRequestProbeEnabled = [bool]$SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe
            SdkDummyNativeExtensionGuardEnabled = [bool]$SdkDummyLoginRequestProbeMode.PatchSdkDummyNativeExtensionGuard
            RemoteUtilRequestGuardEnabled = [bool]$SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard
            VersionQueryProbeEnabled = [bool]$VersionQueryProbeMode.PatchVersionQuerySuccess
            MessagePackDecodePositionEnabled = [bool]$MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError
            MessagePackInternalErrorMessageEnabled = [bool]$MessagePackDecodeDiagnosticMode.PatchDisplayableErrorMessage
            PreextractedBundleEnabled = [bool]$PreextractedBundleMode.PatchAssetExtractorStart
            SdkDummyTitleSceneGuardEnabled = [bool]$SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard
            SdkDummyTitleSceneLoginGate = $SdkDummyTitleSceneLoginGateVerification
            SdkDummyLoginRequestProbe = [ordered]@{
                ClassName = $ChannelDummyClassName
                ScriptName = $ChannelDummyScriptName
                Enabled = [bool]$SdkDummyLoginRequestProbeMode.PatchDummyLoginRequestProbe
                SourceBefore = $ChannelDummyBeforeEvidence
                SourceAfter = $ChannelDummyAfterEvidence
                Replacement = $SdkDummyLoginRequestProbeReplacement
                StaticVerification = $SdkDummyLoginRequestProbeVerification
                DynamicVerificationPerformed = $false
                VerifiesServerResponseOrSdkLoginState = $false
            }
            SdkDummy = [ordered]@{
                ClassName = $DevConfigClassName
                ScriptName = $DevConfigScriptName
                Enabled = [bool]$SdkDummyLoginRequestProbeMode.PatchSdkDummy
                SourceBefore = $SdkDummyBeforeEvidence
                SourceAfter = $SdkDummyAfterEvidence
                Replacement = $SdkDummyReplacement
                Verification = $SdkDummyVerification
            }
            SdkDummyNativeExtensionGuard = [ordered]@{
                Enabled = [bool]$SdkDummyLoginRequestProbeMode.PatchSdkDummyNativeExtensionGuard
                DevConfig = [ordered]@{
                    ClassName = $DevConfigClassName
                    ScriptName = $DevConfigScriptName
                    SourceBefore = $SdkDummyNativeExtensionGuardBeforeEvidence
                    SourceAfter = $SdkDummyAfterEvidence
                    Replacement = $SdkDummyNativeExtensionGuardReplacement
                }
                Verification = $SdkDummyNativeExtensionGuardVerification
                DynamicVerified = $false
            }
            RemoteUtilRequestGuard = [ordered]@{
                ClassName = $RemoteUtilClassName
                MethodName = "getURLRequest"
                Strategy = "abc-method-body"
                Enabled = [bool]$SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard
                IntermediateSwf = $IntermediateSwfEvidence
                EvidenceFile = $RemoteUtilAbcPatchEvidenceFile
                Evidence = $RemoteUtilAbcPatchEvidence
                Verification = $RemoteUtilAbcPatchVerification
                ResponseObservation = "ChannelSDKDummy.loginSuccessHandler and subsequent HTTP requests"
                DynamicVerified = $false
            }
            VersionQueryProbe = [ordered]@{
                ClassName = $VersionClassName
                MethodName = "isQuerySuccess"
                Strategy = "abc-method-body"
                Enabled = [bool]$VersionQueryProbeMode.PatchVersionQuerySuccess
                EvidenceFile = $GbitsVersionQueryProbeEvidenceFile
                Evidence = $GbitsVersionQueryProbeEvidence
                RequiresSdkDummyLoginProbe = [bool]$VersionQueryProbeMode.PatchVersionQuerySuccess
                VerifiesVersionRequest = $false
                DynamicVerified = $false
            }
            MessagePackDecodePosition = [ordered]@{
                ClassName = $RemoteUtilClassName
                MethodName = "requestCompleteHandler"
                Strategy = "abc-method-body"
                Enabled = [bool]$MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError
                EvidenceFile = $RemoteUtilDecodeDiagnosticEvidenceFile
                Evidence = $RemoteUtilDecodeDiagnosticEvidence
                ChangesErrorTextOnly = [bool]$MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError
                DisplaysInternalErrorMessage = [bool]$MessagePackDecodeDiagnosticMode.PatchDisplayableErrorMessage
                InternalErrorMessage = [ordered]@{
                    ClassName = "pinball.common.error.DisplayableError"
                    MethodName = "getDisplayMessage"
                    Strategy = "abc-method-body"
                    Enabled = [bool]$MessagePackDecodeDiagnosticMode.PatchDisplayableErrorMessage
                    Evidence = if ($MessagePackDecodeDiagnosticMode.PatchDisplayableErrorMessage) { $RemoteUtilDecodeDiagnosticEvidence.displayableError } else { $null }
                }
                VerifiesServerResponseOrLoadSuccess = $false
                DynamicVerified = $false
            }
            PreextractedBundle = [ordered]@{
                ClassName = "pinball.loading.initial.AssetExtractor"
                MethodName = "start"
                Strategy = "abc-method-body"
                Enabled = [bool]$PreextractedBundleMode.PatchAssetExtractorStart
                RequiresPreextractedBundle = [bool]$PreextractedBundleMode.RequiresPreextractedBundle
                EvidenceFile = $AssetExtractorPreextractedBundleEvidenceFile
                Evidence = $AssetExtractorPreextractedBundleEvidence
                VerifiesBundleContents = $false
                DynamicVerified = $false
            }
            SdkDummyTitleSceneGuard = [ordered]@{
                ClassName = $TitleSceneClassName
                ScriptName = $TitleSceneScriptName
                Enabled = [bool]$SdkDummyLoginRequestProbeMode.PatchSdkDummyTitleSceneGuard
                SourceBefore = $TitleSceneBeforeEvidence
                SourceAfter = $TitleSceneAfterEvidence
                Replacement = $SdkDummyTitleSceneGuardReplacement
                Verification = $SdkDummyTitleSceneGuardVerification
                DynamicVerified = $false
            }
            SdkDummyRealRemoteBridge = [ordered]@{
                ClassName = $ChannelMainClassName
                ScriptName = $ChannelMainScriptName
                Enabled = [bool]$SdkDummyLoginRequestProbeMode.PatchChannelMainForSdkDummy
                SourceBefore = $ChannelMainBeforeEvidence
                SourceAfter = $ChannelMainAfterEvidence
                Replacement = $SdkDummyRealRemoteBridgeReplacement
                Verification = $SdkDummyRealRemoteBridgeVerification
                DynamicVerified = $false
            }
            PatchedSwf = $PatchedSwfEvidence
            RemovedV1SignatureEntries = $RepackResult.RemovedSignatureEntries
        }
        Signing = [ordered]@{
            KeystorePath = $KeystorePath
            KeystoreType = $KeystoreType
            KeyAlias = $KeyAlias
            KeystoreCreated = -not $KeystoreExisted
            PasswordFileCreated = $PasswordState.Created
            CertificateEvidence = Join-Path $EvidenceDirectory "keytool-certificate.log"
        }
        Output = [ordered]@{
            AlignedUnsignedApk = $AlignedApkEvidence
            SignedApk = $OutputEvidence
            SwfEntry = $OutputSwfEvidence
            AndroidManifestEntry = $OutputAndroidManifestEvidence
            SignatureEvidence = Join-Path $EvidenceDirectory "apksigner-verify.log"
            AlignmentEvidence = Join-Path $EvidenceDirectory "zipalign-output-verify.log"
        }
        Tools = [ordered]@{
            Ffdec = [ordered]@{
                Version = $FfdecVersion[0]
                Launcher = Get-ClientPatchFileEvidence -Path $JavaPath
                Jar = Get-ClientPatchFileEvidence -Path $FfdecJarPath
            }
            AndroidBuildTools = [ordered]@{
                Revision = $BuildToolsRevision
                Zipalign = Get-ClientPatchFileEvidence -Path $ZipalignPath
                ApkSignerVersion = ($ApkSignerVersionResult.Output -join "`n").Trim()
                ApkSignerLauncher = Get-ClientPatchFileEvidence -Path $JavaPath
                ApkSignerJar = Get-ClientPatchFileEvidence -Path $ApkSignerJarPath
            }
            Java = [ordered]@{
                Home = $ResolvedJavaHome
                Version = ($JavaVersionResult.Output -join "`n").Trim()
                Executable = Get-ClientPatchFileEvidence -Path $JavaPath
                CompilerVersion = if ($JavacVersionResult) { ($JavacVersionResult.Output -join "`n").Trim() } else { $null }
                Compiler = if ($JavacVersionResult) { Get-ClientPatchFileEvidence -Path $JavacPath } else { $null }
                KeytoolVersion = ($KeytoolVersionResult.Output -join "`n").Trim()
                Keytool = Get-ClientPatchFileEvidence -Path $KeytoolPath
            }
            RemoteUtilAbcPatch = [ordered]@{
                Enabled = [bool]$SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard
                Source = if ($SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard) { Get-ClientPatchFileEvidence -Path $RemoteUtilAbcPatchSourcePath } else { $null }
                AbcMethodDigestSource = if ($SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard) { Get-ClientPatchFileEvidence -Path $AbcMethodDigestSourcePath } else { $null }
                MethodDigestSource = if ($SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard) { Get-ClientPatchFileEvidence -Path $RemoteUtilMethodDigestSourcePath } else { $null }
                FfdecLibrary = if ($SdkDummyLoginRequestProbeMode.PatchRemoteUtilRequestGuard) { Get-ClientPatchFileEvidence -Path $FfdecLibraryJarPath } else { $null }
            }
            RemoteUtilDecodeDiagnosticPatch = [ordered]@{
                Enabled = [bool]$MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError
                Source = if ($MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError) { Get-ClientPatchFileEvidence -Path $RemoteUtilDecodeDiagnosticPatchSourcePath } else { $null }
                AbcMethodDigestSource = if ($MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError) { Get-ClientPatchFileEvidence -Path $AbcMethodDigestSourcePath } else { $null }
                MethodDigestSource = if ($MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError) { Get-ClientPatchFileEvidence -Path $RemoteUtilMethodDigestSourcePath } else { $null }
                FfdecLibrary = if ($MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError) { Get-ClientPatchFileEvidence -Path $FfdecLibraryJarPath } else { $null }
            }
            AssetExtractorPreextractedBundlePatch = [ordered]@{
                Enabled = [bool]$PreextractedBundleMode.PatchAssetExtractorStart
                Source = if ($PreextractedBundleMode.PatchAssetExtractorStart) { Get-ClientPatchFileEvidence -Path $AssetExtractorPreextractedBundlePatchSourcePath } else { $null }
                MethodDigestSource = if ($PreextractedBundleMode.PatchAssetExtractorStart) { Get-ClientPatchFileEvidence -Path $AbcMethodDigestSourcePath } else { $null }
                FfdecLibrary = if ($PreextractedBundleMode.PatchAssetExtractorStart) { Get-ClientPatchFileEvidence -Path $FfdecLibraryJarPath } else { $null }
            }
            GbitsVersionQueryProbePatch = [ordered]@{
                Enabled = [bool]$VersionQueryProbeMode.PatchVersionQuerySuccess
                Source = if ($VersionQueryProbeMode.PatchVersionQuerySuccess) { Get-ClientPatchFileEvidence -Path $GbitsVersionQueryProbePatchSourcePath } else { $null }
                MethodDigestSource = if ($VersionQueryProbeMode.PatchVersionQuerySuccess) { Get-ClientPatchFileEvidence -Path $AbcMethodDigestSourcePath } else { $null }
                FfdecLibrary = if ($VersionQueryProbeMode.PatchVersionQuerySuccess) { Get-ClientPatchFileEvidence -Path $FfdecLibraryJarPath } else { $null }
            }
            GbitsVersionUrlAbcPatch = [ordered]@{
                Enabled = [bool]$VersionUrlPatchEnabled
                Source = if ($VersionUrlPatchEnabled) { Get-ClientPatchFileEvidence -Path $GbitsVersionUrlAbcPatchSourcePath } else { $null }
                MethodDigestSource = if ($VersionUrlPatchEnabled) { Get-ClientPatchFileEvidence -Path $AbcMethodDigestSourcePath } else { $null }
                FfdecLibrary = if ($VersionUrlPatchEnabled) { Get-ClientPatchFileEvidence -Path $FfdecLibraryJarPath } else { $null }
            }
        }
    }
    $ManifestPath = Join-Path $WorkingDirectory "patch-manifest.json"
    $Manifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $ManifestPath -Encoding utf8

    [pscustomobject][ordered]@{
        OutputApkPath = $ResolvedOutputApkPath
        OutputApkSha256 = $OutputEvidence.Sha256
        ManifestPath = $ManifestPath
        VersionBaseUrl = $TargetVersionUrl
        KeystorePath = $KeystorePath
        KeystoreCreated = -not $KeystoreExisted
    }
} finally {
    [Environment]::SetEnvironmentVariable($PasswordEnvironmentVariable, $OriginalPasswordEnvironmentValue, "Process")
    $env:JAVA_HOME = $OriginalJavaHome
    $env:PATH = $OriginalPath
}
# //// /完成 CN 客户端版本地址补丁和运行时签名 ////
