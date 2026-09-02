# audience: external
# # package-ios-cn-personal-service
# 该入口固定对 CN iOS 客户端应用本地个人服务端点改写, 并将语音与卡池资源接入同一版本链.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InputIpa,
    [Parameter(Mandatory)][string]$Framework,
    [Parameter(Mandatory)][string]$OutputIpa,
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$')][string]$BundleId,
    [Parameter(Mandatory)][string]$DisplayName,
    [Parameter(Mandatory)][string]$CnCdnBundle,
    [string]$Report
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$expectedEndpointNames = @(
    'api_server',
    'primary_version',
    'backup_version',
    'sdk_urls',
    'observed_third_party_urls'
)
$expectedCnVoiceRoleCount = 17
$expectedCnVoiceEntryCount = 325
# //// 严格验证 CN AOT 端点目标 [@x380kkm 2026-08-18] ////
function Test-CnLoopbackEndpoint {
    [CmdletBinding()]
    param([Parameter(Mandatory)][pscustomobject]$Replacement)

    $endpoint = [string]$Replacement.endpoint
    $target = [string]$Replacement.target
    if ($endpoint -eq 'sdk_urls') {
        return $target -eq 'http://<optional-padding-and-userinfo>127.0.0.1:17171' -and
            [int]$Replacement.count -eq 148 -and
            [string]$Replacement.source_mode -in @('original_authorities', 'emulator_proxy')
    }
    if ($endpoint -eq 'observed_third_party_urls') {
        $authorityCounts = $Replacement.authority_counts
        return $target -eq 'http://<optional-padding-and-userinfo>127.1:17171' -and
            [int]$Replacement.count -eq 4 -and
            [string]$Replacement.source_mode -in @('original_authorities', 'port_one_loopback') -and
            [int]$authorityCounts.'api.sobot.com' -eq 1 -and
            [int]$authorityCounts.'img.sobot.com' -eq 2 -and
            [int]$authorityCounts.'www.sobot.com' -eq 1
    }

    try {
        $uri = [Uri]$target
    } catch {
        return $false
    }

    $expectedPath = switch ($endpoint) {
        'api_server' { '/' }
        'primary_version' { '/shijtswy/version/' }
        'backup_version' { '/shijtswy/version/' }
        default { return $false }
    }
    return $uri.Scheme -eq 'http' -and
        $uri.Host -eq '127.0.0.1' -and
        $uri.Port -eq 17171 -and
        $uri.AbsolutePath -eq $expectedPath -and
        [string]::IsNullOrEmpty($uri.Query) -and
        [string]::IsNullOrEmpty($uri.Fragment) -and
        $uri.UserInfo -match '^0*$'
}
# //// /严格验证 CN AOT 端点目标 ////

# //// 读取活动目录中的可选字段 ////
function Get-OptionalPropertyValue {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][AllowNull()][object]$InputObject,
        [Parameter(Mandatory)][string]$Name
    )

    if ($null -eq $InputObject) {
        return $null
    }
    $property = $InputObject.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    $property.Value
}
# //// /读取活动目录中的可选字段 ////

# //// 从兼容补丁模块读取唯一补丁清单 ////
function Get-CnCompatibilityPatchNames {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ModuleDirectory)

    $program = @'
import json
import sys

sys.path.insert(0, sys.argv[1])
from ios_cn_compatibility_patch import CN_1_8_4_COMPATIBILITY_PATCHES

print(json.dumps([patch.name for patch in CN_1_8_4_COMPATIBILITY_PATCHES]))
'@
    $output = @(& uv run --python 3.12 python -c $program $ModuleDirectory 2>&1)
    if ($LASTEXITCODE -ne 0) {
        $errorTail = ($output | Select-Object -Last 20) -join [Environment]::NewLine
        throw "Failed to load the CN compatibility patch catalog.`n$errorTail"
    }
    try {
        @((($output -join [Environment]::NewLine) | ConvertFrom-Json))
    } catch {
        throw 'CN compatibility patch catalog is not valid JSON.'
    }
}
# //// /从兼容补丁模块读取唯一补丁清单 ////

# //// 验证管理页与游戏美术资源随 CN CDN 打包 [@x380kkm 2026-08-21] ////
function Assert-CnArtworkBundle {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$CnCdnBundlePath,
        [Parameter(Mandatory)][string]$EmbeddedIconDirectory
    )

    $catalogPath = Join-Path $CnCdnBundlePath 'activity-catalog.json'
    if (-not (Test-Path -LiteralPath $catalogPath -PathType Leaf)) {
        throw 'CnCdnBundle must contain activity-catalog.json.'
    }
    $catalog = Get-Content -LiteralPath $catalogPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $imageKeys = @(
        foreach ($activity in @($catalog.activities)) {
            $bannerKey = [string](Get-OptionalPropertyValue -InputObject $activity -Name 'banner_key')
            if (-not [string]::IsNullOrWhiteSpace($bannerKey)) {
                $bannerKey
            }
            $imageCandidates = @(Get-OptionalPropertyValue -InputObject $activity -Name 'image_candidates')
            foreach ($candidate in $imageCandidates) {
                $candidateKey = [string](Get-OptionalPropertyValue -InputObject $candidate -Name 'key')
                if (-not [string]::IsNullOrWhiteSpace($candidateKey)) {
                    $candidateKey
                }
            }
        }
    ) | Sort-Object -Unique
    if ($imageKeys.Count -eq 0) {
        throw 'CN activity catalog must reference at least one banner image.'
    }
    $bannerRoot = Join-Path $CnCdnBundlePath 'activity-banners'
    $missingBanners = @($imageKeys | Where-Object {
        -not (Test-Path -LiteralPath (Join-Path $bannerRoot $_) -PathType Leaf)
    })
    if ($missingBanners.Count -gt 0) {
        throw "CnCdnBundle is missing $($missingBanners.Count) activity banner files."
    }

    $gachaWithoutBanners = @(
        foreach ($activity in @($catalog.activities)) {
            $kind = [string](Get-OptionalPropertyValue -InputObject $activity -Name 'kind')
            if ($kind -cne 'gacha') {
                continue
            }
            $bannerKey = [string](Get-OptionalPropertyValue -InputObject $activity -Name 'banner_key')
            $candidateKeys = @(
                foreach ($candidate in @(Get-OptionalPropertyValue -InputObject $activity -Name 'image_candidates')) {
                    [string](Get-OptionalPropertyValue -InputObject $candidate -Name 'key')
                }
            )
            if ($bannerKey -notmatch '^[0-9a-f]{40}\.png$' -or $bannerKey -notin $candidateKeys) {
                [string](Get-OptionalPropertyValue -InputObject $activity -Name 'activity_id')
            }
        }
    )
    if ($gachaWithoutBanners.Count -gt 0) {
        throw "CnCdnBundle contains $($gachaWithoutBanners.Count) gacha activities without banner assets."
    }

    $generatedGachaKeys = @(
        foreach ($activity in @($catalog.activities)) {
            $kind = [string](Get-OptionalPropertyValue -InputObject $activity -Name 'kind')
            $tags = @(Get-OptionalPropertyValue -InputObject $activity -Name 'tags')
            if ($kind -cne 'gacha' -or 'banner:generated' -notin $tags) {
                continue
            }
            $bannerKey = [string](Get-OptionalPropertyValue -InputObject $activity -Name 'banner_key')
            if ($bannerKey -notmatch '^[0-9a-f]{40}\.png$') {
                throw 'Generated CN gacha activity is missing a valid banner_key.'
            }
            $bannerKey
        }
    ) | Sort-Object -Unique
    $gameBannerRoot = Join-Path $CnCdnBundlePath 'production\bundle'
    $missingGameBanners = @($generatedGachaKeys | Where-Object {
        $assetHash = [IO.Path]::GetFileNameWithoutExtension($_)
        $gamePath = Join-Path $gameBannerRoot "$($assetHash.Substring(0, 2))\$($assetHash.Substring(2))"
        -not (Test-Path -LiteralPath $gamePath -PathType Leaf)
    })
    if ($missingGameBanners.Count -gt 0) {
        throw "CnCdnBundle is missing $($missingGameBanners.Count) generated game banner files."
    }

    $requiredIconNames = @(
        Get-ChildItem -LiteralPath $EmbeddedIconDirectory -File -Filter '*.png' |
            ForEach-Object { $_.Name }
    )
    if ($requiredIconNames.Count -eq 0) {
        throw 'Embedded management item icons are missing.'
    }
    $itemIconRoot = Join-Path $CnCdnBundlePath 'management-assets\item-icons'
    $missingIcons = @($requiredIconNames | Where-Object {
        -not (Test-Path -LiteralPath (Join-Path $itemIconRoot $_) -PathType Leaf)
    })
    if ($missingIcons.Count -gt 0) {
        throw "CnCdnBundle is missing $($missingIcons.Count) management item icons."
    }
}
# //// /验证管理页与游戏美术资源随 CN CDN 打包 ////

