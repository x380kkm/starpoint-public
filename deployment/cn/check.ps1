# audience: external
# # cn-health-check
# 此脚本读取 CN 服务的公开健康端点, 用于 Windows 本机部署后的就绪检查.

[CmdletBinding()]
param(
    [string]$BaseUrl = 'http://127.0.0.1:8001',
    [int]$TimeoutSeconds = 5
)

$ErrorActionPreference = 'Stop'

# //// 检查 CN HTTP 和多人端口 [@x380kkm 2026-07-24] ////
if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 120) {
    throw 'TimeoutSeconds 必须介于 1 和 120 之间.'
}

$HealthUrl = "$($BaseUrl.TrimEnd('/'))/healthz"
$Health = Invoke-RestMethod -Method Get -Uri $HealthUrl -TimeoutSec $TimeoutSeconds
if ($Health.status -ne 'ok' -or $Health.service -ne 'starpoint') {
    throw "服务健康状态无效: $HealthUrl"
}
if ($Health.httpPort -isnot [int] -and $Health.httpPort -isnot [long]) {
    throw '健康响应缺少有效 HTTP 端口.'
}
if ($Health.sessionPort -isnot [int] -and $Health.sessionPort -isnot [long]) {
    throw '健康响应缺少有效多人端口.'
}

$Health | ConvertTo-Json -Compress
# //// /检查 CN HTTP 和多人端口 ////
