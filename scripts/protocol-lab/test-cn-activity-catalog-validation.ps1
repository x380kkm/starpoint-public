$ErrorActionPreference = 'Stop'
# audience: internal
# # test-cn-activity-catalog-validation

Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'cn-activity-catalog-validation.psm1') -Force

# //// 校验活动身份替换和图片缺失会阻止发布 [@x380kkm 2026-09-26] ////
$testRoot = Join-Path ([IO.Path]::GetTempPath()) "cn-catalog-$([Guid]::NewGuid().ToString('N'))"
$imageKey = ('a' * 40) + '.png'
$bannerRoot = Join-Path $testRoot 'activity-banners'
$sourcePath = Join-Path $testRoot 'source.json'
$catalogPath = Join-Path $testRoot 'activity-catalog.json'
try {
    New-Item -ItemType Directory -Path $bannerRoot -Force | Out-Null
    $activities = @(
        [pscustomobject]@{ activity_id = 'event:1'; kind = 'event'; banner_key = $imageKey },
        [pscustomobject]@{ activity_id = 'exchange:1'; kind = 'exchange'; image_candidates = @([pscustomobject]@{ key = $imageKey }) }
    )
    $catalog = [pscustomobject]@{ activities = $activities }
    $catalog | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $sourcePath -Encoding UTF8
    Copy-Item -LiteralPath $sourcePath -Destination $catalogPath
    $imagePath = Join-Path $bannerRoot $imageKey
    $image = [Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jR9kAAAAASUVORK5CYII=')
    [IO.File]::WriteAllBytes($imagePath, $image)
    $coverage = Assert-CnActivityCatalogCoverage -SourceCatalogPath $sourcePath -CnCdnBundlePath $testRoot
    if ($coverage.activity_count -ne 2 -or $coverage.image_count -ne 1) { throw 'Catalog coverage counts are incorrect.' }

    $activities[1].activity_id = 'exchange:2'
    $catalog | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $catalogPath -Encoding UTF8
    $identityRejected = $false
    try {
        Assert-CnActivityCatalogCoverage -SourceCatalogPath $sourcePath -CnCdnBundlePath $testRoot | Out-Null
    } catch {
        $identityRejected = $_.Exception.Message.Contains('missing=exchange:1 unexpected=exchange:2', [StringComparison]::Ordinal)
    }
    if (-not $identityRejected) { throw 'Same-count activity ID replacement was accepted.' }

    Copy-Item -LiteralPath $sourcePath -Destination $catalogPath -Force
    Remove-Item -LiteralPath $imagePath
    $imageRejected = $false
    try {
        Assert-CnActivityCatalogCoverage -SourceCatalogPath $sourcePath -CnCdnBundlePath $testRoot | Out-Null
    } catch {
        $imageRejected = $_.Exception.Message.Contains($imageKey, [StringComparison]::Ordinal)
    }
    if (-not $imageRejected) { throw 'Missing activity image was accepted.' }
    [pscustomobject]@{ source_identity = 'passed'; image_dependency = 'passed' } | ConvertTo-Json
} finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
# //// /校验活动身份替换和图片缺失会阻止发布 ////
