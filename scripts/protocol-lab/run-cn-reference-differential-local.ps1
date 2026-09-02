# audience: internal
# # cn-reference-differential-local
# 该脚本在隔离目录中启动参考服务和个人服务, 执行动态差分并保留报告与服务日志.

[CmdletBinding()]
param(
    [string]$ReferenceRoot,
    [string]$ReportPath,
    [int]$RequestTimeoutMs = 30000
)

$ErrorActionPreference = 'Stop'

# //// 分配本机空闲端口 [@x380kkm 2026-08-23] ////
function Get-FreeTcpPort {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try {
        return ([Net.IPEndPoint]$listener.LocalEndpoint).Port
    }
    finally {
        $listener.Stop()
    }
}
# //// /分配本机空闲端口 ////

# //// 等待无头服务就绪 [@x380kkm 2026-08-23] ////
function Wait-ServiceReady {
    param(
        [Diagnostics.Process]$Process,
        [string]$Uri,
        [scriptblock]$Accept
    )

    for ($attempt = 0; $attempt -lt 100; $attempt += 1) {
        if ($Process.HasExited) {
            throw "service exited before becoming ready: $Uri"
        }
        try {
            $response = Invoke-WebRequest -Uri $Uri -TimeoutSec 1
            if (& $Accept $response) {
                return
            }
        }
        catch {
            Start-Sleep -Milliseconds 100
        }
    }
    throw "service did not become ready: $Uri"
}
# //// /等待无头服务就绪 ////

# //// 启动双服务并执行隔离差分 [@x380kkm 2026-08-23] ////
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$workspaceRoot = Split-Path -Parent $repositoryRoot
$serviceRoot = Join-Path $repositoryRoot 'core\personal-service'
$artifactRoot = Join-Path $repositoryRoot 'artifacts\differential'
$runRoot = Join-Path $artifactRoot ('local-' + [guid]::NewGuid().ToString('N'))
$buildTargetRoot = Join-Path $artifactRoot 'target'

# //// 编译当前个人服务 [@x380kkm 2026-08-23] ////
Push-Location $serviceRoot
try {
    & cargo build --target-dir $buildTargetRoot
    if ($LASTEXITCODE -ne 0) {
        throw "personal service build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}
# //// /编译当前个人服务 ////

if ([string]::IsNullOrWhiteSpace($ReferenceRoot)) {
    $ReferenceRoot = Join-Path $workspaceRoot 'startpoint-cn-launcher'
}
$ReferenceRoot = (Resolve-Path -LiteralPath $ReferenceRoot).Path
$referenceRunRoot = Join-Path $runRoot 'reference'
$referenceDatabaseRoot = Join-Path $referenceRunRoot 'db'
$rustRunRoot = Join-Path $runRoot 'rust'
New-Item -ItemType Directory -Path $referenceDatabaseRoot, $rustRunRoot | Out-Null

$referencePort = Get-FreeTcpPort
$sessionPort = Get-FreeTcpPort
$rustPort = Get-FreeTcpPort
$referenceBaseUrl = "http://127.0.0.1:$referencePort"
$rustBaseUrl = "http://127.0.0.1:$rustPort"
$referenceServerRoot = Join-Path $ReferenceRoot 'resources\server'
$referenceNode = Join-Path $ReferenceRoot 'resources\node\node.exe'
$referenceClockPath = Join-Path $PSScriptRoot 'reference-virtual-clock.cjs'
$cdnContainerRoot = Join-Path $workspaceRoot 'startpoint-cn\.cdn'
$cdnRoot = Join-Path $cdnContainerRoot 'cn'
$builtRustExecutable = Join-Path $buildTargetRoot 'debug\personal-service.exe'
$rustExecutable = Join-Path $rustRunRoot 'personal-service.exe'
$corpusPath = Join-Path $PSScriptRoot 'cn-reference-differential-corpus.json'
$generatorPath = Join-Path $PSScriptRoot 'generate-cn-reference-differential-corpus.mjs'
$runnerPath = Join-Path $PSScriptRoot 'run-cn-reference-differential.mjs'
if ([string]::IsNullOrWhiteSpace($ReportPath)) {
    $ReportPath = Join-Path $runRoot 'report.json'
}
else {
    $ReportPath = [IO.Path]::GetFullPath($ReportPath, $repositoryRoot)
}
Copy-Item -LiteralPath $builtRustExecutable -Destination $rustExecutable

# //// 校验参考差分语料与当前路由集合一致 [@x380kkm 2026-08-23] ////
& node $generatorPath --check | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "reference differential corpus is stale"
}
& node $runnerPath --check-corpus --corpus $corpusPath | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "reference differential corpus is invalid"
}
# //// /校验参考差分语料与当前路由集合一致 ////

