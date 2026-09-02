$ErrorActionPreference = 'Stop'
# audience: internal
# # cn-activity-banner-extractor
# 该模块顺序扫描 CN common 和 medium archive, 提取活动目录引用或尺寸匹配的 PNG, 并生成可重建清单.
# 活动目录模式只接受 PathFile 元数据精确匹配的原始对象. CN PNG 输出前恢复标准签名, 模块不修改原 archive.

Set-StrictMode -Version Latest

$script:LowercasePngSignature = [byte[]](0x89, 0x70, 0x6e, 0x67, 0x0d, 0x0a, 0x1a, 0x0a)
$script:StandardPngSignature = [byte[]](0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)
$script:DefaultArchiveDirectories = @(
    'archive-common-full',
    'archive-common-diff',
    'archive-medium-full',
    'archive-medium-diff'
)

# //// 比较字节前缀 [@x380kkm 2026-08-19] ////
function Test-BytePrefix {
    param(
        [Parameter(Mandatory)][byte[]]$Bytes,
        [Parameter(Mandatory)][byte[]]$Prefix
    )

    if ($Bytes.Length -lt $Prefix.Length) { return $false }
    for ($index = 0; $index -lt $Prefix.Length; $index += 1) {
        if ($Bytes[$index] -ne $Prefix[$index]) { return $false }
    }
    return $true
}
# //// /比较字节前缀 ////

# //// 读取流的有界前缀 [@x380kkm 2026-08-19] ////
function Read-StreamPrefix {
    param(
        [Parameter(Mandatory)][System.IO.Stream]$Stream,
        [ValidateRange(1, 4096)][int]$Length
    )

    $buffer = [byte[]]::new($Length)
    $offset = 0
    while ($offset -lt $Length) {
        $read = $Stream.Read($buffer, $offset, $Length - $offset)
        if ($read -eq 0) { break }
        $offset += $read
    }
    if ($offset -eq $Length) { return $buffer }
    return $buffer[0..([Math]::Max(0, $offset - 1))]
}
# //// /读取流的有界前缀 ////

# //// 解析 CN PNG 尺寸 [@x380kkm 2026-08-19] ////
function Get-CnPngInfo {
    param([Parameter(Mandatory)][byte[]]$Prefix)

    if ($Prefix.Length -lt 24) { return $null }
    $usesLowercaseSignature = Test-BytePrefix -Bytes $Prefix -Prefix $script:LowercasePngSignature
    if (-not $usesLowercaseSignature -and -not (Test-BytePrefix -Bytes $Prefix -Prefix $script:StandardPngSignature)) {
        return $null
    }
    if ([Text.Encoding]::ASCII.GetString($Prefix, 12, 4) -ne 'IHDR') { return $null }
    $width = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($Prefix, 16))
    $height = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($Prefix, 20))
    if ($width -le 0 -or $height -le 0) { return $null }
    [pscustomobject]@{
        Width = $width
        Height = $height
        UsesLowercaseSignature = $usesLowercaseSignature
    }
}
# //// /解析 CN PNG 尺寸 ////

# //// 读取候选资源原始字节 [@x380kkm 2026-08-19] ////
function Read-CnArchiveEntryBytes {
    param(
        [Parameter(Mandatory)][System.IO.Compression.ZipArchiveEntry]$Entry,
        [ValidateRange(24, 67108864)][long]$MaximumBytes
    )

    if ($Entry.Length -gt $MaximumBytes) { throw 'CN banner candidate exceeds the configured size limit.' }
    $stream = $Entry.Open()
    $memory = [IO.MemoryStream]::new([int]$Entry.Length)
    try {
        $stream.CopyTo($memory)
        return $memory.ToArray()
    } finally {
        $stream.Dispose()
        $memory.Dispose()
    }
}
# //// /读取候选资源原始字节 ////

# //// 标准化候选 PNG 签名 [@x380kkm 2026-08-19] ////
function ConvertTo-NormalizedCnPng {
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $normalizedBytes = [byte[]]$Bytes.Clone()
    if (Test-BytePrefix -Bytes $normalizedBytes -Prefix $script:LowercasePngSignature) {
        [Array]::Copy($script:StandardPngSignature, $normalizedBytes, $script:StandardPngSignature.Length)
    }
    if (-not (Test-BytePrefix -Bytes $normalizedBytes -Prefix $script:StandardPngSignature)) {
        throw 'CN banner candidate is not a supported PNG.'
    }
    return $normalizedBytes
}
# //// /标准化候选 PNG 签名 ////