# //// 从工作区定位活动目录源文件 [@x380kkm 2026-08-28] ////
function Find-CnRichActivityCatalog {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$CnCdnBundlePath
    )

    $candidates = @(
        (Join-Path $RepositoryRoot 'assets\cn-activity-catalog-source.json'),
        (Join-Path $CnCdnBundlePath 'activity-catalog-source.json'),
        (Join-Path $CnCdnBundlePath 'activity-catalog-rich.json')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    return $null
}
# //// /从工作区定位活动目录源文件 ////

# //// 定位共享构建资源所在工作区 [@x380kkm 2026-09-01] ////
function Resolve-CnPackagingWorkspaceRoot {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$RepositoryRoot)

    $gitCommonDirectory = @(& git -C $RepositoryRoot rev-parse --git-common-dir 2>$null | Select-Object -First 1)
    if ($LASTEXITCODE -eq 0 -and $gitCommonDirectory.Count -eq 1 -and
        -not [string]::IsNullOrWhiteSpace([string]$gitCommonDirectory[0])) {
        $commonDirectory = [string]$gitCommonDirectory[0]
        if (-not [IO.Path]::IsPathRooted($commonDirectory)) {
            $commonDirectory = Join-Path $RepositoryRoot $commonDirectory
        }
        $commonDirectory = [IO.Path]::GetFullPath($commonDirectory)
        if ((Split-Path -Leaf $commonDirectory) -eq '.git') {
            return Split-Path -Parent (Split-Path -Parent $commonDirectory)
        }
    }

    return Split-Path -Parent $RepositoryRoot
}
# //// /定位共享构建资源所在工作区 ////

# //// 从工作区定位启动静态资源覆盖目录 [@x380kkm 2026-08-30] ////
function Find-CnStartupStaticOverlay {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$RepositoryRoot)

    $workspaceRoot = Resolve-CnPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
    $candidates = @(
        (Join-Path $workspaceRoot 'artifacts\ios-real-app-probe\startup-overlay'),
        (Join-Path $workspaceRoot 'starpoint\artifacts\ios-real-app-probe\startup-overlay')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Container) {
            $fileCount = @(Get-ChildItem -LiteralPath $candidate -Recurse -File).Count
            if ($fileCount -eq 46) {
                return (Resolve-Path -LiteralPath $candidate).Path
            }
        }
    }
    return $null
}
# //// /从工作区定位启动静态资源覆盖目录 ////

# //// 安装启动链所需静态资源 [@x380kkm 2026-08-30] ////
function Initialize-CnStartupStaticAssets {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$CnCdnBundlePath
    )

    $sourceRoot = Find-CnStartupStaticOverlay -RepositoryRoot $RepositoryRoot
    if ([string]::IsNullOrWhiteSpace($sourceRoot)) {
        throw 'CN startup static asset overlay is required for client packaging.'
    }
    $files = @(Get-ChildItem -LiteralPath $sourceRoot -Recurse -File)
    if ($files.Count -ne 46) {
        throw "CN startup static asset overlay must contain 46 files, actual=$($files.Count)."
    }
    $copiedCount = 0
    foreach ($file in $files) {
        $relativePath = [IO.Path]::GetRelativePath($sourceRoot, $file.FullName)
        $segments = @($relativePath.Replace('\', '/').Split('/', [StringSplitOptions]::RemoveEmptyEntries))
        if ([IO.Path]::IsPathRooted($relativePath) -or $segments -contains '..' -or
            $segments.Count -eq 0) {
            throw "CN startup static asset path is unsafe: $relativePath"
        }
        $destination = Join-Path $CnCdnBundlePath ($relativePath.Replace('/', [IO.Path]::DirectorySeparatorChar))
        $destinationParent = Split-Path -Parent $destination
        New-Item -ItemType Directory -Force -Path $destinationParent | Out-Null
        if (Test-Path -LiteralPath $destination -PathType Leaf) {
            $sourceDigest = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
            $destinationDigest = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash
            if ($sourceDigest -cne $destinationDigest) {
                throw "CN startup static asset conflicts with existing CDN file: $relativePath"
            }
            continue
        }
        Copy-Item -LiteralPath $file.FullName -Destination $destination
        $copiedCount++
    }
    [pscustomobject][ordered]@{
        source = $sourceRoot
        file_count = $files.Count
        copied_count = $copiedCount
    }
}
# //// /安装启动链所需静态资源 ////

# //// 初始化客户端活动目录 [@x380kkm 2026-08-30] ////
function Initialize-CnActivityCatalog {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$CnCdnBundlePath
    )

    $sourcePath = Find-CnRichActivityCatalog `
        -RepositoryRoot $RepositoryRoot `
        -CnCdnBundlePath $CnCdnBundlePath
    if ([string]::IsNullOrWhiteSpace($sourcePath)) {
        throw 'CN activity catalog source is required for client projection.'
    }
    $source = Get-Content -LiteralPath $sourcePath -Raw -Encoding UTF8 | ConvertFrom-Json
    $activities = @($source.activities)
    $gachaActivities = @($activities | Where-Object { [string]$_.kind -ceq 'gacha' })
    $dailyActivities = @($activities | Where-Object {
        [string]$_.activity_id -match '^(daily-exp-mana|daily-week):'
    })
    if ($activities.Count -ne 922 -or $gachaActivities.Count -ne 483 -or
        $dailyActivities.Count -ne 20) {
        throw 'CN activity catalog source does not contain the complete visible activity set.'
    }
    $catalogPath = Join-Path $CnCdnBundlePath 'activity-catalog.json'
    Copy-Item -LiteralPath $sourcePath -Destination $catalogPath -Force
    [pscustomobject][ordered]@{
        source = $sourcePath
        output = $catalogPath
        activity_count = $activities.Count
        gacha_activity_count = $gachaActivities.Count
        daily_activity_count = $dailyActivities.Count
    }
}
# //// /初始化客户端活动目录 ////

