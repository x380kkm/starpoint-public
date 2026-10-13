# audience: internal
# # adb-emulator-isolation-tests
# 本机 PowerShell 子进程和 TCP 替身验证发现开关的作用范围及模拟器注册报文.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'protocol-lab.psm1') -Force
$Module = Get-Module protocol-lab

# //// 检查发现开关只传入子进程 [@x380kkm 2026-09-20] ////
$OriginalEnvironment = @{}
foreach ($Name in @('ADB_USB', 'ADB_MDNS', 'ADB_EMU', 'ADBHOST')) {
    $OriginalEnvironment[$Name] = [Environment]::GetEnvironmentVariable($Name, 'Process')
}
$ChildProgram = '$ErrorActionPreference = ''Stop''; @{ ADB_USB=$env:ADB_USB; ADB_MDNS=$env:ADB_MDNS; ADB_EMU=$env:ADB_EMU; ADBHOST=$env:ADBHOST } | ConvertTo-Json -Compress'
$ChildResult = & $Module {
    param($PowerShellPath, $ChildProgram)
    Invoke-ProtocolLabProcessWithinTimeout -FilePath $PowerShellPath -Arguments @('-NoProfile', '-Command', $ChildProgram) -Description '子进程环境检查' -TimeoutSeconds 10 -ChildEnvironment (Get-ProtocolLabAdbServerEnvironment)
} (Get-Process -Id $PID).Path $ChildProgram
if ($ChildResult.ExitCode -ne 0) { throw ($ChildResult.Output -join [Environment]::NewLine) }
$ChildValues = ($ChildResult.Output -join '') | ConvertFrom-Json
foreach ($Name in $OriginalEnvironment.Keys) {
    $ExpectedValue = if ($Name -eq 'ADBHOST') { '127.0.0.1' } else { '0' }
    if ($ChildValues.$Name -cne $ExpectedValue) { throw "子进程发现开关不匹配: $Name" }
    if ([Environment]::GetEnvironmentVariable($Name, 'Process') -cne $OriginalEnvironment[$Name]) {
        throw "宿主进程环境发生变化: $Name"
    }
}

# //// 检查服务启动参数与环境边界 [@x380kkm 2026-09-20] ////
& $Module {
    param($ExecutablePath)
    function script:Invoke-ProtocolLabProcessWithinTimeout {
        param($FilePath, $Arguments, $Description, $TimeoutSeconds, $ChildEnvironment)
        $script:IsolationStart = [pscustomobject]@{ Arguments=$Arguments; Environment=$ChildEnvironment }
        [pscustomobject]@{ ExitCode=0; Output=@() }
    }
    Invoke-ProtocolLabAdbServer -AdbPath $ExecutablePath -AdbServerPort 51991 -CommandArguments @('--one-device', 'emulator-5562', 'start-server') | Out-Null
    if (($script:IsolationStart.Arguments -join ' ') -cne '-L tcp:localhost:51991 --one-device emulator-5562 start-server') {
        throw '服务启动参数未绑定本地专用端口'
    }
    foreach ($Name in @('ADB_USB', 'ADB_MDNS', 'ADB_EMU')) {
        if ($script:IsolationStart.Environment[$Name] -cne '0') { throw "服务启动环境缺少 $Name" }
    }
} (Get-Process -Id $PID).Path

# //// 检查所属进程的注册报文和端口一致性 [@x380kkm 2026-09-20] ////
& $Module {
    function script:Get-OwnedProtocolLabProcess {
        param($ProcessId, $ExecutablePath, $StartTimeUtc)
        if ($ProcessId -ne 12345 -or $ExecutablePath -cne 'owned-emulator.exe') {
            throw '模拟器注册遗漏进程归属参数'
        }
        [pscustomobject]@{ Id=$ProcessId }
    }
}
$State = [pscustomobject]@{
    Headless=$true
    AdbIsolation='one-device'
    Serial='emulator-5562'
    Port=5562
    AdbServerPort=51991
    EmulatorProcessId=12345
    EmulatorPath='owned-emulator.exe'
    EmulatorStartTimeUtc='2026-09-20T00:00:00Z'
}
$Listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
try {
    $Listener.Start()
    $ServerPort = ([Net.IPEndPoint]$Listener.LocalEndpoint).Port
    $Accepted = $Listener.AcceptTcpClientAsync()
    Register-ProtocolLabEmulatorTransport -State $State -AdbServerPort $ServerPort
    if (-not $Accepted.Wait(5000)) { throw '模拟器注册未连接到专用服务' }
    $Peer = $Accepted.Result
    try {
        $Peer.GetStream().ReadTimeout = 5000
        $Buffer = [byte[]]::new(64)
        $Received = [IO.MemoryStream]::new()
        try {
            while (($Count = $Peer.GetStream().Read($Buffer, 0, $Buffer.Length)) -gt 0) {
                $Received.Write($Buffer, 0, $Count)
            }
            if ([Text.Encoding]::ASCII.GetString($Received.ToArray()) -cne '0012host:emulator:5563') {
                throw '模拟器注册未保留控制台端口和序列号关系'
            }
        } finally { $Received.Dispose() }
    } finally { $Peer.Dispose() }
    $State.Port = 5564
    $Rejected = $false
    try { Register-ProtocolLabEmulatorTransport -State $State -AdbServerPort $ServerPort }
    catch { $Rejected = $_.Exception.Message -like '模拟器控制台端口与状态不一致:*' }
    if (-not $Rejected) { throw '端口不一致的模拟器被注册' }
} finally {
    $Listener.Stop()
}
Write-Output '子进程环境, 本地服务参数, 所属模拟器注册及端口拒绝行为验证通过.'
