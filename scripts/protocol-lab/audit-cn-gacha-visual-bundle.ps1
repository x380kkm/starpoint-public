# audience: internal
# # audit-cn-gacha-visual-bundle
# 此脚本以最终 gacha master 为扭蛋视觉资源来源, 并核对字段结构、资源可达性、全部候选结果资源、动画配置、seed 池和下载文件契约.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$CdnRoot,
    [Parameter(Mandatory)][string]$ReferenceRoot,
    [Parameter(Mandatory)][string]$CnAssetsRoot,
    [Parameter(Mandatory)][string]$ServiceAssetsRoot,
    [Parameter(Mandatory)][string]$AppAssetRoot,
    [Parameter(Mandatory)][string]$CnAppAssetRoot,
    [Parameter(Mandatory)][string]$PhysicsRoot,
    [string]$ReportPath,
    [string]$RangeBaseUrl,
    [switch]$RepairMalformedEntityDigest
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression.FileSystem

$CdnRoot = [IO.Path]::GetFullPath($CdnRoot)
$ReferenceRoot = [IO.Path]::GetFullPath($ReferenceRoot)
$CnAssetsRoot = [IO.Path]::GetFullPath($CnAssetsRoot)
$ServiceAssetsRoot = [IO.Path]::GetFullPath($ServiceAssetsRoot)
$AppAssetRoot = [IO.Path]::GetFullPath($AppAssetRoot)
$CnAppAssetRoot = [IO.Path]::GetFullPath($CnAppAssetRoot)
$PhysicsRoot = [IO.Path]::GetFullPath($PhysicsRoot)
$DecoderPath = Join-Path $PSScriptRoot "decode-cn-orderedmap.mjs"
$CandidateAuditPath = Join-Path $PSScriptRoot "audit_cn_gacha_candidate_assets.py"
$Utf8 = [Text.UTF8Encoding]::new($false)
$MasterLogicalPaths = [ordered]@{
    Gacha = "master/gacha/gacha.orderedmap"
    Feature = "master/gacha/gacha_feature_content.orderedmap"
}
$ArchiveKinds = [ordered]@{
    "production/upload/" = "common"
    "production/medium_upload/" = "medium"
    "production/android_upload/" = "android"
    "production/ios_upload/" = "ios"
}

foreach ($RequiredPath in @($CdnRoot, $ReferenceRoot, $CnAssetsRoot, $ServiceAssetsRoot, $AppAssetRoot, $CnAppAssetRoot, $PhysicsRoot, $DecoderPath, $CandidateAuditPath)) {
    if (-not (Test-Path -LiteralPath $RequiredPath)) {
        throw "缺少扭蛋视觉审计输入: $RequiredPath"
    }
}

# //// 解析 CN 资源身份和 iOS EntityLists [@x380kkm 2026-08-28] ////
function Get-CnAssetHash {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$LogicalPath)

    $NormalizedPath = $LogicalPath.Replace("\", "/").TrimStart("/")
    while ($NormalizedPath.Contains("//")) {
        $NormalizedPath = $NormalizedPath.Replace("//", "/")
    }
    $Algorithm = [Security.Cryptography.SHA1]::Create()
    try {
        $Bytes = $Utf8.GetBytes($NormalizedPath + "K6R9T9Hz22OpeIGEWB0ui6c6PYFQnJGy")
        return ([BitConverter]::ToString($Algorithm.ComputeHash($Bytes))).Replace("-", "").ToLowerInvariant()
    } finally {
        $Algorithm.Dispose()
    }
}

function Get-CnAssetEntryPaths {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$LogicalPath)

    $Hash = Get-CnAssetHash -LogicalPath $LogicalPath
    $Suffix = "$($Hash.Substring(0, 2))/$($Hash.Substring(2))"
    return @(
        "production/upload/$Suffix",
        "production/medium_upload/$Suffix",
        "production/android_upload/$Suffix",
        "production/ios_upload/$Suffix"
    )
}

function Read-CnEntityManifests {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Root)

    $EntityRoot = Join-Path $Root "entities"
    $PathFile = Join-Path $EntityRoot "PathFile.csv"
    $IosManifestPaths = @(
        Get-ChildItem -LiteralPath $EntityRoot -File -Filter "*-ios_medium.csv" |
            Where-Object Length -gt 0 |
            Sort-Object Name
    )
    if ($IosManifestPaths.Count -gt 0) {
        $ManifestPaths = @($IosManifestPaths)
        if (Test-Path -LiteralPath $PathFile -PathType Leaf) {
            $ManifestPaths += Get-Item -LiteralPath $PathFile
        }
    } else {
        $ManifestPaths = @(
            Get-ChildItem -LiteralPath $EntityRoot -File -Filter "*.csv" |
                Where-Object Length -gt 0 |
                Sort-Object Name
        )
    }
    if ($ManifestPaths.Count -lt 1) {
        throw "CN CDN 缺少 EntityLists: $EntityRoot"
    }
    $ManifestPaths = @($ManifestPaths | Sort-Object FullName -Unique)
    $Manifests = [ordered]@{}
    foreach ($ManifestPath in $ManifestPaths) {
        $Records = @{}
        foreach ($Line in [IO.File]::ReadLines($ManifestPath.FullName, $Utf8)) {
            if ([string]::IsNullOrEmpty($Line)) {
                continue
            }
            $Fields = $Line.Split(",")
            if ($Fields.Count -ne 5) {
                throw "EntityLists 行字段数量错误: $($ManifestPath.FullName)"
            }
            $Records[$Fields[0]] = [pscustomobject][ordered]@{
                EntryPath = $Fields[0]
                Version = $Fields[1]
                ByteLength = [long]::Parse($Fields[2], [Globalization.CultureInfo]::InvariantCulture)
                Digest = $Fields[3]
                AssetKind = $Fields[4]
            }
        }
        $Manifests[$ManifestPath.Name] = $Records
    }
    return $Manifests
}

