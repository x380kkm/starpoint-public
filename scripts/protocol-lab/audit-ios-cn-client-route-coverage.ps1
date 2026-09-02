# audience: internal
# # ios-cn-client-route-coverage
# 此脚本核对当前 iOS Mach-O 的 CN 客户端请求, 间接路径表达式与个人服务注册路由.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ClientExecutable,
    [Parameter(Mandatory)][string]$DecompiledRoot,
    [string]$ReferenceRoot,
    [switch]$AllowMissing
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# //// 规范化 CN 游戏请求路径 [@x380kkm 2026-08-24] ////
function ConvertTo-GamePath {
    param([Parameter(Mandatory)][string]$Path)

    if ($Path.StartsWith('/')) {
        return $Path
    }
    "/api/index.php/$Path"
}
# //// /规范化 CN 游戏请求路径 ////

# //// 枚举反编译客户端请求及源码锚点 [@x380kkm 2026-08-24] ////
function Get-ClientRequestEvidence {
    param([Parameter(Mandatory)][string]$Root)

    $evidence = [ordered]@{}
    $runtimeRequests = [System.Collections.Generic.List[object]]::new()
    $requestMethodPattern = 'start(?:UserRequest(?:Detail(?:WithHeaders)?)?|DebugRequest)'
    $pattern = [regex]"$requestMethodPattern\(`"([^`"]+)`""
    $matches = & rg -n --no-heading --with-filename --glob '*.as' `
        'start(?:UserRequest(?:Detail(?:WithHeaders)?)?|DebugRequest)\("[^"]+"' $Root
    if ($LASTEXITCODE -gt 1) {
        throw "client request scan failed: exit=$LASTEXITCODE"
    }
    foreach ($entry in $matches) {
        $location = [regex]::Match($entry, '^(.*):(\d+):(.*)$')
        if (-not $location.Success) {
            throw "client request evidence is invalid: $entry"
        }
        foreach ($match in $pattern.Matches($location.Groups[3].Value)) {
            $path = ConvertTo-GamePath -Path $match.Groups[1].Value
            if (-not $evidence.Contains($path)) {
                $evidence[$path] = [System.Collections.Generic.List[string]]::new()
            }
            $evidence[$path].Add("$($location.Groups[1].Value):$($location.Groups[2].Value)")
        }
    }

    $indirectCalls = & rg -n --no-heading --with-filename --glob '*.as' `
        'start(?:UserRequest(?:Detail(?:WithHeaders)?)?|DebugRequest)\([^"\r\n]' $Root
    if ($LASTEXITCODE -gt 1) {
        throw "indirect client request scan failed: exit=$LASTEXITCODE"
    }
    $indirectLocations = $indirectCalls | ForEach-Object {
        $location = [regex]::Match($_, '^(.*):(\d+):(.*)$')
        if (-not $location.Success) {
            throw "indirect client request evidence is invalid: $_"
        }
        if ($location.Groups[1].Value -match '[\\/]pinball[\\/]remote[\\/]') {
            [pscustomobject]@{
                File = $location.Groups[1].Value
                Line = [int]$location.Groups[2].Value
                Source = $location.Groups[3].Value
            }
        }
    }
    $routeLiteralPattern = [regex]'"([a-z][a-z0-9_]+(?:/[A-Za-z0-9_]+)+)"'
    $candidatePathsByFile = @{}
    foreach ($file in @($indirectLocations.File | Sort-Object -Unique)) {
        $source = Get-Content -LiteralPath $file -Raw -Encoding UTF8
        $candidatePaths = [System.Collections.Generic.List[string]]::new()
        foreach ($match in $routeLiteralPattern.Matches($source)) {
            $path = ConvertTo-GamePath -Path $match.Groups[1].Value
            if (-not $candidatePaths.Contains($path)) {
                $candidatePaths.Add($path)
            }
            if (-not $evidence.Contains($path)) {
                $evidence[$path] = [System.Collections.Generic.List[string]]::new()
            }
            $line = ([regex]::Matches($source.Substring(0, $match.Index), "`n")).Count + 1
            $evidence[$path].Add("${file}:$line")
        }
        $candidatePathsByFile[$file] = @($candidatePaths | Sort-Object)
    }
    foreach ($location in $indirectLocations) {
        $call = [regex]::Match(
            $location.Source,
            "$requestMethodPattern\((?<expression>[^,\r\n]+)"
        )
        $candidates = @($candidatePathsByFile[$location.File])
        $classification = if ($candidates.Count -gt 0) {
            'resolved-literal-set'
        } elseif ($location.File -match '[\\/]remote[\\/]debug[\\/]') {
            'caller-supplied-diagnostic-path'
        } else {
            'unresolved'
        }
        $runtimeRequests.Add([ordered]@{
            expression = if ($call.Success) { $call.Groups['expression'].Value.Trim() } else { '' }
            evidence = "$($location.File):$($location.Line)"
            classification = $classification
            candidates = $candidates
        })
    }
    [pscustomobject]@{
        Routes = $evidence
        RuntimeRequests = @($runtimeRequests)
    }
}
# //// /枚举反编译客户端请求及源码锚点 ////

# //// 读取参考路由审计结果 [@x380kkm 2026-08-24] ////
function Get-RouteAudit {
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string]$ServerRoot
    )

    $output = & node $ScriptPath --reference-root $ServerRoot --routes-only --report-only
    if ($LASTEXITCODE -ne 0) {
        throw "route coverage audit failed: exit=$LASTEXITCODE"
    }
    $output | ConvertFrom-Json -Depth 100
}
# //// /读取参考路由审计结果 ////

