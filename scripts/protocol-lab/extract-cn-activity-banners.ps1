$ErrorActionPreference = 'Stop'
# audience: external
# # extract-cn-activity-banners
# 此脚本从本机 CN master 生成活动目录并提取其引用的横幅. 输出位于 CDN 根并保持在 Git 忽略范围内.

Set-StrictMode -Version Latest

# //// 读取命令行选项 [@x380kkm 2026-08-19] ////
function Read-OptionValue {
    param(
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][ref]$Index,
        [Parameter(Mandatory)][string]$Name
    )

    $nextIndex = $Index.Value + 1
    if ($nextIndex -ge $Arguments.Count -or $Arguments[$nextIndex].StartsWith('-')) {
        throw "$Name 缺少非空值."
    }
    $Index.Value = $nextIndex
    return $Arguments[$nextIndex]
}

function Read-IntegerOption {
    param(
        [Parameter(Mandatory)][string]$Value,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][int]$Minimum,
        [Parameter(Mandatory)][int]$Maximum
    )

    $parsed = 0
    if (-not [int]::TryParse($Value, [ref]$parsed) -or $parsed -lt $Minimum -or $parsed -gt $Maximum) {
        throw "$Name 必须位于 $Minimum 到 $Maximum."
    }
    return $parsed
}

function Read-DoubleOption {
    param(
        [Parameter(Mandatory)][string]$Value,
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][double]$Minimum,
        [Parameter(Mandatory)][double]$Maximum
    )

    $parsed = 0.0
    if (-not [double]::TryParse(
        $Value,
        [Globalization.NumberStyles]::Float,
        [Globalization.CultureInfo]::InvariantCulture,
        [ref]$parsed
    ) -or $parsed -lt $Minimum -or $parsed -gt $Maximum) {
        throw "$Name 必须位于 $Minimum 到 $Maximum."
    }
    return $parsed
}
# //// /读取命令行选项 ////

# //// 解析提取器命令行 [@x380kkm 2026-08-19] ////
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$parameters = @{
    CdnRoot = Join-Path $repositoryRoot '.cdn\cn'
    MinimumWidth = 480
    MinimumHeight = 80
    MaximumHeight = 720
    MinimumAspectRatio = 2.2
    MaximumAspectRatio = 8.0
}
$generateCatalogSource = $true
$argumentList = [string[]]$args
for ($index = 0; $index -lt $argumentList.Count; $index += 1) {
    $argument = $argumentList[$index]
    switch ($argument) {
        { $_ -in @('-CdnRoot', '--cdn-root') } {
            $parameters.CdnRoot = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
        }
        { $_ -in @('-OutputDirectory', '--output-directory') } {
            $parameters.OutputDirectory = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
        }
        { $_ -in @('-CandidateManifestPath', '--candidate-manifest') } {
            $parameters.CandidateManifestPath = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
        }
        { $_ -in @('-CatalogSourcePath', '--catalog-source') } {
            $parameters.CatalogSourcePath = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
        }
        { $_ -in @('-CatalogOutputPath', '--catalog-output') } {
            $parameters.CatalogOutputPath = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
        }
        { $_ -in @('-AllCandidates', '--all-candidates') } {
            $generateCatalogSource = $false
        }
        { $_ -in @('-MinimumWidth', '--minimum-width') } {
            $value = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
            $parameters.MinimumWidth = Read-IntegerOption -Value $value -Name $argument -Minimum 64 -Maximum 8192
        }
        { $_ -in @('-MinimumHeight', '--minimum-height') } {
            $value = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
            $parameters.MinimumHeight = Read-IntegerOption -Value $value -Name $argument -Minimum 32 -Maximum 4096
        }
        { $_ -in @('-MaximumHeight', '--maximum-height') } {
            $value = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
            $parameters.MaximumHeight = Read-IntegerOption -Value $value -Name $argument -Minimum 32 -Maximum 4096
        }
        { $_ -in @('-MinimumAspectRatio', '--minimum-aspect-ratio') } {
            $value = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
            $parameters.MinimumAspectRatio = Read-DoubleOption -Value $value -Name $argument -Minimum 1.0 -Maximum 20.0
        }
        { $_ -in @('-MaximumAspectRatio', '--maximum-aspect-ratio') } {
            $value = Read-OptionValue -Arguments $argumentList -Index ([ref]$index) -Name $argument
            $parameters.MaximumAspectRatio = Read-DoubleOption -Value $value -Name $argument -Minimum 1.0 -Maximum 20.0
        }
        { $_ -in @('-Help', '--help', '-h') } {
            Write-Output '用法: extract-cn-activity-banners.ps1 [--cdn-root PATH] [--output-directory PATH] [--candidate-manifest PATH] [--catalog-source PATH] [--catalog-output PATH] [--all-candidates]'
            exit 0
        }
        default { throw "未知参数: $argument" }
    }
}
# //// /解析提取器命令行 ////

# //// 从 CN master 生成已验证的活动资源映射 [@x380kkm 2026-08-19] ////
if ($generateCatalogSource -and -not $parameters.ContainsKey('CatalogSourcePath')) {
    $resolvedCdnRoot = (Resolve-Path -LiteralPath $parameters.CdnRoot).Path
    $catalogSourcePath = Join-Path $resolvedCdnRoot 'activity-catalog-source.json'
    $generatorPath = Join-Path $PSScriptRoot 'generate-cn-activity-catalog.mjs'
    $null = & node $generatorPath --cdn-root $resolvedCdnRoot --output $catalogSourcePath --client-version '1.8.1'
    if ($LASTEXITCODE -ne 0) { throw "CN activity catalog generation failed: exit=$LASTEXITCODE" }
    $parameters.CatalogSourcePath = $catalogSourcePath
}
# //// /从 CN master 生成已验证的活动资源映射 ////

Import-Module (Join-Path $PSScriptRoot 'cn-activity-banner-extractor.psm1') -Force
$result = Export-CnActivityBannerCandidates @parameters
$result | ConvertTo-Json -Depth 4