function Find-CnEntityRecord {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][System.Collections.IDictionary]$Manifests,
        [Parameter(Mandatory)][string]$LogicalPath
    )

    $Matches = @()
    foreach ($EntryPath in Get-CnAssetEntryPaths -LogicalPath $LogicalPath) {
        foreach ($ManifestName in $Manifests.Keys) {
            $Records = $Manifests[$ManifestName]
            if ($Records.ContainsKey($EntryPath)) {
                $Matches += [pscustomobject]@{ ManifestName = $ManifestName; Record = $Records[$EntryPath] }
            }
        }
    }
    if ($Matches.Count -eq 0) {
        return $null
    }
    $Identity = @($Matches | ForEach-Object {
        "$($_.Record.EntryPath),$($_.Record.ByteLength),$($_.Record.Digest),$($_.Record.AssetKind)"
    } | Sort-Object -Unique)
    if ($Identity.Count -ne 1) {
        throw "扭蛋资源在 EntityLists 中不一致: $LogicalPath"
    }
    $Preferred = @($Matches | Sort-Object @{ Expression = {
        if ($_.ManifestName -ceq "PathFile.csv") {
            return 1
        }
        if ($_.ManifestName -cmatch "-ios_medium\\.csv$") {
            return 0
        }
        return 2
    } }, @{ Expression = { $_.ManifestName } })[0]
    return $Preferred.Record
}

function Get-CnEntityRecord {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][System.Collections.IDictionary]$Manifests,
        [Parameter(Mandatory)][string]$LogicalPath
    )

    $Record = Find-CnEntityRecord -Manifests $Manifests -LogicalPath $LogicalPath
    if ($null -eq $Record) {
        throw "EntityLists 缺少扭蛋资源: $LogicalPath"
    }
    return $Record
}
# //// /解析 CN 资源身份和 iOS EntityLists ////

# //// 读取并验证归档中的目标资源 [@x380kkm 2026-08-22] ////
function Get-CnEntityDigest {
    [CmdletBinding()]
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $Algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        return [Convert]::ToBase64String($Algorithm.ComputeHash($Bytes)).TrimEnd("=").Replace("+", "_").Replace("/", "-")
    } finally {
        $Algorithm.Dispose()
    }
}

function Get-ArchiveKind {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$EntryPath)

    foreach ($Prefix in $ArchiveKinds.Keys) {
        if ($EntryPath.StartsWith($Prefix, [StringComparison]::Ordinal)) {
            return $ArchiveKinds[$Prefix]
        }
    }
    throw "未知 CN 归档类型: $EntryPath"
}

function Read-CnArchiveAssets {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][System.Collections.IDictionary]$Targets,
        [switch]$AllowMissing
    )

    $Results = @{}
    $Kinds = @($Targets.Values | ForEach-Object { Get-ArchiveKind -EntryPath $_.EntryPath } | Sort-Object -Unique)
    foreach ($Kind in $Kinds) {
        foreach ($Suffix in @("full", "diff")) {
            $Directory = Join-Path $Root "archive-$Kind-$Suffix"
            if (-not (Test-Path -LiteralPath $Directory -PathType Container)) {
                continue
            }
            foreach ($ArchivePath in @(Get-ChildItem -LiteralPath $Directory -File -Filter "*.zip" | Sort-Object Name)) {
                $Archive = [IO.Compression.ZipFile]::OpenRead($ArchivePath.FullName)
                try {
                    foreach ($Entry in $Archive.Entries) {
                        if (-not $Targets.Contains($Entry.FullName) -or $Results.ContainsKey($Entry.FullName)) {
                            continue
                        }
                        $Record = $Targets[$Entry.FullName]
                        if ($Entry.Length -ne $Record.ByteLength) {
                            continue
                        }
                        $Stream = $Entry.Open()
                        $Memory = [IO.MemoryStream]::new()
                        try {
                            $Stream.CopyTo($Memory)
                            $Bytes = $Memory.ToArray()
                        } finally {
                            $Memory.Dispose()
                            $Stream.Dispose()
                        }
                        if ((Get-CnEntityDigest -Bytes $Bytes) -cne $Record.Digest) {
                            continue
                        }
                        $Results[$Entry.FullName] = [pscustomobject][ordered]@{
                            Bytes = $Bytes
                            Archive = $ArchivePath.FullName
                            EntryPath = $Entry.FullName
                        }
                    }
                } finally {
                    $Archive.Dispose()
                }
            }
        }
    }
    foreach ($Target in $Targets.Values) {
        if (-not $AllowMissing -and -not $Results.ContainsKey($Target.EntryPath)) {
            throw "CN 归档缺少 EntityLists 指定资源: $($Target.EntryPath)"
        }
    }
    return $Results
}

function Read-CnLogicalAssets {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][System.Collections.IDictionary]$Manifests,
        [Parameter(Mandatory)][string[]]$LogicalPaths,
        [switch]$AllowMissing
    )

    $Targets = @{}
    $Records = [ordered]@{}
    foreach ($LogicalPath in $LogicalPaths) {
        $Record = Get-CnEntityRecord -Manifests $Manifests -LogicalPath $LogicalPath
        $Targets[$Record.EntryPath] = $Record
        $Records[$LogicalPath] = $Record
    }
    $ArchiveAssets = Read-CnArchiveAssets -Root $Root -Targets $Targets -AllowMissing:$AllowMissing
    $Assets = [ordered]@{}
    foreach ($LogicalPath in $LogicalPaths) {
        $Record = $Records[$LogicalPath]
        $Archived = $ArchiveAssets[$Record.EntryPath]
        if ($null -eq $Archived) {
            continue
        }
        $Assets[$LogicalPath] = [pscustomobject][ordered]@{
            LogicalPath = $LogicalPath
            Record = $Record
            Bytes = $Archived.Bytes
            Archive = $Archived.Archive
        }
    }
    return $Assets
}
# //// /读取并验证归档中的目标资源 ////

# //// 修复自定义扭蛋 master 的 EntityLists 摘要 [@x380kkm 2026-08-22] ////
function Get-LegacyCnGachaEntityDigest {
    [CmdletBinding()]
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $Algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        return [Convert]::ToBase64String($Algorithm.ComputeHash($Bytes)).TrimEnd("=").Replace("+", "-").Replace("/", "_")
    } finally {
        $Algorithm.Dispose()
    }
}

