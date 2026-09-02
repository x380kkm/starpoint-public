# audience: internal
# # build-cn-gacha-cdn-diff
# 此脚本生成 CN 扭蛋, 免费活动和功能入口 master 的 CDN 差分 ZIP, 并保留与 iOS bundle 互斥的原始配置 master.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SourceArchivePath,
    [Parameter(Mandatory)][string]$CampaignSourceArchivePath,
    [Parameter(Mandatory)][string]$ConfigSourceArchivePath,
    [Parameter(Mandatory)][string]$OutputArchivePath,
    [Parameter(Mandatory)][string]$PathManifestPath,
    [Parameter(Mandatory)][string[]]$EntityManifestPath,
    [Parameter(Mandatory)][string]$ArchiveLocation,
    [ValidateNotNullOrEmpty()][string]$EntryName = "production/upload/15/83d96aad4b9a46d19d19b6555d3f4232b29e25",
    [ValidateNotNullOrEmpty()][string]$CampaignEntryName = "production/upload/74/e7be1b0da0fd069aba147e2fd92da2b427d86e",
    [ValidateNotNullOrEmpty()][string]$FeatureBannerEntryName = "production/upload/8c/82c8e05db4ce4f8a06bcd25f90bbda2967808b",
    [ValidateNotNullOrEmpty()][string]$ConfigEntryName = "production/upload/97/858974661335a60a4bca865a9a283179fff13b",
    [ValidateNotNullOrEmpty()][string]$OriginalVersion = "1.4.54",
    [ValidateNotNullOrEmpty()][string]$TargetVersion = "1.4.55"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem

$ExpectedConfigBytes = 16
$ExpectedConfigHash = "q2jQu62wukiOw66K67meHjDUR3A9Gl4uLmyZJGLQZ6w"

$PatcherPath = Join-Path $PSScriptRoot "patch-cn-gacha-master.mjs"
$SourceArchivePath = [IO.Path]::GetFullPath($SourceArchivePath)
$CampaignSourceArchivePath = [IO.Path]::GetFullPath($CampaignSourceArchivePath)
$ConfigSourceArchivePath = [IO.Path]::GetFullPath($ConfigSourceArchivePath)
$OutputArchivePath = [IO.Path]::GetFullPath($OutputArchivePath)
$PathManifestPath = [IO.Path]::GetFullPath($PathManifestPath)
$EntityManifestPath = @($EntityManifestPath | ForEach-Object { [IO.Path]::GetFullPath($_) })

if ($SourceArchivePath -ceq $OutputArchivePath -or
    $CampaignSourceArchivePath -ceq $OutputArchivePath -or
    $ConfigSourceArchivePath -ceq $OutputArchivePath) {
    throw "源 ZIP 与输出 ZIP 必须使用不同路径"
}
foreach ($RequiredPath in @($SourceArchivePath, $CampaignSourceArchivePath, $ConfigSourceArchivePath, $PathManifestPath, $PatcherPath) + $EntityManifestPath) {
    if (-not (Test-Path -LiteralPath $RequiredPath -PathType Leaf)) {
        throw "缺少输入文件: $RequiredPath"
    }
}

# //// 计算文件的两种 SHA-256 清单值 [@x380kkm 2026-08-22] ////
function Get-CnGachaFileSha256Base64 {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$FilePath)

    $Stream = [IO.File]::OpenRead($FilePath)
    $Algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        return [Convert]::ToBase64String($Algorithm.ComputeHash($Stream))
    } finally {
        $Algorithm.Dispose()
        $Stream.Dispose()
    }
}

function Get-CnGachaEntitySha256 {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$FilePath)

    return (Get-CnGachaFileSha256Base64 -FilePath $FilePath).TrimEnd("=").Replace("+", "_").Replace("/", "-")
}
# //// /计算文件的两种 SHA-256 清单值 ////