$referenceEnvironment = @{
    DB_DIR = $referenceDatabaseRoot
    CDN_DIR = $cdnContainerRoot
    CN_DIFFERENTIAL_NOW = '2024-07-23T12:00:00.000Z'
    CN_LISTEN_HOST = '127.0.0.1'
    CN_LISTEN_PORT = [string]$referencePort
    CN_PUBLIC_HOST = '127.0.0.1'
    NODE_OPTIONS = "--require=$referenceClockPath"
    SESSION_HOST = '127.0.0.1'
    SESSION_PORT = [string]$sessionPort
    SESSION_PUBLIC_HOST = '127.0.0.1'
}
$rustEnvironment = @{
    CN_REFERENCE_PLAYER_BASELINE = '1'
}
$reference = Start-Process -FilePath $referenceNode -ArgumentList 'out/cn-server.js' `
    -WorkingDirectory $referenceServerRoot -Environment $referenceEnvironment `
    -RedirectStandardOutput (Join-Path $referenceRunRoot 'stdout.log') `
    -RedirectStandardError (Join-Path $referenceRunRoot 'stderr.log') `
    -WindowStyle Hidden -PassThru
$rust = Start-Process -FilePath $rustExecutable `
    -ArgumentList @('--root', $rustRunRoot, '--cdn-root', $cdnRoot, '--port', [string]$rustPort, '--session-port', '0') `
    -Environment $rustEnvironment `
    -RedirectStandardOutput (Join-Path $rustRunRoot 'stdout.log') `
    -RedirectStandardError (Join-Path $rustRunRoot 'stderr.log') `
    -WindowStyle Hidden -PassThru

$runnerExitCode = 2
try {
    Wait-ServiceReady -Process $reference -Uri "$referenceBaseUrl/debug" `
        -Accept { param($response) $response.StatusCode -eq 200 -and $response.Content -eq 'OK' }
    Wait-ServiceReady -Process $rust -Uri "$rustBaseUrl/health" `
        -Accept { param($response) $response.StatusCode -eq 200 }

    Invoke-RestMethod -Method Get `
        -Uri "$referenceBaseUrl/api/server/time?time=2024-07-23T12:00:00Z" | Out-Null

    $timeBody = @{
        enabled = $true
        iso = '2024-07-23T12:00:00.000Z'
        rate = 1.0
    } | ConvertTo-Json -Compress
    Invoke-RestMethod -Method Put -Uri "$rustBaseUrl/v1/time" `
        -ContentType 'application/json' -Body $timeBody | Out-Null

    & node $runnerPath --reference-base-url $referenceBaseUrl --rust-base-url $rustBaseUrl `
        --corpus $corpusPath --report $ReportPath --timeout-ms $RequestTimeoutMs | Out-Null
    $runnerExitCode = $LASTEXITCODE
}
finally {
    foreach ($process in @($reference, $rust)) {
        if ($null -ne $process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id
            $process.WaitForExit()
        }
    }
}

$report = Get-Content -LiteralPath $ReportPath -Raw -Encoding UTF8 | ConvertFrom-Json
[pscustomobject]@{
    report = $ReportPath
    run_root = $runRoot
    summary = $report.summary
    exit_code = $runnerExitCode
} | ConvertTo-Json -Depth 5
exit $runnerExitCode
# //// /启动双服务并执行隔离差分 ////