function Repair-CnGachaEntityDigest {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Root)

    $EntryPath = (Get-CnAssetEntryPaths -LogicalPath $MasterLogicalPaths.Gacha)[0]
    $ArchiveRoot = Join-Path $Root "archive-common-diff"
    $Candidates = @()
    foreach ($ArchivePath in @(Get-ChildItem -LiteralPath $ArchiveRoot -File -Filter "starpoint-gacha-*.zip" | Sort-Object Name)) {
        $Archive = [IO.Compression.ZipFile]::OpenRead($ArchivePath.FullName)
        try {
            $Entry = $Archive.GetEntry($EntryPath)
            if ($null -eq $Entry) {
                continue
            }
            $Stream = $Entry.Open()
            $Memory = [IO.MemoryStream]::new()
            try {
                $Stream.CopyTo($Memory)
                $Bytes = $Memory.ToArray()
            } finally {
                $Memory.Dispose()
                $Stream.Dispose()
            }
            $Candidates += [pscustomobject]@{
                Archive = $ArchivePath.Name
                Bytes = $Bytes
                Digest = Get-CnEntityDigest -Bytes $Bytes
                LegacyDigest = Get-LegacyCnGachaEntityDigest -Bytes $Bytes
            }
        } finally {
            $Archive.Dispose()
        }
    }
    if ($Candidates.Count -lt 1) {
        throw "扭蛋差分缺少 gacha master 条目"
    }
    $CandidateIdentity = @($Candidates | ForEach-Object { "$($_.Bytes.Length),$($_.Digest)" } | Sort-Object -Unique)
    if ($CandidateIdentity.Count -ne 1) {
        throw "扭蛋差分中的 gacha master 字节不一致"
    }
    $Expected = $Candidates[0]
    $Repairs = @()
    foreach ($EntityPath in @(Get-ChildItem -LiteralPath (Join-Path $Root "entities") -File -Filter "*.csv" | Sort-Object Name)) {
        $EntityText = [IO.File]::ReadAllText($EntityPath.FullName, $Utf8)
        $RowPattern = '(?m)^' + [Regex]::Escape($EntryPath) + ',([^,\r\n]+),(\d+),([^,\r\n]+),common(?=\r?$)'
        $Matches = [Regex]::Matches($EntityText, $RowPattern)
        if ($Matches.Count -ne 1) {
            throw "EntityLists 中 gacha master 行数量错误: $($EntityPath.FullName)"
        }
        $Match = $Matches[0]
        $CurrentLength = [long]::Parse($Match.Groups[2].Value, [Globalization.CultureInfo]::InvariantCulture)
        $CurrentDigest = $Match.Groups[3].Value
        if ($CurrentLength -ne $Expected.Bytes.Length) {
            throw "EntityLists 中 gacha master 长度与差分不一致: $($EntityPath.FullName)"
        }
        if ($CurrentDigest -ceq $Expected.Digest) {
            continue
        }
        if ($CurrentDigest -cne $Expected.LegacyDigest) {
            throw "EntityLists 中 gacha master 摘要与差分不一致: $($EntityPath.FullName)"
        }
        $Replacement = "$EntryPath,$($Match.Groups[1].Value),$CurrentLength,$($Expected.Digest),common"
        $UpdatedText = [Regex]::Replace(
            $EntityText,
            $RowPattern,
            [Text.RegularExpressions.MatchEvaluator]{ param($Ignored) $Replacement }
        )
        $TemporaryPath = "$($EntityPath.FullName).starpoint-gacha-digest.tmp"
        [IO.File]::WriteAllText($TemporaryPath, $UpdatedText, $Utf8)
        [IO.File]::Move($TemporaryPath, $EntityPath.FullName, $true)
        $Repairs += [pscustomobject][ordered]@{
            Manifest = $EntityPath.Name
            EntryPath = $EntryPath
            Digest = $Expected.Digest
        }
    }
    return $Repairs
}
# //// /修复自定义扭蛋 master 的 EntityLists 摘要 ////

# //// 解码 master 并推导全部扭蛋池的资源引用 [@x380kkm 2026-08-28] ////
function ConvertFrom-CnOrderedMapBytes {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][byte[]]$Bytes,
        [Parameter(Mandatory)][string]$TemporaryRoot,
        [Parameter(Mandatory)][string]$Name
    )

    $InputPath = Join-Path $TemporaryRoot "$Name.orderedmap"
    [IO.File]::WriteAllBytes($InputPath, $Bytes)
    $JsonLines = @(& node $DecoderPath $InputPath)
    if ($LASTEXITCODE -ne 0) {
        throw "orderedmap 解码失败: $Name"
    }
    return ($JsonLines -join "`n") | ConvertFrom-Json -AsHashtable
}

function Get-CnGachaPools {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][System.Collections.IDictionary]$GachaMaster,
        [Parameter(Mandatory)][System.Collections.IDictionary]$FeatureMaster
    )

    $Pools = @()
    $PoolIds = @($GachaMaster.Keys | Sort-Object { [long]::Parse($_, [Globalization.CultureInfo]::InvariantCulture) })
    foreach ($PoolId in $PoolIds) {
        $Row = @($GachaMaster[$PoolId])
        if ($Row.Count -lt 47) {
            throw "gacha master 行列不足: $PoolId"
        }
        $Features = @()
        if ($FeatureMaster.Contains($PoolId)) {
            foreach ($FeatureEntry in $FeatureMaster[$PoolId].GetEnumerator()) {
                $FeatureRow = @($FeatureEntry.Value)
                $Feature = [ordered]@{ Index = $FeatureEntry.Key; Kind = $FeatureRow[0] }
                if ($FeatureRow[0] -ceq "0") {
                    $Feature.AssetPath = $FeatureRow[2]
                    $Feature.Type = "movie"
                } elseif ($FeatureRow[0] -ceq "1") {
                    $Feature.AssetPath = $FeatureRow[1]
                    $Feature.Type = "image"
                } elseif ($FeatureRow[0] -ceq "2") {
                    $Feature.PreviewKey = $FeatureRow[4]
                    $Feature.CharacterId = $FeatureRow[5]
                    $Feature.Type = "skill-preview"
                } else {
                    throw "gacha feature content 类型未知: pool=$PoolId index=$($FeatureEntry.Key)"
                }
                $Features += [pscustomobject]$Feature
            }
        }
        $MovieIds = @(@($Row[17], $Row[18], $Row[40]) | Where-Object {
            -not [string]::IsNullOrEmpty($_) -and $_ -cne "(None)"
        } | Sort-Object -Unique)
        $Pools += [pscustomobject][ordered]@{
            Id = $PoolId
            StringId = $Row[0]
            Title = $Row[1]
            Banner = $Row[3]
            Detail = $Row[12]
            MovieIds = $MovieIds
            Features = $Features
        }
    }
    return $Pools
}

