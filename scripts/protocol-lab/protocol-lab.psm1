# audience: internal
# # protocol-lab
# 此模块提供 Android 模拟器协议实验所需的路径解析, ADB 调用, 状态保存和进程归属检查.
# 所有运行时产物写入仓库同级的 artifacts, 源码目录不保存设备状态或捕获内容.

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# //// 解析实验目录和 Android SDK 位置 [@x380kkm 2026-07-20] ////
function Get-ProtocolLabPaths {
    [CmdletBinding()]
    param(
        [string]$RepositoryRoot,
        [string]$SdkRoot,
        [string]$AvdHome
    )

    if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
        $RepositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    }

    $RepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
    $WorkspaceRoot = [IO.Path]::GetFullPath((Split-Path -Parent $RepositoryRoot))
    $AndroidArtifactsRoot = Join-Path $WorkspaceRoot "artifacts\android-sdk"
    if ([string]::IsNullOrWhiteSpace($SdkRoot)) {
        $SdkRoot = Join-Path $AndroidArtifactsRoot "sdk"
    }
    if ([string]::IsNullOrWhiteSpace($AvdHome)) {
        $AvdHome = Join-Path $AndroidArtifactsRoot "avd"
    }

    $ArtifactsRoot = Join-Path $WorkspaceRoot "artifacts\protocol-lab"
    $StateDirectory = Join-Path $ArtifactsRoot "state"
    [pscustomobject]@{
        RepositoryRoot = $RepositoryRoot
        WorkspaceRoot = $WorkspaceRoot
        AndroidArtifactsRoot = [IO.Path]::GetFullPath($AndroidArtifactsRoot)
        SdkRoot = [IO.Path]::GetFullPath($SdkRoot)
        AvdHome = [IO.Path]::GetFullPath($AvdHome)
        ArtifactsRoot = [IO.Path]::GetFullPath($ArtifactsRoot)
        RunDirectory = [IO.Path]::GetFullPath((Join-Path $ArtifactsRoot "runs"))
        StateDirectory = [IO.Path]::GetFullPath($StateDirectory)
        EmulatorStatePath = [IO.Path]::GetFullPath((Join-Path $StateDirectory "emulator.json"))
        ProbeStatePath = [IO.Path]::GetFullPath((Join-Path $StateDirectory "multiplayer-probe.json"))
    }
}
# //// /解析实验目录和 Android SDK 位置 ////

# //// 验证外部工具文件存在 [@x380kkm 2026-07-20] ////
function Assert-ProtocolLabFile {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        [string]$Description
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Description 不存在: $Path"
    }
}
# //// /验证外部工具文件存在 ////

# //// 按参数, 状态, 环境和兼容默认值解析 ADB server 端口 [@x380kkm 2026-08-18] ////
function Resolve-ProtocolLabAdbServerPort {
    [CmdletBinding()]
    param(
        [AllowNull()]
        [Nullable[int]]$RequestedPort,
        [AllowNull()]
        [object]$State
    )

    if ($null -ne $RequestedPort) {
        if ($RequestedPort -lt 1 -or $RequestedPort -gt 65535) {
            throw "ADB server 端口超出有效范围: $RequestedPort"
        }
        return [int]$RequestedPort
    }

    if ($null -ne $State -and $null -ne $State.PSObject.Properties["AdbServerPort"]) {
        $StatePort = 0
        if (-not [int]::TryParse([string]$State.AdbServerPort, [ref]$StatePort) -or $StatePort -lt 1 -or $StatePort -gt 65535) {
            throw "模拟器状态中的 ADB server 端口无效: $($State.AdbServerPort)"
        }
        return $StatePort
    }

    $EnvironmentPortText = [Environment]::GetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", [EnvironmentVariableTarget]::Process)
    if ([string]::IsNullOrEmpty($EnvironmentPortText)) {
        return 5037
    }

    $EnvironmentPort = 0
    if (-not [int]::TryParse($EnvironmentPortText, [ref]$EnvironmentPort) -or $EnvironmentPort -lt 1 -or $EnvironmentPort -gt 65535) {
        throw "ANDROID_ADB_SERVER_PORT 无效: $EnvironmentPortText"
    }
    $EnvironmentPort
}
# //// /按参数, 状态, 环境和兼容默认值解析 ADB server 端口 ////