# //// 比较当前 iOS 请求与个人服务路由 [@x380kkm 2026-08-24] ////
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$workspaceRoot = Split-Path -Parent $repositoryRoot
if ([string]::IsNullOrWhiteSpace($ReferenceRoot)) {
    $ReferenceRoot = Join-Path $workspaceRoot 'startpoint-cn-launcher'
}
$referenceServerRoot = (Resolve-Path -LiteralPath (
    Join-Path $ReferenceRoot 'resources\server'
)).Path
$clientPath = (Resolve-Path -LiteralPath $ClientExecutable).Path
$decompiledPath = (Resolve-Path -LiteralPath $DecompiledRoot).Path
$routeAudit = Get-RouteAudit `
    -ScriptPath (Join-Path $PSScriptRoot 'audit-cn-reference-route-coverage.mjs') `
    -ServerRoot $referenceServerRoot
$referencePaths = @($routeAudit.covered.routes.path) | Sort-Object -Unique
$localPaths = @($referencePaths + $routeAudit.extra.routes.path) | Sort-Object -Unique
$requestSurface = Get-ClientRequestEvidence -Root $decompiledPath
$requestEvidence = $requestSurface.Routes
$binaryText = [System.Text.Encoding]::Latin1.GetString(
    [System.IO.File]::ReadAllBytes($clientPath)
)

$currentRoutes = foreach ($path in $requestEvidence.Keys) {
    $relativePath = $path -replace '^/api/index\.php/', ''
    $offset = $binaryText.IndexOf($relativePath, [System.StringComparison]::Ordinal)
    if ($offset -lt 0) {
        continue
    }
    [ordered]@{
        path = $path
        binary_offset = ('0x{0:x}' -f $offset)
        reference = $path -in $referencePaths
        local = $path -in $localPaths
        evidence = @($requestEvidence[$path])
    }
}
$currentRoutes = @($currentRoutes | Sort-Object path)
$missing = @($currentRoutes | Where-Object { -not $_.local })
$decompiledOnly = @($requestEvidence.Keys | Where-Object {
    $relativePath = $_ -replace '^/api/index\.php/', ''
    $binaryText.IndexOf($relativePath, [System.StringComparison]::Ordinal) -lt 0
} | Sort-Object)

[ordered]@{
    client_executable = $clientPath
    decompiled_root = $decompiledPath
    reference_root = $ReferenceRoot
    summary = [ordered]@{
        decompiled = $requestEvidence.Count
        current_ios = $currentRoutes.Count
        implemented = @($currentRoutes | Where-Object local).Count
        missing = $missing.Count
        decompiled_only = $decompiledOnly.Count
        runtime_calls = $requestSurface.RuntimeRequests.Count
        runtime_resolved = @($requestSurface.RuntimeRequests | Where-Object {
            $_.classification -eq 'resolved-literal-set'
        }).Count
        runtime_unresolved = @($requestSurface.RuntimeRequests | Where-Object {
            $_.classification -eq 'unresolved'
        }).Count
    }
    current_routes = $currentRoutes
    missing = $missing
    decompiled_only = $decompiledOnly
    runtime_requests = $requestSurface.RuntimeRequests
} | ConvertTo-Json -Depth 20

if ($missing.Count -gt 0 -and -not $AllowMissing) {
    exit 1
}
# //// /比较当前 iOS 请求与个人服务路由 ////
