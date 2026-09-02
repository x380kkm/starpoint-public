# audience: internal
# # audit-cn-response-contracts
#
# 该脚本核查目标客户端和抓包确认的响应编码, 字段形状和持久状态.

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$repositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$workspaceRoot = Split-Path -Parent $repositoryRoot
$missionAssetRoot = Join-Path $workspaceRoot 'startpoint-cn-launcher\resources\server\assets'
$missionFixture = Join-Path $repositoryRoot 'core\personal-service\assets\cn-mission-master.json'
$missionGenerator = Join-Path $PSScriptRoot 'generate-cn-mission-fixture.mjs'
$corpusGenerator = Join-Path $PSScriptRoot 'generate-cn-reference-differential-corpus.mjs'
$manifestPath = Join-Path $repositoryRoot 'core\personal-service\Cargo.toml'

# //// 核查确认响应使用的派生目录和直接场景 [@x380kkm 2026-08-25] ////
Push-Location -LiteralPath $repositoryRoot
try {
    node $missionGenerator --asset-root $missionAssetRoot --output $missionFixture --check
    node $corpusGenerator --check
    cargo +1.78.0 test --manifest-path $manifestPath `
        --test cn_asset `
        --test cn_activity_raid_rush `
        --test cn_character `
        --test cn_mission `
        --test cn_shop
} finally {
    Pop-Location
}
# //// /核查确认响应使用的派生目录和直接场景 ////