# //// 查找可供 SDK 工具使用的 JDK 17 [@x380kkm 2026-07-20] ////
function Resolve-ProtocolLabJavaHome {
    [CmdletBinding()]
    param([string]$JavaHome)

    $Candidates = [Collections.Generic.List[string]]::new()
    if (-not [string]::IsNullOrWhiteSpace($JavaHome)) {
        $Candidates.Add($JavaHome)
    }
    if (-not [string]::IsNullOrWhiteSpace($env:JAVA_HOME)) {
        $Candidates.Add($env:JAVA_HOME)
    }

    $MicrosoftRoot = Join-Path $env:LOCALAPPDATA "Programs\Microsoft"
    if (Test-Path -LiteralPath $MicrosoftRoot -PathType Container) {
        Get-ChildItem -LiteralPath $MicrosoftRoot -Directory -Filter "jdk-17*" |
            Sort-Object Name -Descending |
            ForEach-Object { $Candidates.Add($_.FullName) }
    }

    foreach ($Candidate in $Candidates) {
        $Resolved = [IO.Path]::GetFullPath($Candidate)
        if (Test-Path -LiteralPath (Join-Path $Resolved "bin\java.exe") -PathType Leaf) {
            return $Resolved
        }
    }

    throw "未找到 JDK 17. 请设置 JAVA_HOME 或传入 -JavaHome."
}
# //// /查找可供 SDK 工具使用的 JDK 17 ////

# //// 查找可直接启动探针的 Python 3.12 [@x380kkm 2026-07-20] ////
function Resolve-ProtocolLabPythonPath {
    [CmdletBinding()]
    param([string]$PythonPath)

    if (-not [string]::IsNullOrWhiteSpace($PythonPath)) {
        $Resolved = [IO.Path]::GetFullPath($PythonPath)
        Assert-ProtocolLabFile -Path $Resolved -Description "Python"
        return $Resolved
    }

    $UvCommand = Get-Command uv -ErrorAction SilentlyContinue
    if ($null -ne $UvCommand) {
        $UvPython = @(& $UvCommand.Source python find 3.12 2>$null | ForEach-Object { $_.ToString() })
        if ($LASTEXITCODE -eq 0 -and $UvPython.Count -gt 0) {
            $Resolved = [IO.Path]::GetFullPath($UvPython[-1].Trim())
            if (Test-Path -LiteralPath $Resolved -PathType Leaf) {
                return $Resolved
            }
        }
    }

    $PythonCommand = Get-Command python -ErrorAction SilentlyContinue
    if ($null -ne $PythonCommand -and (Test-Path -LiteralPath $PythonCommand.Source -PathType Leaf)) {
        return [IO.Path]::GetFullPath($PythonCommand.Source)
    }

    throw "未找到 Python 3.12. 请安装 uv 管理的 Python 或传入 -PythonPath."
}
# //// /查找可直接启动探针的 Python 3.12 ////