# //// 将富目录引用补入当前 EntityLists 清单 [@x380kkm 2026-08-28] ////
function Ensure-CnEntityRecords {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$CnCdnBundlePath,
        [AllowNull()][string]$RichCatalogPath
    )

    if ([string]::IsNullOrWhiteSpace($RichCatalogPath) -or
        -not (Test-Path -LiteralPath $RichCatalogPath -PathType Leaf)) {
        return 0
    }
    $catalog = Get-Content -LiteralPath $RichCatalogPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $required = @{}
    foreach ($activity in @($catalog.activities)) {
        if ([string](Get-OptionalPropertyValue -InputObject $activity -Name 'kind') -ceq 'gacha') {
            continue
        }
        foreach ($candidate in @(Get-OptionalPropertyValue -InputObject $activity -Name 'image_candidates')) {
            $entry = [string](Get-OptionalPropertyValue -InputObject $candidate -Name 'source_entry')
            $hash = [string](Get-OptionalPropertyValue -InputObject $candidate -Name 'source_hash')
            if ([string]::IsNullOrWhiteSpace($entry)) { continue }
            if ($entry -notmatch '^production/(?:(?:android|ios|medium)_)?upload/[a-f0-9]{2}/[a-f0-9]{38}$' -or
                $hash -notmatch '^[a-f0-9]{40}$') {
                throw "CN rich activity catalog contains an invalid EntityLists reference: $entry"
            }
            $entryHash = $entry -replace '^production/(?:(?:android|ios|medium)_)?upload/','' -replace '/',''
            if ($entryHash -cne $hash) {
                throw "CN rich activity catalog hash does not match EntityLists reference: $entry"
            }
            $version = [string](Get-OptionalPropertyValue -InputObject $candidate -Name 'source_version')
            $length = Get-OptionalPropertyValue -InputObject $candidate -Name 'source_byte_length'
            $digest = [string](Get-OptionalPropertyValue -InputObject $candidate -Name 'source_digest')
            $assetKind = [string](Get-OptionalPropertyValue -InputObject $candidate -Name 'source_asset_kind')
            if ([string]::IsNullOrWhiteSpace($version) -or $null -eq $length -or
                [long]$length -lt 24 -or $digest -notmatch '^[A-Za-z0-9_-]{43}$') {
                throw "CN rich activity catalog contains incomplete EntityLists metadata: $entry"
            }
            if ([string]::IsNullOrWhiteSpace($assetKind)) {
                $assetKind = if ($entry -like 'production/medium_upload/*') { 'medium' } else { 'common' }
            }
            $record = [pscustomobject]@{
                entry = $entry
                version = $version
                length = [long]$length
                digest = $digest
                asset_kind = $assetKind
            }
            $existingRecord = $required[$entry]
            if ($null -ne $existingRecord -and
                ($existingRecord.version -cne $record.version -or
                 $existingRecord.length -ne $record.length -or
                 $existingRecord.digest -cne $record.digest -or
                 $existingRecord.asset_kind -cne $record.asset_kind)) {
                throw "CN rich activity catalog contains conflicting metadata: $entry"
            }
            $required[$entry] = $record
        }
    }
    if ($required.Count -eq 0) { return 0 }

    $manifestPaths = @(
        (Join-Path $CnCdnBundlePath 'entities\PathFile.csv')
    ) + @(
        Get-ChildItem -LiteralPath (Join-Path $CnCdnBundlePath 'entities') -File -Filter '*-ios_medium.csv' |
            ForEach-Object { $_.FullName }
    )
    $addedCount = 0
    foreach ($manifestPath in ($manifestPaths | Sort-Object -Unique)) {
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
            throw "CN EntityLists manifest is missing: $manifestPath"
        }
        $existing = @{}
        foreach ($line in Get-Content -LiteralPath $manifestPath -Encoding UTF8) {
            $fields = $line.Split(',')
            if ($fields.Count -eq 5) { $existing[$fields[0]] = $fields }
        }
        $linesToAppend = [Collections.Generic.List[string]]::new()
        foreach ($record in $required.Values) {
            $old = $existing[$record.entry]
            if ($null -ne $old) {
                if ($old[1] -cne $record.version -or [long]$old[2] -ne $record.length -or
                    $old[3] -cne $record.digest -or $old[4] -cne $record.asset_kind) {
                    throw "CN EntityLists contains conflicting metadata: $($record.entry)"
                }
                continue
            }
            $linesToAppend.Add("$($record.entry),$($record.version),$($record.length),$($record.digest),$($record.asset_kind)")
        }
        if ($linesToAppend.Count -gt 0) {
            $prefix = if ((Get-Item -LiteralPath $manifestPath).Length -gt 0) { "`n" } else { '' }
            [IO.File]::AppendAllText(
                $manifestPath,
                $prefix + ($linesToAppend -join "`n") + "`n",
                [Text.UTF8Encoding]::new($false)
            )
            $addedCount += $linesToAppend.Count
        }
    }
    return $addedCount
}
# //// /将富目录引用补入当前 EntityLists 清单 ////

# //// 校验每日活动候选在目录和 EntityLists 中可达 [@x380kkm 2026-08-28] ////
function Assert-CnDailyActivityCoverage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$CnCdnBundlePath,
        [AllowNull()][string]$RichCatalogPath
    )

    $catalogPath = Join-Path $CnCdnBundlePath 'activity-catalog.json'
    $catalog = Get-Content -LiteralPath $catalogPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $activities = @($catalog.activities)
    $gacha = @($activities | Where-Object { [string]$_.kind -ceq 'gacha' })
    if ($activities.Count -ne 922 -or $gacha.Count -ne 483) {
        throw "CN activity catalog must retain 922 activities and 483 gacha entries, activities=$($activities.Count) gacha=$($gacha.Count)."
    }
    if ([string]::IsNullOrWhiteSpace($RichCatalogPath)) {
        return [pscustomobject]@{
            activity_count = $activities.Count
            gacha_activity_count = $gacha.Count
            daily_activity_count = $null
            daily_candidate_count = $null
            unique_candidate_count = $null
        }
    }
    $richCatalog = Get-Content -LiteralPath $RichCatalogPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $richDaily = @($richCatalog.activities | Where-Object {
        [string]$_.activity_id -match '^(daily-exp-mana|daily-week):'
    })
    $daily = @($catalog.activities | Where-Object {
        [string]$_.activity_id -match '^(daily-exp-mana|daily-week):'
    })
    if ($richDaily.Count -ne 20 -or $daily.Count -ne $richDaily.Count) {
        throw "CN daily activity catalog must retain 20 entries, rich=$($richDaily.Count) output=$($daily.Count)."
    }

    $outputById = @{}
    foreach ($activity in $daily) { $outputById[[string]$activity.activity_id] = $activity }
    $candidateRefs = @()
    foreach ($activity in $richDaily) {
        $outputActivity = $outputById[[string]$activity.activity_id]
        if ($null -eq $outputActivity) {
            throw "CN daily activity is missing from output catalog: $($activity.activity_id)."
        }
        $outputKeys = @($outputActivity.image_candidates | ForEach-Object { [string]$_.key })
        foreach ($candidate in @($activity.image_candidates)) {
            $hash = [string]$candidate.source_hash
            $entry = [string]$candidate.source_entry
            if ($hash -notmatch '^[a-f0-9]{40}$' -or $entry -notmatch '^production/(?:(?:android|ios|medium)_)?upload/[a-f0-9]{2}/[a-f0-9]{38}$') {
                throw "CN daily activity contains invalid image identity: $($activity.activity_id)."
            }
            $key = "$hash.png"
            if ($key -notin $outputKeys) {
                throw "CN daily image candidate is missing from output catalog: $($activity.activity_id) $key."
            }
            if (-not (Test-Path -LiteralPath (Join-Path $CnCdnBundlePath "activity-banners\$key") -PathType Leaf)) {
                throw "CN daily image candidate file is missing: $key."
            }
            $candidateRefs += [pscustomobject]@{ hash = $hash; source_entry = $entry; key = $key }
        }
    }

    $manifestPaths = @(
        (Join-Path $CnCdnBundlePath 'entities\PathFile.csv')
    ) + @(
        Get-ChildItem -LiteralPath (Join-Path $CnCdnBundlePath 'entities') -File -Filter '*-ios_medium.csv' |
            ForEach-Object { $_.FullName }
    )
    foreach ($manifestPath in ($manifestPaths | Sort-Object -Unique)) {
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
            throw "CN EntityLists manifest is missing: $manifestPath."
        }
        $entries = @{}
        foreach ($line in Get-Content -LiteralPath $manifestPath -Encoding UTF8) {
            $fields = $line.Split(',')
            if ($fields.Count -eq 5) { $entries[$fields[0]] = $fields }
        }
        $missing = @($candidateRefs | Where-Object { $_.source_entry -notin $entries.Keys })
        if ($missing.Count -gt 0) {
            throw "CN EntityLists lacks $($missing.Count) daily image records: $($missing[0].source_entry)."
        }
    }
    return [pscustomobject]@{
        activity_count = $activities.Count
        gacha_activity_count = $gacha.Count
        daily_activity_count = $daily.Count
        daily_candidate_count = $candidateRefs.Count
        unique_candidate_count = @($candidateRefs.hash | Sort-Object -Unique).Count
    }
}
# //// /校验每日活动候选在目录和 EntityLists 中可达 ////