function Get-CnGachaMasterStructure {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Label,
        [Parameter(Mandatory)][System.Collections.IDictionary]$GachaMaster,
        [Parameter(Mandatory)][System.Collections.IDictionary]$FeatureMaster
    )

    $GachaRowLengths = [Collections.Generic.HashSet[int]]::new()
    foreach ($PoolId in $GachaMaster.Keys) {
        $RowValue = $GachaMaster[$PoolId]
        if ($RowValue -isnot [Collections.IList]) {
            throw "$Label gacha master 行类型错误: $PoolId"
        }
        $Row = @($RowValue)
        if (@($Row | Where-Object { $_ -isnot [string] }).Count -gt 0) {
            throw "$Label gacha master 行包含非文本字段: $PoolId"
        }
        if ($Row.Count -lt 47 -or [string]::IsNullOrEmpty($Row[0]) -or
            [string]::IsNullOrEmpty($Row[3]) -or [string]::IsNullOrEmpty($Row[12])) {
            throw "$Label gacha master 视觉字段结构错误: $PoolId"
        }
        [void]$GachaRowLengths.Add($Row.Count)
    }

    $FeatureRowLengths = [Collections.Generic.HashSet[int]]::new()
    $FeatureKinds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $FeatureKindCounts = [ordered]@{ "0" = 0; "1" = 0; "2" = 0 }
    $FeatureRows = 0
    foreach ($PoolId in $FeatureMaster.Keys) {
        if (-not $GachaMaster.Contains($PoolId)) {
            throw "$Label gacha feature content 引用了未定义扭蛋池: $PoolId"
        }
        $FeatureTable = $FeatureMaster[$PoolId]
        if ($FeatureTable -isnot [System.Collections.IDictionary]) {
            throw "$Label gacha feature content 池类型错误: $PoolId"
        }
        foreach ($FeatureEntry in $FeatureTable.GetEnumerator()) {
            $RowValue = $FeatureEntry.Value
            if ($RowValue -isnot [Collections.IList]) {
                throw "$Label gacha feature content 行类型错误: pool=$PoolId index=$($FeatureEntry.Key)"
            }
            $Row = @($RowValue)
            if (@($Row | Where-Object { $_ -isnot [string] }).Count -gt 0 -or $Row.Count -lt 9) {
                throw "$Label gacha feature content 行结构错误: pool=$PoolId index=$($FeatureEntry.Key)"
            }
            $Kind = $Row[0]
            if (($Kind -ceq "0" -and [string]::IsNullOrEmpty($Row[2])) -or
                ($Kind -ceq "1" -and [string]::IsNullOrEmpty($Row[1])) -or
                ($Kind -ceq "2" -and [string]::IsNullOrEmpty($Row[5])) -or
                $Kind -cnotin @("0", "1", "2")) {
                throw "$Label gacha feature content 字段结构错误: pool=$PoolId index=$($FeatureEntry.Key)"
            }
            [void]$FeatureRowLengths.Add($Row.Count)
            [void]$FeatureKinds.Add($Kind)
            $FeatureKindCounts[$Kind] += 1
            $FeatureRows += 1
        }
    }

    return [pscustomobject][ordered]@{
        Label = $Label
        GachaRows = $GachaMaster.Count
        GachaRowLengths = @($GachaRowLengths | Sort-Object)
        FeaturePools = $FeatureMaster.Count
        FeatureRows = $FeatureRows
        FeatureRowLengths = @($FeatureRowLengths | Sort-Object)
        FeatureKinds = @($FeatureKinds | Sort-Object)
        FeatureKindCounts = $FeatureKindCounts
    }
}

function Test-CnGachaMasterStructure {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][System.Collections.IDictionary]$CurrentGacha,
        [Parameter(Mandatory)][System.Collections.IDictionary]$CurrentFeature,
        [Parameter(Mandatory)][System.Collections.IDictionary]$ReferenceGacha,
        [Parameter(Mandatory)][System.Collections.IDictionary]$ReferenceFeature
    )

    $Current = Get-CnGachaMasterStructure -Label "当前" -GachaMaster $CurrentGacha -FeatureMaster $CurrentFeature
    $Reference = Get-CnGachaMasterStructure -Label "CN 参考" -GachaMaster $ReferenceGacha -FeatureMaster $ReferenceFeature
    foreach ($PropertyName in @("GachaRowLengths", "FeatureRowLengths", "FeatureKinds")) {
        $CurrentValue = $Current.$PropertyName | ConvertTo-Json -Compress -Depth 10
        $ReferenceValue = $Reference.$PropertyName | ConvertTo-Json -Compress -Depth 10
        if ($CurrentValue -cne $ReferenceValue) {
            throw "gacha master 跨来源字段结构不一致: $PropertyName current=$CurrentValue reference=$ReferenceValue"
        }
    }
    $SharedGachaRows = 0
    foreach ($PoolId in $CurrentGacha.Keys) {
        if (-not $ReferenceGacha.Contains($PoolId)) {
            continue
        }
        if (@($CurrentGacha[$PoolId]).Count -ne @($ReferenceGacha[$PoolId]).Count) {
            throw "gacha master 共享扭蛋池行结构不一致: $PoolId"
        }
        $SharedGachaRows += 1
    }
    $SharedFeatureRows = 0
    foreach ($PoolId in $CurrentFeature.Keys) {
        if (-not $ReferenceFeature.Contains($PoolId)) {
            continue
        }
        $CurrentTable = $CurrentFeature[$PoolId]
        $ReferenceTable = $ReferenceFeature[$PoolId]
        foreach ($FeatureIndex in $CurrentTable.Keys) {
            if (-not $ReferenceTable.Contains($FeatureIndex)) {
                continue
            }
            $CurrentRow = @($CurrentTable[$FeatureIndex])
            $ReferenceRow = @($ReferenceTable[$FeatureIndex])
            if ($CurrentRow.Count -ne $ReferenceRow.Count -or $CurrentRow[0] -cne $ReferenceRow[0]) {
                throw "gacha feature content 共享行结构不一致: pool=$PoolId index=$FeatureIndex"
            }
            $SharedFeatureRows += 1
        }
    }
    return [pscustomobject][ordered]@{
        Current = $Current
        Reference = $Reference
        SharedGachaRows = $SharedGachaRows
        SharedFeatureRows = $SharedFeatureRows
        Compatible = $true
    }
}