# //// 在截止时间内执行指定模拟器的 ADB 进程 [@x380kkm 2026-07-20] ////
function Invoke-ProtocolLabAdbWithinTimeout {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [Parameter(Mandatory)]
        [string[]]$CommandArguments,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [Parameter(Mandatory)]
        [ValidateRange(1, 300)]
        [int]$TimeoutSeconds
    )

    $StartInfo = [Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $AdbPath
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    foreach ($Argument in @("-P", $AdbServerPort.ToString([Globalization.CultureInfo]::InvariantCulture), "-s", $Serial) + $CommandArguments) {
        $StartInfo.ArgumentList.Add($Argument)
    }

    $Process = [Diagnostics.Process]::new()
    $Process.StartInfo = $StartInfo
    try {
        $CleanupTimeoutMilliseconds = 2000
        if (-not $Process.Start()) {
            throw "ADB 进程未启动: $AdbPath"
        }
        $StandardOutputTask = $Process.StandardOutput.ReadToEndAsync()
        $StandardErrorTask = $Process.StandardError.ReadToEndAsync()
        $Completed = $Process.WaitForExit($TimeoutSeconds * 1000)
        $KillFailure = $null
        $TerminationCompleted = $Completed
        if (-not $Completed) {
            try {
                $Process.Kill($true)
            } catch {
                try {
                    $Process.Kill()
                } catch {
                    $KillFailure = $_.Exception.Message
                }
            }
            $TerminationCompleted = $Process.WaitForExit($CleanupTimeoutMilliseconds)
        }

        $OutputTasksCompleted = $false
        $OutputFailure = $null
        try {
            $OutputTasks = [Threading.Tasks.Task[]]@($StandardOutputTask, $StandardErrorTask)
            $OutputTasksCompleted = [Threading.Tasks.Task]::WaitAll($OutputTasks, $CleanupTimeoutMilliseconds)
        } catch {
            $OutputFailure = $_.Exception.Message
        }
        $Output = @()
        if ($OutputTasksCompleted) {
            $Output = @(
                foreach ($Text in @($StandardOutputTask.GetAwaiter().GetResult(), $StandardErrorTask.GetAwaiter().GetResult())) {
                    if (-not [string]::IsNullOrEmpty($Text)) {
                        $Text -split "\r?\n" | Where-Object { -not [string]::IsNullOrEmpty($_) }
                    }
                }
            )
        }
        if (-not $Completed) {
            $Output += "ADB 命令在 $TimeoutSeconds 秒后超时."
        }
        if (-not $TerminationCompleted) {
            $Output += "ADB 进程未在 $CleanupTimeoutMilliseconds 毫秒的清理期限内退出."
        }
        if (-not $OutputTasksCompleted) {
            $Output += "ADB 输出未在 $CleanupTimeoutMilliseconds 毫秒的清理期限内关闭."
        }
        if (-not [string]::IsNullOrEmpty($KillFailure)) {
            $Output += "ADB 进程终止失败: $KillFailure"
        }
        if (-not [string]::IsNullOrEmpty($OutputFailure)) {
            $Output += "ADB 输出读取失败: $OutputFailure"
        }

        [pscustomobject]@{
            # 退出码 124 表示宿主侧超时.
            ExitCode = if ($Completed -and $OutputTasksCompleted) { $Process.ExitCode } else { 124 }
            Output = $Output
        }
    } finally {
        $Process.Dispose()
    }
}
# //// /在截止时间内执行指定模拟器的 ADB 进程 ////

# //// 执行指定模拟器的 ADB 命令并保留完整错误 [@x380kkm 2026-07-20] ////
function Invoke-ProtocolLabAdb {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [Parameter(Mandatory)]
        [string[]]$CommandArguments,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [switch]$AllowFailure,
        [ValidateRange(0, 300)]
        [int]$TimeoutSeconds = 0
    )

    Assert-ProtocolLabFile -Path $AdbPath -Description "adb"
    if ($TimeoutSeconds -gt 0) {
        $Result = Invoke-ProtocolLabAdbWithinTimeout -AdbPath $AdbPath -Serial $Serial -CommandArguments $CommandArguments -AdbServerPort $AdbServerPort -TimeoutSeconds $TimeoutSeconds
        $Output = $Result.Output
        $ExitCode = $Result.ExitCode
    } else {
        $Output = @(& $AdbPath -P $AdbServerPort -s $Serial @CommandArguments 2>&1 | ForEach-Object { $_.ToString() })
        $ExitCode = $LASTEXITCODE
    }
    if ($ExitCode -ne 0 -and -not $AllowFailure) {
        $Rendered = $Output -join [Environment]::NewLine
        throw "ADB 命令失败, exit=${ExitCode}: adb -P $AdbServerPort -s $Serial $($CommandArguments -join ' ')`n$Rendered"
    }

    [pscustomobject]@{
        ExitCode = $ExitCode
        Output = $Output
    }
}
# //// /执行指定模拟器的 ADB 命令并保留完整错误 ////