# //// 读取活动目录资源缺口 [@x380kkm 2026-08-28] ////
function Get-CnCatalogResourceGaps {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [object[]]$ResourceGaps
    )

    @($ResourceGaps | Where-Object {
        [string]$_.kind -match '^(?:catalog_|missing_catalog_|invalid_catalog_)'
    })
}
# //// /读取活动目录资源缺口 ////

# //// 定位 CN 卡池差分源归档 [@x380kkm 2026-08-30] ////
function Resolve-CnGachaSourceArchive {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$WorkspaceRoot,
        [Parameter(Mandatory)][string]$RelativePath
    )

    $sourceRoot = Join-Path $WorkspaceRoot 'starpoint\.cdn\bundles\ios-1.4.54-initial'
    $candidate = Join-Path $sourceRoot $RelativePath
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        return (Resolve-Path -LiteralPath $candidate).Path
    }
    throw "CN clean gacha source archive is missing: $candidate"
}
# //// /定位 CN 卡池差分源归档 ////

# //// 递增客户端资源版本 [@x380kkm 2026-08-30] ////
function Get-CnNextAssetVersion {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Version)

    $match = [regex]::Match($Version, '^(?<prefix>\d+\.\d+)\.(?<patch>\d+)$')
    if (-not $match.Success) {
        throw "CN asset version is invalid: $Version"
    }
    return "$($match.Groups['prefix'].Value).$([int]$match.Groups['patch'].Value + 1)"
}
# //// /递增客户端资源版本 ////

# //// 构建 CN 卡池地区与临时入口差分 [@x380kkm 2026-08-30] ////
function Update-CnGachaMasterDiff {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$CnCdnBundlePath
    )

    $workspaceRoot = Resolve-CnPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
    $builder = Join-Path $RepositoryRoot 'scripts\protocol-lab\build-cn-gacha-cdn-diff.ps1'
    $sourceArchive = Resolve-CnGachaSourceArchive `
        -WorkspaceRoot $workspaceRoot `
        -RelativePath 'archive-common-diff\pinball-1.4.47-1.4.48-1-74436299.zip'
    $campaignSourceArchive = Resolve-CnGachaSourceArchive `
        -WorkspaceRoot $workspaceRoot `
        -RelativePath 'archive-common-diff\pinball-1.4.49-1.4.50-1-2f8c317d.zip'
    $configSourceArchive = Resolve-CnGachaSourceArchive `
        -WorkspaceRoot $workspaceRoot `
        -RelativePath 'archive-common-full\pinball-1.4.0-194-f028ba68.zip'
    if (-not (Test-Path -LiteralPath $builder -PathType Leaf)) {
        throw "CN gacha diff builder is missing: $builder"
    }

    $entityRoot = Join-Path $CnCdnBundlePath 'entities'
    $pathManifest = Join-Path $CnCdnBundlePath 'path'
    $entityManifestPaths = @(
        (Join-Path $entityRoot 'PathFile.csv'),
        (Join-Path $entityRoot '10939-ios_medium.csv'),
        (Join-Path $entityRoot '10939-android_medium.csv')
    )
    if (-not (Test-Path -LiteralPath $pathManifest -PathType Leaf) -or
        @($entityManifestPaths | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) }).Count -ne 0) {
        throw 'CN gacha diff input is missing path or EntityLists manifests.'
    }

    $pathDocument = Get-Content -LiteralPath $pathManifest -Raw -Encoding UTF8 | ConvertFrom-Json
    $originalVersion = [string]$pathDocument.info.target_asset_version
    $targetVersion = Get-CnNextAssetVersion -Version $originalVersion
    $outputArchive = Join-Path $CnCdnBundlePath "archive-common-diff\starpoint-gacha-$originalVersion-$targetVersion.zip"
    $baseLocation = $null
    foreach ($group in @($pathDocument.diff)) {
        foreach ($archive in @($group.archive)) {
            $location = [string]$archive.location
            if ([string]::IsNullOrWhiteSpace($location)) {
                continue
            }
            $normalizedLocation = $location.Replace('\', '/')
            $marker = $normalizedLocation.IndexOf('/archive-', [StringComparison]::OrdinalIgnoreCase)
            if ($marker -ge 0) {
                $baseLocation = $normalizedLocation.Substring(0, $marker)
                break
            }
        }
        if ($null -ne $baseLocation) {
            break
        }
    }
    if ([string]::IsNullOrWhiteSpace($baseLocation)) {
        throw 'CN gacha path manifest has no archive location prefix.'
    }
    $archiveName = "starpoint-gacha-$originalVersion-$targetVersion.zip"
    $arguments = @{
        SourceArchivePath = $sourceArchive
        CampaignSourceArchivePath = $campaignSourceArchive
        ConfigSourceArchivePath = $configSourceArchive
        OutputArchivePath = $outputArchive
        PathManifestPath = $pathManifest
        EntityManifestPath = $entityManifestPaths
        ArchiveLocation = "$baseLocation/archive-common-diff/$archiveName"
        OriginalVersion = $originalVersion
        TargetVersion = $targetVersion
    }
    $output = @(& $builder @arguments 2>&1)
    if ($LASTEXITCODE -ne 0) {
        $errorTail = ($output | Select-Object -Last 20) -join [Environment]::NewLine
        throw "CN gacha master diff failed.`n$errorTail"
    }
    if (-not (Test-Path -LiteralPath $outputArchive -PathType Leaf)) {
        throw 'CN gacha master diff did not produce an archive.'
    }
    $result = @($output | Where-Object {
        $null -ne $_.PSObject.Properties['Version'] -and
        $null -ne $_.PSObject.Properties['GachaCount']
    } | Select-Object -Last 1)
    if ($result.Count -ne 1) {
        throw 'CN gacha master diff did not return a structured report.'
    }
    $report = $result[0]
    if ([string]$report.Version -ne $targetVersion -or
        [int]$report.RetainedOriginalCount -ne 584 -or
        [int]$report.TemporaryAliasCount -ne 473 -or
        [int]$report.GachaCount -ne 1057 -or
        [int]$report.RetainedOriginalCampaignCount -ne 41 -or
        [int]$report.TemporaryCampaignAliasCount -ne 123 -or
        [int]$report.GachaCampaignCount -ne 164 -or
        [int]$report.ProjectedFeatureLinkCount -ne 11 -or
        $report.Zip64 -ne $false) {
        throw 'CN gacha master diff report does not match the complete CN pool projection.'
    }
    [pscustomobject][ordered]@{
        original_version = $originalVersion
        version = [string]$report.Version
        archive = $outputArchive
        archive_bytes = [int64](Get-Item -LiteralPath $outputArchive).Length
        zip64 = [bool]$report.Zip64
        retained_original_count = [int]$report.RetainedOriginalCount
        excluded_regional_alias_count = [int]$report.ExcludedRegionalAliasCount
        normalized_coverage_alias_count = [int]$report.NormalizedCoverageAliasCount
        temporary_alias_count = [int]$report.TemporaryAliasCount
        gacha_count = [int]$report.GachaCount
        retained_original_campaign_count = [int]$report.RetainedOriginalCampaignCount
        temporary_campaign_alias_count = [int]$report.TemporaryCampaignAliasCount
        gacha_campaign_count = [int]$report.GachaCampaignCount
        projected_feature_link_count = [int]$report.ProjectedFeatureLinkCount
        source_mode = 'ios-1.4.54-initial'
    }
}
# //// /构建 CN 卡池地区与临时入口差分 ////

# //// 定位 CN 语音覆盖使用的 master 文件 [@x380kkm 2026-08-29] ////
function Resolve-CnVoiceMasterPath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][string]$LogicalPath
    )

    $relativePath = $LogicalPath.Replace('/', [IO.Path]::DirectorySeparatorChar)
    $directCandidates = @(
        (Join-Path $Root $relativePath),
        (Join-Path $Root ([IO.Path]::GetFileName($LogicalPath)))
    )
    foreach ($candidate in $directCandidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    $matches = @(
        Get-ChildItem -LiteralPath $Root -Recurse -File -Filter ([IO.Path]::GetFileName($LogicalPath)) |
            ForEach-Object { $_.FullName }
    )
    if ($matches.Count -ne 1) {
        throw "CN voice master source is ambiguous: $LogicalPath root=$Root"
    }
    return (Resolve-Path -LiteralPath $matches[0]).Path
}
# //// /定位 CN 语音覆盖使用的 master 文件 ////

