# audience: internal
# # capture-android-cn-gacha-headless
# 此脚本在专用 Android Emulator 中执行 CN 客户端的无窗口启动, 下载和教程跳过交互, 并保存屏幕, logcat 和 HTTP observation.
# APK 必须已经把版本地址和 API 地址指向调用方提供的本地隔离服务, 且模拟器使用 1080x2400 分辨率.

[CmdletBinding()]
param(
    [string]$RepositoryRoot,
    [string]$SdkRoot,
    [string]$Serial = "emulator-5554",
    [Parameter(Mandatory)][string]$ApkPath,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$MetadataPath,
    [string]$ServiceBaseUrl = "http://127.0.0.1:8001",
    [Parameter(Mandatory)][string]$ManagementToken,
    [string]$VirtualTimeIso = "2025-07-10T00:00:00.000Z",
    [switch]$KeepInstalledData
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
$RepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
$WorkspaceRoot = [IO.Path]::GetFullPath((Split-Path -Parent $RepositoryRoot))
if ([string]::IsNullOrWhiteSpace($SdkRoot)) {
    $SdkRoot = Join-Path $WorkspaceRoot "artifacts\android-sdk\sdk"
}
$AdbPath = [IO.Path]::GetFullPath((Join-Path $SdkRoot "platform-tools\adb.exe"))
$ApkPath = [IO.Path]::GetFullPath($ApkPath)
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$MetadataPath = [IO.Path]::GetFullPath($MetadataPath)
$PackageName = "com.leiting.wf"
$RemoteScreenshotPath = "/sdcard/starpoint-gacha-headless.png"

if (-not (Test-Path -LiteralPath $AdbPath -PathType Leaf)) { throw "ADB 不存在: $AdbPath" }
if (-not (Test-Path -LiteralPath $ApkPath -PathType Leaf)) { throw "APK 不存在: $ApkPath" }
if ($Serial -notmatch '^emulator-[0-9]+$') { throw "场景只接受专用 Emulator serial: $Serial" }
$ServiceUri = [Uri]$ServiceBaseUrl
if (-not $ServiceUri.IsLoopback -or $ServiceUri.Scheme -ne "http") {
    throw "隔离服务必须使用本机 HTTP loopback 地址: $ServiceBaseUrl"
}
if ([string]::IsNullOrWhiteSpace($ManagementToken)) { throw "管理 token 不能为空." }
if ([DateTimeOffset]::MinValue -eq [DateTimeOffset]::Parse($VirtualTimeIso, [Globalization.CultureInfo]::InvariantCulture)) {
    throw "虚拟时间无效: $VirtualTimeIso"
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

# //// 执行一个 ADB 命令并拒绝非零退出码 [@x380kkm 2026-08-22] ////
function Invoke-ScenarioAdb {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $output = @(& $AdbPath -s $Serial @Arguments 2>&1 | ForEach-Object { $_.ToString() })
    if ($LASTEXITCODE -ne 0) {
        throw "ADB 命令失败: $($Arguments -join ' '); $($output -join ' ')"
    }
    $output
}

# //// 保存当前模拟器屏幕并返回证据路径 [@x380kkm 2026-08-22] ////
function Save-ScenarioScreenshot {
    param([Parameter(Mandatory)][string]$Name)

    $targetPath = Join-Path $OutputDirectory ("{0}.png" -f $Name)
    Invoke-ScenarioAdb -Arguments @("shell", "screencap", "-p", $RemoteScreenshotPath) | Out-Null
    Invoke-ScenarioAdb -Arguments @("pull", $RemoteScreenshotPath, $targetPath) | Out-Null
    $script:ScreenshotPaths.Add($targetPath)
    $targetPath
}

# //// 点击一个游戏坐标并等待画面提交 [@x380kkm 2026-08-22] ////
function Invoke-ScenarioTap {
    param(
        [Parameter(Mandatory)][int]$X,
        [Parameter(Mandatory)][int]$Y,
        [ValidateRange(1, 59)][int]$WaitSeconds = 6
    )

    Invoke-ScenarioAdb -Arguments @("shell", "input", "tap", $X.ToString(), $Y.ToString()) | Out-Null
    Start-Sleep -Seconds $WaitSeconds
}

# //// 执行无头客户端场景并生成证据报告 [@x380kkm 2026-08-22] ////
$startedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
$metadataLineCount = if (Test-Path -LiteralPath $MetadataPath -PathType Leaf) {
    @(Get-Content -LiteralPath $MetadataPath -Encoding UTF8).Count
} else {
    0
}
$ScreenshotPaths = [Collections.Generic.List[string]]::new()

$headers = @{ Authorization = "Bearer $ManagementToken" }
$timeBody = @{ enabled = $true; iso = $VirtualTimeIso; rate = 1 } | ConvertTo-Json -Compress
Invoke-RestMethod -Uri "$ServiceBaseUrl/manage/api/time" -Headers $headers -Method Put -ContentType "application/json" -Body $timeBody -TimeoutSec 10 | Out-Null

$deviceState = (Invoke-ScenarioAdb -Arguments @("get-state") | Select-Object -Last 1).Trim()
if ($deviceState -ne "device") { throw "Emulator 尚未在线: $Serial state=$deviceState" }
if (-not $KeepInstalledData) {
    $installed = @(Invoke-ScenarioAdb -Arguments @("shell", "pm", "list", "packages", $PackageName)) -join "`n"
    if ($installed -match [regex]::Escape("package:$PackageName")) {
        Invoke-ScenarioAdb -Arguments @("uninstall", $PackageName) | Out-Null
    }
}
Invoke-ScenarioAdb -Arguments @("install", "-r", "-d", "-g", $ApkPath) | Out-Null
Invoke-ScenarioAdb -Arguments @("logcat", "-c") | Out-Null
Invoke-ScenarioAdb -Arguments @("shell", "monkey", "-p", $PackageName, "-c", "android.intent.category.LAUNCHER", "1") | Out-Null

Start-Sleep -Seconds 6
Invoke-ScenarioTap -X 790 -Y 2285 -WaitSeconds 7
Invoke-ScenarioTap -X 930 -Y 620 -WaitSeconds 45
Start-Sleep -Seconds 30
Save-ScenarioScreenshot -Name "01-title" | Out-Null

Invoke-ScenarioTap -X 540 -Y 1935 -WaitSeconds 25
Save-ScenarioScreenshot -Name "02-download-choice" | Out-Null
Invoke-ScenarioTap -X 270 -Y 1490 -WaitSeconds 30
Save-ScenarioScreenshot -Name "03-download-progress" | Out-Null
Invoke-ScenarioTap -X 270 -Y 1490 -WaitSeconds 8
Save-ScenarioScreenshot -Name "04-tutorial-skip" | Out-Null
Invoke-ScenarioTap -X 270 -Y 1490 -WaitSeconds 30
Save-ScenarioScreenshot -Name "05-tutorial-skip-confirmed" | Out-Null
Invoke-ScenarioTap -X 540 -Y 1760 -WaitSeconds 20
Save-ScenarioScreenshot -Name "06-gameplay-demo" | Out-Null

foreach ($index in 1..4) {
    Invoke-ScenarioTap -X 790 -Y 2070 -WaitSeconds 6
    Save-ScenarioScreenshot -Name ("07-gameplay-demo-{0}" -f $index) | Out-Null
}
Start-Sleep -Seconds 30
Save-ScenarioScreenshot -Name "08-final" | Out-Null

$logcatPath = Join-Path $OutputDirectory "logcat.txt"
Invoke-ScenarioAdb -Arguments @("logcat", "-d", "-v", "threadtime") | Set-Content -LiteralPath $logcatPath -Encoding UTF8
$actionScriptErrorPath = Join-Path $OutputDirectory "client-errors.txt"
Get-Content -LiteralPath $logcatPath -Encoding UTF8 |
    Select-String -Pattern 'ActionScript|FATAL EXCEPTION|RemoteError|Error #|No\.H[0-9]+|SIG(SEGV|ABRT)|uncaught' -CaseSensitive:$false |
    ForEach-Object { $_.Line } |
    Set-Content -LiteralPath $actionScriptErrorPath -Encoding UTF8

$newObservations = if (Test-Path -LiteralPath $MetadataPath -PathType Leaf) {
    @(Get-Content -LiteralPath $MetadataPath -Encoding UTF8 | Select-Object -Skip $metadataLineCount)
} else {
    @()
}
$observationPath = Join-Path $OutputDirectory "http-observations.jsonl"
$newObservations | Set-Content -LiteralPath $observationPath -Encoding UTF8
$parsedObservations = @($newObservations | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | ForEach-Object { $_ | ConvertFrom-Json })
$httpFailures = @($parsedObservations | Where-Object { [int]$_.status -ge 400 })
$report = [ordered]@{
    schemaVersion = 1
    startedAtUtc = $startedAtUtc
    finishedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
    serial = $Serial
    packageName = $PackageName
    apkPath = $ApkPath
    serviceBaseUrl = $ServiceBaseUrl
    virtualTimeIso = $VirtualTimeIso
    screenshots = @($ScreenshotPaths)
    logcatPath = $logcatPath
    clientErrorsPath = $actionScriptErrorPath
    observationsPath = $observationPath
    observationCount = $parsedObservations.Count
    lastObservation = $parsedObservations | Select-Object -Last 1
    httpFailures = $httpFailures
    loadObserved = @($parsedObservations | Where-Object { $_.method -eq "POST" -and $_.path -eq "/api/index.php/load" }).Count -gt 0
    gachaExecutionObserved = @($parsedObservations | Where-Object { $_.method -eq "POST" -and $_.path -eq "/api/index.php/gacha/exec" }).Count -gt 0
}
$reportPath = Join-Path $OutputDirectory "headless-gacha-report.json"
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding UTF8
$report
# //// /执行无头客户端场景并生成证据报告 ////