# //// 定位 ZIP 结束记录 [@x380kkm 2026-08-22] ////
function Find-CnGachaLastByteSequence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][byte[]]$Bytes,
        [Parameter(Mandatory)][byte[]]$Needle
    )

    for ($Index = $Bytes.Length - $Needle.Length; $Index -ge 0; $Index -= 1) {
        $Matches = $true
        for ($NeedleIndex = 0; $NeedleIndex -lt $Needle.Length; $NeedleIndex += 1) {
            if ($Bytes[$Index + $NeedleIndex] -ne $Needle[$NeedleIndex]) {
                $Matches = $false
                break
            }
        }
        if ($Matches) {
            return $Index
        }
    }
    return -1
}

function Assert-CnGachaLegacyZip {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ZipPath)

    $Bytes = [IO.File]::ReadAllBytes($ZipPath)
    $EndOffset = Find-CnGachaLastByteSequence -Bytes $Bytes -Needle ([byte[]](0x50, 0x4b, 0x05, 0x06))
    if ($EndOffset -lt 0 -or $EndOffset + 22 -gt $Bytes.Length) {
        throw "ZIP 缺少有效结束记录: $ZipPath"
    }
    if ($EndOffset -ge 20 -and
        [BitConverter]::ToUInt32($Bytes, $EndOffset - 20) -eq 0x07064b50) {
        throw "ZIP 包含 ZIP64 结束定位记录: $ZipPath"
    }
    if ([BitConverter]::ToUInt16($Bytes, $EndOffset + 8) -eq 0xffff -or
        [BitConverter]::ToUInt16($Bytes, $EndOffset + 10) -eq 0xffff -or
        [BitConverter]::ToUInt32($Bytes, $EndOffset + 12) -eq 0xffffffff -or
        [BitConverter]::ToUInt32($Bytes, $EndOffset + 16) -eq 0xffffffff) {
        throw "ZIP 使用 ZIP64 结束字段: $ZipPath"
    }
}
# //// /定位 ZIP 结束记录 ////

# //// 从源归档提取指定 master [@x380kkm 2026-08-24] ////
function Copy-CnGachaArchiveEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ArchivePath,
        [Parameter(Mandatory)][string]$EntryPath,
        [Parameter(Mandatory)][string]$DestinationPath
    )

    $Archive = [IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        $Entries = @($Archive.Entries | Where-Object { $_.FullName -ceq $EntryPath })
        if ($Entries.Count -ne 1) {
            throw "源 ZIP 的 master 条目数量不正确: entry=$EntryPath count=$($Entries.Count)"
        }
        $SourceStream = $Entries[0].Open()
        $OutputStream = [IO.File]::Create($DestinationPath)
        try {
            $SourceStream.CopyTo($OutputStream)
        } finally {
            $OutputStream.Dispose()
            $SourceStream.Dispose()
        }
    } finally {
        $Archive.Dispose()
    }
}
# //// /从源归档提取指定 master ////

