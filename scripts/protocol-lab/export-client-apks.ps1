# audience: external
# # export-client-apks
# 此脚本从指定 Android 设备导出游戏的 base APK 和全部 split APK, 并为每个文件记录 SHA-256.
# 此脚本默认查找国际版包名 com.kakaogames.wdfp, 不修改已安装应用或应用数据.

[CmdletBinding()]
param(
    [string]$SdkRoot,
    [string]$AvdHome,
    [string]$Serial,
    [ValidateRange(1, 65535)]
    [Nullable[int]]$AdbServerPort,
    [string]$PackageName = "com.kakaogames.wdfp",
    [string]$OutputDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

# //// 导出设备中已安装客户端的全部 APK [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot -AvdHome $AvdHome
$AdbPath = Join-Path $Paths.SdkRoot "platform-tools\adb.exe"
Assert-ProtocolLabFile -Path $AdbPath -Description "adb"
$State = Read-ProtocolLabState -Path $Paths.EmulatorStatePath
Assert-ProtocolLabEmulatorState -State $State
if ([string]::IsNullOrWhiteSpace($Serial)) {
    $Serial = [string]$State.Serial
} elseif ($Serial -cne [string]$State.Serial) {
    throw "ADB serial 与本项目记录的 Emulator 不一致: requested=$Serial state=$($State.Serial)"
}
$ResolvedAdbServerPort = Resolve-ProtocolLabAdbServerPort -RequestedPort $AdbServerPort -State $State
Start-ProtocolLabAdbServer -AdbPath $AdbPath -Serial $Serial -AdbServerPort $ResolvedAdbServerPort | Out-Null

$PackagePaths = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "pm", "path", $PackageName) -AdbServerPort $ResolvedAdbServerPort -AllowFailure
if ($PackagePaths.ExitCode -ne 0) {
    throw "设备中未安装客户端: package=$PackageName serial=$Serial"
}
$RemotePaths = @($PackagePaths.Output | Where-Object { $_ -match "^package:" } | ForEach-Object { $_.Substring(8).Trim() })
if ($RemotePaths.Count -eq 0) {
    throw "设备没有返回 APK 路径: package=$PackageName serial=$Serial"
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $Timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    $OutputDirectory = Join-Path $Paths.ArtifactsRoot "client-apks\$PackageName\$Timestamp"
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$Files = foreach ($RemotePath in $RemotePaths) {
    $FileName = [IO.Path]::GetFileName($RemotePath)
    $LocalPath = Join-Path $OutputDirectory $FileName
    $PullResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("pull", $RemotePath, $LocalPath) -AdbServerPort $ResolvedAdbServerPort -AllowFailure
    if ($PullResult.ExitCode -ne 0) {
        throw "ADB pull 失败: $RemotePath"
    }
    [ordered]@{
        FileName = $FileName
        RemotePath = $RemotePath
        Bytes = (Get-Item -LiteralPath $LocalPath).Length
        Sha256 = (Get-FileHash -LiteralPath $LocalPath -Algorithm SHA256).Hash
    }
}

$Manifest = [ordered]@{
    SchemaVersion = 1
    PackageName = $PackageName
    Serial = $Serial
    ExportedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
    Files = @($Files)
}
$ManifestPath = Join-Path $OutputDirectory "manifest.json"
$Manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $ManifestPath -Encoding utf8
[pscustomobject]@{
    PackageName = $PackageName
    Serial = $Serial
    AdbServerPort = $ResolvedAdbServerPort
    OutputDirectory = $OutputDirectory
    ApkCount = $RemotePaths.Count
    ManifestPath = $ManifestPath
}
# //// /导出设备中已安装客户端的全部 APK ////
