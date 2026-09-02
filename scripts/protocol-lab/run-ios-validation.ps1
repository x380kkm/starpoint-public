# audience: external
# # run-ios-validation
# 此脚本从一个 Git commit 执行本地预检, 远端构建和单 Simulator 诊断, 并输出脱敏报告.

[CmdletBinding()]
param(
    [string]$RepositoryRoot,
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._:-]*$')]
    [string]$SshHost = "starpoint-mac",
    [ValidatePattern('^[A-Za-z0-9 .()_-]+$')]
    [string]$SimulatorName = "iPhone 17 Pro",
    [string]$Commit = "HEAD",
    [string]$OutputRoot,
    [string]$DeviceIpaPath,
    [string]$DiagnosticCdnRoot,
    [ValidateRange(1, 3)]
    [int]$ThrottleLimit = 3,
    [switch]$RebuildDeviceIpa
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# //// 执行一次验证并用退出码返回结果 [@x380kkm 2026-08-18] ////
Import-Module (Join-Path $PSScriptRoot "ios-validation.psm1") -Force

$result = Invoke-IosValidation @PSBoundParameters
$result
if ($result.Status -ne "passed") {
    exit 1
}
# //// /执行一次验证并用退出码返回结果 ////
