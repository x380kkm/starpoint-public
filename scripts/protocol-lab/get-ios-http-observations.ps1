# audience: external
# # get-ios-http-observations
# 此脚本从 USB iPhone 的 Starpoint App 容器取得 HTTP, AIR 和 SDK 观察记录.
# 输出保留原始 HTTP 状态历史, 并另列窗口内最新状态和客户端错误.

[CmdletBinding()]
param(
    [ValidatePattern('^[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$')]
    [string]$BundleId = 'dev.starpoint.worldflipper.cn.local',

    [string]$Udid,

    [string]$OutputDirectory,

    [Alias('Baseline')]
    [string]$BaselineDirectory,

    [string]$StartedAt,

    [string]$EndedAt,

    [ValidatePattern('^[+-]\d{2}:\d{2}$')]
    [string]$LocalTimezone = '+08:00'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$artifactRoot = Join-Path $repositoryRoot 'artifacts\ios-device-live'
$analyzerPath = Join-Path $PSScriptRoot 'analyze-ios-device-observations.py'
$remoteDatabaseRoot = 'Library/Application Support/StarpointPersonalService'
$remoteClientRoot = 'Library/Application Support/dev.starpoint.worldflipper.cn.local/Local Store/custom_Release_Ios'
$databaseFileNames = @(
    'personal-service.sqlite3',
    'personal-service.sqlite3-wal',
    'personal-service.sqlite3-shm'
)


# //// 解析唯一 USB iPhone [@x380kkm 2026-08-27] ////
function Resolve-IosDeviceUdid {
    param([string]$RequestedUdid)

    if (-not [string]::IsNullOrWhiteSpace($RequestedUdid)) {
        return $RequestedUdid
    }
    $deviceJson = & pymobiledevice3 usbmux list --usb --simple
    if ($LASTEXITCODE -ne 0) { throw '无法枚举 USB iPhone.' }
    $devices = @($deviceJson | ConvertFrom-Json)
    if ($devices.Count -ne 1) {
        throw "需要连接一台 USB iPhone, 当前检测到 $($devices.Count) 台."
    }
    [string]$devices[0]
}
# //// /解析唯一 USB iPhone ////


# //// 在 App 容器执行 AFC 命令 [@x380kkm 2026-08-27] ////
function Invoke-IosAfcCommands {
    param(
        [Parameter(Mandatory)][string]$DeviceUdid,
        [Parameter(Mandatory)][string[]]$Commands
    )

    $previousAllUsersProfile = $env:ALLUSERSPROFILE
    try {
        if ([string]::IsNullOrWhiteSpace($env:ALLUSERSPROFILE)) {
            $env:ALLUSERSPROFILE = $env:ProgramData
        }
        $output = @($Commands + 'exit') |
            & pymobiledevice3 apps afc $BundleId --udid $DeviceUdid 2>&1
        $exitCode = $LASTEXITCODE
    } finally {
        $env:ALLUSERSPROFILE = $previousAllUsersProfile
    }
    [pscustomobject]@{
        exit_code = $exitCode
        output = @($output)
    }
}


function ConvertTo-AfcPath {
    param([Parameter(Mandatory)][string]$Path)

    $normalized = $Path.Replace('\', '/')
    if ($normalized.Contains("'")) { throw 'AFC 路径不能包含单引号.' }
    $normalized
}
# //// /在 App 容器执行 AFC 命令 ////


# //// 拉取必需数据库和可选客户端诊断 [@x380kkm 2026-08-27] ////
function Copy-IosRequiredDatabase {
    param(
        [Parameter(Mandatory)][string]$DeviceUdid,
        [Parameter(Mandatory)][string]$DestinationDirectory
    )

    $commands = foreach ($fileName in $databaseFileNames) {
        $localPath = ConvertTo-AfcPath (Join-Path $DestinationDirectory $fileName)
        "pull '$remoteDatabaseRoot/$fileName' '$localPath'"
    }
    $result = Invoke-IosAfcCommands -DeviceUdid $DeviceUdid -Commands $commands
    [IO.File]::WriteAllLines(
        (Join-Path $DestinationDirectory 'afc-database.log'),
        [string[]]$result.output,
        [Text.UTF8Encoding]::new($false)
    )
    $captureResults = foreach ($fileName in $databaseFileNames) {
        $destination = Join-Path $DestinationDirectory $fileName
        [pscustomobject]@{
            remote_path = "$remoteDatabaseRoot/$fileName"
            destination = $destination
            copied = Test-Path -LiteralPath $destination -PathType Leaf
            exit_code = $result.exit_code
        }
    }
    if (-not $captureResults[0].copied) {
        throw '无法从 App 容器取得观察数据库.'
    }
    $captureResults
}


function Copy-IosOptionalDiagnostic {
    param(
        [Parameter(Mandatory)][string]$DeviceUdid,
        [Parameter(Mandatory)][string]$RemotePath,
        [Parameter(Mandatory)][string]$DestinationPath,
        [Parameter(Mandatory)][string]$LogPath
    )

    $localPath = ConvertTo-AfcPath $DestinationPath
    $result = Invoke-IosAfcCommands -DeviceUdid $DeviceUdid -Commands @(
        "pull '$RemotePath' '$localPath'"
    )
    [IO.File]::WriteAllLines(
        $LogPath,
        [string[]]$result.output,
        [Text.UTF8Encoding]::new($false)
    )
    [pscustomobject]@{
        remote_path = $RemotePath
        destination = $DestinationPath
        copied = $result.exit_code -eq 0 -and (Test-Path -LiteralPath $DestinationPath)
        exit_code = $result.exit_code
    }
}
# //// /拉取必需数据库和可选客户端诊断 ////


# //// 固定 AIR 最新报告文件名 [@x380kkm 2026-08-27] ////
function Copy-LatestAirReportFiles {
    param(
        [Parameter(Mandatory)][string]$ReportRoot,
        [Parameter(Mandatory)][string]$DestinationDirectory
    )

    $latestDirectory = Get-ChildItem -LiteralPath $ReportRoot -Recurse -Directory |
        Where-Object { $_.Name -match '^\d{4}_\d{2}_\d{2}_\d{2}_\d{2}_\d{2}_\d{3}$' } |
        Sort-Object Name -Descending |
        Where-Object {
            (Test-Path -LiteralPath (Join-Path $_.FullName 'info.json') -PathType Leaf) -or
            (Test-Path -LiteralPath (Join-Path $_.FullName 'replay.log') -PathType Leaf)
        } |
        Select-Object -First 1
    if ($null -eq $latestDirectory) { return @() }

    $latestInfo = Get-Item -LiteralPath (Join-Path $latestDirectory.FullName 'info.json') -ErrorAction SilentlyContinue
    $latestReplay = Get-Item -LiteralPath (Join-Path $latestDirectory.FullName 'replay.log') -ErrorAction SilentlyContinue

    $results = @()
    if ($null -ne $latestInfo) {
        $latestInfoDestination = Join-Path $DestinationDirectory 'latest-info.json'
        Copy-Item -LiteralPath $latestInfo.FullName -Destination $latestInfoDestination -Force
        $results += [pscustomobject]@{
            remote_path = "derived:latest-report/$($latestDirectory.Name)/info.json"
            destination = $latestInfoDestination
            copied = $true
            exit_code = 0
        }
    }

    if ($null -ne $latestReplay) {
        $latestReplayDestination = Join-Path $DestinationDirectory 'latest-replay.log'
        Copy-Item -LiteralPath $latestReplay.FullName -Destination $latestReplayDestination -Force
        $results += [pscustomobject]@{
            remote_path = "derived:latest-report/$($latestDirectory.Name)/replay.log"
            destination = $latestReplayDestination
            copied = $true
            exit_code = 0
        }
    }
    $results
}
# //// /固定 AIR 最新报告文件名 ////


# //// 导出合并后的观察报告 [@x380kkm 2026-08-27] ////
$deviceUdid = Resolve-IosDeviceUdid -RequestedUdid $Udid
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $runName = "$(Get-Date -Format 'yyyyMMdd-HHmmss')-ios-observations"
    $OutputDirectory = Join-Path $artifactRoot $runName
}
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
$clientRoot = Join-Path $outputRoot 'client'
New-Item -ItemType Directory -Path $clientRoot -Force | Out-Null

$captureStartedAt = [DateTimeOffset]::UtcNow.ToString('o')

$baselineRoot = if ([string]::IsNullOrWhiteSpace($BaselineDirectory)) {
    $null
} else {
    $resolved = [IO.Path]::GetFullPath($BaselineDirectory)
    if (-not (Test-Path -LiteralPath $resolved)) {
        throw "基线目录不存在: $resolved"
    }
    $resolved
}

$databaseResults = @(
    Copy-IosRequiredDatabase `
        -DeviceUdid $deviceUdid `
        -DestinationDirectory $outputRoot
)
$captureResults = @(
    $databaseResults
    Copy-IosOptionalDiagnostic `
        -DeviceUdid $deviceUdid `
        -RemotePath "$remoteClientRoot/latest.log" `
        -DestinationPath (Join-Path $clientRoot 'latest.log') `
        -LogPath (Join-Path $outputRoot 'afc-latest.log')
    Copy-IosOptionalDiagnostic `
        -DeviceUdid $deviceUdid `
        -RemotePath "$remoteClientRoot/latest-replay.log" `
        -DestinationPath (Join-Path $clientRoot 'latest-replay.log') `
        -LogPath (Join-Path $outputRoot 'afc-latest-replay.log')
    Copy-IosOptionalDiagnostic `
        -DeviceUdid $deviceUdid `
        -RemotePath "$remoteClientRoot/latest-info.json" `
        -DestinationPath (Join-Path $clientRoot 'latest-info.json') `
        -LogPath (Join-Path $outputRoot 'afc-latest-info.log')
    Copy-IosOptionalDiagnostic `
        -DeviceUdid $deviceUdid `
        -RemotePath "$remoteClientRoot/report" `
        -DestinationPath (Join-Path $clientRoot 'reports') `
        -LogPath (Join-Path $outputRoot 'afc-reports.log')
    Copy-IosOptionalDiagnostic `
        -DeviceUdid $deviceUdid `
        -RemotePath 'Documents/SobotLog' `
        -DestinationPath (Join-Path $clientRoot 'SobotLog') `
        -LogPath (Join-Path $outputRoot 'afc-sobot.log')
)
$reportRoot = Join-Path $clientRoot 'reports'
if (Test-Path -LiteralPath $reportRoot -PathType Container) {
    $captureResults += @(
        Copy-LatestAirReportFiles `
            -ReportRoot $reportRoot `
            -DestinationDirectory $clientRoot
    )
}

$jsonEncoding = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText(
    (Join-Path $outputRoot 'capture-files.json'),
    ($captureResults | ConvertTo-Json -AsArray -Depth 4),
    $jsonEncoding
)

$analysisEndedAt = if ([string]::IsNullOrWhiteSpace($EndedAt)) {
    [DateTimeOffset]::UtcNow.ToString('o')
} else {
    $EndedAt
}
$effectiveStartedAt = if (-not [string]::IsNullOrWhiteSpace($StartedAt)) {
    $StartedAt
} else {
    $null
}
[IO.File]::WriteAllText(
    (Join-Path $outputRoot 'capture-metadata.json'),
    ([ordered]@{
        device_udid = $deviceUdid
        bundle_id = $BundleId
        capture_started_at = $captureStartedAt
        captured_at = $analysisEndedAt
        started_at = $effectiveStartedAt
        baseline_root = $baselineRoot
    } | ConvertTo-Json -Depth 4),
    $jsonEncoding
)
$analyzerArguments = @(
    $outputRoot,
    '--output-root', $outputRoot,
    '--ended-at', $analysisEndedAt,
    '--local-timezone', $LocalTimezone
)
if ($null -ne $effectiveStartedAt) {
    $analyzerArguments += @('--started-at', $effectiveStartedAt)
}
if ($null -ne $baselineRoot) {
    $analyzerArguments += @('--baseline-root', $baselineRoot)
}
$reportJson = & uv run --python 3.12 $analyzerPath @analyzerArguments
if ($LASTEXITCODE -ne 0) { throw '无法分析 iPhone 观察记录.' }
$report = $reportJson | ConvertFrom-Json

Write-Output "设备: $deviceUdid"
$displayStartedAt = if ([string]::IsNullOrWhiteSpace([string]$report.started_at)) { '全部历史' } else { [string]$report.started_at }
Write-Output "筛选: $($report.selection_mode), $displayStartedAt -> $($report.ended_at)"
Write-Output "HTTP: 当前错误 $($report.http_current_error_count), 已恢复 $($report.http_recovered_count), 599 传输失败 $($report.transport_current_failure_count)"
Write-Output "客户端: 内部错误 $($report.client_internal_error_count), ATS 请求前阻断 $($report.ats_pre_request_count), 其他 $($report.client_error_count - $report.client_internal_error_count - $report.ats_pre_request_count)"
Write-Output "结论: $($report.status)"
Write-Output "输出: $outputRoot"

if ($report.latest_failures.Count -gt 0) {
    Write-Output ''
    Write-Output '最近错误:'
    $report.latest_failures |
        Select-Object occurred_at, kind, code, message |
        Format-Table -AutoSize |
        Out-String -Width 240 |
        Write-Output
}
# //// /导出合并后的观察报告 ////