# //// 原子写入字节和 UTF-8 JSON [@x380kkm 2026-08-19] ////
function Write-AtomicBytes {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][byte[]]$Bytes
    )

    $parent = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    $temporaryPath = "$Path.$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        [IO.File]::WriteAllBytes($temporaryPath, $Bytes)
        Move-Item -LiteralPath $temporaryPath -Destination $Path -Force
    } finally {
        if (Test-Path -LiteralPath $temporaryPath) {
            Remove-Item -LiteralPath $temporaryPath -Force
        }
    }
}

function Write-AtomicJson {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Value,
        [ValidateRange(2, 100)][int]$Depth = 20
    )

    $json = ($Value | ConvertTo-Json -Depth $Depth) + [Environment]::NewLine
    Write-AtomicBytes -Path $Path -Bytes ([Text.UTF8Encoding]::new($false).GetBytes($json))
}
# //// /原子写入字节和 UTF-8 JSON ////

# //// 计算标准化图片摘要 [@x380kkm 2026-08-19] ////
function Get-Sha256Hex {
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return [Convert]::ToHexString($sha256.ComputeHash($Bytes)).ToLowerInvariant()
    } finally {
        $sha256.Dispose()
    }
}
# //// /计算标准化图片摘要 ////

# //// 计算 PathFile 使用的 CN SHA-256 摘要 [@x380kkm 2026-08-19] ////
function Get-CnEntityDigest {
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $base64 = [Convert]::ToBase64String($sha256.ComputeHash($Bytes))
        return $base64.TrimEnd('=').Replace('+', '_').Replace('/', '-')
    } finally {
        $sha256.Dispose()
    }
}
# //// /计算 PathFile 使用的 CN SHA-256 摘要 ////