function Get-CnGachaVisualReferences {
    [CmdletBinding()]
    param([Parameter(Mandatory)][object[]]$Pools)

    $References = [Collections.Generic.List[object]]::new()
    foreach ($Pool in $Pools) {
        $References.Add([pscustomobject][ordered]@{
            PoolId = $Pool.Id
            FeatureIndex = $null
            Kind = "banner"
            MasterValue = $Pool.Banner
            LogicalPath = if ($Pool.Banner.EndsWith(".png", [StringComparison]::OrdinalIgnoreCase)) { $Pool.Banner } else { "$($Pool.Banner).png" }
        })
        $References.Add([pscustomobject][ordered]@{
            PoolId = $Pool.Id
            FeatureIndex = $null
            Kind = "detail"
            MasterValue = $Pool.Detail
            LogicalPath = if ($Pool.Detail.EndsWith(".html.deflate", [StringComparison]::OrdinalIgnoreCase)) { $Pool.Detail } else { "$($Pool.Detail).html.deflate" }
        })
        foreach ($MovieId in $Pool.MovieIds) {
            $References.Add([pscustomobject][ordered]@{
                PoolId = $Pool.Id
                FeatureIndex = $null
                Kind = "movie"
                MasterValue = $MovieId
                LogicalPath = "gacha/$MovieId.gacha.amf3.deflate"
            })
        }
        foreach ($Feature in $Pool.Features) {
            if ($Feature.Type -ceq "movie") {
                $MoviePath = $Feature.AssetPath
                $References.Add([pscustomobject][ordered]@{
                    PoolId = $Pool.Id
                    FeatureIndex = $Feature.Index
                    Kind = "feature-movie"
                    MasterValue = $MoviePath
                    LogicalPath = "$MoviePath.movie.amf3.deflate"
                })
            } elseif ($Feature.Type -ceq "image") {
                $ImagePath = $Feature.AssetPath
                $References.Add([pscustomobject][ordered]@{
                    PoolId = $Pool.Id
                    FeatureIndex = $Feature.Index
                    Kind = "feature-image"
                    MasterValue = $ImagePath
                    LogicalPath = if ($ImagePath.EndsWith(".png", [StringComparison]::OrdinalIgnoreCase)) { $ImagePath } else { "$ImagePath.png" }
                })
            }
        }
    }
    return @($References)
}

function Find-CnAppAssetLocations {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][string]$LogicalPath
    )

    $Hash = Get-CnAssetHash -LogicalPath $LogicalPath
    $RelativeHashPath = Join-Path $Hash.Substring(0, 2) $Hash.Substring(2)
    $Locations = @()
    foreach ($DirectoryName in @("bundle", "ios_bundle", "medium_bundle", "ios_medium_bundle", "small_bundle", "ios_small_bundle")) {
        $CandidatePath = Join-Path (Join-Path $Root "production\$DirectoryName") $RelativeHashPath
        if (Test-Path -LiteralPath $CandidatePath -PathType Leaf) {
            $File = Get-Item -LiteralPath $CandidatePath
            if ($File.Length -lt 1) {
                throw "App bundle 扭蛋资源为空: $LogicalPath"
            }
            $Locations += [pscustomobject][ordered]@{
                RelativePath = $File.FullName.Substring($Root.Length + 1).Replace("\", "/")
                ByteLength = $File.Length
            }
        }
    }
    return $Locations
}

function Resolve-CnGachaVisualAssets {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][string]$AppRoot,
        [Parameter(Mandatory)][System.Collections.IDictionary]$Manifests,
        [Parameter(Mandatory)][object[]]$References
    )

    $ReferencesByPath = [ordered]@{}
    foreach ($Reference in $References) {
        if (-not $ReferencesByPath.Contains($Reference.LogicalPath)) {
            $ReferencesByPath[$Reference.LogicalPath] = @()
        }
        $ReferencesByPath[$Reference.LogicalPath] += $Reference
    }

    $EntityPaths = @()
    $EntityFallbackRows = @()
    $BundledRows = @()
    $Missing = @()
    foreach ($LogicalPath in $ReferencesByPath.Keys) {
        $Consumers = @($ReferencesByPath[$LogicalPath])
        $Record = Find-CnEntityRecord -Manifests $Manifests -LogicalPath $LogicalPath
        if ($null -ne $Record) {
            $BundleLocations = @(Find-CnAppAssetLocations -Root $AppRoot -LogicalPath $LogicalPath)
            $CdnBundleLocations = @(Find-CnAppAssetLocations -Root $Root -LogicalPath $LogicalPath)
            if ($BundleLocations.Count -gt 0 -or $CdnBundleLocations.Count -gt 0) {
                $EntityFallbackRows += [pscustomobject][ordered]@{
                    LogicalPath = $LogicalPath
                    Kinds = @($Consumers.Kind | Sort-Object -Unique)
                    PoolIds = @($Consumers.PoolId | Sort-Object -Unique)
                    Source = "EntityLists + local bundle"
                    Locations = @($BundleLocations + $CdnBundleLocations)
                }
                continue
            }
            $EntityPaths += $LogicalPath
            continue
        }
        $Locations = @(Find-CnAppAssetLocations -Root $AppRoot -LogicalPath $LogicalPath)
        $CdnLocations = @(Find-CnAppAssetLocations -Root $Root -LogicalPath $LogicalPath)
        $Locations += $CdnLocations
        if ($Locations.Count -lt 1) {
            $Missing += [pscustomobject][ordered]@{
                LogicalPath = $LogicalPath
                Kinds = @($Consumers.Kind | Sort-Object -Unique)
                PoolIds = @($Consumers.PoolId | Sort-Object -Unique)
            }
            continue
        }
        $BundledRows += [pscustomobject][ordered]@{
            LogicalPath = $LogicalPath
            Kinds = @($Consumers.Kind | Sort-Object -Unique)
            PoolIds = @($Consumers.PoolId | Sort-Object -Unique)
            Source = "local bundle"
            Locations = $Locations
        }
    }
    $EntityAssets = [ordered]@{}
    if ($EntityPaths.Count -gt 0) {
        $EntityAssets = Read-CnLogicalAssets -Root $Root -Manifests $Manifests -LogicalPaths @($EntityPaths | Sort-Object -Unique) -AllowMissing
    }
    $EntityRows = @()
    foreach ($LogicalPath in @($EntityPaths | Sort-Object -Unique)) {
        if (-not $EntityAssets.Contains($LogicalPath)) {
            $Missing += [pscustomobject][ordered]@{
                LogicalPath = $LogicalPath
                Kinds = @($ReferencesByPath[$LogicalPath].Kind | Sort-Object -Unique)
                PoolIds = @($ReferencesByPath[$LogicalPath].PoolId | Sort-Object -Unique)
            }
            continue
        }
        $Asset = $EntityAssets[$LogicalPath]
        $Consumers = @($ReferencesByPath[$LogicalPath])
        $EntityRows += [pscustomobject][ordered]@{
            LogicalPath = $LogicalPath
            Kinds = @($Consumers.Kind | Sort-Object -Unique)
            PoolIds = @($Consumers.PoolId | Sort-Object -Unique)
            EntryPath = $Asset.Record.EntryPath
            Version = $Asset.Record.Version
            ByteLength = $Asset.Record.ByteLength
            Digest = $Asset.Record.Digest
            Source = "EntityLists + archive"
            Archive = [IO.Path]::GetFileName($Asset.Archive)
        }
    }
    if ($Missing.Count -gt 0) {
        $Preview = @($Missing | Select-Object -First 50 | ForEach-Object {
            "$($_.LogicalPath) [kinds=$($_.Kinds -join ','); pools=$($_.PoolIds -join ',')]"
        }) -join "; "
        throw "最终 gacha master 存在不可达资源: count=$($Missing.Count); $Preview"
    }
    return [pscustomobject][ordered]@{
        References = $References.Count
        LogicalAssets = $ReferencesByPath.Count
        EntityAssets = $EntityRows.Count
        BundledAssets = $BundledRows.Count
        Assets = @($EntityRows) + @($EntityFallbackRows) + @($BundledRows)
    }
}
# //// /解码 master 并推导全部扭蛋池的资源引用 ////