# //// 生成 CN 语音源内容指纹 [@x380kkm 2026-08-29] ////
function Get-CnVoiceOverlaySourceFingerprint {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$RepositoryRoot)

    $workspaceRoot = Resolve-CnPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
    $cnMasterRoot = Join-Path $workspaceRoot 'archive'
    $jpMasterRoot = Join-Path $workspaceRoot 'archive\jp-voice-masters'
    $masterSpecs = @(
        [pscustomobject]@{ source_kind = 'cn_master'; root = $cnMasterRoot; logical_path = 'master/character/character.orderedmap' },
        [pscustomobject]@{ source_kind = 'cn_master'; root = $cnMasterRoot; logical_path = 'master/character/character_speech.orderedmap' },
        [pscustomobject]@{ source_kind = 'cn_master'; root = $cnMasterRoot; logical_path = 'master/character/character_text.orderedmap' },
        [pscustomobject]@{ source_kind = 'cn_master'; root = $cnMasterRoot; logical_path = 'master/string/ui_string.orderedmap' },
        [pscustomobject]@{ source_kind = 'cn_master'; root = $cnMasterRoot; logical_path = 'master/asset/voice_asset.orderedmap' },
        [pscustomobject]@{ source_kind = 'jp_master'; root = $jpMasterRoot; logical_path = 'master/character/character_text.orderedmap' }
    )
    $scriptSpecs = @(
        [pscustomobject]@{ source_kind = 'build_script'; path = Join-Path $RepositoryRoot 'scripts\protocol-lab\build-cn-voice-overlay.py' },
        [pscustomobject]@{ source_kind = 'archive_script'; path = Join-Path $RepositoryRoot 'scripts\protocol-lab\cn_voice_overlay_archive.py' },
        [pscustomobject]@{ source_kind = 'range_reader'; path = Join-Path $RepositoryRoot 'scripts\protocol-lab\regional_zip_range.py' }
    )
    $records = [System.Collections.Generic.List[object]]::new()
    foreach ($spec in $masterSpecs) {
        $path = Resolve-CnVoiceMasterPath -Root $spec.root -LogicalPath $spec.logical_path
        $item = Get-Item -LiteralPath $path
        [void]$records.Add([pscustomobject]@{
                source_kind = $spec.source_kind
                logical_path = $spec.logical_path
                relative_path = [IO.Path]::GetRelativePath($workspaceRoot, $path).Replace('\', '/')
                byte_length = [int64]$item.Length
                sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
            })
    }
    foreach ($spec in $scriptSpecs) {
        $path = (Resolve-Path -LiteralPath $spec.path).Path
        $item = Get-Item -LiteralPath $path
        [void]$records.Add([pscustomobject]@{
                source_kind = $spec.source_kind
                logical_path = ''
                relative_path = [IO.Path]::GetRelativePath($workspaceRoot, $path).Replace('\', '/')
                byte_length = [int64]$item.Length
                sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
            })
    }

    $orderedRecords = @($records | Sort-Object source_kind, logical_path, relative_path)
    $canonical = ConvertTo-Json -InputObject $orderedRecords -Compress -Depth 4
    $hasher = [Security.Cryptography.SHA256]::Create()
    try {
        $digest = [Convert]::ToHexString(
            $hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($canonical))
        ).ToLowerInvariant()
    } finally {
        $hasher.Dispose()
    }
    [pscustomobject]@{
        schema = 1
        algorithm = 'sha256'
        digest = $digest
        files = $orderedRecords
    }
}
# //// /生成 CN 语音源内容指纹 ////

# //// 读取并验证可复用的 CN 语音报告 [@x380kkm 2026-08-29] ////
function Read-CnVoiceOverlayReportIfCurrent {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ReportPath,
        [Parameter(Mandatory)][pscustomobject]$SourceFingerprint
    )

    if (-not (Test-Path -LiteralPath $ReportPath -PathType Leaf)) {
        return $null
    }
    try {
        $report = Get-Content -LiteralPath $ReportPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $reportFingerprint = Get-OptionalPropertyValue -InputObject $report -Name 'source_fingerprint'
        $archive = Get-OptionalPropertyValue -InputObject $report -Name 'archive'
        $masters = @(Get-OptionalPropertyValue -InputObject $report -Name 'masters')
        $assets = @(Get-OptionalPropertyValue -InputObject $report -Name 'assets')
        $roles = @(Get-OptionalPropertyValue -InputObject $report -Name 'roles')
        $invalidRoles = @($roles | Where-Object {
            @(Get-OptionalPropertyValue -InputObject $_ -Name 'missing_paths').Count -ne 0
        })
        if ($null -eq $reportFingerprint -or
            [int](Get-OptionalPropertyValue -InputObject $reportFingerprint -Name 'schema') -ne 1 -or
            [string](Get-OptionalPropertyValue -InputObject $reportFingerprint -Name 'algorithm') -ne 'sha256' -or
            [string](Get-OptionalPropertyValue -InputObject $reportFingerprint -Name 'digest') -ne [string]$SourceFingerprint.digest -or
            [int](Get-OptionalPropertyValue -InputObject $report -Name 'role_count') -ne $expectedCnVoiceRoleCount -or
            [int](Get-OptionalPropertyValue -InputObject $report -Name 'missing_count') -ne 0 -or
            [int](Get-OptionalPropertyValue -InputObject $report -Name 'total_asset_count') -ne $expectedCnVoiceEntryCount -or
            $masters.Count -ne 5 -or
            $assets.Count -ne 320 -or
            $roles.Count -ne $expectedCnVoiceRoleCount -or
            $invalidRoles.Count -ne 0 -or
            $null -eq $archive -or
            [string](Get-OptionalPropertyValue -InputObject $archive -Name 'relative_path') -ne 'archive-ios-diff/starpoint-cn-voice-overlay-ios.zip' -or
            [int](Get-OptionalPropertyValue -InputObject $archive -Name 'entry_count') -ne $expectedCnVoiceEntryCount -or
            (Get-OptionalPropertyValue -InputObject $archive -Name 'zip64') -ne $false) {
            return $null
        }

        $archivePath = Join-Path (Split-Path -Parent $ReportPath) 'archive-ios-diff\starpoint-cn-voice-overlay-ios.zip'
        if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
            return $null
        }
        $archiveBytes = [IO.File]::ReadAllBytes($archivePath)
        $archiveDigest = [Convert]::ToBase64String(
            [Security.Cryptography.SHA256]::HashData($archiveBytes)
        )
        if ([int64](Get-OptionalPropertyValue -InputObject $archive -Name 'byte_length') -ne $archiveBytes.Length -or
            [string](Get-OptionalPropertyValue -InputObject $archive -Name 'sha256') -ne $archiveDigest) {
            return $null
        }
        $zip = [IO.Compression.ZipFile]::OpenRead($archivePath)
        try {
            if ($zip.Entries.Count -ne $expectedCnVoiceEntryCount) {
                return $null
            }
        } finally {
            $zip.Dispose()
        }
        return $report
    } catch {
        return $null
    }
}
# //// /读取并验证可复用的 CN 语音报告 ////