# //// 计算现有图片摘要并验证标准 PNG 签名 [@x380kkm 2026-08-19] ////
function Get-ExistingPngMetadata {
    param(
        [Parameter(Mandatory)][string]$Path,
        [ValidateRange(24, 67108864)][long]$MaximumBytes
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    $file = Get-Item -LiteralPath $Path
    if ($file.Length -lt 24 -or $file.Length -gt $MaximumBytes) { return $null }
    $stream = [IO.File]::OpenRead($file.FullName)
    try {
        $prefix = Read-StreamPrefix -Stream $stream -Length 8
        if (-not (Test-BytePrefix -Bytes $prefix -Prefix $script:StandardPngSignature)) { return $null }
        $stream.Position = 0
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try {
            $digest = [Convert]::ToHexString($sha256.ComputeHash($stream)).ToLowerInvariant()
        } finally {
            $sha256.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
    [pscustomobject]@{
        ByteLength = $file.Length
        Sha256 = $digest
    }
}
# //// /计算现有图片摘要并验证标准 PNG 签名 ////

# //// 构造可恢复写入的候选清单 [@x380kkm 2026-08-19] ////
function New-CandidateManifest {
    param(
        [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$CandidatesByKey,
        [Parameter(Mandatory)][int]$ArchiveCount,
        [Parameter(Mandatory)]$Selection,
        [string[]]$MissingHashes = @()
    )

    $candidates = @($CandidatesByKey.Values | Sort-Object source_hash)
    [pscustomobject][ordered]@{
        schema = 'starpoint-cn-activity-banner-candidates'
        version = 1
        generated_at = [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        archive_count = $ArchiveCount
        candidate_count = $candidates.Count
        missing_requested_candidates = @($MissingHashes | Sort-Object)
        selection = $Selection
        candidates = $candidates
    }
}
# //// /构造可恢复写入的候选清单 ////

# //// 读取活动的多来源图片声明 [@x380kkm 2026-08-19] ////
function Get-ActivityImageSourceCandidates {
    param([Parameter(Mandatory)]$Activity)

    $imageCandidates = Get-OptionalPropertyValue -Value $Activity -Name 'image_candidates'
    if ($null -ne $imageCandidates) { return @($imageCandidates) }

    $legacyHash = [string](Get-OptionalPropertyValue -Value $Activity -Name 'banner_candidate')
    if ([string]::IsNullOrWhiteSpace($legacyHash)) { return @() }
    return @([pscustomobject]@{
        source_hash = $legacyHash
        source_type = 'activity_banner'
        logical_path = [string](Get-OptionalPropertyValue -Value $Activity -Name 'banner_logical_path')
        source_entry = Get-OptionalPropertyValue -Value $Activity -Name 'banner_source_entry'
        source_version = Get-OptionalPropertyValue -Value $Activity -Name 'banner_source_version'
        source_byte_length = Get-OptionalPropertyValue -Value $Activity -Name 'banner_source_byte_length'
        source_digest = Get-OptionalPropertyValue -Value $Activity -Name 'banner_source_digest'
        association_confidence = 'legacy'
        evidence = 'legacy:banner_candidate'
    })
}
# //// /读取活动的多来源图片声明 ////

# //// 读取活动目录要求的精确资源记录 [@x380kkm 2026-08-19] ////
function Get-CatalogImageRequests {
    param([Parameter(Mandatory)]$CatalogSource)

    $requests = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
    foreach ($activity in @($CatalogSource.activities)) {
        foreach ($image in @(Get-ActivityImageSourceCandidates -Activity $activity)) {
            $candidate = [string](Get-OptionalPropertyValue -Value $image -Name 'source_hash')
            if ($candidate -notmatch '^[a-f0-9]{40}$') {
                throw "Activity $($activity.activity_id) has an invalid image candidate hash: $candidate"
            }

            $logicalPath = [string](Get-OptionalPropertyValue -Value $image -Name 'logical_path')
            $sourceEntry = [string](Get-OptionalPropertyValue -Value $image -Name 'source_entry')
            $sourceVersion = [string](Get-OptionalPropertyValue -Value $image -Name 'source_version')
            $sourceByteLengthValue = Get-OptionalPropertyValue -Value $image -Name 'source_byte_length'
            $sourceDigest = [string](Get-OptionalPropertyValue -Value $image -Name 'source_digest')
            $entryMatch = [regex]::Match(
                $sourceEntry,
                '^production/(?:(?:android|ios|medium)_)?upload/(?<prefix>[a-f0-9]{2})/(?<object>[a-f0-9]{38})$'
            )
            if (-not $entryMatch.Success -or "$($entryMatch.Groups['prefix'].Value)$($entryMatch.Groups['object'].Value)" -ne $candidate) {
                throw "Activity $($activity.activity_id) has an invalid image source entry: $sourceEntry"
            }
            if ($logicalPath -and ($logicalPath -notmatch '^[A-Za-z0-9_./-]{1,512}$' -or $logicalPath.Contains('..'))) {
                throw "Activity $($activity.activity_id) has an invalid image logical path."
            }
            if ([string]::IsNullOrWhiteSpace($sourceVersion)) {
                throw "Activity $($activity.activity_id) requires source_version."
            }
            if ($null -eq $sourceByteLengthValue) {
                throw "Activity $($activity.activity_id) requires source_byte_length."
            }
            $sourceByteLength = [long]$sourceByteLengthValue
            if ($sourceByteLength -lt 24 -or $sourceDigest -notmatch '^[A-Za-z0-9_-]{43}$') {
                throw "Activity $($activity.activity_id) has invalid image source metadata."
            }

            $request = [pscustomobject]@{
                SourceHash = $candidate
                SourceEntry = $sourceEntry
                SourceVersion = $sourceVersion
                SourceByteLength = $sourceByteLength
                SourceDigest = $sourceDigest
            }
            if ($requests.ContainsKey($candidate)) {
                $existing = $requests[$candidate]
                $sameRecord = (
                    $existing.SourceEntry -eq $request.SourceEntry -and
                    $existing.SourceVersion -eq $request.SourceVersion -and
                    $existing.SourceByteLength -eq $request.SourceByteLength -and
                    $existing.SourceDigest -eq $request.SourceDigest
                )
                if (-not $sameRecord) {
                    throw "Activities reference conflicting image source records: $candidate"
                }
                continue
            }
            $requests.Add($candidate, $request)
        }
    }
    return ,$requests
}
# //// /读取活动目录要求的精确资源记录 ////

# //// 读取可选 JSON 属性 [@x380kkm 2026-08-19] ////
function Get-OptionalPropertyValue {
    param(
        [Parameter(Mandatory)]$Value,
        [Parameter(Mandatory)][string]$Name
    )

    $property = $Value.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}
# //// /读取可选 JSON 属性 ////

# //// 将已验证的活动资源映射转换为服务 manifest [@x380kkm 2026-08-19] ////
function ConvertTo-ActivityCatalogManifest {
    param(
        [Parameter(Mandatory)]$CatalogSource,
        [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$CandidatesByKey
    )

    if ([int]$CatalogSource.format_version -ne 1) { throw 'Activity catalog source format_version must be 1.' }
    $activities = @($CatalogSource.activities)
    $activityIds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $resultActivities = foreach ($activity in $activities) {
        $activityId = [string]$activity.activity_id
        $name = [string]$activity.name
        $kind = [string]$activity.kind
        if ($activityId -notmatch '^[A-Za-z0-9:._-]{1,128}$') { throw "Invalid activity_id: $activityId" }
        if (-not $activityIds.Add($activityId)) { throw "Duplicate activity_id: $activityId" }
        if ([string]::IsNullOrWhiteSpace($name) -or [string]::IsNullOrWhiteSpace($kind)) {
            throw "Activity $activityId requires name and kind."
        }
        $imageCandidates = @(
            foreach ($image in @(Get-ActivityImageSourceCandidates -Activity $activity)) {
                $sourceHash = [string](Get-OptionalPropertyValue -Value $image -Name 'source_hash')
                if ($sourceHash -notmatch '^[a-f0-9]{40}$') {
                    throw "Activity $activityId has an invalid image candidate hash: $sourceHash"
                }
                $candidateKey = "$sourceHash.png"
                if (-not $CandidatesByKey.ContainsKey($candidateKey)) { continue }
                $sourceType = [string](Get-OptionalPropertyValue -Value $image -Name 'source_type')
                $evidence = [string](Get-OptionalPropertyValue -Value $image -Name 'evidence')
                if ($sourceType -notmatch '^[a-z0-9_-]{1,64}$') {
                    throw "Activity $activityId has an invalid image source_type."
                }
                if ([string]::IsNullOrWhiteSpace($evidence) -or $evidence.Length -gt 256 -or
                    $evidence -match '^(?:[A-Za-z]:[\\/]|/)') {
                    throw "Activity $activityId has invalid image evidence."
                }
                $candidate = $CandidatesByKey[$candidateKey]
                [pscustomobject][ordered]@{
                    key = $candidateKey
                    width = [int]$candidate.width
                    height = [int]$candidate.height
                    source_type = $sourceType
                    evidence = $evidence
                }
            }
        )
        $entry = [ordered]@{
            activity_id = $activityId
            name = $name
            kind = $kind
            tags = @((Get-OptionalPropertyValue -Value $activity -Name 'tags') | ForEach-Object { [string]$_ })
            description = [string](Get-OptionalPropertyValue -Value $activity -Name 'description')
            image_candidates = $imageCandidates
        }
        if ($imageCandidates.Count -gt 0) {
            $entry.banner_key = $imageCandidates[0].key
            $entry.banner_width = $imageCandidates[0].width
            $entry.banner_height = $imageCandidates[0].height
        }
        $defaultStartAt = Get-OptionalPropertyValue -Value $activity -Name 'default_start_at_ms'
        $defaultEndAt = Get-OptionalPropertyValue -Value $activity -Name 'default_end_at_ms'
        if ($null -ne $defaultStartAt) { $entry.default_start_at_ms = [long]$defaultStartAt }
        if ($null -ne $defaultEndAt) { $entry.default_end_at_ms = [long]$defaultEndAt }
        [pscustomobject]$entry
    }
    $region = Get-OptionalPropertyValue -Value $CatalogSource -Name 'region'
    $clientVersion = Get-OptionalPropertyValue -Value $CatalogSource -Name 'client_version'
    $assetVersion = Get-OptionalPropertyValue -Value $CatalogSource -Name 'asset_version'
    [pscustomobject][ordered]@{
        format_version = 1
        region = if ($region) { [string]$region } else { 'cn' }
        client_version = if ($clientVersion) { [string]$clientVersion } else { $null }
        asset_version = if ($assetVersion) { [string]$assetVersion } else { $null }
        generated_at = [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        activities = @($resultActivities)
    }
}
# //// /将已验证的活动资源映射转换为服务 manifest ////

# //// 提取 CN 活动横幅候选并生成清单 [@x380kkm 2026-08-19] ////
function Export-CnActivityBannerCandidates {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$CdnRoot,
        [string]$OutputDirectory,
        [string]$CandidateManifestPath,
        [string]$CatalogSourcePath,
        [string]$CatalogOutputPath,
        [ValidateRange(64, 8192)][int]$MinimumWidth = 480,
        [ValidateRange(32, 4096)][int]$MinimumHeight = 80,
        [ValidateRange(32, 4096)][int]$MaximumHeight = 720,
        [ValidateRange(1.0, 20.0)][double]$MinimumAspectRatio = 2.2,
        [ValidateRange(1.0, 20.0)][double]$MaximumAspectRatio = 8.0,
        [ValidateRange(24, 67108864)][long]$MaximumBannerBytes = 16777216,
        [string[]]$ArchiveDirectories = $script:DefaultArchiveDirectories
    )

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $resolvedCdnRoot = (Resolve-Path -LiteralPath $CdnRoot).Path
    $OutputDirectory = if ($OutputDirectory) { $OutputDirectory } else { Join-Path $resolvedCdnRoot 'activity-banners' }
    $CandidateManifestPath = if ($CandidateManifestPath) { $CandidateManifestPath } else { Join-Path $resolvedCdnRoot 'activity-banner-candidates.json' }
    $CatalogOutputPath = if ($CatalogOutputPath) { $CatalogOutputPath } else { Join-Path $resolvedCdnRoot 'activity-catalog.json' }
    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

    $catalogSource = if ($CatalogSourcePath) {
        Get-Content -LiteralPath $CatalogSourcePath -Raw -Encoding UTF8 | ConvertFrom-Json
    } else {
        $null
    }
    $requestedCandidates = if ($null -ne $catalogSource) {
        Get-CatalogImageRequests -CatalogSource $catalogSource
    } else {
        ,([Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal))
    }
    $candidatesByKey = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
    $selection = [pscustomobject][ordered]@{
        minimum_width = $MinimumWidth
        minimum_height = $MinimumHeight
        maximum_height = $MaximumHeight
        minimum_aspect_ratio = $MinimumAspectRatio
        maximum_aspect_ratio = $MaximumAspectRatio
        maximum_banner_bytes = $MaximumBannerBytes
    }
    $archiveCount = 0
    $allRequestedCandidatesFound = $false
    foreach ($directoryName in $ArchiveDirectories) {
        if ($directoryName -notmatch '^archive-(common|medium)-(full|diff)$') {
            throw "Unsupported CN archive directory: $directoryName"
        }
        $archiveDirectory = Join-Path $resolvedCdnRoot $directoryName
        if (-not (Test-Path -LiteralPath $archiveDirectory -PathType Container)) { continue }
        foreach ($archiveFile in Get-ChildItem -LiteralPath $archiveDirectory -File -Filter '*.zip' | Sort-Object Name) {
            $archiveCount += 1
            $archive = [IO.Compression.ZipFile]::OpenRead($archiveFile.FullName)
            try {
                foreach ($entry in $archive.Entries) {
                    if ($entry.Length -lt 24 -or $entry.Length -gt $MaximumBannerBytes) { continue }
                    $normalizedEntryPath = $entry.FullName.Replace('\', '/')
                    if ($normalizedEntryPath -notmatch '/(?<prefix>[a-f0-9]{2})/(?<object>[a-f0-9]{38})$') { continue }
                    $sourcePrefix = $Matches.prefix
                    $objectId = $Matches.object
                    $sourceHash = "$sourcePrefix$objectId"
                    $request = $null
                    if ($requestedCandidates.Count -gt 0) {
                        if (-not $requestedCandidates.TryGetValue($sourceHash, [ref]$request)) { continue }
                        if ($normalizedEntryPath -ne $request.SourceEntry -or $entry.Length -ne $request.SourceByteLength) {
                            continue
                        }
                    }

                    $key = "$sourceHash.png"
                    if ($candidatesByKey.ContainsKey($key)) { continue }
                    $entryStream = $entry.Open()
                    try {
                        $prefix = Read-StreamPrefix -Stream $entryStream -Length 24
                    } finally {
                        $entryStream.Dispose()
                    }
                    $png = Get-CnPngInfo -Prefix $prefix
                    if ($null -eq $png) { continue }
                    $aspectRatio = $png.Width / [double]$png.Height
                    $outsideBannerBounds = (
                        $png.Width -lt $MinimumWidth -or
                        $png.Height -lt $MinimumHeight -or
                        $png.Height -gt $MaximumHeight -or
                        $aspectRatio -lt $MinimumAspectRatio -or
                        $aspectRatio -gt $MaximumAspectRatio
                    )
                    $invalidRequestedImageSize = $null -ne $request -and (
                        $png.Width -lt 32 -or $png.Height -lt 32 -or
                        $png.Width -gt 8192 -or $png.Height -gt 8192
                    )
                    if (($null -eq $request -and $outsideBannerBounds) -or $invalidRequestedImageSize) {
                        continue
                    }

                    $rawBytes = Read-CnArchiveEntryBytes -Entry $entry -MaximumBytes $MaximumBannerBytes
                    $sourceDigest = Get-CnEntityDigest -Bytes $rawBytes
                    if ($null -ne $request -and $sourceDigest -ne $request.SourceDigest) { continue }
                    $outputPath = Join-Path $OutputDirectory $key
                    $existing = Get-ExistingPngMetadata -Path $outputPath -MaximumBytes $MaximumBannerBytes
                    $bytes = ConvertTo-NormalizedCnPng -Bytes $rawBytes
                    $byteLength = $bytes.Length
                    $sha256 = Get-Sha256Hex -Bytes $bytes
                    if ($null -eq $existing -or $existing.Sha256 -ne $sha256) {
                        Write-AtomicBytes -Path $outputPath -Bytes $bytes
                    }
                    $relativeArchive = "$directoryName/$($archiveFile.Name)"
                    $candidatesByKey.Add($key, [pscustomobject][ordered]@{
                        key = $key
                        source_hash = $sourceHash
                        source_prefix = $sourcePrefix
                        source_object_id = $objectId
                        width = $png.Width
                        height = $png.Height
                        aspect_ratio = [Math]::Round($aspectRatio, 4)
                        byte_length = $byteLength
                        sha256 = $sha256
                        source_archive = $relativeArchive
                        source_entry = $normalizedEntryPath
                        source_version = if ($null -ne $request) { $request.SourceVersion } else { $null }
                        source_byte_length = $rawBytes.Length
                        source_digest = $sourceDigest
                        normalized_png_signature = $png.UsesLowercaseSignature
                    })
                }
            } finally {
                $archive.Dispose()
            }
            if ($archiveCount % 25 -eq 0) {
                $checkpoint = New-CandidateManifest -CandidatesByKey $candidatesByKey -ArchiveCount $archiveCount -Selection $selection
                Write-AtomicJson -Path $CandidateManifestPath -Value $checkpoint
            }
            if ($requestedCandidates.Count -gt 0 -and $candidatesByKey.Count -eq $requestedCandidates.Count) {
                $allRequestedCandidatesFound = $true
                break
            }
        }
        if ($allRequestedCandidatesFound) { break }
    }

    $missingHashes = @($requestedCandidates.Keys | Where-Object { -not $candidatesByKey.ContainsKey("$_.png") } | Sort-Object)
    $manifest = New-CandidateManifest -CandidatesByKey $candidatesByKey -ArchiveCount $archiveCount -Selection $selection -MissingHashes $missingHashes
    Write-AtomicJson -Path $CandidateManifestPath -Value $manifest

    $catalog = $null
    if ($null -ne $catalogSource) {
        $catalog = ConvertTo-ActivityCatalogManifest -CatalogSource $catalogSource -CandidatesByKey $candidatesByKey
        Write-AtomicJson -Path $CatalogOutputPath -Value $catalog
    }
    $catalogActivities = if ($null -ne $catalog) { @($catalog.activities) } else { @() }
    $catalogActivitiesWithImages = @($catalogActivities | Where-Object { @($_.image_candidates).Count -gt 0 })

    return [pscustomobject]@{
        ArchiveCount = $archiveCount
        CandidateCount = $candidatesByKey.Count
        MissingCandidateCount = $missingHashes.Count
        CatalogActivityCount = if ($null -ne $catalog) { $catalogActivities.Count } else { $null }
        CatalogActivityWithImageCount = if ($null -ne $catalog) { $catalogActivitiesWithImages.Count } else { $null }
        CatalogActivityWithoutImageCount = if ($null -ne $catalog) { $catalogActivities.Count - $catalogActivitiesWithImages.Count } else { $null }
        OutputDirectory = $OutputDirectory
        CandidateManifestPath = $CandidateManifestPath
        CatalogOutputPath = if ($CatalogSourcePath) { $CatalogOutputPath } else { $null }
    }
}
# //// /提取 CN 活动横幅候选并生成清单 ////

Export-ModuleMember -Function Export-CnActivityBannerCandidates