# //// 等待 Android framework 完成首次启动 [@x380kkm 2026-07-20] ////
function Wait-ProtocolLabAndroidBoot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [ValidateRange(30, 1800)]
        [int]$TimeoutSeconds = 180,
        [Parameter(Mandatory)]
        [System.Diagnostics.Process]$EmulatorProcess,
        [Parameter(Mandatory)]
        [string]$EmulatorStderrPath,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $DeviceConnected = $false
    do {
        if ($EmulatorProcess.HasExited) {
            $StderrTail = if (Test-Path -LiteralPath $EmulatorStderrPath -PathType Leaf) {
                @(Get-Content -LiteralPath $EmulatorStderrPath -Tail 40) -join [Environment]::NewLine
            } else {
                "stderr 文件不存在: $EmulatorStderrPath"
            }
            throw "Emulator 在 ADB 就绪前退出, exit=$($EmulatorProcess.ExitCode).`n$StderrTail"
        }

        $DeviceState = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("get-state") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
        if ($DeviceState.ExitCode -eq 0 -and ($DeviceState.Output -join "").Trim() -eq "device") {
            $DeviceConnected = $true
            break
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $Deadline)
    if (-not $DeviceConnected) {
        throw "ADB 设备未在 $TimeoutSeconds 秒内连接: $Serial"
    }

    do {
        if ($EmulatorProcess.HasExited) {
            $StderrTail = if (Test-Path -LiteralPath $EmulatorStderrPath -PathType Leaf) {
                @(Get-Content -LiteralPath $EmulatorStderrPath -Tail 40) -join [Environment]::NewLine
            } else {
                "stderr 文件不存在: $EmulatorStderrPath"
            }
            throw "Emulator 在 Android 启动完成前退出, exit=$($EmulatorProcess.ExitCode).`n$StderrTail"
        }

        $Result = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "getprop", "sys.boot_completed") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
        if ($Result.ExitCode -eq 0 -and ($Result.Output -join "").Trim() -eq "1") {
            return
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $Deadline)

    throw "Android 未在 $TimeoutSeconds 秒内完成启动: $Serial"
}
# //// /等待 Android framework 完成首次启动 ////

# //// 判断默认路由是否固定经过 Emulator 宿主网关 [@x380kkm 2026-07-20] ////
function Test-ProtocolLabDefaultCaptureRoute {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [string[]]$DefaultRoutes,
        [Parameter(Mandatory)]
        [string]$HostAddress
    )

    if ($DefaultRoutes.Count -ne 1) {
        return $false
    }
    $NormalizedRoute = ($DefaultRoutes[0] -replace "[ \t]+", " ").Trim()
    $NormalizedRoute -eq "default via $HostAddress dev eth0 onlink"
}
# //// /判断默认路由是否固定经过 Emulator 宿主网关 ////

# //// 判断优先级 1 是否先查询主路由表 [@x380kkm 2026-07-20] ////
function Test-ProtocolLabMainRouteRule {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [string[]]$PriorityOneRules
    )

    if ($PriorityOneRules.Count -ne 1) {
        return $false
    }
    $NormalizedRule = ($PriorityOneRules[0] -replace "[ \t]+", " ").Trim()
    $NormalizedRule -eq "1: from all lookup main"
}
# //// /判断优先级 1 是否先查询主路由表 ////

# //// 读取捕获默认路由和主路由表规则的当前状态 [@x380kkm 2026-07-20] ////
function Get-ProtocolLabCaptureRouteState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [Parameter(Mandatory)]
        [string]$HostAddress,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    $RouteResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "route", "show", "default") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
    $RuleResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "rule", "show") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
    $RouteText = $RouteResult.Output -join "`n"
    $RuleText = $RuleResult.Output -join "`n"
    $DefaultRoutes = @($RouteText -split "\r?\n" | Where-Object { $_ -match "^default(?:[ \t]|$)" })
    $PriorityOneRules = @($RuleText -split "\r?\n" | Where-Object { $_ -match "^[ \t]*1:" })
    $HasDefaultRoute = $RouteResult.ExitCode -eq 0 -and (Test-ProtocolLabDefaultCaptureRoute -DefaultRoutes $DefaultRoutes -HostAddress $HostAddress)
    $HasMainRouteRule = $RuleResult.ExitCode -eq 0 -and (Test-ProtocolLabMainRouteRule -PriorityOneRules $PriorityOneRules)

    [pscustomobject]@{
        RouteQueryExitCode = $RouteResult.ExitCode
        RuleQueryExitCode = $RuleResult.ExitCode
        RouteText = $RouteText
        RuleText = $RuleText
        DefaultRouteCount = $DefaultRoutes.Count
        PriorityOneRuleCount = $PriorityOneRules.Count
        HasDefaultRoute = $HasDefaultRoute
        HasMainRouteRule = $HasMainRouteRule
        IsReady = $HasDefaultRoute -and $HasMainRouteRule
    }
}
# //// /读取捕获默认路由和主路由表规则的当前状态 ////