# //// 补齐 CN 缺失角色语音 [@x380kkm 2026-08-29] ////
function Update-CnVoiceOverlay {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$CnCdnBundlePath
    )

    $workspaceRoot = Resolve-CnPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
    $generator = Join-Path $RepositoryRoot 'scripts\protocol-lab\build-cn-voice-overlay.py'
    $installer = Join-Path $RepositoryRoot 'scripts\protocol-lab\cn_voice_overlay_archive.py'
    $outputRoot = Join-Path $RepositoryRoot 'artifacts\voice-overlay-final'
    $reportPath = Join-Path $outputRoot 'voice-overlay-report.json'
    $sourceFingerprint = Get-CnVoiceOverlaySourceFingerprint -RepositoryRoot $RepositoryRoot
    $currentReport = Read-CnVoiceOverlayReportIfCurrent `
        -ReportPath $reportPath `
        -SourceFingerprint $sourceFingerprint
    if ($null -eq $currentReport) {
        $cnMasterRoot = Join-Path $workspaceRoot 'archive'
        $jpMasterRoot = Join-Path $workspaceRoot 'archive\jp-voice-masters'
        $cacheRoot = Join-Path $workspaceRoot 'archive\regional-zip-cache'
        $generationOutput = @(& uv run --python 3.12 --script $generator `
            --cn-master-root $cnMasterRoot `
            --jp-master-root $jpMasterRoot `
            --output-root $outputRoot `
            --cache-root $cacheRoot `
            --cn-asset-root $CnCdnBundlePath 2>&1)
        if ($LASTEXITCODE -ne 0) {
            $errorTail = ($generationOutput | Select-Object -Last 20) -join [Environment]::NewLine
            throw "CN voice overlay generation failed.`n$errorTail"
        }
        $sourceFingerprint = Get-CnVoiceOverlaySourceFingerprint -RepositoryRoot $RepositoryRoot
        $generatedReport = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $generatedReport | Add-Member -Force -NotePropertyName source_fingerprint -NotePropertyValue $sourceFingerprint
        $generatedReport | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $reportPath -Encoding UTF8
    }

    $runId = [Guid]::NewGuid().ToString('N')
    $auditPath = Join-Path ([IO.Path]::GetTempPath()) "starpoint-voice-overlay-$runId.audit.json"
    try {
        $installOutput = @(& uv run --python 3.12 --script $installer `
            --cdn-root $CnCdnBundlePath `
            --report $reportPath `
            --audit-report $auditPath 2>&1)
        if ($LASTEXITCODE -ne 0) {
            $errorTail = ($installOutput | Select-Object -Last 20) -join [Environment]::NewLine
            throw "CN voice overlay installation failed.`n$errorTail"
        }
        if (-not (Test-Path -LiteralPath $auditPath -PathType Leaf)) {
            throw 'CN voice overlay installation did not produce an audit report.'
        }
        $audit = Get-Content -LiteralPath $auditPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $reportData = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $expectedVoiceEntryCount = @($reportData.masters).Count + @($reportData.assets).Count
        $auditDigestMatchesReport = [string]$audit.archive.sha256 -eq [string]$reportData.archive.sha256 -and
            [int64]$audit.archive.size -eq [int64]$reportData.archive.byte_length
        $installedRelativePath = [string]$audit.archive.relative_path
        $installedPathParts = @($installedRelativePath.Replace('\', '/').Split('/') | Where-Object { $_ })
        $installedArchivePath = Join-Path $CnCdnBundlePath $installedRelativePath
        $installedArchiveMatchesReport = -not [IO.Path]::IsPathRooted($installedRelativePath) -and
            '..' -notin $installedPathParts -and
            (Test-Path -LiteralPath $installedArchivePath -PathType Leaf)
        if ($installedArchiveMatchesReport) {
            $installedArchiveBytes = [IO.File]::ReadAllBytes($installedArchivePath)
            $installedArchiveMatchesReport = $installedArchiveBytes.Length -eq [int64]$reportData.archive.byte_length -and
                [Convert]::ToBase64String([Security.Cryptography.SHA256]::HashData($installedArchiveBytes)) -eq
                    [string]$reportData.archive.sha256
        }
        if ([int]$audit.role_count -ne $expectedCnVoiceRoleCount -or
            [int]$audit.archive.entry_count -ne $expectedCnVoiceEntryCount -or
            $expectedVoiceEntryCount -ne $expectedCnVoiceEntryCount -or
            [int]$reportData.total_asset_count -ne $expectedCnVoiceEntryCount -or
            [int]$reportData.missing_count -ne 0 -or
            $audit.archive.zip64 -ne $false -or
            -not $auditDigestMatchesReport -or
            -not $installedArchiveMatchesReport -or
            [string]$audit.source_report -ne [string]$reportPath -or
            [string]$audit.archive.relative_path -notmatch '^archive-ios-diff/(?:starpoint-cn|starpoint-ios)-voice-overlay-') {
            throw 'CN voice overlay audit is incomplete.'
        }
        $audit | Add-Member -Force -NotePropertyName source_fingerprint -NotePropertyValue $sourceFingerprint
        $audit | Add-Member -Force -NotePropertyName report_reused -NotePropertyValue ($null -ne $currentReport)
        $audit
    } finally {
        Remove-Item -LiteralPath $auditPath -Force -ErrorAction SilentlyContinue
    }
}
# //// /补齐 CN 缺失角色语音 ////

# //// 补齐卡池活动目录与客户端 banner 资源 [@x380kkm 2026-08-25] ////
function Update-CnGachaBanners {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$CnCdnBundlePath
    )

    $generator = Join-Path $RepositoryRoot 'scripts\protocol-lab\generate_cn_gacha_banners.py'
    $archiveInstaller = Join-Path $RepositoryRoot 'scripts\protocol-lab\cn_gacha_banner_client_archive.py'
    $richCatalog = Find-CnRichActivityCatalog -RepositoryRoot $RepositoryRoot -CnCdnBundlePath $CnCdnBundlePath
    if ([string]::IsNullOrWhiteSpace($richCatalog)) {
        throw 'CN rich activity catalog source is required for activity resource projection.'
    }
    $catalogSeed = Initialize-CnActivityCatalog `
        -RepositoryRoot $RepositoryRoot `
        -CnCdnBundlePath $CnCdnBundlePath
    $startupStaticAssets = Initialize-CnStartupStaticAssets `
        -RepositoryRoot $RepositoryRoot `
        -CnCdnBundlePath $CnCdnBundlePath
    $catalog = [string]$catalogSeed.output
    $entityRecordCount = Ensure-CnEntityRecords -CnCdnBundlePath $CnCdnBundlePath -RichCatalogPath $richCatalog
    $manifest = Join-Path $CnCdnBundlePath 'entities\PathFile.csv'
    $regionPolicy = Join-Path $RepositoryRoot 'assets\gacha-region-policy.json'
    $runId = [Guid]::NewGuid().ToString('N')
    $reportPath = Join-Path ([IO.Path]::GetTempPath()) "starpoint-gacha-banner-$runId.json"
    $auditPath = Join-Path ([IO.Path]::GetTempPath()) "starpoint-gacha-banner-$runId.audit.json"
    try {
        $arguments = @(
            '--source-cdn-root', $CnCdnBundlePath,
            '--catalog', $catalog,
            '--output-cdn-root', $CnCdnBundlePath,
            '--app-asset-root', $CnCdnBundlePath,
            '--manifest', $manifest,
            '--region-policy', $regionPolicy,
            '--report', $reportPath
        )
        if (-not [string]::IsNullOrWhiteSpace($richCatalog) -and
            (Test-Path -LiteralPath $richCatalog -PathType Leaf)) {
            $arguments += @('--rich-catalog', $richCatalog)
        }
        $output = @(& uv run --script $generator @arguments 2>&1)
        if ($LASTEXITCODE -ne 0) {
            $errorTail = ($output | Select-Object -Last 20) -join [Environment]::NewLine
            throw "CN gacha banner generation failed.`n$errorTail"
        }
        if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) {
            throw 'CN gacha banner generation did not produce a report.'
        }
        $bannerReport = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ([int]$bannerReport.catalog_candidate_requested_count -ne
            [int]$bannerReport.catalog_candidate_materialized_count) {
            throw 'CN rich activity catalog contains unreachable image candidates.'
        }
        $catalogGaps = @(Get-CnCatalogResourceGaps -ResourceGaps @($bannerReport.resource_gaps))
        if ($catalogGaps.Count -gt 0) {
            throw "CN rich activity catalog resource gaps: $($catalogGaps.Count)."
        }

        $archiveArguments = @(
            '--cdn-root', $CnCdnBundlePath,
            '--report', $reportPath,
            '--audit-report', $auditPath
        )
        $archiveOutput = @(& uv run --script $archiveInstaller @archiveArguments 2>&1)
        if ($LASTEXITCODE -ne 0) {
            $errorTail = ($archiveOutput | Select-Object -Last 20) -join [Environment]::NewLine
            throw "CN gacha banner client archive failed.`n$errorTail"
        }
        if (-not (Test-Path -LiteralPath $auditPath -PathType Leaf)) {
            throw 'CN gacha banner client archive did not produce an audit report.'
        }
        $bannerAudit = Get-Content -LiteralPath $auditPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $coverage = Assert-CnDailyActivityCoverage -CnCdnBundlePath $CnCdnBundlePath -RichCatalogPath $richCatalog
        if ($null -ne $coverage -and $coverage.daily_activity_count -ne $null) {
            Write-Verbose ("CN daily coverage: " + ($coverage | ConvertTo-Json -Compress))
        }
        Write-Verbose "CN EntityLists records added: $entityRecordCount"

        $featureRestorer = Join-Path $RepositoryRoot 'scripts\protocol-lab\restore_cn_gacha_feature_images.py'
        $featureReportPath = Join-Path ([IO.Path]::GetTempPath()) "starpoint-gacha-feature-$runId.json"
        try {
            $featureArguments = @(
                '--cdn-root', $CnCdnBundlePath,
                '--app-asset-root', $CnCdnBundlePath
            )
            $workspaceRoot = Resolve-CnPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
            $featureSourceRoots = @(
                (Join-Path $workspaceRoot 'artifacts\cn-cdn\runtime\.cdn\cn')
            ) + @(
                Get-ChildItem -LiteralPath (Join-Path $workspaceRoot 'archive') -Directory -ErrorAction SilentlyContinue |
                    ForEach-Object { Join-Path $_.FullName 'derived-cdn-final' }
            ) + @(
                Get-ChildItem -LiteralPath (Join-Path $workspaceRoot 'artifacts\protocol-lab\ios-analysis') -Directory -ErrorAction SilentlyContinue |
                    ForEach-Object { Join-Path $_.FullName 'unpacked\Payload\worldflipper.app\asset' }
            ) + @(
                Get-ChildItem -LiteralPath (Join-Path $workspaceRoot 'artifacts\ios-real-app-probe') -Directory -ErrorAction SilentlyContinue |
                    ForEach-Object { Join-Path $_.FullName 'Payload\worldflipper.app\asset' }
            )
            foreach ($sourceRoot in ($featureSourceRoots | Sort-Object -Unique)) {
                if (Test-Path -LiteralPath $sourceRoot -PathType Container) {
                    $featureArguments += @('--source-root', $sourceRoot)
                }
            }
            $featureArguments += @('--report', $featureReportPath)
            $featureOutput = @(& uv run --script $featureRestorer @featureArguments 2>&1)
            if ($LASTEXITCODE -ne 0) {
                $errorTail = ($featureOutput | Select-Object -Last 20) -join [Environment]::NewLine
                throw "CN gacha feature image restoration failed.`n$errorTail"
            }
            if (-not (Test-Path -LiteralPath $featureReportPath -PathType Leaf)) {
                throw 'CN gacha feature image restoration did not produce a report.'
            }
            $featureReport = Get-Content -LiteralPath $featureReportPath -Raw -Encoding UTF8 | ConvertFrom-Json
            if ([int]$featureReport.feature_reference_count -ne
                ([int]$featureReport.reachable_before + [int]$featureReport.restored_count)) {
                throw 'CN gacha feature image report does not close all references.'
            }
            Write-Verbose ("CN gacha feature image coverage: " + ($featureReport | ConvertTo-Json -Compress))
        } finally {
            Remove-Item -LiteralPath $featureReportPath -Force -ErrorAction SilentlyContinue
        }
    } finally {
        Remove-Item -LiteralPath $reportPath, $auditPath -Force -ErrorAction SilentlyContinue
    }
    [pscustomobject][ordered]@{
        entity_record_count = [int]$entityRecordCount
        catalog_activity_count = [int]$catalogSeed.activity_count
        catalog_gacha_activity_count = [int]$catalogSeed.gacha_activity_count
        catalog_daily_activity_count = [int]$catalogSeed.daily_activity_count
        catalog_candidate_requested_count = [int]$bannerReport.catalog_candidate_requested_count
        catalog_candidate_materialized_count = [int]$bannerReport.catalog_candidate_materialized_count
        banner_asset_count = [int]$bannerAudit.banner_asset_count
        banner_archive_entry_count = [int]$bannerAudit.archive.entry_count
        feature_reference_count = [int]$featureReport.feature_reference_count
        feature_restored_count = [int]$featureReport.restored_count
        feature_generated_count = [int]$featureReport.generated_count
        startup_static_asset_count = [int]$startupStaticAssets.file_count
        startup_static_asset_copied_count = [int]$startupStaticAssets.copied_count
        daily_activity_count = if ($null -eq $coverage) { $null } else { [int]$coverage.daily_activity_count }
        daily_candidate_count = if ($null -eq $coverage) { $null } else { [int]$coverage.daily_candidate_count }
    }
}
# //// /补齐卡池活动目录与客户端 banner 资源 ////