# //// 生成扭蛋差分 ZIP 和关联清单 [@x380kkm 2026-08-22] ////
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("starpoint-gacha-diff-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TempRoot | Out-Null
try {
    $InputMap = Join-Path $TempRoot "gacha.original.orderedmap"
    $PatchedMap = Join-Path $TempRoot "gacha.patched.orderedmap"
    $CampaignInputMap = Join-Path $TempRoot "gacha_campaign.original.orderedmap"
    $CampaignPatchedMap = Join-Path $TempRoot "gacha_campaign.patched.orderedmap"
    $FeatureBannerInputMap = Join-Path $TempRoot "feature_banner.original.orderedmap"
    $FeatureBannerPatchedMap = Join-Path $TempRoot "feature_banner.patched.orderedmap"
    $ConfigMap = Join-Path $TempRoot "config.orderedmap"
    $TemporaryArchive = Join-Path $TempRoot ([IO.Path]::GetFileName($OutputArchivePath))

    Copy-CnGachaArchiveEntry -ArchivePath $SourceArchivePath -EntryPath $EntryName -DestinationPath $InputMap
    Copy-CnGachaArchiveEntry -ArchivePath $CampaignSourceArchivePath -EntryPath $CampaignEntryName -DestinationPath $CampaignInputMap
    Copy-CnGachaArchiveEntry -ArchivePath $SourceArchivePath -EntryPath $FeatureBannerEntryName -DestinationPath $FeatureBannerInputMap
    Copy-CnGachaArchiveEntry -ArchivePath $ConfigSourceArchivePath -EntryPath $ConfigEntryName -DestinationPath $ConfigMap
    if ((Get-Item -LiteralPath $ConfigMap).Length -ne $ExpectedConfigBytes -or
        (Get-CnGachaEntitySha256 -FilePath $ConfigMap) -cne $ExpectedConfigHash) {
        throw "客户端配置 master 与 iOS bundle 不互斥"
    }

    $PatcherOutput = @(& node $PatcherPath --input $InputMap --output $PatchedMap `
            --campaign-input $CampaignInputMap --campaign-output $CampaignPatchedMap `
            --feature-banner-input $FeatureBannerInputMap --feature-banner-output $FeatureBannerPatchedMap 2>&1)
    if ($LASTEXITCODE -ne 0) {
        $ErrorTail = ($PatcherOutput | Select-Object -Last 20) -join [Environment]::NewLine
        throw "gacha master patcher 失败: exit=$LASTEXITCODE`n$ErrorTail"
    }
    try {
        $PatchReport = ($PatcherOutput -join [Environment]::NewLine) | ConvertFrom-Json
    } catch {
        throw "gacha master patcher 未返回有效报告"
    }
    $PatchedEntries = @(
        [pscustomobject][ordered]@{
            Name = "Gacha"
            EntryName = $EntryName
            Path = $PatchedMap
            Bytes = (Get-Item -LiteralPath $PatchedMap).Length
            Hash = (Get-CnGachaEntitySha256 -FilePath $PatchedMap)
        },
        [pscustomobject][ordered]@{
            Name = "GachaCampaign"
            EntryName = $CampaignEntryName
            Path = $CampaignPatchedMap
            Bytes = (Get-Item -LiteralPath $CampaignPatchedMap).Length
            Hash = (Get-CnGachaEntitySha256 -FilePath $CampaignPatchedMap)
        },
        [pscustomobject][ordered]@{
            Name = "FeatureBanner"
            EntryName = $FeatureBannerEntryName
            Path = $FeatureBannerPatchedMap
            Bytes = (Get-Item -LiteralPath $FeatureBannerPatchedMap).Length
            Hash = (Get-CnGachaEntitySha256 -FilePath $FeatureBannerPatchedMap)
        },
        [pscustomobject][ordered]@{
            Name = "Config"
            EntryName = $ConfigEntryName
            Path = $ConfigMap
            Bytes = (Get-Item -LiteralPath $ConfigMap).Length
            Hash = (Get-CnGachaEntitySha256 -FilePath $ConfigMap)
        }
    )

    $DiffArchive = [IO.Compression.ZipFile]::Open($TemporaryArchive, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($PatchedEntry in $PatchedEntries) {
            $Entry = $DiffArchive.CreateEntry($PatchedEntry.EntryName, [IO.Compression.CompressionLevel]::Optimal)
            $Entry.LastWriteTime = [DateTimeOffset]::new(1980, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
            $EntryStream = $Entry.Open()
            $PatchedStream = [IO.File]::OpenRead($PatchedEntry.Path)
            try {
                $PatchedStream.CopyTo($EntryStream)
            } finally {
                $PatchedStream.Dispose()
                $EntryStream.Dispose()
            }
        }
    } finally {
        $DiffArchive.Dispose()
    }

    $ReadbackArchive = [IO.Compression.ZipFile]::OpenRead($TemporaryArchive)
    try {
        if ($ReadbackArchive.Entries.Count -ne $PatchedEntries.Count) {
            throw "差分 ZIP 条目不符合预期"
        }
        foreach ($PatchedEntry in $PatchedEntries) {
            $ReadbackEntry = $ReadbackArchive.GetEntry($PatchedEntry.EntryName)
            if ($null -eq $ReadbackEntry -or $ReadbackEntry.Length -ne $PatchedEntry.Bytes) {
                throw "差分 ZIP 的 master 条目不正确: $($PatchedEntry.EntryName)"
            }
        }
    } finally {
        $ReadbackArchive.Dispose()
    }
    Assert-CnGachaLegacyZip -ZipPath $TemporaryArchive

    $OrderedMapBytes = $PatchedEntries[0].Bytes
    $OrderedMapHash = $PatchedEntries[0].Hash
    $CampaignOrderedMapBytes = $PatchedEntries[1].Bytes
    $CampaignOrderedMapHash = $PatchedEntries[1].Hash
    $FeatureBannerOrderedMapBytes = $PatchedEntries[2].Bytes
    $FeatureBannerOrderedMapHash = $PatchedEntries[2].Hash
    $ConfigOrderedMapBytes = $PatchedEntries[3].Bytes
    $ConfigOrderedMapHash = $PatchedEntries[3].Hash
    $ArchiveBytes = (Get-Item -LiteralPath $TemporaryArchive).Length
    $ArchiveHash = Get-CnGachaFileSha256Base64 -FilePath $TemporaryArchive
    $Utf8 = [Text.UTF8Encoding]::new($false)

    $Manifest = [IO.File]::ReadAllText($PathManifestPath, $Utf8) | ConvertFrom-Json
    $VersionFields = [ordered]@{
        client_asset_version = $OriginalVersion
        target_asset_version = $TargetVersion
        eventual_target_asset_version = $TargetVersion
    }
    foreach ($PropertyName in $VersionFields.Keys) {
        if (-not $Manifest.info.PSObject.Properties[$PropertyName]) {
            throw "path 缺少版本字段: $PropertyName"
        }
        $Manifest.info.$PropertyName = $VersionFields[$PropertyName]
    }

    $TargetDiffs = @($Manifest.diff | Where-Object {
        $_.version -ceq $TargetVersion -and $_.original_version -ceq $OriginalVersion
    })
    if ($TargetDiffs.Count -gt 1) {
        throw "path 包含重复目标差分: $OriginalVersion -> $TargetVersion"
    }
    $NewDiff = [pscustomobject][ordered]@{
        version = $TargetVersion
        original_version = $OriginalVersion
        archive = @(
            [pscustomobject][ordered]@{
                location = $ArchiveLocation
                size = $ArchiveBytes
                sha256 = $ArchiveHash
            }
        )
    }
    if ($TargetDiffs.Count -eq 0) {
        $Manifest.diff = @($Manifest.diff) + $NewDiff
    } else {
        $Manifest.diff = @($Manifest.diff | ForEach-Object {
            if ($_.version -ceq $TargetVersion -and $_.original_version -ceq $OriginalVersion) {
                $NewDiff
            } else {
                $_
            }
        })
    }
    $TemporaryManifest = Join-Path $TempRoot "path"
    [IO.File]::WriteAllText(
        $TemporaryManifest,
        ($Manifest | ConvertTo-Json -Depth 100 -Compress),
        $Utf8
    )

    $TemporaryEntities = @()
    foreach ($EntityPath in $EntityManifestPath) {
        $EntityText = [IO.File]::ReadAllText($EntityPath, $Utf8)
        $NewEntityText = $EntityText
        foreach ($PatchedEntry in $PatchedEntries) {
            $RowPattern = '(?m)^' + [Regex]::Escape($PatchedEntry.EntryName) + ',[^,\r\n]+,\d+,[^,\r\n]+,common(?=\r?$)'
            $RowMatches = [Regex]::Matches($NewEntityText, $RowPattern)
            if ($RowMatches.Count -ne 1) {
                throw "entities 中目标行数量不正确: file=$EntityPath entry=$($PatchedEntry.EntryName) count=$($RowMatches.Count)"
            }
            $RowReplacement = "$($PatchedEntry.EntryName),$TargetVersion,$($PatchedEntry.Bytes),$($PatchedEntry.Hash),common"
            $NewEntityText = [Regex]::Replace(
                $NewEntityText,
                $RowPattern,
                [Text.RegularExpressions.MatchEvaluator]{ param($Match) $RowReplacement }
            )
        }
        $TemporaryEntity = Join-Path $TempRoot ([IO.Path]::GetFileName($EntityPath))
        [IO.File]::WriteAllText($TemporaryEntity, $NewEntityText, $Utf8)
        $TemporaryEntities += [pscustomobject]@{
            Source = $TemporaryEntity
            Destination = $EntityPath
        }
    }

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutputArchivePath) | Out-Null
    [IO.File]::Move($TemporaryArchive, $OutputArchivePath, $true)
    [IO.File]::Move($TemporaryManifest, $PathManifestPath, $true)
    foreach ($Entity in $TemporaryEntities) {
        [IO.File]::Move($Entity.Source, $Entity.Destination, $true)
    }

    [pscustomobject][ordered]@{
        Version = $TargetVersion
        ZipEntries = $PatchedEntries.Count
        Zip64 = $false
        RetainedOriginalCount = [int]$PatchReport.gacha.retainedOriginalCount
        ExcludedRegionalAliasCount = [int]$PatchReport.gacha.excludedRegionalAliasCount
        NormalizedCoverageAliasCount = [int]$PatchReport.gacha.normalizedCoverageAliasCount
        TemporaryAliasCount = [int]$PatchReport.gacha.temporaryAliasCount
        GachaCount = [int]$PatchReport.gacha.retainedOriginalCount + [int]$PatchReport.gacha.temporaryAliasCount
        RetainedOriginalCampaignCount = [int]$PatchReport.gachaCampaign.retainedOriginalCampaignCount
        TemporaryCampaignAliasCount = [int]$PatchReport.gachaCampaign.temporaryCampaignAliasCount
        GachaCampaignCount = [int]$PatchReport.gachaCampaign.retainedOriginalCampaignCount + [int]$PatchReport.gachaCampaign.temporaryCampaignAliasCount
        ProjectedFeatureLinkCount = [int]$PatchReport.featureBanner.projectedLinkCount
        OrderedMapBytes = $OrderedMapBytes
        OrderedMapSha256 = $OrderedMapHash
        CampaignOrderedMapBytes = $CampaignOrderedMapBytes
        CampaignOrderedMapSha256 = $CampaignOrderedMapHash
        FeatureBannerOrderedMapBytes = $FeatureBannerOrderedMapBytes
        FeatureBannerOrderedMapSha256 = $FeatureBannerOrderedMapHash
        ConfigOrderedMapBytes = $ConfigOrderedMapBytes
        ConfigOrderedMapSha256 = $ConfigOrderedMapHash
        ArchiveBytes = $ArchiveBytes
        ArchiveSha256 = $ArchiveHash
    }
} finally {
    $ResolvedTempRoot = [IO.Path]::GetFullPath($TempRoot)
    $SystemTempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $ResolvedTempRoot.StartsWith($SystemTempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝清理非临时目录: $ResolvedTempRoot"
    }
    if (Test-Path -LiteralPath $ResolvedTempRoot) {
        Remove-Item -LiteralPath $ResolvedTempRoot -Recurse -Force
    }
}
# //// /生成扭蛋差分 ZIP 和关联清单 ////