# //// 在有限重试内修复并验证捕获路由 [@x380kkm 2026-07-20] ////
function Repair-ProtocolLabCaptureRoute {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [string]$HostAddress = "10.0.2.2",
        [ValidateRange(1, 10)]
        [int]$MaximumAttempts = 3,
        [ValidateRange(0, 5000)]
        [int]$RetryDelayMilliseconds = 500,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    $Failures = [Collections.Generic.List[string]]::new()
    $MaximumRuleDeletesPerAttempt = 4
    $State = $null
    for ($Attempt = 1; $Attempt -le $MaximumAttempts; $Attempt++) {
        $State = Get-ProtocolLabCaptureRouteState -AdbPath $AdbPath -Serial $Serial -HostAddress $HostAddress -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
        if ($State.IsReady) {
            return
        }

        if ($State.RouteQueryExitCode -ne 0) {
            $Failures.Add("attempt=${Attempt} route-query exit=$($State.RouteQueryExitCode): $($State.RouteText)")
        } elseif (-not $State.HasDefaultRoute) {
            $RouteFlush = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "route", "flush", "default") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
            if ($RouteFlush.ExitCode -ne 0) {
                $Failures.Add("attempt=${Attempt} route-flush exit=$($RouteFlush.ExitCode): $($RouteFlush.Output -join ' ')")
            }
            $RouteRepair = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "route", "replace", "default", "via", $HostAddress, "dev", "eth0", "onlink") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
            if ($RouteRepair.ExitCode -ne 0) {
                $Failures.Add("attempt=${Attempt} route-repair exit=$($RouteRepair.ExitCode): $($RouteRepair.Output -join ' ')")
            }
        }

        if ($State.RuleQueryExitCode -ne 0) {
            $Failures.Add("attempt=${Attempt} rule-query exit=$($State.RuleQueryExitCode): $($State.RuleText)")
        } elseif (-not $State.HasMainRouteRule) {
            $RuleDeleteCount = [Math]::Min($State.PriorityOneRuleCount, $MaximumRuleDeletesPerAttempt)
            for ($RuleDeleteIndex = 0; $RuleDeleteIndex -lt $RuleDeleteCount; $RuleDeleteIndex++) {
                $RuleDelete = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "rule", "del", "priority", "1") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
                if ($RuleDelete.ExitCode -ne 0) {
                    $Failures.Add("attempt=${Attempt} rule-delete exit=$($RuleDelete.ExitCode): $($RuleDelete.Output -join ' ')")
                    break
                }
            }
            $RuleRepair = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "rule", "add", "priority", "1", "lookup", "main") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
            if ($RuleRepair.ExitCode -ne 0) {
                $Failures.Add("attempt=${Attempt} rule-repair exit=$($RuleRepair.ExitCode): $($RuleRepair.Output -join ' ')")
            }
        }

        $State = Get-ProtocolLabCaptureRouteState -AdbPath $AdbPath -Serial $Serial -HostAddress $HostAddress -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
        if ($State.IsReady) {
            return
        }
        if ($Attempt -lt $MaximumAttempts -and $RetryDelayMilliseconds -gt 0) {
            Start-Sleep -Milliseconds $RetryDelayMilliseconds
        }
    }

    $FailureText = if ($Failures.Count -gt 0) { $Failures -join "; " } else { "none" }
    $ExpectedRoute = "default via $HostAddress dev eth0 onlink"
    throw "捕获路由自愈失败: expectedRoute='$ExpectedRoute' actualRoutes='$($State.RouteText)' routeQueryExit=$($State.RouteQueryExitCode); expectedRule='priority 1 lookup main' actualRules='$($State.RuleText)' ruleQueryExit=$($State.RuleQueryExitCode); failures=$FailureText"
}
# //// /在有限重试内修复并验证捕获路由 ////

