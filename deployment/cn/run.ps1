# audience: external
# # cn-windows-runner
# 此脚本准备 CN CDN, 检查 Windows 运行环境, 安装依赖, 构建项目, 按请求探测本地服务并以前台进程启动 CN 服务.

[CmdletBinding()]
param(
    [string]$CdnDirectory,
    [string]$DownloadDirectory,
    [switch]$SkipInstall,
    [switch]$SkipCdnDownload,
    [switch]$KeepCdnParts,
    [switch]$AdoptExistingCdn,
    [switch]$ValidateOnly,
    [switch]$HealthCheck
)

$ErrorActionPreference = 'Stop'
$ScriptDirectory = $PSScriptRoot
$RepositoryRoot = (Resolve-Path (Join-Path $ScriptDirectory '..\..')).Path
$EnvironmentFile = Join-Path $RepositoryRoot '.env.cn'
if ($PSBoundParameters.ContainsKey('CdnDirectory') -and [string]::IsNullOrWhiteSpace($CdnDirectory)) {
    throw 'CdnDirectory 不能为空.'
}
if ($PSBoundParameters.ContainsKey('DownloadDirectory') -and [string]::IsNullOrWhiteSpace($DownloadDirectory)) {
    throw 'DownloadDirectory 不能为空.'
}
if (-not $PSBoundParameters.ContainsKey('CdnDirectory')) {
    $CdnDirectory = Join-Path $RepositoryRoot '.cdn'
}
$CdnDirectory = [System.IO.Path]::GetFullPath($CdnDirectory)

# //// 检查 Windows CN 服务运行条件 [@x380kkm 2026-07-22] ////
foreach ($CommandName in @('node', 'npm', 'tar')) {
    if (-not (Get-Command $CommandName -ErrorAction SilentlyContinue)) {
        throw "缺少命令: $CommandName."
    }
}
$NodeVersion = [version](& node -p 'process.versions.node')
if ($NodeVersion -lt [version]'20.6.0') {
    throw "需要 Node.js 20.6.0 或更高版本. 当前版本: $NodeVersion."
}
$ManifestPath = Join-Path $ScriptDirectory 'cdn-manifest.json'
$CnResourceVersion = & node -e "const fs=require('node:fs'); process.stdout.write(JSON.parse(fs.readFileSync(process.argv[1])).resVersion)" $ManifestPath
# //// /检查 Windows CN 服务运行条件 ////

# //// 生成部署环境使用的随机十六进制凭据 [@x380kkm 2026-07-22] ////
function New-RandomHexSecret {
    param([int]$ByteCount)

    $Bytes = New-Object byte[] $ByteCount
    $Random = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $Random.GetBytes($Bytes)
    }
    finally {
        $Random.Dispose()
    }
    return -join ($Bytes | ForEach-Object { $_.ToString('x2') })
}
# //// /生成部署环境使用的随机十六进制凭据 ////

# //// 同步部署环境中的 CN CDN 配置 [@x380kkm 2026-07-27] ////
function Set-CnCdnEnvironmentValues {
    param(
        [string]$Path,
        [string]$CdnDirectory,
        [string]$ResourceVersion
    )

    $EnvironmentText = if (Test-Path -LiteralPath $Path) {
        [System.IO.File]::ReadAllText($Path)
    }
    else {
        ''
    }
    $ManagedValues = @(
        [pscustomobject]@{ Name = 'CDN_DIR'; Value = $CdnDirectory },
        [pscustomobject]@{ Name = 'CN_RES_VERSION'; Value = $ResourceVersion }
    )
    foreach ($ManagedValue in $ManagedValues) {
        $Pattern = "(?m)^$([regex]::Escape($ManagedValue.Name))=.*(?:\r?\n|$)"
        $EnvironmentText = [regex]::Replace($EnvironmentText, $Pattern, '')
    }
    if ($EnvironmentText.Length -gt 0 -and -not $EnvironmentText.EndsWith([Environment]::NewLine)) {
        $EnvironmentText += [Environment]::NewLine
    }
    $EnvironmentText += ($ManagedValues | ForEach-Object {
        "$($_.Name)=$($_.Value)"
    }) -join [Environment]::NewLine
    $EnvironmentText += [Environment]::NewLine
    $Utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, $EnvironmentText, $Utf8WithoutBom)
}
# //// /同步部署环境中的 CN CDN 配置 ////

# //// 探测本地 CN HTTP 服务健康状态 [@x380kkm 2026-07-24] ////
function Invoke-LocalHealthProbe {
    $ProbeScript = @'
const configuredPort = Number.parseInt(process.env.LISTEN_PORT ?? '', 10);
const port = Number.isNaN(configuredPort) ? 8000 : configuredPort;
const healthUrl = `http://127.0.0.1:${port}/healthz`;
const healthProbeTimeoutMilliseconds = 5000;

(async () => {
    const response = await fetch(healthUrl, { signal: AbortSignal.timeout(healthProbeTimeoutMilliseconds) });
    const body = await response.json();
    const isExpectedService = response.ok
        && body.status === 'ok'
        && body.service === 'starpoint'
        && body.httpPort === port
        && Number.isInteger(body.sessionPort);
    if (!isExpectedService) {
        throw new Error(`unexpected health response (HTTP ${response.status})`);
    }
    process.stdout.write(healthUrl);
})().catch((error) => {
    console.error(`CN health probe failed for ${healthUrl}: ${error.message}`);
    process.exit(1);
});
'@
    $HealthUrl = & node "--env-file=$EnvironmentFile" -e $ProbeScript
    if ($LASTEXITCODE -ne 0) {
        throw "CN 本地健康探针失败, 退出码 $LASTEXITCODE."
    }
    return $HealthUrl
}
# //// /探测本地 CN HTTP 服务健康状态 ////

