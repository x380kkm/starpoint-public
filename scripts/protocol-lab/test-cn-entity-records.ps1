$ErrorActionPreference = 'Stop'
# audience: internal
# # test-cn-entity-records

Set-StrictMode -Version Latest

# //// 载入打包器的资源记录合并函数 [@x380kkm 2026-09-26] ////
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile(
    (Join-Path $PSScriptRoot 'package-ios-cn-personal-service.ps1'), [ref]$tokens, [ref]$parseErrors
)
if ($parseErrors.Count -gt 0) { throw ($parseErrors | Out-String) }
foreach ($functionName in @('Get-OptionalPropertyValue', 'Ensure-CnEntityRecords')) {
    $definition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $functionName
    }, $false)
    if ($null -eq $definition) { throw "打包函数不存在: $functionName" }
    . ([ScriptBlock]::Create($definition.Extent.Text))
}
# //// /载入打包器的资源记录合并函数 ////

# //// 校验资源迁移保持分发版本并拒绝内容冲突 [@x380kkm 2026-09-26] ////
$testRoot = Join-Path ([IO.Path]::GetTempPath()) "cn-entity-records-$([Guid]::NewGuid().ToString('N'))"
try {
    $entityRoot = Join-Path $testRoot 'entities'
    New-Item -ItemType Directory -Path $entityRoot | Out-Null
    $manifestPaths = @((Join-Path $entityRoot 'PathFile.csv'), (Join-Path $entityRoot '10939-ios_medium.csv'))
    $entry = 'production/upload/aa/' + ('a' * 38)
    $digest = 'B' * 43
    $sourcePath = Join-Path $testRoot 'source.json'
    $source = @{ activities = @(@{ kind = 'event'; image_candidates = @(@{
        source_entry = $entry; source_hash = 'a' * 40; source_version = '1.4.61'
        source_byte_length = 128; source_digest = $digest; source_asset_kind = 'common'
    }) }) }
    $source | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $sourcePath -Encoding UTF8
    $currentLine = "$entry,1.4.65,128,$digest,common"
    foreach ($manifestPath in $manifestPaths) {
        $currentLine | Set-Content -LiteralPath $manifestPath -Encoding UTF8
    }
    $added = Ensure-CnEntityRecords -CnCdnBundlePath $testRoot -RichCatalogPath $sourcePath
    if ($added -ne 0) { throw '相同内容的迁移资源产生了重复记录.' }
    foreach ($manifestPath in $manifestPaths) {
        if ((Get-Content -LiteralPath $manifestPath -Encoding UTF8) -cne $currentLine) {
            throw '迁移资源的分发版本发生了变化.'
        }
    }

    foreach ($conflict in @(
        "$entry,1.4.65,129,$digest,common",
        "$entry,1.4.65,128,$('C' * 43),common",
        "$entry,1.4.65,128,$digest,medium"
    )) {
        $conflict | Set-Content -LiteralPath $manifestPaths[0] -Encoding UTF8
        $rejected = $false
        try { Ensure-CnEntityRecords -CnCdnBundlePath $testRoot -RichCatalogPath $sourcePath | Out-Null }
        catch { $rejected = $_.Exception.Message.Contains('conflicting metadata') }
        if (-not $rejected) { throw '长度, 摘要或资源类型冲突未被拒绝.' }
    }

    Clear-Content -LiteralPath $manifestPaths[0]
    $added = Ensure-CnEntityRecords -CnCdnBundlePath $testRoot -RichCatalogPath $sourcePath
    $expectedLine = "$entry,1.4.61,128,$digest,common"
    if ($added -ne 1 -or (Get-Content -LiteralPath $manifestPaths[0] -Encoding UTF8) -cne $expectedLine) {
        throw '缺少的资源记录未按目录来源补入.'
    }
    [pscustomobject]@{ relocation = 'passed'; content_conflicts = 'passed'; missing_record = 'passed' } | ConvertTo-Json
} finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
# //// /校验资源迁移保持分发版本并拒绝内容冲突 ////
