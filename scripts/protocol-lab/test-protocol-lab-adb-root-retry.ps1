# audience: internal
# # test-protocol-lab-adb-root-retry
# 此脚本验证 ADB root 的瞬态失败重试和最终错误内容.

$ErrorActionPreference = "Stop"

# //// 断言测试条件成立 [@x380kkm 2026-07-29] ////
function Assert-TestCondition {
    param(
        [Parameter(Mandatory)]
        [bool]$Condition,
        [Parameter(Mandatory)]
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}
# //// /断言测试条件成立 ////

# //// 验证 closed 后的 root 重试成功 [@x380kkm 2026-07-29] ////
function Test-ProtocolLabAdbRootRetriesTransientFailure {
    param([Parameter(Mandatory)][System.Management.Automation.PSModuleInfo]$Module)

    $Result = & $Module {
        $script:VirtualNow = [DateTime]::ParseExact("2026-07-29T12:00:00", "yyyy-MM-ddTHH:mm:ss", [Globalization.CultureInfo]::InvariantCulture)
        $script:RootResponses = @(
            [pscustomobject]@{ ExitCode = 1; Output = @("closed") },
            [pscustomobject]@{ ExitCode = 0; Output = @("restarting adbd as root") }
        )
        $script:RootInvocationCount = 0
        $script:CommandTimeoutSeconds = @()
        function Get-Date {
            $script:VirtualNow
        }
        function Invoke-ProtocolLabAdb {
            param([string]$AdbPath, [string]$Serial, [string[]]$CommandArguments, [int]$AdbServerPort, [switch]$AllowFailure, [int]$TimeoutSeconds)

            $Response = $script:RootResponses[$script:RootInvocationCount]
            $script:RootInvocationCount++
            $script:CommandTimeoutSeconds += $TimeoutSeconds
            $Response
        }

        $RootResult = Invoke-ProtocolLabAdbRootWithRetry -AdbPath "adb.exe" -Serial "emulator-5554" -AdbServerPort 5041 -TimeoutSeconds 6 -MaximumAttempts 2 -RetryDelayMilliseconds 0
        [pscustomobject]@{
            AttemptCount = $script:RootInvocationCount
            ExitCode = $RootResult.ExitCode
            CommandTimeoutSeconds = $script:CommandTimeoutSeconds
        }
    }

    Assert-TestCondition -Condition ($Result.AttemptCount -eq 2) -Message "瞬态 closed 后未再次执行 adb root"
    Assert-TestCondition -Condition ($Result.ExitCode -eq 0) -Message "重试后的 adb root 未返回成功"
    Assert-TestCondition -Condition ($Result.CommandTimeoutSeconds.Count -eq 2 -and ($Result.CommandTimeoutSeconds | Where-Object { $_ -ne 2 }).Count -eq 0) -Message "6 秒 root 截止时间未提供 2 秒命令预算"
}
# //// /验证 closed 后的 root 重试成功 ////

# //// 验证持续 root 失败保留最后一次 ADB 错误 [@x380kkm 2026-07-29] ////
function Test-ProtocolLabAdbRootReportsFinalFailure {
    param([Parameter(Mandatory)][System.Management.Automation.PSModuleInfo]$Module)

    $FailureMessage = & $Module {
        $script:RootResponses = @(
            [pscustomobject]@{ ExitCode = 6; Output = @("closed-first") },
            [pscustomobject]@{ ExitCode = 7; Output = @("closed-final") }
        )
        $script:RootInvocationCount = 0
        function Invoke-ProtocolLabAdb {
            param([string]$AdbPath, [string]$Serial, [string[]]$CommandArguments, [int]$AdbServerPort, [switch]$AllowFailure, [int]$TimeoutSeconds)

            $Response = $script:RootResponses[$script:RootInvocationCount]
            $script:RootInvocationCount++
            $Response
        }

        try {
            Invoke-ProtocolLabAdbRootWithRetry -AdbPath "adb.exe" -Serial "emulator-5554" -AdbServerPort 5041 -TimeoutSeconds 6 -MaximumAttempts 2 -RetryDelayMilliseconds 0
            throw "持续失败的 adb root 未抛出错误"
        } catch {
            [pscustomobject]@{
                AttemptCount = $script:RootInvocationCount
                Message = $_.Exception.Message
            }
        }
    }

    Assert-TestCondition -Condition ($FailureMessage.AttemptCount -eq 2) -Message "持续失败的 adb root 未达到重试上限"
    Assert-TestCondition -Condition ($FailureMessage.Message -match "lastExit=7") -Message "最终错误未包含最后 exit"
    Assert-TestCondition -Condition ($FailureMessage.Message -match "lastOutput=closed-final") -Message "最终错误未包含最后 output"
}
# //// /验证持续 root 失败保留最后一次 ADB 错误 ////

# //// 验证 root 重试遵守 15 秒截止时间和清理预留 [@x380kkm 2026-07-29] ////
function Test-ProtocolLabAdbRootRespectsDeadlineAndCleanupReserve {
    param([Parameter(Mandatory)][System.Management.Automation.PSModuleInfo]$Module)

    $Result = & $Module {
        $script:StartedAt = [DateTime]::ParseExact("2026-07-29T12:00:00", "yyyy-MM-ddTHH:mm:ss", [Globalization.CultureInfo]::InvariantCulture)
        $script:VirtualNow = $script:StartedAt
        $script:RootInvocationCount = 0
        $script:RootCalls = @()
        function Get-Date {
            $script:VirtualNow
        }
        function Start-Sleep {
            param([int]$Seconds = 0, [int]$Milliseconds = 0)

            $script:VirtualNow = $script:VirtualNow.AddSeconds($Seconds).AddMilliseconds($Milliseconds)
        }
        function Invoke-ProtocolLabAdb {
            param([string]$AdbPath, [string]$Serial, [string[]]$CommandArguments, [int]$AdbServerPort, [switch]$AllowFailure, [int]$TimeoutSeconds)

            $script:RootInvocationCount++
            $script:RootCalls += [pscustomobject]@{
                At = $script:VirtualNow
                TimeoutSeconds = $TimeoutSeconds
            }
            [pscustomobject]@{ ExitCode = 1; Output = @("closed") }
        }

        try {
            Invoke-ProtocolLabAdbRootWithRetry -AdbPath "adb.exe" -Serial "emulator-5554" -AdbServerPort 5041 -TimeoutSeconds 15
            throw "持续失败的 adb root 未抛出错误"
        } catch {
            [pscustomobject]@{
                AttemptCount = $script:RootInvocationCount
                Calls = $script:RootCalls
                FinishedAt = $script:VirtualNow
                Message = $_.Exception.Message
            }
        }
    }

    $Deadline = $Result.Calls[0].At.AddSeconds(15)
    Assert-TestCondition -Condition ($Result.FinishedAt -eq $Deadline) -Message "默认 root 重试未持续到 15 秒截止时间"
    Assert-TestCondition -Condition ($Result.AttemptCount -eq $Result.Calls.Count) -Message "错误 attempts 未反映实际 root 调用次数"
    Assert-TestCondition -Condition ($Result.Message -match "attempts=$($Result.AttemptCount)") -Message "最终错误未包含实际 root 调用次数"
    foreach ($Call in $Result.Calls) {
        $RemainingSeconds = [int][Math]::Floor(($Deadline - $Call.At).TotalSeconds)
        Assert-TestCondition -Condition ($Call.At -lt $Deadline) -Message "截止时间后仍执行了 adb root"
        Assert-TestCondition -Condition ($Call.TimeoutSeconds -gt 0 -and $Call.TimeoutSeconds -le ($RemainingSeconds - 4)) -Message "单次 root 命令 timeout 未为清理预留 4 秒"
    }
}
# //// /验证 root 重试遵守 15 秒截止时间和清理预留 ////

# //// 验证 root 短截止时间在参数绑定阶段拒绝 [@x380kkm 2026-07-29] ////
function Test-ProtocolLabAdbRootRejectsShortTimeout {
    param([Parameter(Mandatory)][System.Management.Automation.PSModuleInfo]$Module)

    $Failure = & $Module {
        try {
            Invoke-ProtocolLabAdbRootWithRetry -AdbPath "adb.exe" -Serial "emulator-5554" -AdbServerPort 5041 -TimeoutSeconds 5
            $false
        } catch [System.Management.Automation.ParameterBindingException] {
            $true
        }
    }

    Assert-TestCondition -Condition $Failure -Message "不足 6 秒的 root 截止时间未在参数绑定阶段拒绝"
}
# //// /验证 root 短截止时间在参数绑定阶段拒绝 ////

# //// 运行 ADB root 重试单元测试 [@x380kkm 2026-07-29] ////
$ModulePath = Join-Path $PSScriptRoot "protocol-lab.psm1"
$Module = Import-Module -Name $ModulePath -Force -PassThru
try {
    Test-ProtocolLabAdbRootRetriesTransientFailure -Module $Module
    Test-ProtocolLabAdbRootReportsFinalFailure -Module $Module
    Test-ProtocolLabAdbRootRespectsDeadlineAndCleanupReserve -Module $Module
    Test-ProtocolLabAdbRootRejectsShortTimeout -Module $Module
} finally {
    Remove-Module -ModuleInfo $Module -Force
}
# //// /运行 ADB root 重试单元测试 ////
