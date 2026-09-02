# audience: external
# # personal-service-windows-runner
# 此脚本构建并启动 loopback 个人服务. 服务收到标准输入 stop 或 quit 后提交状态并退出.

[CmdletBinding()]
param(
    [string]$Root,
    [string]$CdnRoot,
    [int]$Port = 17171,
    [switch]$SkipBuild,
    [switch]$ShowManagementToken,
    [switch]$LogHttpAccess
)

$ErrorActionPreference = 'Stop'
$ScriptDirectory = $PSScriptRoot
$RepositoryRoot = (Resolve-Path (Join-Path $ScriptDirectory '..\..')).Path
$ServiceRoot = if ([string]::IsNullOrWhiteSpace($Root)) {
    Join-Path $RepositoryRoot 'data\personal-service'
} else {
    [System.IO.Path]::GetFullPath($Root)
}
$ConfiguredCdnRoot = if ([string]::IsNullOrWhiteSpace($CdnRoot)) {
    $env:STARPOINT_PERSONAL_SERVICE_CDN_ROOT
} else {
    $CdnRoot
}
$AssetRoot = if ([string]::IsNullOrWhiteSpace($ConfiguredCdnRoot)) {
    Join-Path $ServiceRoot 'cdn\cn'
} else {
    [System.IO.Path]::GetFullPath($ConfiguredCdnRoot)
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw '缺少命令: cargo.'
}
if ($Port -lt 0 -or $Port -gt 65535) {
    throw 'Port 必须在 0 到 65535 之间.'
}
New-Item -ItemType Directory -Force -Path $ServiceRoot | Out-Null
New-Item -ItemType Directory -Force -Path $AssetRoot | Out-Null

Push-Location $RepositoryRoot
try {
    if (-not $SkipBuild) {
        & cargo build --locked --release --manifest-path core/personal-service/Cargo.toml --bin personal-service
        if ($LASTEXITCODE -ne 0) { throw "personal-service 构建失败, 退出码 $LASTEXITCODE." }
    }
    $BinaryPath = Join-Path $RepositoryRoot 'core\personal-service\target\release\personal-service.exe'
    if (-not (Test-Path -LiteralPath $BinaryPath)) {
        throw "找不到个人服务二进制: $BinaryPath."
    }
    $Arguments = @('--root', $ServiceRoot, '--cdn-root', $AssetRoot, '--port', $Port.ToString())
    if ($ShowManagementToken) { $Arguments += '--show-management-token' }
    if ($LogHttpAccess) { $Arguments += '--log-http-access' }
    & $BinaryPath @Arguments
    if ($LASTEXITCODE -ne 0) { throw "个人服务退出码: $LASTEXITCODE." }
} finally {
    Pop-Location
}
