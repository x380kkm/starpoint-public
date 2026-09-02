$ErrorActionPreference = 'Stop'
# audience: internal
# # test-cn-activity-banner-extractor
# 此脚本用小型 archive 验证 CN PNG 签名恢复, PathFile 版本选择, 横幅筛选, 清单生成和活动映射校验.

Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'cn-activity-banner-extractor.psm1') -Force

# //// 断言测试条件成立 [@x380kkm 2026-08-19] ////
function Assert-TestCondition {
    param([Parameter(Mandatory)][bool]$Condition, [Parameter(Mandatory)][string]$Message)
    if (-not $Condition) { throw $Message }
}
# //// /断言测试条件成立 ////

# //// 构造最小 PNG 字节 [@x380kkm 2026-08-19] ////
function New-TestPngBytes {
    param(
        [Parameter(Mandatory)][int]$Width,
        [Parameter(Mandatory)][int]$Height,
        [switch]$LowercaseSignature,
        [ValidateRange(0, 16)][int]$PaddingLength = 0
    )

    $bytes = [byte[]]::new(32 + $PaddingLength)
    $signature = if ($LowercaseSignature) {
        [byte[]](0x89, 0x70, 0x6e, 0x67, 0x0d, 0x0a, 0x1a, 0x0a)
    } else {
        [byte[]](0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)
    }
    [Array]::Copy($signature, $bytes, $signature.Length)
    $bytes[11] = 13
    [Array]::Copy([Text.Encoding]::ASCII.GetBytes('IHDR'), 0, $bytes, 12, 4)
    $widthBytes = [BitConverter]::GetBytes([Net.IPAddress]::HostToNetworkOrder($Width))
    $heightBytes = [BitConverter]::GetBytes([Net.IPAddress]::HostToNetworkOrder($Height))
    [Array]::Copy($widthBytes, 0, $bytes, 16, 4)
    [Array]::Copy($heightBytes, 0, $bytes, 20, 4)
    return $bytes
}
# //// /构造最小 PNG 字节 ////

# //// 写入测试 archive 条目 [@x380kkm 2026-08-19] ////
function Add-TestArchiveEntry {
    param(
        [Parameter(Mandatory)][IO.Compression.ZipArchive]$Archive,
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][byte[]]$Bytes
    )

    $entry = $Archive.CreateEntry($Path, [IO.Compression.CompressionLevel]::NoCompression)
    $stream = $entry.Open()
    try {
        $stream.Write($Bytes, 0, $Bytes.Length)
    } finally {
        $stream.Dispose()
    }
}
# //// /写入测试 archive 条目 ////

# //// 计算测试资源的 CN EntityLists 摘要 [@x380kkm 2026-08-19] ////
function Get-TestCnEntityDigest {
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $base64 = [Convert]::ToBase64String($sha256.ComputeHash($Bytes))
        return $base64.TrimEnd('=').Replace('+', '_').Replace('/', '-')
    } finally {
        $sha256.Dispose()
    }
}
# //// /计算测试资源的 CN EntityLists 摘要 ////

