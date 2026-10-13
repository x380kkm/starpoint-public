# audience: internal
# # cn-activity-catalog-validation
# 活动目录沿用源目录的完整 ID 集合, 每个图片引用指向可读取的 PNG 文件.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# //// 读取活动目录身份集合 [@x380kkm 2026-09-26] ////
function Get-CnActivityCatalogIdentity {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$CatalogPath)

    $catalog = Get-Content -LiteralPath $CatalogPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $activities = @($catalog.activities)
    $ids = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($activity in $activities) {
        $id = [string]$activity.activity_id
        if ([string]::IsNullOrWhiteSpace($id) -or -not $ids.Add($id)) {
            throw "Activity catalog contains an empty or duplicate ID: $id."
        }
    }
    if ($ids.Count -eq 0) { throw 'Activity catalog contains no activities.' }
    [pscustomobject]@{
        activity_ids = @($ids | Sort-Object -CaseSensitive)
        activity_count = $ids.Count
        gacha_activity_count = @($activities | Where-Object { [string]$_.kind -ceq 'gacha' }).Count
        daily_activity_count = @($activities | Where-Object { [string]$_.activity_id -match '^(daily-exp-mana|daily-week):' }).Count
    }
}
# //// /读取活动目录身份集合 ////

# //// 校验完整活动身份集合 [@x380kkm 2026-09-26] ////
function Assert-CnActivityCatalogIds {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string[]]$ExpectedIds,
        [Parameter(Mandatory)][AllowEmptyCollection()][string[]]$ActualIds
    )

    $expected = [Collections.Generic.HashSet[string]]::new($ExpectedIds, [StringComparer]::Ordinal)
    $actual = [Collections.Generic.HashSet[string]]::new($ActualIds, [StringComparer]::Ordinal)
    if ($actual.Count -ne $ActualIds.Count -or -not $expected.SetEquals($actual)) {
        $missing = @($ExpectedIds | Where-Object { -not $actual.Contains($_) })
        $unexpected = @($ActualIds | Where-Object { -not $expected.Contains($_) })
        throw "Activity catalog IDs differ from source: missing=$($missing -join ',') unexpected=$($unexpected -join ',')."
    }
}
# //// /校验完整活动身份集合 ////

# //// 校验物化活动目录及图片依赖 [@x380kkm 2026-09-26] ////
function Assert-CnActivityCatalogCoverage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$SourceCatalogPath,
        [Parameter(Mandatory)][string]$CnCdnBundlePath
    )

    $catalogPath = Join-Path $CnCdnBundlePath 'activity-catalog.json'
    $source = Get-CnActivityCatalogIdentity -CatalogPath $SourceCatalogPath
    $output = Get-CnActivityCatalogIdentity -CatalogPath $catalogPath
    Assert-CnActivityCatalogIds -ExpectedIds $source.activity_ids -ActualIds $output.activity_ids
    $catalog = Get-Content -LiteralPath $catalogPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $keys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($activity in @($catalog.activities)) {
        if ($null -ne $activity.PSObject.Properties['banner_key'] -and $activity.banner_key) {
            $null = $keys.Add([string]$activity.banner_key)
        }
        if ($null -ne $activity.PSObject.Properties['image_candidates']) {
            foreach ($candidate in @($activity.image_candidates)) {
                $null = $keys.Add([string]$candidate.key)
            }
        }
    }
    if ($keys.Count -eq 0) { throw 'Activity catalog contains no image dependencies.' }
    $signature = [byte[]]@(137, 80, 78, 71, 13, 10, 26, 10)
    foreach ($key in $keys) {
        if ($key -cnotmatch '^[a-f0-9]{40}\.png$') { throw "Invalid activity image key: $key." }
        $path = Join-Path $CnCdnBundlePath "activity-banners/$key"
        $stream = [IO.File]::OpenRead($path)
        try {
            $header = [byte[]]::new(8)
            if ($stream.Length -le 8 -or $stream.Read($header, 0, 8) -ne 8) {
                throw "Activity image is empty or truncated: $key."
            }
            for ($index = 0; $index -lt 8; $index++) {
                if ($header[$index] -ne $signature[$index]) { throw "Activity image is not PNG: $key." }
            }
        } finally {
            $stream.Dispose()
        }
    }
    $output | Add-Member -NotePropertyName image_count -NotePropertyValue $keys.Count
    $output
}
# //// /校验物化活动目录及图片依赖 ////

Export-ModuleMember -Function Get-CnActivityCatalogIdentity, Assert-CnActivityCatalogIds, Assert-CnActivityCatalogCoverage
