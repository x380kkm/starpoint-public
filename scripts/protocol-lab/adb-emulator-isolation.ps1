# audience: internal
# # adb-emulator-isolation
# ADB 服务的发现开关只写入子进程环境, 模拟器通过已验证的进程状态显式注册.
# host:emulator 使用 ADB TCP 端口, 服务按相邻控制台端口保留 emulator-* 序列号.

$ErrorActionPreference = 'Stop'

# //// 返回专用服务的设备发现设置 [@x380kkm 2026-09-20] ////
function Get-ProtocolLabAdbServerEnvironment {
    @{
        ADB_USB = '0'
        ADB_MDNS = '0'
        ADB_EMU = '0'
        ADBHOST = '127.0.0.1'
    }
}

# //// 注册状态文件所属的运行中模拟器 [@x380kkm 2026-09-20] ////
function Register-ProtocolLabEmulatorTransport {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][object]$State,
        [Parameter(Mandatory)][ValidateRange(1, 65535)][int]$AdbServerPort
    )

    Assert-ProtocolLabEmulatorState -State $State
    Assert-ProtocolLabAdbServerPort -AdbServerPort $AdbServerPort
    $ConsolePort = 0
    $SerialPort = ([string]$State.Serial).Substring('emulator-'.Length)
    if (-not [int]::TryParse($SerialPort, [ref]$ConsolePort) -or
        $ConsolePort -lt 1 -or $ConsolePort -ge 65535 -or [int]$State.Port -ne $ConsolePort) {
        throw "模拟器控制台端口与状态不一致: $($State.Serial)"
    }
    $Process = Get-OwnedProtocolLabProcess -ProcessId ([int]$State.EmulatorProcessId) -ExecutablePath ([string]$State.EmulatorPath) -StartTimeUtc ([datetime]$State.EmulatorStartTimeUtc)
    if ($null -eq $Process) {
        throw "记录的模拟器进程已停止: $($State.EmulatorProcessId)"
    }

    $Client = [Net.Sockets.TcpClient]::new()
    try {
        if (-not $Client.ConnectAsync([Net.IPAddress]::Loopback, $AdbServerPort).Wait(5000)) {
            throw "专用 ADB server 连接超时: $AdbServerPort"
        }
        $Service = "host:emulator:$($ConsolePort + 1)"
        $Payload = [Text.Encoding]::ASCII.GetBytes(('{0:x4}{1}' -f $Service.Length, $Service))
        $Stream = $Client.GetStream()
        $Stream.WriteTimeout = 5000
        $Stream.Write($Payload, 0, $Payload.Length)
    } finally {
        $Client.Dispose()
    }
}