# //// 核对 CN 字节来源和动画 seed 池 [@x380kkm 2026-08-22] ////
function Test-CnAppAssetParity {
    [CmdletBinding()]
    param()

    $CurrentBaseRoot = Join-Path $AppAssetRoot "production\ios_bundle"
    $ReferenceBaseRoot = Join-Path $CnAppAssetRoot "production\ios_bundle"
    foreach ($BaseRoot in @($CurrentBaseRoot, $ReferenceBaseRoot)) {
        if (-not (Test-Path -LiteralPath $BaseRoot -PathType Container)) {
            throw "包内基础资源目录缺失: $BaseRoot"
        }
    }
    $ReferenceFiles = @(Get-ChildItem -LiteralPath $ReferenceBaseRoot -Recurse -File | Sort-Object FullName)
    $CurrentFiles = @(Get-ChildItem -LiteralPath $CurrentBaseRoot -Recurse -File | Sort-Object FullName)
    if ($ReferenceFiles.Count -ne $CurrentFiles.Count) {
        throw "包内基础资源文件数量与 CN 原包不一致"
    }
    $CurrentByPath = @{}
    foreach ($CurrentFile in $CurrentFiles) {
        $RelativePath = $CurrentFile.FullName.Substring($CurrentBaseRoot.Length + 1)
        $CurrentByPath[$RelativePath] = $CurrentFile
    }
    $TotalBytes = 0L
    foreach ($ReferenceFile in $ReferenceFiles) {
        $RelativePath = $ReferenceFile.FullName.Substring($ReferenceBaseRoot.Length + 1)
        if (-not $CurrentByPath.ContainsKey($RelativePath)) {
            throw "包内基础资源缺少 CN 文件: $RelativePath"
        }
        $CurrentFile = $CurrentByPath[$RelativePath]
        if ($CurrentFile.Length -ne $ReferenceFile.Length -or
            (Get-FileHash -LiteralPath $CurrentFile.FullName -Algorithm SHA256).Hash -cne
            (Get-FileHash -LiteralPath $ReferenceFile.FullName -Algorithm SHA256).Hash) {
            throw "包内基础资源与 CN 原包不一致: $RelativePath"
        }
        $TotalBytes += $CurrentFile.Length
    }
    return [pscustomobject][ordered]@{
        Source = "CN iOS 1.8.4 app asset"
        Files = $CurrentFiles.Count
        Bytes = $TotalBytes
    }
}

function Read-CnGachaMovieConfigCoverage {
    [CmdletBinding()]
    param([Parameter(Mandatory)][object[]]$Pools)

    $MovieIds = @($Pools | ForEach-Object { $_.MovieIds } | Sort-Object -Unique)
    $Coverage = @()
    foreach ($MovieId in $MovieIds) {
        $ConfigPath = Join-Path (Join-Path $CnAssetsRoot "gacha_movie_configs") "$MovieId.amf3"
        if (-not (Test-Path -LiteralPath $ConfigPath -PathType Leaf)) {
            throw "CN 动画配置源文件缺失: $ConfigPath"
        }
        $Bytes = [IO.File]::ReadAllBytes($ConfigPath)
        $Coverage += [pscustomobject][ordered]@{
            MovieId = $MovieId
            LogicalPath = "gacha/$MovieId.gacha.amf3.deflate"
            ByteLength = $Bytes.Length
            Digest = Get-CnEntityDigest -Bytes $Bytes
            Source = "CN gacha_movie_configs"
        }
    }
    return $Coverage
}