# //// 在截止时间内重试 ADB root 并保留最后一次失败 [@x380kkm 2026-07-29] ////
function Invoke-ProtocolLabAdbRootWithRetry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [ValidateRange(6, 60)]
        [int]$TimeoutSeconds = 15,
        [ValidateRange(1, 120)]
        [int]$MaximumAttempts = 40,
        [ValidateRange(0, 5000)]
        [int]$RetryDelayMilliseconds = 1500,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 300)]
        [int]$CommandTimeoutSeconds = 300
    )

    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $CleanupReserveSeconds = 4
    $LastResult = $null
    $InvocationCount = 0
    for ($Attempt = 1; $Attempt -le $MaximumAttempts; $Attempt++) {
        $RemainingMilliseconds = [Math]::Max(0, [int][Math]::Floor(($Deadline - (Get-Date)).TotalMilliseconds))
        $RemainingSeconds = [int][Math]::Floor($RemainingMilliseconds / 1000)
        $RemainingCommandTimeoutSeconds = $RemainingSeconds - $CleanupReserveSeconds
        if ($RemainingCommandTimeoutSeconds -lt 1) {
            if ($RemainingMilliseconds -gt 0) {
                Start-Sleep -Milliseconds $RemainingMilliseconds
            }
            break
        }
        $InvocationTimeoutSeconds = [Math]::Min($RemainingCommandTimeoutSeconds, $CommandTimeoutSeconds)

        $InvocationCount++
        $LastResult = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("root") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $InvocationTimeoutSeconds
        if ($LastResult.ExitCode -eq 0) {
            return $LastResult
        }

        $RemainingMilliseconds = [Math]::Max(0, [int][Math]::Floor(($Deadline - (Get-Date)).TotalMilliseconds))
        if ($Attempt -eq $MaximumAttempts -or $RemainingMilliseconds -eq 0) {
            break
        }
        if ($RetryDelayMilliseconds -gt 0) {
            Start-Sleep -Milliseconds ([Math]::Min($RetryDelayMilliseconds, $RemainingMilliseconds))
        }
    }

    $LastOutput = if ($null -eq $LastResult) { "" } else { $LastResult.Output -join [Environment]::NewLine }
    $LastExitCode = if ($null -eq $LastResult) { "unknown" } else { $LastResult.ExitCode }
    throw "ADB root 在 ${TimeoutSeconds} 秒内失败: serial=$Serial attempts=$InvocationCount lastExit=$LastExitCode lastOutput=$LastOutput"
}
# //// /在截止时间内重试 ADB root 并保留最后一次失败 ////