# //// 验证 CN iOS 1.8.4 包装报告 [@x380kkm 2026-08-18] ////
function Assert-CnPackageReport {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][pscustomobject]$ReportData,
        [Parameter(Mandatory)][string[]]$ExpectedCompatibilityPatchNames
    )

    if ($ReportData.requires_resigning -ne $true -or $ReportData.installable -ne $false) {
        throw 'CN iOS package must remain explicitly unsigned.'
    }
    if ($ReportData.framework_architecture -ne 'arm64') {
        throw "CN iOS package Framework architecture is not arm64: $($ReportData.framework_architecture)."
    }
    $gachaMasterDiff = Get-OptionalPropertyValue -InputObject $ReportData -Name 'cn_gacha_master_diff'
    if ($null -eq $gachaMasterDiff -or
        [string]$gachaMasterDiff.version -ne (Get-CnNextAssetVersion -Version ([string]$gachaMasterDiff.original_version)) -or
        [int]$gachaMasterDiff.retained_original_count -ne 584 -or
        [int]$gachaMasterDiff.temporary_alias_count -ne 473 -or
        [int]$gachaMasterDiff.gacha_count -ne 1057 -or
        [int]$gachaMasterDiff.retained_original_campaign_count -ne 41 -or
        [int]$gachaMasterDiff.temporary_campaign_alias_count -ne 123 -or
        [int]$gachaMasterDiff.gacha_campaign_count -ne 164 -or
        [int]$gachaMasterDiff.projected_feature_link_count -ne 11 -or
        $gachaMasterDiff.zip64 -ne $false) {
        throw 'CN iOS package did not include the complete gacha master projection.'
    }
    $gachaVisual = Get-OptionalPropertyValue -InputObject $ReportData -Name 'cn_gacha_visual'
    if ($null -eq $gachaVisual -or
        [int]$gachaVisual.catalog_activity_count -ne 922 -or
        [int]$gachaVisual.catalog_gacha_activity_count -ne 483 -or
        [int]$gachaVisual.catalog_daily_activity_count -ne 20 -or
        [int]$gachaVisual.catalog_candidate_requested_count -ne
         [int]$gachaVisual.catalog_candidate_materialized_count -or
         [int]$gachaVisual.banner_asset_count -le 0 -or
         [int]$gachaVisual.banner_archive_entry_count -le 0 -or
         [int]$gachaVisual.startup_static_asset_count -ne 46) {
        throw 'CN iOS package did not include the complete activity, banner and startup resource projection.'
     }
    if ([string]$ReportData.cn_cdn_bundle_path -ne 'StarpointCNCDN' -or
        [string]$ReportData.cn_cdn_bundle_mode -ne 'direct' -or
        [int64]$ReportData.cn_cdn_file_count -le 0 -or
        [int64]$ReportData.cn_cdn_total_size -le 0) {
        throw 'CN iOS package must contain a non-empty StarpointCNCDN bundle.'
    }

    $replacements = @($ReportData.cn_endpoint_replacements)
    $actualEndpoints = @($replacements | ForEach-Object { [string]$_.endpoint })
    $uniqueEndpoints = @($actualEndpoints | Sort-Object -Unique)
    $missingEndpoints = @($expectedEndpointNames | Where-Object { $_ -notin $actualEndpoints })
    if ($replacements.Count -ne $expectedEndpointNames.Count -or
        $uniqueEndpoints.Count -ne $expectedEndpointNames.Count -or
        $missingEndpoints.Count -ne 0) {
        throw 'CN iOS package did not report all five endpoint replacement categories.'
    }
    foreach ($replacement in $replacements) {
        if (-not (Test-CnLoopbackEndpoint -Replacement $replacement)) {
            throw "CN iOS package contains an invalid local endpoint: $($replacement.endpoint)."
        }
    }

    $compatibilityPatches = @($ReportData.cn_compatibility_patches)
    $actualPatchNames = @($compatibilityPatches | ForEach-Object { [string]$_.patch })
    $uniquePatchNames = @($actualPatchNames | Sort-Object -Unique)
    $missingPatchNames = @($ExpectedCompatibilityPatchNames | Where-Object { $_ -notin $actualPatchNames })
    if ($compatibilityPatches.Count -ne $ExpectedCompatibilityPatchNames.Count -or
        $uniquePatchNames.Count -ne $ExpectedCompatibilityPatchNames.Count -or
        $missingPatchNames.Count -ne 0) {
        throw 'CN iOS package did not report the complete 1.8.4 compatibility patch set.'
    }

    $invalidPatchEvidence = @($compatibilityPatches | Where-Object {
        [string]$_.status -notin @('applied', 'already_applied') -or
        [int64]$_.offset -lt 0 -or
        [int]$_.bytes -le 0 -or
        [string]$_.source_sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$_.target_sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$_.source_window_sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$_.target_window_sha256 -notmatch '^[0-9a-f]{64}$'
    })
    if ($invalidPatchEvidence.Count -ne 0) {
        throw 'CN iOS package contains invalid 1.8.4 compatibility patch evidence.'
    }

    return $actualEndpoints
}
# //// /验证 CN iOS 1.8.4 包装报告 ////

