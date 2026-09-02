# audience: internal
# # stage-cn-activity-banner-fallbacks
# 此脚本校验区域回退资源, 写入客户端基础资源路径, 并同步管理页活动目录和横幅.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$CdnRoot,
    [Parameter(Mandatory)][string]$AppAssetRoot,
    [string]$ManifestPath = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')) 'assets\regional-fallbacks\manifest.json')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script:LowercasePngSignature = [byte[]](0x89, 0x70, 0x6e, 0x67, 0x0d, 0x0a, 0x1a, 0x0a)
$script:StandardPngSignature = [byte[]](0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)
$script:CnAssetHashSalt = 'K6R9T9Hz22OpeIGEWB0ui6c6PYFQnJGy'

# //// 判断字节数组是否具有指定前缀 [@x380kkm 2026-08-24] ////
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
# //// /判断字节数组是否具有指定前缀 ////

# //// 计算 CN 客户端使用的带盐资源哈希 [@x380kkm 2026-08-24] ////
function Get-CnAssetHash {
    param([Parameter(Mandatory)][string]$LogicalPath)

    $normalizedPath = $LogicalPath.Replace('\', '/').TrimStart('/')
    while ($normalizedPath.Contains('//')) {
        $normalizedPath = $normalizedPath.Replace('//', '/')
    }
    $algorithm = [Security.Cryptography.SHA1]::Create()
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes($normalizedPath + $script:CnAssetHashSalt)
        return ([BitConverter]::ToString($algorithm.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $algorithm.Dispose()
    }
}
# //// /计算 CN 客户端使用的带盐资源哈希 ////

# //// 读取 PNG IHDR 中的大端整数 [@x380kkm 2026-08-24] ////
function Read-BigEndianUInt32 {
    param(
        [Parameter(Mandatory)][byte[]]$Bytes,
        [Parameter(Mandatory)][int]$Offset
    )

    if ($Offset -lt 0 -or $Offset + 4 -gt $Bytes.Length) {
        throw 'PNG IHDR 超出资源范围.'
    }
    return [uint32](
        ([uint32]$Bytes[$Offset] -shl 24) -bor
        ([uint32]$Bytes[$Offset + 1] -shl 16) -bor
        ([uint32]$Bytes[$Offset + 2] -shl 8) -bor
        [uint32]$Bytes[$Offset + 3]
    )
}
# //// /读取 PNG IHDR 中的大端整数 ////

# //// 将客户端伪 PNG 签名转换为管理页标准 PNG [@x380kkm 2026-08-24] ////
function ConvertTo-ManagementPng {
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $normalized = [byte[]]$Bytes.Clone()
    if (Test-BytePrefix -Bytes $normalized -Prefix $script:LowercasePngSignature) {
        [Array]::Copy($script:StandardPngSignature, $normalized, $script:StandardPngSignature.Length)
    } elseif (-not (Test-BytePrefix -Bytes $normalized -Prefix $script:StandardPngSignature)) {
        throw '区域回退资源不是受支持的 PNG.'
    }
    if ($normalized.Length -lt 24 -or [Text.Encoding]::ASCII.GetString($normalized, 12, 4) -cne 'IHDR') {
        throw '区域回退资源缺少 PNG IHDR.'
    }
    return ,$normalized
}
# //// /将客户端伪 PNG 签名转换为管理页标准 PNG ////

# //// 原子写入活动目录 JSON [@x380kkm 2026-08-24] ////
function Write-AtomicJson {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Value
    )

    $temporaryPath = "$Path.$([Guid]::NewGuid().ToString('N')).tmp"
    $json = $Value | ConvertTo-Json -Depth 32
    [IO.File]::WriteAllText($temporaryPath, "$json`n", [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporaryPath -Destination $Path -Force
}
# //// /原子写入活动目录 JSON ////

# //// 写入区域活动横幅并同步对应目录项 [@x380kkm 2026-08-24] ////
function Install-ActivityBannerFallback {
    param(
        [Parameter(Mandatory)]$Definition,
        [Parameter(Mandatory)]$Catalog,
        [Parameter(Mandatory)][string]$ManifestDirectory,
        [Parameter(Mandatory)][string]$BannerDirectory,
        [Parameter(Mandatory)][string]$ResolvedAppAssetRoot
    )

    $assetPath = [IO.Path]::GetFullPath((Join-Path $ManifestDirectory ([string]$Definition.asset_file)))
    if (-not $assetPath.StartsWith($ManifestDirectory + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "区域回退资源超出 manifest 目录: $assetPath"
    }
    $bytes = [IO.File]::ReadAllBytes($assetPath)
    $sha256 = (Get-FileHash -LiteralPath $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($bytes.Length -ne [int64]$Definition.expected_size -or $sha256 -cne [string]$Definition.expected_sha256) {
        throw "区域回退资源摘要不匹配: $assetPath"
    }

    $logicalPath = [string]$Definition.logical_path
    $assetHash = Get-CnAssetHash -LogicalPath $logicalPath
    $expectedFileName = "$assetHash.png"
    if ([IO.Path]::GetFileName($assetPath) -cne $expectedFileName) {
        throw "区域回退资源文件名与 logical path 不匹配: $assetPath"
    }
    $normalized = ConvertTo-ManagementPng -Bytes $bytes
    $width = Read-BigEndianUInt32 -Bytes $normalized -Offset 16
    $height = Read-BigEndianUInt32 -Bytes $normalized -Offset 20
    if ($width -ne [uint32]$Definition.width -or $height -ne [uint32]$Definition.height) {
        throw "区域回退资源尺寸不匹配: ${width}x${height}"
    }

    New-Item -ItemType Directory -Path $BannerDirectory -Force | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $BannerDirectory $expectedFileName), $normalized)
    $bundlePath = Join-Path $ResolvedAppAssetRoot "production\bundle\$($assetHash.Substring(0, 2))\$($assetHash.Substring(2))"
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($bundlePath)) -Force | Out-Null
    [IO.File]::WriteAllBytes($bundlePath, $bytes)

    $activityIds = @($Definition.activity_ids | ForEach-Object { [string]$_ })
    $activities = @($Catalog.activities | Where-Object { $activityIds -contains [string]$_.activity_id })
    if ($activities.Count -ne $activityIds.Count) {
        throw "区域回退活动数量不匹配: logical_path=$logicalPath"
    }
    foreach ($activity in $activities) {
        $activity.tags = @($activity.tags | Where-Object { $_ -cne 'banner:unresolved' })
        $activity.description = ([string]$activity.description).Replace(' 当前包内未解析到对应纹理.', '')
        $candidate = [pscustomobject][ordered]@{
            key = $expectedFileName
            width = [int]$width
            height = [int]$height
            source_type = 'activity_banner'
            evidence = [string]$Definition.evidence
        }
        $activity.image_candidates = @($candidate) + @($activity.image_candidates | Where-Object {
            [string]$_.key -cne $expectedFileName
        })
        $activity | Add-Member -NotePropertyName banner_key -NotePropertyValue $expectedFileName -Force
        $activity | Add-Member -NotePropertyName banner_width -NotePropertyValue ([int]$width) -Force
        $activity | Add-Member -NotePropertyName banner_height -NotePropertyValue ([int]$height) -Force
    }

    return [pscustomobject][ordered]@{
        LogicalPath = $logicalPath
        SourceRegion = [string]$Definition.source_region
        ClientVersion = [string]$Definition.client_version
        ActivityCount = $activities.Count
        AssetHash = $assetHash
        Width = [int]$width
        Height = [int]$height
        GameAssetPath = $bundlePath
        ManagementAssetPath = Join-Path $BannerDirectory $expectedFileName
    }
}
# //// /写入区域活动横幅并同步对应目录项 ////

$resolvedCdnRoot = (Resolve-Path -LiteralPath $CdnRoot).Path
$resolvedAppAssetRoot = (Resolve-Path -LiteralPath $AppAssetRoot).Path
$resolvedManifestPath = (Resolve-Path -LiteralPath $ManifestPath).Path
$manifestDirectory = [IO.Path]::GetDirectoryName($resolvedManifestPath)
$catalogPath = Join-Path $resolvedCdnRoot 'activity-catalog.json'
$bannerDirectory = Join-Path $resolvedCdnRoot 'activity-banners'
$manifest = Get-Content -LiteralPath $resolvedManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$catalog = Get-Content -LiteralPath $catalogPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ([int]$manifest.format_version -ne 1 -or $null -eq $manifest.fallbacks) {
    throw '区域回退 manifest 结构无效.'
}

$results = @($manifest.fallbacks | ForEach-Object {
    Install-ActivityBannerFallback -Definition $_ -Catalog $catalog -ManifestDirectory $manifestDirectory `
        -BannerDirectory $bannerDirectory -ResolvedAppAssetRoot $resolvedAppAssetRoot
})
Write-AtomicJson -Path $catalogPath -Value $catalog

[pscustomobject][ordered]@{
    FallbackCount = $results.Count
    CatalogPath = $catalogPath
    Results = $results
}