# //// 启用可由 Emulator 自带 PCAP 捕获的以太网路由 [@x380kkm 2026-07-20] ////
function Enable-ProtocolLabCaptureRoute {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [string]$HostAddress = "10.0.2.2",
        [ValidateRange(6, 60)]
        [int]$RootTimeoutSeconds = 15,
        [ValidateRange(1, 120)]
        [int]$RootMaximumAttempts = 40,
        [ValidateRange(0, 5000)]
        [int]$RootRetryDelayMilliseconds = 1500,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    $RootResult = Invoke-ProtocolLabAdbRootWithRetry -AdbPath $AdbPath -Serial $Serial -TimeoutSeconds $RootTimeoutSeconds -MaximumAttempts $RootMaximumAttempts -RetryDelayMilliseconds $RootRetryDelayMilliseconds -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
    $ReconnectDeadline = (Get-Date).AddSeconds(60)
    $DeviceReconnected = $false
    do {
        $DeviceState = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("get-state") -AdbServerPort $AdbServerPort -AllowFailure -TimeoutSeconds $CommandTimeoutSeconds
        if ($DeviceState.ExitCode -eq 0 -and ($DeviceState.Output -join "").Trim() -eq "device") {
            $DeviceReconnected = $true
            break
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $ReconnectDeadline)
    if (-not $DeviceReconnected) {
        throw "adbd 以 root 重启后未在 60 秒内重新连接: $Serial"
    }

    $Identity = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "id") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds
    if (($Identity.Output -join " ") -notmatch "uid=0\(root\)") {
        throw "系统镜像不支持 adb root: $($RootResult.Output -join ' ')"
    }

    Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "svc", "wifi", "disable") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
    $Deadline = (Get-Date).AddSeconds(20)
    do {
        $Routes = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "route") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds
        $RouteText = $Routes.Output -join "`n"
        if ($RouteText -match "(?m)^10\.0\.2\.0/24 dev eth0(?:\s+.*)?$") {
            break
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $Deadline)
    if ($RouteText -notmatch "(?m)^10\.0\.2\.0/24 dev eth0(?:\s+.*)?$") {
        Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "link", "set", "eth0", "up") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
        Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "address", "replace", "10.0.2.15/24", "dev", "eth0") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
        $Routes = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ip", "route") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds
        $RouteText = $Routes.Output -join "`n"
        if ($RouteText -notmatch "(?m)^10\.0\.2\.0/24 dev eth0(?:\s+.*)?$") {
            throw "eth0 静态地址配置失败: $RouteText"
        }
    }

    Repair-ProtocolLabCaptureRoute -AdbPath $AdbPath -Serial $Serial -HostAddress $HostAddress -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
    Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ping", "-c", "2", $HostAddress) -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
}
# //// /启用可由 Emulator 自带 PCAP 捕获的以太网路由 ////

# //// 验证 PCAP 在设备发包后真实增长 [@x380kkm 2026-07-20] ////
function Test-ProtocolLabPcapGrowth {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [Parameter(Mandatory)]
        [string]$PcapPath,
        [string]$HostAddress = "10.0.2.2",
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    if (-not (Test-Path -LiteralPath $PcapPath -PathType Leaf)) {
        throw "Emulator 未创建 PCAP: $PcapPath"
    }
    $Before = (Get-Item -LiteralPath $PcapPath).Length
    Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "ping", "-c", "8", "-s", "1400", "-i", "0.02", $HostAddress) -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
    Start-Sleep -Seconds 1
    $After = (Get-Item -LiteralPath $PcapPath).Length
    if ($After -le $Before) {
        throw "PCAP 未在设备发包后增长: before=$Before after=$After path=$PcapPath"
    }

    [pscustomobject]@{
        BeforeBytes = $Before
        AfterBytes = $After
        AddedBytes = $After - $Before
    }
}
# //// /验证 PCAP 在设备发包后真实增长 ////

# //// 原子保存实验进程状态 [@x380kkm 2026-07-20] ////
function Write-ProtocolLabState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        [object]$State
    )

    $Directory = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $Directory | Out-Null
    $TemporaryPath = "$Path.tmp"
    $State | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $TemporaryPath -Encoding utf8
    Move-Item -LiteralPath $TemporaryPath -Destination $Path -Force
}
# //// /原子保存实验进程状态 ////

# //// 读取实验进程状态 [@x380kkm 2026-07-20] ////
function Read-ProtocolLabState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [switch]$AllowMissing
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        if ($AllowMissing) {
            return $null
        }
        throw "实验状态文件不存在: $Path"
    }
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}
# //// /读取实验进程状态 ////