function Read-CnGachaSeedCoverage {
    [CmdletBinding()]
    param()

    $SeedSources = [ordered]@{
        normal = "gacha_movie_seeds_normal.json"
        fes = "gacha_movie_seeds_fes.json"
        normal_guarantee = "gacha_movie_seeds_normal_guarantee.json"
        fes_guarantee = "gacha_movie_seeds_fes_guarantee.json"
    }
    $Coverage = @()
    foreach ($MovieId in $SeedSources.Keys) {
        $SeedPath = Join-Path $ServiceAssetsRoot $SeedSources[$MovieId]
        if (-not (Test-Path -LiteralPath $SeedPath -PathType Leaf)) {
            throw "扭蛋动画 seed 文件缺失: $SeedPath"
        }
        $Document = [IO.File]::ReadAllText($SeedPath, $Utf8) | ConvertFrom-Json -AsHashtable
        $Rarities = [ordered]@{}
        $RequiredRarities = if ($MovieId -in @("normal", "fes")) { @("1", "2", "3") } else { @("1", "2") }
        foreach ($Rarity in @("1", "2", "3")) {
            $Seeds = @($Document[$Rarity]["0"])
            if ($Rarity -in $RequiredRarities -and $Seeds.Count -lt 1) {
                throw "扭蛋动画 seed 池为空: movie=$MovieId rarity=$Rarity"
            }
            $Rarities[$Rarity] = $Seeds.Count
        }
        $Coverage += [pscustomobject][ordered]@{
            MovieId = $MovieId
            Source = $SeedSources[$MovieId]
            RaritySeedCounts = $Rarities
        }
    }
    return $Coverage
}
# //// /核对 CN 字节来源和动画 seed 池 ////

# //// 验证差分 ZIP 和 HTTP Range 契约 [@x380kkm 2026-08-22] ////
function Get-FileSha256Base64 {
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

function Assert-LegacyZip {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$FilePath)

    $Bytes = [IO.File]::ReadAllBytes($FilePath)
    $EndOffset = -1
    for ($Index = $Bytes.Length - 22; $Index -ge [Math]::Max(0, $Bytes.Length - 65557); $Index -= 1) {
        if ($Bytes[$Index] -eq 0x50 -and $Bytes[$Index + 1] -eq 0x4b -and
            $Bytes[$Index + 2] -eq 0x05 -and $Bytes[$Index + 3] -eq 0x06) {
            $EndOffset = $Index
            break
        }
    }
    if ($EndOffset -lt 0) {
        throw "ZIP 缺少结束记录: $FilePath"
    }
    if (($EndOffset -ge 20 -and [BitConverter]::ToUInt32($Bytes, $EndOffset - 20) -eq 0x07064b50) -or
        [BitConverter]::ToUInt16($Bytes, $EndOffset + 8) -eq 0xffff -or
        [BitConverter]::ToUInt16($Bytes, $EndOffset + 10) -eq 0xffff -or
        [BitConverter]::ToUInt32($Bytes, $EndOffset + 12) -eq 0xffffffff -or
        [BitConverter]::ToUInt32($Bytes, $EndOffset + 16) -eq 0xffffffff) {
        throw "ZIP 使用 ZIP64: $FilePath"
    }
    return $Bytes
}

function Test-CnGachaHttpRange {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Url,
        [Parameter(Mandatory)][byte[]]$ExpectedBytes
    )

    $Client = [Net.Http.HttpClient]::new()
    try {
        $Request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get, $Url)
        $Request.Headers.Range = [Net.Http.Headers.RangeHeaderValue]::new(0, 31)
        $Response = $Client.SendAsync($Request).GetAwaiter().GetResult()
        try {
            $Body = $Response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
            if ($Response.StatusCode -ne [Net.HttpStatusCode]::PartialContent -or $Body.Length -ne 32) {
                throw "扭蛋差分 HTTP Range 响应错误: $Url"
            }
            for ($Index = 0; $Index -lt $Body.Length; $Index += 1) {
                if ($Body[$Index] -ne $ExpectedBytes[$Index]) {
                    throw "扭蛋差分 HTTP Range 字节错误: $Url"
                }
            }
            $ExpectedRange = "bytes 0-31/$($ExpectedBytes.Length)"
            if ($Response.Content.Headers.ContentRange.ToString() -cne $ExpectedRange) {
                throw "扭蛋差分 HTTP Content-Range 错误: $Url"
            }
        } finally {
            $Response.Dispose()
            $Request.Dispose()
        }
    } finally {
        $Client.Dispose()
    }
    return "bytes=0-31"
}

function Test-CnGachaZipContracts {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Root)

    $ManifestPath = Join-Path $Root "path"
    $Manifest = [IO.File]::ReadAllText($ManifestPath, $Utf8) | ConvertFrom-Json
    $Contracts = @()
    foreach ($Diff in @($Manifest.diff)) {
        foreach ($ArchiveInfo in @($Diff.archive)) {
            $ArchiveUri = [Uri]$ArchiveInfo.location
            $ArchiveName = [IO.Path]::GetFileName($ArchiveUri.AbsolutePath)
            if (-not (
                $ArchiveName.StartsWith("starpoint-gacha-", [StringComparison]::Ordinal) -or
                $ArchiveName.StartsWith("starpoint-feature-images-", [StringComparison]::Ordinal)
            )) {
                continue
            }
            $ArchiveDirectory = @(
                $ArchiveUri.AbsolutePath.Trim('/').Split('/') |
                    Where-Object { $_ -match '^archive-(?:common|medium|android|ios)-(?:full|diff)$' } |
                    Select-Object -Last 1
            )
            if ($ArchiveDirectory.Count -ne 1) {
                throw "扭蛋差分 URL 缺少归档目录: $($ArchiveInfo.location)"
            }
            $ArchiveDirectory = [string]$ArchiveDirectory[0]
            $ArchivePath = Join-Path (Join-Path $Root $ArchiveDirectory) $ArchiveName
            if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
                throw "path 引用的扭蛋差分不存在: $ArchiveName"
            }
            $Bytes = Assert-LegacyZip -FilePath $ArchivePath
            if ($Bytes.Length -ne [long]$ArchiveInfo.size -or
                (Get-FileSha256Base64 -FilePath $ArchivePath) -cne [string]$ArchiveInfo.sha256) {
                throw "扭蛋差分与 path 的长度或 SHA-256 不一致: $ArchiveName"
            }
            $Range = "local-bytes=0-31"
            if (-not [string]::IsNullOrEmpty($RangeBaseUrl)) {
                $Url = $RangeBaseUrl.TrimEnd("/") + "/patch/cn/$ArchiveDirectory/" + $ArchiveName
                $Range = Test-CnGachaHttpRange -Url $Url -ExpectedBytes $Bytes
            }
            $Archive = [IO.Compression.ZipFile]::OpenRead($ArchivePath)
            try {
                if ($Archive.Entries.Count -lt 1) {
                    throw "扭蛋差分 ZIP 为空: $ArchiveName"
                }
                $EntryCount = $Archive.Entries.Count
            } finally {
                $Archive.Dispose()
            }
            $Contracts += [pscustomobject][ordered]@{
                Version = $Diff.version
                OriginalVersion = $Diff.original_version
                Archive = $ArchiveName
                ArchiveDirectory = $ArchiveDirectory
                ByteLength = $Bytes.Length
                Sha256 = $ArchiveInfo.sha256
                Zip64 = $false
                Entries = $EntryCount
                Range = $Range
            }
        }
    }
    if ($Contracts.Count -lt 1) {
        throw "path 未引用扭蛋差分 ZIP"
    }
    return $Contracts
}
# //// /验证差分 ZIP 和 HTTP Range 契约 ////

