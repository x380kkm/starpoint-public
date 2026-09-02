# audience: external
# # start-emulator
# 此脚本启动无界面 root-capable Android 模拟器, 禁用 virtio-wifi, 并持续写入可分析的 PCAP.
# 此脚本固定使用 Emulator 的以太网后端, 再为 eth0 添加到宿主 10.0.2.2 的默认路由.

[CmdletBinding()]
param(
    [string]$SdkRoot,
    [string]$AvdHome,
    [string]$AvdName = "starpoint-cn-api35-x86_64-64g",
    [ValidateRange(5554, 5584)]
    [int]$Port = 5554,
    [ValidateRange(30, 1800)]
    [int]$BootTimeoutSeconds = 900,
    [ValidateRange(1, 30)]
    [int]$AdbCommandTimeoutSeconds = 5,
    [ValidateRange(1, 65535)]
    [Nullable[int]]$AdbServerPort,
    [ValidateSet("auto", "host", "swiftshader_indirect")]
    [string]$GpuMode = "swiftshader_indirect",
    [string]$HostAddress = "10.0.2.2"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

# //// 启动模拟器并验证 ADB, root, 路由和 PCAP [@x380kkm 2026-07-20] ////
if ($Port % 2 -ne 0) {
    throw "Android Emulator 控制台端口必须是偶数: $Port"
}

$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot -AvdHome $AvdHome
$EmulatorPath = Join-Path $Paths.SdkRoot "emulator\emulator.exe"
$AdbPath = Join-Path $Paths.SdkRoot "platform-tools\adb.exe"
$AvdConfigPath = Join-Path $Paths.AvdHome "$AvdName.avd\config.ini"
Assert-ProtocolLabFile -Path $EmulatorPath -Description "Android Emulator"
Assert-ProtocolLabFile -Path $AdbPath -Description "adb"
Assert-ProtocolLabFile -Path $AvdConfigPath -Description "AVD config"

if (Test-Path -LiteralPath $Paths.EmulatorStatePath -PathType Leaf) {
    throw "模拟器状态文件已存在. 请先运行 stop-emulator.ps1: $($Paths.EmulatorStatePath)"
}
$ResolvedAdbServerPort = Resolve-ProtocolLabAdbServerPort -RequestedPort $AdbServerPort -State $null

$Serial = "emulator-$Port"
$AdbServerStarted = $false
try {
    Start-ProtocolLabAdbServer -AdbPath $AdbPath -Serial $Serial -AdbServerPort $ResolvedAdbServerPort -TimeoutSeconds $AdbCommandTimeoutSeconds | Out-Null
    $AdbServerStarted = $true
    $ExistingDevices = Get-ProtocolLabAdbDevices -AdbPath $AdbPath -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds
    $ExistingDevice = @($ExistingDevices.Output | Where-Object { $_ -match "^$([regex]::Escape($Serial))\s+\S+" })
    if ($ExistingDevices.ExitCode -eq 0 -and $ExistingDevice.Count -gt 0) {
        throw "ADB serial 已被占用: $Serial"
    }
} catch {
    if ($AdbServerStarted) {
        Stop-ProtocolLabAdbServer -AdbPath $AdbPath -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds | Out-Null
    }
    throw
}

$Timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$RunDirectory = Join-Path $Paths.RunDirectory "emulator-$Timestamp"
New-Item -ItemType Directory -Force -Path $RunDirectory | Out-Null
$PcapPath = Join-Path $RunDirectory "emulator.pcap"
$StdoutPath = Join-Path $RunDirectory "emulator.stdout.log"
$StderrPath = Join-Path $RunDirectory "emulator.stderr.log"

$EmulatorEnvironment = [ordered]@{
    ANDROID_SDK_ROOT = $Paths.SdkRoot
    ANDROID_AVD_HOME = $Paths.AvdHome
    ANDROID_ADB_SERVER_PORT = $ResolvedAdbServerPort.ToString([Globalization.CultureInfo]::InvariantCulture)
}
$OriginalEmulatorEnvironment = @{}
foreach ($Name in $EmulatorEnvironment.Keys) {
    $OriginalEmulatorEnvironment[$Name] = [Environment]::GetEnvironmentVariable($Name, [EnvironmentVariableTarget]::Process)
}
$Arguments = @(
    "-avd", $AvdName,
    "-port", $Port.ToString(),
    "-no-window",
    "-no-audio",
    "-no-boot-anim",
    "-gpu", $GpuMode,
    "-no-snapshot",
    "-feature", "-Wifi",
    "-tcpdump", $PcapPath
)

$EmulatorProcess = $null
$OriginalProxy = $null
$ProxyWasDisabled = $false
try {
    try {
        foreach ($Name in $EmulatorEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($Name, $EmulatorEnvironment[$Name], [EnvironmentVariableTarget]::Process)
        }
        $EmulatorProcess = Start-Process -FilePath $EmulatorPath -ArgumentList $Arguments -WorkingDirectory $RunDirectory -WindowStyle Hidden -PassThru -RedirectStandardOutput $StdoutPath -RedirectStandardError $StderrPath
    } finally {
        foreach ($Name in $OriginalEmulatorEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($Name, $OriginalEmulatorEnvironment[$Name], [EnvironmentVariableTarget]::Process)
        }
    }
    Wait-ProtocolLabAndroidBoot -AdbPath $AdbPath -Serial $Serial -TimeoutSeconds $BootTimeoutSeconds -EmulatorProcess $EmulatorProcess -EmulatorStderrPath $StderrPath -AdbServerPort $ResolvedAdbServerPort -CommandTimeoutSeconds $AdbCommandTimeoutSeconds
    Enable-ProtocolLabCaptureRoute -AdbPath $AdbPath -Serial $Serial -HostAddress $HostAddress -AdbServerPort $ResolvedAdbServerPort -CommandTimeoutSeconds $AdbCommandTimeoutSeconds
    $AbiResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "getprop", "ro.product.cpu.abilist") -AdbServerPort $ResolvedAdbServerPort -TimeoutSeconds $AdbCommandTimeoutSeconds
    $DeviceAbis = ($AbiResult.Output -join "").Trim()
    if ($DeviceAbis.Split(",") -notcontains "arm64-v8a") {
        throw "系统镜像未公开 CN APK 所需的 ARM64 ABI: $DeviceAbis"
    }
    $BridgeResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "getprop", "ro.dalvik.vm.native.bridge") -AdbServerPort $ResolvedAdbServerPort -TimeoutSeconds $AdbCommandTimeoutSeconds
    $NativeBridge = ($BridgeResult.Output -join "").Trim()
    if ([string]::IsNullOrWhiteSpace($NativeBridge) -or $NativeBridge -eq "0") {
        throw "系统镜像未启用 ARM native bridge."
    }
    $OriginalProxy = Disable-ProtocolLabProxy -AdbPath $AdbPath -Serial $Serial -AdbServerPort $ResolvedAdbServerPort -CommandTimeoutSeconds $AdbCommandTimeoutSeconds
    $ProxyWasDisabled = $true
    Repair-ProtocolLabCaptureRoute -AdbPath $AdbPath -Serial $Serial -HostAddress $HostAddress -AdbServerPort $ResolvedAdbServerPort -CommandTimeoutSeconds $AdbCommandTimeoutSeconds
    $CaptureCheck = Test-ProtocolLabPcapGrowth -AdbPath $AdbPath -Serial $Serial -PcapPath $PcapPath -HostAddress $HostAddress -AdbServerPort $ResolvedAdbServerPort -CommandTimeoutSeconds $AdbCommandTimeoutSeconds

    $State = [ordered]@{
        SchemaVersion = 4
        AvdName = $AvdName
        Headless = $true
        AdbIsolation = "one-device"
        Serial = $Serial
        Port = $Port
        HostAddress = $HostAddress
        AdbPath = $AdbPath
        AdbServerPort = $ResolvedAdbServerPort
        EmulatorPath = $EmulatorPath
        EmulatorProcessId = $EmulatorProcess.Id
        EmulatorStartTimeUtc = $EmulatorProcess.StartTime.ToUniversalTime().ToString("o")
        OriginalProxy = $OriginalProxy
        DeviceAbis = $DeviceAbis
        NativeBridge = $NativeBridge
        RunDirectory = $RunDirectory
        PcapPath = $PcapPath
        StdoutPath = $StdoutPath
        StderrPath = $StderrPath
        StartedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
    }
    Write-ProtocolLabState -Path $Paths.EmulatorStatePath -State $State

    [pscustomobject]@{
        AvdName = $AvdName
        Serial = $Serial
        AdbServerPort = $ResolvedAdbServerPort
        ProcessId = $EmulatorProcess.Id
        PcapPath = $PcapPath
        PcapBytes = $CaptureCheck.AfterBytes
        DeviceAbis = $DeviceAbis
        NativeBridge = $NativeBridge
        StatePath = $Paths.EmulatorStatePath
    }
} catch {
    if ($ProxyWasDisabled) {
        try {
            Restore-ProtocolLabProxy -AdbPath $AdbPath -Serial $Serial -ProxyValue $OriginalProxy -AdbServerPort $ResolvedAdbServerPort -CommandTimeoutSeconds $AdbCommandTimeoutSeconds
        } catch {
            Write-Warning "Android 全局 HTTP 代理恢复失败: $($_.Exception.Message)"
        }
    }
    if ($null -ne $EmulatorProcess -and -not $EmulatorProcess.HasExited) {
        try {
            Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("emu", "kill") -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds | Out-Null
        } catch {
            Write-Warning "Emulator 平滑停止失败: $($_.Exception.Message)"
        }
        Start-Sleep -Seconds 2
        if (-not $EmulatorProcess.HasExited) {
            $EmulatorProcess | Stop-Process -Force
        }
    }
    if ($AdbServerStarted) {
        Stop-ProtocolLabAdbServer -AdbPath $AdbPath -AdbServerPort $ResolvedAdbServerPort -AllowFailure -TimeoutSeconds $AdbCommandTimeoutSeconds | Out-Null
    }
    throw
}
# //// /启动模拟器并验证 ADB, root, 路由和 PCAP ////