# //// 准备 Windows CN CDN [@x380kkm 2026-07-22] ////
$PrepareArguments = @(
    (Join-Path $ScriptDirectory 'prepare-cdn.mjs'),
    '--cdn-dir',
    $CdnDirectory
)
if ($DownloadDirectory) {
    $PrepareArguments += @('--download-dir', [System.IO.Path]::GetFullPath($DownloadDirectory))
}
if ($SkipCdnDownload) {
    $PrepareArguments += '--skip-download'
}
if ($KeepCdnParts) {
    $PrepareArguments += '--keep-parts'
}
if ($AdoptExistingCdn) {
    $PrepareArguments += '--adopt-existing'
}
if ($ValidateOnly) {
    $PrepareArguments += '--validate-existing'
}
& node @PrepareArguments
if ($LASTEXITCODE -ne 0) {
    throw "CN CDN 准备失败, 退出码 $LASTEXITCODE."
}
if ($ValidateOnly) {
    Write-Output "CN CDN 完整性预检通过: $CdnDirectory"
    exit 0
}
# //// /准备 Windows CN CDN ////

# //// 创建 Windows CN 私有环境文件 [@x380kkm 2026-07-22] ////
if (-not (Test-Path -LiteralPath $EnvironmentFile)) {
    $AdminToken = New-RandomHexSecret -ByteCount 32
    $AdminPassword = New-RandomHexSecret -ByteCount 24
    $CdnValue = $CdnDirectory.Replace('\', '/')
    $EnvironmentLines = @(
        'LISTEN_HOST=0.0.0.0',
        'LISTEN_PORT=8001',
        "CDN_DIR=$CdnValue",
        "CN_RES_VERSION=$CnResourceVersion",
        "MANAGEMENT_ADMIN_TOKEN=$AdminToken",
        'MANAGEMENT_ADMIN_USERNAME=admin',
        "MANAGEMENT_ADMIN_PASSWORD=$AdminPassword"
    )
    $Utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText(
        $EnvironmentFile,
        (($EnvironmentLines -join [Environment]::NewLine) + [Environment]::NewLine),
        $Utf8WithoutBom
    )
}
else {
    $EnvironmentText = [System.IO.File]::ReadAllText($EnvironmentFile)
    if ($EnvironmentText -notmatch '(?m)^MANAGEMENT_ADMIN_PASSWORD=.+$') {
        $AdminPassword = New-RandomHexSecret -ByteCount 24
        $AdditionalLines = @()
        if ($EnvironmentText -notmatch '(?m)^MANAGEMENT_ADMIN_USERNAME=') {
            $AdditionalLines += 'MANAGEMENT_ADMIN_USERNAME=admin'
        }
        $AdditionalLines += "MANAGEMENT_ADMIN_PASSWORD=$AdminPassword"
        $Prefix = if ($EnvironmentText.EndsWith("`n")) { '' } else { [Environment]::NewLine }
        $Utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::AppendAllText(
            $EnvironmentFile,
            ($Prefix + ($AdditionalLines -join [Environment]::NewLine) + [Environment]::NewLine),
            $Utf8WithoutBom
        )
    }
}
$CdnValue = $CdnDirectory.Replace('\', '/')
Set-CnCdnEnvironmentValues -Path $EnvironmentFile -CdnDirectory $CdnValue -ResourceVersion $CnResourceVersion
# //// /创建 Windows CN 私有环境文件 ////

# //// 构建, 探测并启动 Windows CN 服务 [@x380kkm 2026-07-24] ////
$PreviousCdnDirectory = $env:CDN_DIR
$PreviousCnResourceVersion = $env:CN_RES_VERSION
Push-Location $RepositoryRoot
try {
    if (-not $SkipInstall) {
        & npm ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci 失败, 退出码 $LASTEXITCODE."
        }
    }
    & npm run build
    if ($LASTEXITCODE -ne 0) {
        throw "npm run build 失败, 退出码 $LASTEXITCODE."
    }

    if ($HealthCheck) {
        $HealthUrl = Invoke-LocalHealthProbe
        Write-Output "CN 本地健康探针通过: $HealthUrl"
        exit 0
    }

    if ($ValidateOnly) {
        Write-Output 'CN 部署校验通过.'
        exit 0
    }

    Write-Output 'CN 服务: http://127.0.0.1:8001'
    Write-Output '管理页面: http://127.0.0.1:8001/manage'
    Write-Output "管理登录凭据和环境变量: $EnvironmentFile"
    $env:CDN_DIR = $CdnDirectory
    $env:CN_RES_VERSION = $CnResourceVersion
    & node "--env-file=$EnvironmentFile" out/start.js
    exit $LASTEXITCODE
}
finally {
    if ($null -eq $PreviousCdnDirectory) {
        Remove-Item Env:CDN_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:CDN_DIR = $PreviousCdnDirectory
    }
    if ($null -eq $PreviousCnResourceVersion) {
        Remove-Item Env:CN_RES_VERSION -ErrorAction SilentlyContinue
    }
    else {
        $env:CN_RES_VERSION = $PreviousCnResourceVersion
    }
    Pop-Location
}
# //// /构建, 探测并启动 Windows CN 服务 ////
