# audience: external
# # start-multiplayer-probe
# 此脚本在 CN 默认多人端口 8003 启动 TCP 和 UDP 记录器, 并把 PID 和捕获目录写入实验状态.
# 模拟器通过 10.0.2.2 访问此探针, 记录器不发送推测的服务器响应.

[CmdletBinding()]
param(
    [ValidateRange(1, 65535)]
    [int]$Port = 8003,
    [string]$ListenHost = "0.0.0.0",
    [string]$PythonPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

# //// 启动并确认多人协议探针进入监听状态 [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths
if (Test-Path -LiteralPath $Paths.ProbeStatePath -PathType Leaf) {
    throw "多人探针状态文件已存在. 请先运行 stop-multiplayer-probe.ps1: $($Paths.ProbeStatePath)"
}
$PythonPath = Resolve-ProtocolLabPythonPath -PythonPath $PythonPath
$ProbeScriptPath = Join-Path $PSScriptRoot "multiplayer_probe.py"
Assert-ProtocolLabFile -Path $ProbeScriptPath -Description "multiplayer probe"

$Timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$RunDirectory = Join-Path $Paths.RunDirectory "multiplayer-probe-$Timestamp"
New-Item -ItemType Directory -Force -Path $RunDirectory | Out-Null
$CaptureDirectory = Join-Path $RunDirectory "capture"
$StdoutPath = Join-Path $RunDirectory "probe.stdout.log"
$StderrPath = Join-Path $RunDirectory "probe.stderr.log"
$Arguments = @($ProbeScriptPath, "--listen-host", $ListenHost, "--port", $Port.ToString(), "--output", $CaptureDirectory)
$Process = Start-Process -FilePath $PythonPath -ArgumentList $Arguments -WorkingDirectory $RunDirectory -WindowStyle Hidden -PassThru -RedirectStandardOutput $StdoutPath -RedirectStandardError $StderrPath

$EventPath = Join-Path $CaptureDirectory "events.jsonl"
$Deadline = (Get-Date).AddSeconds(15)
do {
    if ($Process.HasExited) {
        $ErrorText = if (Test-Path -LiteralPath $StderrPath) { Get-Content -LiteralPath $StderrPath -Raw } else { "" }
        throw "多人协议探针启动失败, exit=$($Process.ExitCode): $ErrorText"
    }
    if (Test-Path -LiteralPath $EventPath -PathType Leaf) {
        $Ready = Select-String -LiteralPath $EventPath -SimpleMatch '"event": "ready"' -Quiet
        if ($Ready) {
            break
        }
    }
    Start-Sleep -Milliseconds 200
} while ((Get-Date) -lt $Deadline)
if (-not $Ready) {
    $Process | Stop-Process -Force
    throw "多人协议探针未在 15 秒内进入监听状态."
}

$State = [ordered]@{
    SchemaVersion = 1
    ListenHost = $ListenHost
    Port = $Port
    PythonPath = $PythonPath
    ProcessId = $Process.Id
    ProcessStartTimeUtc = $Process.StartTime.ToUniversalTime().ToString("o")
    RunDirectory = $RunDirectory
    CaptureDirectory = $CaptureDirectory
    EventPath = $EventPath
    StdoutPath = $StdoutPath
    StderrPath = $StderrPath
    StartedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
}
Write-ProtocolLabState -Path $Paths.ProbeStatePath -State $State
[pscustomobject]@{
    ProcessId = $Process.Id
    ListenHost = $ListenHost
    Port = $Port
    CaptureDirectory = $CaptureDirectory
    EventPath = $EventPath
    StatePath = $Paths.ProbeStatePath
}
# //// /启动并确认多人协议探针进入监听状态 ////
