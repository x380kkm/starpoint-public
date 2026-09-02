# audience: external
# # install-client
# 此脚本把一个 APK 安装到状态文件记录的模拟器, 验证包名和设备 ABI, 再启动客户端并保存首轮 logcat.
# 此脚本只操作本次协议实验的 ADB serial, 不连接其他 Android 设备.
# 默认启动 AppEntry; 调用方可以显式指定其他已声明 Activity.
# 安装前验证目标 Activity 已在 APK Manifest 中声明.

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ApkPath,
    [string]$PackageName = "com.leiting.wf",
    [string]$LaunchActivity = "air.com.leiting.wf.AppEntry",
    [string]$RequiredAbi = "arm64-v8a",
    [int[]]$RequiredServerPorts = @(8001, 8003),
    [ValidateRange(1, 120)]
    [int]$LaunchWaitSeconds = 10,
    [ValidateRange(1, 65535)]
    [Nullable[int]]$AdbServerPort,
    [string]$SdkRoot,
    [string]$AvdHome
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

# //// 验证 APK 声明目标启动 Activity [@x380kkm 2026-07-27] ////
function Assert-ProtocolLabDeclaredActivity {
    param(
        [Parameter(Mandatory)][string]$ApkAnalyzerPath,
        [Parameter(Mandatory)][string]$ApkPath,
        [Parameter(Mandatory)][string]$ActivityName
    )

    $ManifestOutput = @(& $ApkAnalyzerPath manifest print $ApkPath 2>&1 | ForEach-Object { $_.ToString() })
    $ManifestExitCode = $LASTEXITCODE
    if ($ManifestExitCode -ne 0) {
        throw "无法读取 APK Manifest: activity=$ActivityName apk=$ApkPath exit=$ManifestExitCode"
    }

    $Manifest = $ManifestOutput -join "`n"
    $ActivityPattern = "<activity\b[^>]*\bandroid:name\s*=\s*`"$([regex]::Escape($ActivityName))`""
    if ($Manifest -notmatch $ActivityPattern) {
        throw "APK 未声明启动 Activity: activity=$ActivityName apk=$ApkPath"
    }
}
# //// /验证 APK 声明目标启动 Activity ////

# //// 安装并启动 CN 客户端后保存可诊断日志 [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot -AvdHome $AvdHome
$State = Read-ProtocolLabState -Path $Paths.EmulatorStatePath
$ResolvedAdbServerPort = Resolve-ProtocolLabAdbServerPort -RequestedPort $AdbServerPort -State $State
$Process = Get-OwnedProtocolLabProcess -ProcessId ([int]$State.EmulatorProcessId) -ExecutablePath ([string]$State.EmulatorPath) -StartTimeUtc ([datetime]$State.EmulatorStartTimeUtc)
if ($null -eq $Process) {
    throw "记录的 Emulator 进程已退出: $($State.EmulatorProcessId)"
}

$ResolvedApkPath = [IO.Path]::GetFullPath($ApkPath)
Assert-ProtocolLabFile -Path $ResolvedApkPath -Description "客户端 APK"
$ApkSha256 = (Get-FileHash -LiteralPath $ResolvedApkPath -Algorithm SHA256).Hash.ToLowerInvariant()
$ApkAnalyzerPath = Join-Path $Paths.SdkRoot "cmdline-tools\latest\bin\apkanalyzer.bat"
Assert-ProtocolLabFile -Path $ApkAnalyzerPath -Description "apkanalyzer"
$env:JAVA_HOME = Resolve-ProtocolLabJavaHome
$PackageOutput = @(& $ApkAnalyzerPath manifest application-id $ResolvedApkPath 2>&1 | ForEach-Object { $_.ToString() })
$PackageExitCode = $LASTEXITCODE
$ActualPackageName = ($PackageOutput | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Last 1).Trim()
if ($PackageExitCode -ne 0 -or $ActualPackageName -ne $PackageName) {
    throw "APK 包名不匹配: expected=$PackageName actual=$ActualPackageName exit=$PackageExitCode"
}
Assert-ProtocolLabDeclaredActivity -ApkAnalyzerPath $ApkAnalyzerPath -ApkPath $ResolvedApkPath -ActivityName $LaunchActivity

$AdbPath = [string]$State.AdbPath
$Serial = [string]$State.Serial
$AbiResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "getprop", "ro.product.cpu.abilist") -AdbServerPort $ResolvedAdbServerPort
$DeviceAbis = ($AbiResult.Output -join "").Trim()
if ($DeviceAbis.Split(",") -notcontains $RequiredAbi) {
    throw "模拟器 ABI 不兼容: required=$RequiredAbi actual=$DeviceAbis"
}

$ServerHost = [string]$State.HostAddress
Repair-ProtocolLabCaptureRoute -AdbPath $AdbPath -Serial $Serial -HostAddress $ServerHost -AdbServerPort $ResolvedAdbServerPort
foreach ($ServerPort in $RequiredServerPorts) {
    if ($ServerPort -lt 1 -or $ServerPort -gt 65535) {
        throw "服务器端口超出有效范围: $ServerPort"
    }
    $PortResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "toybox", "nc", "-z", "-w", "3", $ServerHost, $ServerPort.ToString()) -AdbServerPort $ResolvedAdbServerPort -AllowFailure
    if ($PortResult.ExitCode -ne 0) {
        throw "模拟器无法绕过代理直连本机服务器: ${ServerHost}:$ServerPort $($PortResult.Output -join ' ')"
    }
}

$InstallResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("install", "-r", "-t", $ResolvedApkPath) -AdbServerPort $ResolvedAdbServerPort
if (($InstallResult.Output -join "`n") -notmatch "(?m)^Success\s*$") {
    throw "ADB 未确认 APK 安装成功: $($InstallResult.Output -join ' ')"
}

$PackageResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "pm", "path", $PackageName) -AdbServerPort $ResolvedAdbServerPort
if (($PackageResult.Output -join "") -notmatch "package:") {
    throw "模拟器中没有目标包: $PackageName"
}

Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("logcat", "-c") -AdbServerPort $ResolvedAdbServerPort | Out-Null
Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "am", "force-stop", $PackageName) -AdbServerPort $ResolvedAdbServerPort | Out-Null
$Component = "$PackageName/$LaunchActivity"
$LaunchResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "am", "start", "-W", "-n", $Component) -AdbServerPort $ResolvedAdbServerPort
Start-Sleep -Seconds $LaunchWaitSeconds

$PidResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "pidof", $PackageName) -AdbServerPort $ResolvedAdbServerPort -AllowFailure
$ClientLogPath = Join-Path ([string]$State.RunDirectory) "client-launch.log"
$LogResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("logcat", "-d", "-v", "threadtime") -AdbServerPort $ResolvedAdbServerPort
$LogResult.Output | Set-Content -LiteralPath $ClientLogPath -Encoding utf8
$ProcessIds = ($PidResult.Output -join " ").Trim()
if ($PidResult.ExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($ProcessIds)) {
    throw "客户端启动后没有运行进程, 请检查日志: $ClientLogPath"
}

[pscustomobject]@{
    PackageName = $PackageName
    ApkSha256 = $ApkSha256
    Component = $Component
    DeviceAbis = $DeviceAbis
    ServerHost = $ServerHost
    ServerPorts = $RequiredServerPorts
    ProcessIds = $ProcessIds
    LaunchOutput = $LaunchResult.Output
    LogPath = $ClientLogPath
    Serial = $Serial
    AdbServerPort = $ResolvedAdbServerPort
}
# //// /安装并启动 CN 客户端后保存可诊断日志 ////