# //// 解析并验证 CN iOS 包装输入 [@x380kkm 2026-08-18] ////
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$expectedCompatibilityPatchNames = Get-CnCompatibilityPatchNames -ModuleDirectory $PSScriptRoot
$inputCandidatePath = [IO.Path]::GetFullPath($InputIpa)
$outputPath = [IO.Path]::GetFullPath($OutputIpa)
if ($inputCandidatePath -eq $outputPath) {
    throw 'Input IPA and output IPA must be different.'
}
if ([IO.Path]::GetExtension($outputPath) -ne '.ipa') {
    throw 'Output IPA path must use the .ipa extension.'
}

$reportPath = if ($Report) {
    [IO.Path]::GetFullPath($Report)
} else {
    "$outputPath.package.json"
}
if ($outputPath -eq $reportPath) {
    throw 'Output IPA and report must be different.'
}
if ([IO.Path]::GetExtension($reportPath) -ne '.json') {
    throw 'Report path must use the .json extension.'
}

$outputParent = Split-Path -Parent $outputPath
$reportParent = Split-Path -Parent $reportPath
New-Item -ItemType Directory -Force -Path $outputParent, $reportParent | Out-Null
$runId = [Guid]::NewGuid().ToString('N')
$temporaryOutputPath = Join-Path $outputParent ".$([IO.Path]::GetFileName($outputPath)).$runId.tmp"
$temporaryReportPath = Join-Path $reportParent ".$([IO.Path]::GetFileName($reportPath)).$runId.tmp"
# //// /解析并验证 CN iOS 包装输入 ////

# //// 原子生成并验证 CN 本地个人服务包 [@x380kkm 2026-08-18] ////
Remove-Item -LiteralPath $outputPath, $reportPath -Force -ErrorAction SilentlyContinue
try {
    $inputPath = (Resolve-Path -LiteralPath $InputIpa).Path
    $frameworkPath = (Resolve-Path -LiteralPath $Framework).Path
    $cnCdnBundlePath = (Resolve-Path -LiteralPath $CnCdnBundle).Path
    if (-not (Test-Path -LiteralPath $cnCdnBundlePath -PathType Container)) {
        throw 'CnCdnBundle must point to a directory.'
    }
    $gachaMasterDiff = Update-CnGachaMasterDiff `
        -RepositoryRoot $repositoryRoot `
        -CnCdnBundlePath $cnCdnBundlePath
    $voiceOverlay = Update-CnVoiceOverlay `
        -RepositoryRoot $repositoryRoot `
        -CnCdnBundlePath $cnCdnBundlePath
    $gachaVisual = Update-CnGachaBanners -RepositoryRoot $repositoryRoot -CnCdnBundlePath $cnCdnBundlePath
    $embeddedIconDirectory = Join-Path $repositoryRoot 'core\personal-service\web\management\assets\item-icons'
    Assert-CnArtworkBundle -CnCdnBundlePath $cnCdnBundlePath -EmbeddedIconDirectory $embeddedIconDirectory
    $packageScript = Join-Path $repositoryRoot 'scripts\protocol-lab\package_ios_personal_service.py'
    $arguments = @(
        '--python', '3.12', $packageScript,
        $inputPath,
        '--framework', $frameworkPath,
        '--output', $temporaryOutputPath,
        '--bundle-id', $BundleId,
        '--display-name', $DisplayName,
        '--cn-cdn-bundle', $cnCdnBundlePath,
        '--patch-cn-endpoints',
        '--report', $temporaryReportPath
    )

    $packageOutput = @(& uv run @arguments 2>&1)
    $packageExitCode = $LASTEXITCODE
    if ($packageExitCode -ne 0) {
        $errorTail = ($packageOutput | Select-Object -Last 20) -join [Environment]::NewLine
        throw "CN iOS packaging failed with exit code $packageExitCode.`n$errorTail"
    }
    if (-not (Test-Path -LiteralPath $temporaryOutputPath) -or
        -not (Test-Path -LiteralPath $temporaryReportPath)) {
        throw 'CN iOS packaging did not produce both the IPA and report.'
    }

    $reportData = Get-Content -Raw -Encoding UTF8 -LiteralPath $temporaryReportPath | ConvertFrom-Json
    $reportData | Add-Member -NotePropertyName cn_gacha_master_diff -NotePropertyValue $gachaMasterDiff
    $reportData | Add-Member -NotePropertyName cn_gacha_visual -NotePropertyValue $gachaVisual
    $reportData | Add-Member -NotePropertyName cn_voice_overlay -NotePropertyValue $voiceOverlay
    $actualEndpoints = Assert-CnPackageReport `
        -ReportData $reportData `
        -ExpectedCompatibilityPatchNames $expectedCompatibilityPatchNames
    $reportData.output_ipa = $outputPath
    $reportData | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $temporaryReportPath -Encoding UTF8
    Move-Item -LiteralPath $temporaryOutputPath -Destination $outputPath
    Move-Item -LiteralPath $temporaryReportPath -Destination $reportPath
} catch {
    Remove-Item -LiteralPath $outputPath, $reportPath -Force -ErrorAction SilentlyContinue
    throw
} finally {
    Remove-Item -LiteralPath $temporaryOutputPath, $temporaryReportPath -Force -ErrorAction SilentlyContinue
}

[pscustomobject]@{
    status = 'passed'
    output_ipa = $outputPath
    report = $reportPath
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $outputPath).Hash.ToLowerInvariant()
    endpoint_categories = $actualEndpoints
    personal_service_authority = '127.0.0.1:17171'
    requires_resigning = $reportData.requires_resigning
} | ConvertTo-Json -Depth 4
# //// /原子生成并验证 CN 本地个人服务包 ////
