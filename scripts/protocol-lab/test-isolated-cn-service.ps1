# audience: internal
# # isolated-cn-service-tests
# 此脚本验证隔离 CN 服务启动和停止脚本的静态边界, 不启动服务或模拟器.

$ErrorActionPreference = "Stop"

function Assert-TestCondition {
    param([Parameter(Mandatory)][bool]$Condition, [Parameter(Mandatory)][string]$Message)
    if (-not $Condition) { throw $Message }
}

$scriptRoot = Split-Path -Parent $PSCommandPath
$startPath = Join-Path $scriptRoot "start-isolated-cn-service.ps1"
$stopPath = Join-Path $scriptRoot "stop-isolated-cn-service.ps1"
$startText = Get-Content -LiteralPath $startPath -Raw -Encoding UTF8
$stopText = Get-Content -LiteralPath $stopPath -Raw -Encoding UTF8

Assert-TestCondition -Condition ($startText.Contains('[Parameter(Mandatory)][string]$ManagementToken')) -Message "启动脚本必须显式接收临时管理 token."
Assert-TestCondition -Condition ($startText.Contains('MANAGEMENT_ADMIN_PASSWORD = ([Convert]::ToHexString')) -Message "启动脚本不得写入固定管理密码."
Assert-TestCondition -Condition (-not ($startText.Contains('cn-management-test-token') -or $startText.Contains('local-test-only'))) -Message "启动脚本包含固定测试凭据."
Assert-TestCondition -Condition ($startText.Contains('CN_PROTOCOL_METADATA_LOG')) -Message "启动脚本未隔离安全 HTTP 元数据路径."
Assert-TestCondition -Condition ($startText.Contains('DATABASE_PATH')) -Message "启动脚本未隔离游戏数据库路径."
Assert-TestCondition -Condition ($stopText.Contains('$sameExecutable')) -Message "停止脚本未校验可执行文件归属."
Assert-TestCondition -Condition ($stopText.Contains('$sameRoot')) -Message "停止脚本未校验工作目录归属."
Assert-TestCondition -Condition ($stopText.Contains('$sameRunDirectory')) -Message "停止脚本未校验状态目录归属."
Assert-TestCondition -Condition ($stopText.Contains('$sameStart')) -Message "停止脚本未校验进程启动时间."
Assert-TestCondition -Condition ($stopText.Contains('DateTimeStyles]::AssumeUniversal')) -Message "停止脚本未按 UTC 解析启动时间."
Assert-TestCondition -Condition ($startText.Contains('EntryPath = $entryPath')) -Message "启动脚本未记录绝对入口路径."
Assert-TestCondition -Condition ($stopText.Contains('$state.EntryPath')) -Message "停止脚本未读取绝对入口路径."
Assert-TestCondition -Condition ($stopText.Contains('out/start.js')) -Message "停止脚本未限制旧版入口命令行."

Write-Output "Isolated CN service script tests passed."