# //// 确认 PID 仍属于本次启动的可执行文件 [@x380kkm 2026-07-20] ////
function Get-OwnedProtocolLabProcess {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [int]$ProcessId,
        [Parameter(Mandatory)]
        [string]$ExecutablePath,
        [Parameter(Mandatory)]
        [datetime]$StartTimeUtc
    )

    $Process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $Process) {
        return $null
    }

    $ActualPath = [IO.Path]::GetFullPath($Process.Path)
    $ExpectedPath = [IO.Path]::GetFullPath($ExecutablePath)
    $StartDelta = [math]::Abs(($Process.StartTime.ToUniversalTime() - $StartTimeUtc.ToUniversalTime()).TotalSeconds)
    if ($ActualPath -ne $ExpectedPath -or $StartDelta -gt 2) {
        throw "PID $ProcessId 不再属于记录的实验进程."
    }
    $Process
}
# //// /确认 PID 仍属于本次启动的可执行文件 ////

# //// 读取 Android 全局 HTTP 代理值 [@x380kkm 2026-07-20] ////
function Get-ProtocolLabProxy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    $Result = Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "settings", "get", "global", "http_proxy") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds
    ($Result.Output -join "").Trim()
}
# //// /读取 Android 全局 HTTP 代理值 ////

# //// 禁用 Android 全局 HTTP 代理并返回原值 [@x380kkm 2026-07-20] ////
function Disable-ProtocolLabProxy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    $OriginalProxy = Get-ProtocolLabProxy -AdbPath $AdbPath -Serial $Serial -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
    Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "settings", "put", "global", "http_proxy", ":0") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
    $CurrentProxy = Get-ProtocolLabProxy -AdbPath $AdbPath -Serial $Serial -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
    if ($CurrentProxy -ne ":0") {
        throw "Android 全局 HTTP 代理未禁用: $CurrentProxy"
    }

    $OriginalProxy
}
# //// /禁用 Android 全局 HTTP 代理并返回原值 ////

# //// 恢复 Android 全局 HTTP 代理原值 [@x380kkm 2026-07-20] ////
function Restore-ProtocolLabProxy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$AdbPath,
        [Parameter(Mandatory)]
        [string]$Serial,
        [AllowEmptyString()]
        [string]$ProxyValue,
        [ValidateRange(1, 65535)]
        [int]$AdbServerPort = 5037,
        [ValidateRange(1, 30)]
        [int]$CommandTimeoutSeconds = 5
    )

    if ([string]::IsNullOrWhiteSpace($ProxyValue) -or $ProxyValue -eq "null") {
        Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "settings", "delete", "global", "http_proxy") -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
        $RestoredProxy = Get-ProtocolLabProxy -AdbPath $AdbPath -Serial $Serial -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
        if (-not [string]::IsNullOrWhiteSpace($RestoredProxy) -and $RestoredProxy -ne "null") {
            throw "Android 全局 HTTP 代理未恢复为空值: $RestoredProxy"
        }
        return
    }

    Invoke-ProtocolLabAdb -AdbPath $AdbPath -Serial $Serial -CommandArguments @("shell", "settings", "put", "global", "http_proxy", $ProxyValue) -AdbServerPort $AdbServerPort -TimeoutSeconds $CommandTimeoutSeconds | Out-Null
    $RestoredProxy = Get-ProtocolLabProxy -AdbPath $AdbPath -Serial $Serial -AdbServerPort $AdbServerPort -CommandTimeoutSeconds $CommandTimeoutSeconds
    if ($RestoredProxy -ne $ProxyValue) {
        throw "Android 全局 HTTP 代理未恢复: expected=$ProxyValue actual=$RestoredProxy"
    }
}
# //// /恢复 Android 全局 HTTP 代理原值 ////

Export-ModuleMember -Function @(
    "Assert-ProtocolLabFile",
    "Disable-ProtocolLabProxy",
    "Enable-ProtocolLabCaptureRoute",
    "Get-OwnedProtocolLabProcess",
    "Get-ProtocolLabPaths",
    "Get-ProtocolLabProxy",
    "Invoke-ProtocolLabAdb",
    "Read-ProtocolLabState",
    "Repair-ProtocolLabCaptureRoute",
    "Resolve-ProtocolLabAdbServerPort",
    "Restore-ProtocolLabProxy",
    "Resolve-ProtocolLabJavaHome",
    "Resolve-ProtocolLabPythonPath",
    "Test-ProtocolLabPcapGrowth",
    "Wait-ProtocolLabAndroidBoot",
    "Write-ProtocolLabState"
)
