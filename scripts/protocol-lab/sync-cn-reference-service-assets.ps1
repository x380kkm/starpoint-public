# audience: internal
# # sync-cn-reference-service-assets
#
# 该脚本将个人服务编译使用的 CN JSON 资产同步到 startpoint-cn-launcher 的对应版本.

$ErrorActionPreference = 'Stop'

$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$workspaceRoot = Split-Path -Parent $projectRoot
$referenceAssetRoot = Join-Path $workspaceRoot 'startpoint-cn-launcher\resources\server\assets'
$serviceSourceRoot = Join-Path $projectRoot 'core\personal-service\src'

if (-not (Test-Path -LiteralPath $referenceAssetRoot -PathType Container)) {
    throw "Reference asset directory does not exist: $referenceAssetRoot"
}

# //// 解析并同步个人服务引用的同名 JSON 资产 [@x380kkm 2026-08-23] ////
$assetTargets = @{}
foreach ($sourceFile in Get-ChildItem -LiteralPath $serviceSourceRoot -Recurse -File -Filter '*.rs') {
    $source = Get-Content -LiteralPath $sourceFile.FullName -Raw -Encoding UTF8
    foreach ($match in [regex]::Matches($source, 'include_str!\(\s*"([^"]+\.json)"\s*\)')) {
        $targetPath = [System.IO.Path]::GetFullPath(
            (Join-Path $sourceFile.DirectoryName $match.Groups[1].Value)
        )
        $referencePath = Join-Path $referenceAssetRoot ([System.IO.Path]::GetFileName($targetPath))
        if (Test-Path -LiteralPath $referencePath -PathType Leaf) {
            $assetTargets[$targetPath] = $referencePath
        }
    }
}

$updated = @()
foreach ($targetPath in $assetTargets.Keys | Sort-Object) {
    $referencePath = $assetTargets[$targetPath]
    $targetHash = if (Test-Path -LiteralPath $targetPath -PathType Leaf) {
        (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash
    }
    else {
        $null
    }
    $referenceHash = (Get-FileHash -LiteralPath $referencePath -Algorithm SHA256).Hash
    if ($targetHash -eq $referenceHash) {
        continue
    }
    Copy-Item -LiteralPath $referencePath -Destination $targetPath
    (Get-Item -LiteralPath $targetPath).LastWriteTimeUtc = [DateTime]::UtcNow
    $updated += [System.IO.Path]::GetRelativePath($projectRoot, $targetPath)
}

[pscustomobject]@{
    Compared = $assetTargets.Count
    Updated = $updated.Count
    Files = $updated
} | ConvertTo-Json -Depth 4
# //// /解析并同步个人服务引用的同名 JSON 资产 ////