# //// 验证完整提取和活动映射流程 [@x380kkm 2026-08-19] ////
Assert-TestCondition (
    (Get-TestCnEntityDigest -Bytes ([byte[]](0x00, 0x02))) -eq '-PCmxwDdE_J0tvuo3uqN2bJuTu3eNJVxfKyECMnFF38'
) 'CN EntityLists 摘要字母表不正确.'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = Join-Path ([IO.Path]::GetTempPath()) "starpoint-activity-banner-$([Guid]::NewGuid().ToString('N'))"
try {
    $cdnRoot = Join-Path $root 'cn'
    $fullArchiveDirectory = Join-Path $cdnRoot 'archive-common-full'
    $diffArchiveDirectory = Join-Path $cdnRoot 'archive-common-diff'
    New-Item -ItemType Directory -Force -Path $fullArchiveDirectory | Out-Null
    New-Item -ItemType Directory -Force -Path $diffArchiveDirectory | Out-Null
    $bannerObjectId = '0123456789abcdef0123456789abcdef012345'
    $firstBannerHash = "01$bannerObjectId"
    $secondBannerHash = "02$bannerObjectId"
    $squareObjectId = '89abcdef0123456789abcdef0123456789abcd'
    $nonPngObjectId = 'fedcba9876543210fedcba9876543210fedcba'
    $nonPngHash = "03$nonPngObjectId"
    $firstSourceEntry = "production/upload/01/$bannerObjectId"
    $secondSourceEntry = "production/upload/02/$bannerObjectId"
    $firstSourceVersion = '1.4.48'
    $secondSourceVersion = '1.4.38'
    $firstCurrentBannerBytes = New-TestPngBytes -Width 1440 -Height 488 -LowercaseSignature
    $secondCurrentBannerBytes = New-TestPngBytes -Width 1920 -Height 540 -LowercaseSignature -PaddingLength 4
    $firstSourceDigest = Get-TestCnEntityDigest -Bytes $firstCurrentBannerBytes
    $secondSourceDigest = Get-TestCnEntityDigest -Bytes $secondCurrentBannerBytes

    $fullArchivePath = Join-Path $fullArchiveDirectory 'pinball-test.zip'
    $fullArchive = [IO.Compression.ZipFile]::Open($fullArchivePath, [IO.Compression.ZipArchiveMode]::Create)
    try {
        Add-TestArchiveEntry -Archive $fullArchive -Path $firstSourceEntry -Bytes (New-TestPngBytes -Width 1200 -Height 400 -LowercaseSignature)
        Add-TestArchiveEntry -Archive $fullArchive -Path $secondSourceEntry -Bytes (New-TestPngBytes -Width 1280 -Height 320 -LowercaseSignature)
        Add-TestArchiveEntry -Archive $fullArchive -Path "production/upload/89/$squareObjectId" -Bytes (New-TestPngBytes -Width 512 -Height 512)
        Add-TestArchiveEntry -Archive $fullArchive -Path "production/upload/03/$nonPngObjectId" -Bytes ([byte[]]::new(1048576))
        Add-TestArchiveEntry -Archive $fullArchive -Path 'production/upload/aa/not-a-safe-hash' -Bytes (New-TestPngBytes -Width 1440 -Height 488)
    } finally {
        $fullArchive.Dispose()
    }

    $diffArchivePath = Join-Path $diffArchiveDirectory 'pinball-test.zip'
    $diffArchive = [IO.Compression.ZipFile]::Open($diffArchivePath, [IO.Compression.ZipArchiveMode]::Create)
    try {
        Add-TestArchiveEntry -Archive $diffArchive -Path $firstSourceEntry -Bytes $firstCurrentBannerBytes
        Add-TestArchiveEntry -Archive $diffArchive -Path $secondSourceEntry -Bytes $secondCurrentBannerBytes
    } finally {
        $diffArchive.Dispose()
    }

    $catalogSourcePath = Join-Path $root 'catalog-source.json'
    [IO.File]::WriteAllText(
        $catalogSourcePath,
        @"
{
  "format_version": 1,
  "region": "cn",
  "client_version": "1.8.1",
  "activities": [
    {
      "activity_id": "raid:1",
      "name": "测试活动",
      "kind": "raid",
      "tags": ["test"],
      "description": "测试横幅",
      "image_candidates": [{
        "source_hash": "$firstBannerHash",
        "source_type": "activity_banner",
        "logical_path": "quest/event/banner/test.png",
        "source_entry": "$firstSourceEntry",
        "source_version": "$firstSourceVersion",
        "source_byte_length": $($firstCurrentBannerBytes.Length),
        "source_digest": "$firstSourceDigest",
        "association_confidence": "direct-field",
        "evidence": "master:raid_event:field:3"
      }],
      "default_start_at_ms": 1893542400000,
      "default_end_at_ms": 1893715200000
    },
    {
      "activity_id": "raid:2",
      "name": "同名哈希后缀活动",
      "kind": "raid",
      "tags": ["test"],
      "description": "验证完整哈希",
      "banner_candidate": "$secondBannerHash",
      "banner_source_entry": "$secondSourceEntry",
      "banner_source_version": "$secondSourceVersion",
      "banner_source_byte_length": $($secondCurrentBannerBytes.Length),
      "banner_source_digest": "$secondSourceDigest"
    }
  ]
}
"@,
        [Text.UTF8Encoding]::new($false)
    )

    $bannerDirectory = Join-Path $cdnRoot 'activity-banners'
    New-Item -ItemType Directory -Force -Path $bannerDirectory | Out-Null
    $staleBannerBytes = New-TestPngBytes -Width 1600 -Height 400
    [IO.File]::WriteAllBytes(
        (Join-Path $bannerDirectory "$firstBannerHash.png"),
        $staleBannerBytes
    )
    $staleSha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $staleDigest = [Convert]::ToHexString($staleSha256.ComputeHash($staleBannerBytes)).ToLowerInvariant()
    } finally {
        $staleSha256.Dispose()
    }
    [IO.File]::WriteAllText(
        (Join-Path $cdnRoot 'activity-banner-candidates.json'),
        ([pscustomobject][ordered]@{
            schema = 'starpoint-cn-activity-banner-candidates'
            version = 1
            candidates = @([pscustomobject][ordered]@{
                key = "$firstBannerHash.png"
                source_hash = $firstBannerHash
                width = 1600
                height = 400
                sha256 = $staleDigest
            })
        } | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )

    $result = Export-CnActivityBannerCandidates -CdnRoot $cdnRoot -CatalogSourcePath $catalogSourcePath
    Assert-TestCondition ($result.ArchiveCount -eq 2) '提取器未扫描旧 full 和当前 diff archive.'
    Assert-TestCondition ($result.CandidateCount -eq 2) '提取器未提取活动目录要求的两个横幅.'
    $bannerPath = Join-Path $cdnRoot "activity-banners\$firstBannerHash.png"
    $secondBannerPath = Join-Path $cdnRoot "activity-banners\$secondBannerHash.png"
    $bannerBytes = [IO.File]::ReadAllBytes($bannerPath)
    $secondBannerBytes = [IO.File]::ReadAllBytes($secondBannerPath)
    Assert-TestCondition ($bannerBytes[1] -eq 0x50 -and $bannerBytes[2] -eq 0x4e -and $bannerBytes[3] -eq 0x47) '小写 PNG 签名未恢复.'
    $extractedWidth = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($bannerBytes, 16))
    $extractedHeight = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($bannerBytes, 20))
    Assert-TestCondition ($extractedWidth -eq 1440 -and $extractedHeight -eq 488) '未经验证的旧 banner 未从当前 archive 重建.'
    $secondExtractedWidth = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($secondBannerBytes, 16))
    $secondExtractedHeight = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($secondBannerBytes, 20))
    Assert-TestCondition ($secondExtractedWidth -eq 1920 -and $secondExtractedHeight -eq 540) '完整资源哈希未隔离相同对象后缀.'

    $candidateManifest = Get-Content -LiteralPath $result.CandidateManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-TestCondition ($candidateManifest.schema -eq 'starpoint-cn-activity-banner-candidates') '候选清单 schema 不正确.'
    Assert-TestCondition ($candidateManifest.candidate_count -eq 2) '候选清单数量不正确.'
    $expectedCandidates = @(
        [pscustomobject]@{
            Key = "$firstBannerHash.png"
            SourceHash = $firstBannerHash
            SourceEntry = $firstSourceEntry
            SourceVersion = $firstSourceVersion
            SourceByteLength = $firstCurrentBannerBytes.Length
            SourceDigest = $firstSourceDigest
        },
        [pscustomobject]@{
            Key = "$secondBannerHash.png"
            SourceHash = $secondBannerHash
            SourceEntry = $secondSourceEntry
            SourceVersion = $secondSourceVersion
            SourceByteLength = $secondCurrentBannerBytes.Length
            SourceDigest = $secondSourceDigest
        }
    )
    foreach ($expected in $expectedCandidates) {
        $matches = @($candidateManifest.candidates | Where-Object { $_.key -eq $expected.Key })
        Assert-TestCondition ($matches.Count -eq 1) "候选清单缺少 $($expected.Key)."
        $candidate = $matches[0]
        Assert-TestCondition ($candidate.source_archive -eq 'archive-common-diff/pinball-test.zip') '候选清单未选择 PathFile 指定的 diff archive.'
        Assert-TestCondition ($candidate.source_hash -eq $expected.SourceHash) '候选清单没有保存完整资源哈希.'
        Assert-TestCondition ($candidate.source_entry -eq $expected.SourceEntry) '候选清单没有保存 PathFile 指定的资源条目.'
        Assert-TestCondition ($candidate.source_version -eq $expected.SourceVersion) '候选清单没有保存 PathFile 指定的资源版本.'
        Assert-TestCondition ($candidate.source_byte_length -eq $expected.SourceByteLength) '候选清单没有保存 PathFile 指定的原始长度.'
        Assert-TestCondition ($candidate.source_digest -eq $expected.SourceDigest) '候选清单没有保存 PathFile 指定的原始摘要.'
    }
    Assert-TestCondition (-not (($candidateManifest | ConvertTo-Json -Depth 20) -match [regex]::Escape($root))) '候选清单包含本机绝对路径.'

    $catalog = Get-Content -LiteralPath $result.CatalogOutputPath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-TestCondition ($result.CatalogActivityCount -eq 2) '提取结果未报告活动总数.'
    Assert-TestCondition ($result.CatalogActivityWithImageCount -eq 2) '提取结果未报告有图活动数.'
    Assert-TestCondition ($result.CatalogActivityWithoutImageCount -eq 0) '提取结果错误报告无图活动.'
    Assert-TestCondition ($catalog.activities[0].banner_key -eq "$firstBannerHash.png") '活动映射未转换为安全 banner key.'
    Assert-TestCondition ($catalog.activities[0].banner_width -eq 1440 -and $catalog.activities[0].banner_height -eq 488) '活动映射未保留 banner 尺寸.'
    Assert-TestCondition ($catalog.activities[0].image_candidates.Count -eq 1) '活动映射未保留图片候选数组.'
    Assert-TestCondition ($catalog.activities[0].image_candidates[0].source_type -eq 'activity_banner') '活动映射未保留图片来源类型.'
    Assert-TestCondition ($catalog.activities[0].image_candidates[0].evidence -eq 'master:raid_event:field:3') '活动映射未保留脱敏证据.'
    Assert-TestCondition ($catalog.activities[1].banner_key -eq "$secondBannerHash.png") '相同对象后缀活动发生资源键冲突.'
    Assert-TestCondition ($catalog.activities[1].image_candidates[0].evidence -eq 'legacy:banner_candidate') '旧 banner 字段未转换为兼容图片候选.'
    Assert-TestCondition (-not ($catalog.activities[0].PSObject.Properties.Name -contains 'banner_candidate')) '服务 manifest 保留了提取器私有字段.'

    $discoveryOutput = Join-Path $root 'discovery-banners'
    $discoveryManifestPath = Join-Path $root 'discovery-candidates.json'
    $discoveryResult = Export-CnActivityBannerCandidates `
        -CdnRoot $cdnRoot `
        -OutputDirectory $discoveryOutput `
        -CandidateManifestPath $discoveryManifestPath
    Assert-TestCondition ($discoveryResult.CandidateCount -eq 2) '资产发现模式未筛除非 PNG 和非横幅图片.'
    $discoveryManifest = Get-Content -LiteralPath $discoveryManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-TestCondition (-not (@($discoveryManifest.candidates.source_hash) -contains $nonPngHash)) '非 PNG 对象进入了候选清单.'

    $missingBannerHash = 'ffffffffffffffffffffffffffffffffffffffff'
    $missingSourceEntry = "production/upload/ff/$($missingBannerHash.Substring(2))"
    $missingBannerBytes = New-TestPngBytes -Width 1440 -Height 488
    $invalidSourcePath = Join-Path $root 'invalid-catalog-source.json'
    [IO.File]::WriteAllText(
        $invalidSourcePath,
        ([pscustomobject][ordered]@{
            format_version = 1
            activities = @([pscustomobject][ordered]@{
                activity_id = 'raid:3'
                name = '缺失'
                kind = 'raid'
                banner_candidate = $missingBannerHash
                banner_source_entry = $missingSourceEntry
                banner_source_version = '1.4.48'
                banner_source_byte_length = $missingBannerBytes.Length
                banner_source_digest = Get-TestCnEntityDigest -Bytes $missingBannerBytes
            })
        } | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $missingResult = Export-CnActivityBannerCandidates -CdnRoot $cdnRoot -CatalogSourcePath $invalidSourcePath
    Assert-TestCondition ($missingResult.MissingCandidateCount -eq 1) '缺失图片未进入提取结果统计.'
    Assert-TestCondition ($missingResult.CatalogActivityCount -eq 1) '缺图时未报告活动总数.'
    Assert-TestCondition ($missingResult.CatalogActivityWithoutImageCount -eq 1) '缺图时未报告无图活动数.'
    $missingManifest = Get-Content -LiteralPath $missingResult.CandidateManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-TestCondition ($missingManifest.missing_requested_candidates[0] -eq $missingBannerHash) '缺失图片未进入候选清单.'
    $catalogWithoutImage = Get-Content -LiteralPath $missingResult.CatalogOutputPath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-TestCondition ($catalogWithoutImage.activities.Count -eq 1) '缺失图片导致活动被删除.'
    Assert-TestCondition ($catalogWithoutImage.activities[0].image_candidates.Count -eq 0) '缺失图片被错误发布.'
    Assert-TestCondition (-not ($catalogWithoutImage.activities[0].PSObject.Properties.Name -contains 'banner_key')) '缺失图片生成了 legacy banner 字段.'
    Write-Output 'CN activity banner extractor tests passed.'
} finally {
    if (Test-Path -LiteralPath $root) {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
# //// /验证完整提取和活动映射流程 ////