# //// 生成扭蛋视觉清单 [@x380kkm 2026-08-28] ////
$TemporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ("starpoint-gacha-visual-audit-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TemporaryRoot | Out-Null
try {
    $Repairs = @()
    if ($RepairMalformedEntityDigest) {
        $Repairs = @(Repair-CnGachaEntityDigest -Root $CdnRoot)
    }
    $CurrentManifests = Read-CnEntityManifests -Root $CdnRoot
    $ReferenceManifests = Read-CnEntityManifests -Root $ReferenceRoot
    $MasterPaths = @($MasterLogicalPaths.Values)
    $CurrentMasters = Read-CnLogicalAssets -Root $CdnRoot -Manifests $CurrentManifests -LogicalPaths $MasterPaths
    $ReferenceMasters = Read-CnLogicalAssets -Root $ReferenceRoot -Manifests $ReferenceManifests -LogicalPaths $MasterPaths

    $CurrentGacha = ConvertFrom-CnOrderedMapBytes -Bytes $CurrentMasters[$MasterLogicalPaths.Gacha].Bytes -TemporaryRoot $TemporaryRoot -Name "gacha-current"
    $CurrentFeature = ConvertFrom-CnOrderedMapBytes -Bytes $CurrentMasters[$MasterLogicalPaths.Feature].Bytes -TemporaryRoot $TemporaryRoot -Name "feature-current"
    $ReferenceGacha = ConvertFrom-CnOrderedMapBytes -Bytes $ReferenceMasters[$MasterLogicalPaths.Gacha].Bytes -TemporaryRoot $TemporaryRoot -Name "gacha-reference"
    $ReferenceFeature = ConvertFrom-CnOrderedMapBytes -Bytes $ReferenceMasters[$MasterLogicalPaths.Feature].Bytes -TemporaryRoot $TemporaryRoot -Name "feature-reference"
    $MasterStructure = Test-CnGachaMasterStructure `
        -CurrentGacha $CurrentGacha `
        -CurrentFeature $CurrentFeature `
        -ReferenceGacha $ReferenceGacha `
        -ReferenceFeature $ReferenceFeature
    $Pools = @(Get-CnGachaPools -GachaMaster $CurrentGacha -FeatureMaster $CurrentFeature)
    $VisualReferences = @(Get-CnGachaVisualReferences -Pools $Pools)
    $VisualCoverage = Resolve-CnGachaVisualAssets `
        -Root $CdnRoot `
        -AppRoot $AppAssetRoot `
        -Manifests $CurrentManifests `
        -References $VisualReferences
    $BaseAssetParity = Test-CnAppAssetParity
    $MovieConfigCoverage = @(Read-CnGachaMovieConfigCoverage -Pools $Pools)
    $SeedCoverage = @(Read-CnGachaSeedCoverage)
    $CandidateAuditJson = @(& uv run --python 3.12 $CandidateAuditPath `
        --cdn-root $CdnRoot `
        --app-asset-root $AppAssetRoot `
        --reference-cdn-root $ReferenceRoot `
        --service-assets-root $ServiceAssetsRoot `
        --reference-assets-root $CnAssetsRoot `
        --physics-root $PhysicsRoot)
    if ($LASTEXITCODE -ne 0) {
        throw "全部扭蛋候选资源审计执行失败"
    }
    $CandidateAudit = ($CandidateAuditJson -join "`n") | ConvertFrom-Json -Depth 100
    if ($CandidateAudit.status -cne "ok") {
        $GapKinds = @($CandidateAudit.gaps | Group-Object kind | Sort-Object Count -Descending | ForEach-Object {
            "$($_.Name)=$($_.Count)"
        }) -join ", "
        throw "全部扭蛋候选资源审计发现缺口: $GapKinds"
    }
    $ZipContracts = @(Test-CnGachaZipContracts -Root $CdnRoot)

    $Report = [pscustomobject][ordered]@{
        CdnAssetVersion = ([IO.File]::ReadAllText((Join-Path $CdnRoot "path"), $Utf8) | ConvertFrom-Json).info.target_asset_version
        MasterStructure = $MasterStructure
        Pools = $Pools
        VisualCoverage = [pscustomobject][ordered]@{
            References = $VisualCoverage.References
            LogicalAssets = $VisualCoverage.LogicalAssets
            EntityAssets = $VisualCoverage.EntityAssets
            BundledAssets = $VisualCoverage.BundledAssets
        }
        Assets = $VisualCoverage.Assets
        BaseAssetParity = $BaseAssetParity
        MovieConfigCoverage = $MovieConfigCoverage
        SeedCoverage = $SeedCoverage
        CandidateAudit = $CandidateAudit
        ZipContracts = $ZipContracts
        Repairs = $Repairs
    }
    $ReportJson = $Report | ConvertTo-Json -Depth 30
    if (-not [string]::IsNullOrEmpty($ReportPath)) {
        $ResolvedReportPath = [IO.Path]::GetFullPath($ReportPath)
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $ResolvedReportPath) | Out-Null
        [IO.File]::WriteAllText($ResolvedReportPath, $ReportJson, $Utf8)
    }
    Write-Output $ReportJson
} finally {
    $ResolvedTemporaryRoot = [IO.Path]::GetFullPath($TemporaryRoot)
    $SystemTemporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $ResolvedTemporaryRoot.StartsWith($SystemTemporaryRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝清理非临时目录: $ResolvedTemporaryRoot"
    }
    if (Test-Path -LiteralPath $ResolvedTemporaryRoot) {
        Remove-Item -LiteralPath $ResolvedTemporaryRoot -Recurse -Force
    }
}
# //// /生成扭蛋视觉清单 ////
