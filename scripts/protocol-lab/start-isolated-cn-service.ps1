# audience: external
# # start-isolated-cn-service
# 此脚本以前台可控的子进程启动隔离 CN 服务, 并把数据库, 管理数据库, 状态和安全 HTTP 元数据写入指定实验目录.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RunDirectory,
    [string]$RepositoryRoot,
    [ValidateRange(1, 65535)][int]$HttpPort = 8001,
    [ValidateRange(1, 65535)][int]$SessionPort = 8003,
    [string]$ServerHost = "10.0.2.2",
    [string]$CdnDirectory,
    [string]$ResourceVersion = "1.4.54",
    [Parameter(Mandatory)][string]$ManagementToken
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
$RepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
$RunDirectory = [IO.Path]::GetFullPath($RunDirectory)
if ([string]::IsNullOrWhiteSpace($CdnDirectory)) {
    $CdnDirectory = Join-Path $RepositoryRoot ".cdn"
}
$CdnDirectory = [IO.Path]::GetFullPath($CdnDirectory)
if ($ManagementToken.Length -lt 32) { throw "管理 token 长度不足." }
New-Item -ItemType Directory -Force -Path $RunDirectory | Out-Null
$statePath = Join-Path $RunDirectory "service-state.json"
if (Test-Path -LiteralPath $statePath) { throw "服务状态文件已存在: $statePath" }

$environment = @{
    LISTEN_HOST = "0.0.0.0"
    LISTEN_PORT = $HttpPort.ToString()
    SESSION_HOST = "0.0.0.0"
    SESSION_PORT = $SessionPort.ToString()
    SESSION_PUBLIC_HOST = $ServerHost
    CDN_DIR = $CdnDirectory
    CN_CDN_BASE_URL = "http://$ServerHost`:$HttpPort/patch/cn"
    CN_API_HOST = "$ServerHost`:$HttpPort"
    CN_API_SCHEME = "http"
    CN_RES_VERSION = $ResourceVersion
    CN_PROTOCOL_METADATA_LOG = (Join-Path $RunDirectory "cn-http-metadata.jsonl")
    DATABASE_PATH = (Join-Path $RunDirectory "starpoint.sqlite")
    MANAGEMENT_ACCESS_DATABASE_PATH = (Join-Path $RunDirectory "management-control.db")
    MANAGEMENT_STATE_FILE = (Join-Path $RunDirectory "management-state.json")
    MANAGEMENT_BACKUP_DIR = (Join-Path $RunDirectory "backups")
    MANAGEMENT_ADMIN_USERNAME = "admin"
    MANAGEMENT_ADMIN_PASSWORD = ([Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(24))).ToLowerInvariant()
    MANAGEMENT_ADMIN_TOKEN = $ManagementToken
}

$node = (Get-Command node -ErrorAction Stop).Source
$entryPath = [IO.Path]::GetFullPath((Join-Path $RepositoryRoot "out\start.js"))
$stdoutPath = Join-Path $RunDirectory "service.stdout.log"
$stderrPath = Join-Path $RunDirectory "service.stderr.log"
$argumentList = @("--env-file-if-exists=.env.cn", $entryPath)
$previousEnvironment = @{}
$environment.GetEnumerator() | ForEach-Object {
    $previousEnvironment[$_.Key] = [Environment]::GetEnvironmentVariable($_.Key, "Process")
    [Environment]::SetEnvironmentVariable($_.Key, $_.Value, "Process")
}
try {
    $process = Start-Process -FilePath $node -ArgumentList $argumentList -WorkingDirectory $RepositoryRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
} finally {
    foreach ($key in $previousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($key, $previousEnvironment[$key], "Process")
    }
}
$state = [ordered]@{
    SchemaVersion = 1
    ProcessId = $process.Id
    ProcessStartTimeUtc = $process.StartTime.ToUniversalTime().ToString("o")
    ExecutablePath = $node
    EntryPath = $entryPath
    RepositoryRoot = $RepositoryRoot
    RunDirectory = $RunDirectory
    HttpPort = $HttpPort
    SessionPort = $SessionPort
    DatabasePath = $environment.DATABASE_PATH
    MetadataPath = $environment.CN_PROTOCOL_METADATA_LOG
    StdoutPath = $stdoutPath
    StderrPath = $stderrPath
    StartedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
}
ConvertTo-Json $state -Depth 4 | Set-Content -LiteralPath $statePath -Encoding UTF8
Start-Sleep -Seconds 3
if ($process.HasExited) {
    $errorText = if (Test-Path -LiteralPath $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw -Encoding UTF8 } else { "" }
    Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
    throw "CN 服务启动失败: exit=$($process.ExitCode) $errorText"
}
try {
    $health = Invoke-WebRequest -Uri "http://127.0.0.1:$HttpPort/healthz" -UseBasicParsing -TimeoutSec 10
    if ($health.StatusCode -ne 200) { throw "health status=$($health.StatusCode)" }
} catch {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    Remove-Item -LiteralPath $statePath -Force -ErrorAction SilentlyContinue
    throw "CN 服务健康检查失败: $($_.Exception.Message)"
}
[pscustomobject]@{
    RunDirectory = $RunDirectory
    ProcessId = $process.Id
    HttpPort = $HttpPort
    SessionPort = $SessionPort
    DatabasePath = $state.DatabasePath
    MetadataPath = $state.MetadataPath
    StatePath = $statePath
}
