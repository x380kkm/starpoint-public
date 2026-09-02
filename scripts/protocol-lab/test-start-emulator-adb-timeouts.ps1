# audience: internal
# # test-start-emulator-adb-timeouts
# 此脚本以本机替身和 AST 验证有限 ADB 超时和独立 server 端口传递.

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

# //// 从 AST 中返回唯一命名函数 [@x380kkm 2026-07-29] ////
function Get-FunctionAst {
    param(
        [Parameter(Mandatory)]
        [System.Management.Automation.Language.Ast]$Ast,
        [Parameter(Mandatory)]
        [string]$Name
    )

    $Definitions = @($Ast.FindAll({ param($Node) $Node -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true))
    $Matches = @($Definitions | Where-Object { $_.Name -eq $Name })
    Assert-TestCondition -Condition ($Matches.Count -eq 1) -Message "未找到唯一函数: $Name"
    $Matches[0]
}
# //// /从 AST 中返回唯一命名函数 ////

# //// 从 AST 中返回指定命令调用 [@x380kkm 2026-07-29] ////
function Get-CommandAsts {
    param(
        [Parameter(Mandatory)]
        [System.Management.Automation.Language.Ast]$Ast,
        [Parameter(Mandatory)]
        [string]$CommandName
    )

    $Commands = @($Ast.FindAll({ param($Node) $Node -is [System.Management.Automation.Language.CommandAst] }, $true))
    @($Commands | Where-Object { $_.GetCommandName() -eq $CommandName })
}
# //// /从 AST 中返回指定命令调用 ////

# //// 验证启动器和模块的 ADB 调用都传入有限超时 [@x380kkm 2026-07-29] ////
function Test-StaticAdbTimeoutCoverage {
    param(
        [Parameter(Mandatory)]
        [string]$StartEmulatorPath,
        [Parameter(Mandatory)]
        [string]$ModulePath
    )

    $StartTokens = $null
    $StartErrors = $null
    $StartAst = [System.Management.Automation.Language.Parser]::ParseFile($StartEmulatorPath, [ref]$StartTokens, [ref]$StartErrors)
    Assert-TestCondition -Condition ($StartErrors.Count -eq 0) -Message "start-emulator.ps1 AST 解析失败: $($StartErrors -join '; ')"

    $TimeoutParameters = @($StartAst.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq "AdbCommandTimeoutSeconds" })
    Assert-TestCondition -Condition ($TimeoutParameters.Count -eq 1) -Message "启动器未定义 AdbCommandTimeoutSeconds"
    Assert-TestCondition -Condition ($TimeoutParameters[0].Extent.Text -match '(?s)\[ValidateRange\(1,\s*30\)\].*\[int\]\$AdbCommandTimeoutSeconds\s*=\s*5') -Message "启动器 ADB 超时未使用有限默认值"

    $DirectAdbCalls = @(Get-CommandAsts -Ast $StartAst -CommandName "Invoke-ProtocolLabAdb")
    Assert-TestCondition -Condition ($DirectAdbCalls.Count -eq 3) -Message "启动器的直接 ADB 调用数量已变化, 请更新超时覆盖测试"
    foreach ($Call in $DirectAdbCalls) {
        Assert-TestCondition -Condition ($Call.Extent.Text -match '(?s)-TimeoutSeconds\s+\$AdbCommandTimeoutSeconds\b') -Message "启动器存在未传入 AdbCommandTimeoutSeconds 的 ADB 调用: $($Call.Extent.Text)"
    }

    $ForwardedCommands = @{
        "Wait-ProtocolLabAndroidBoot" = 1
        "Enable-ProtocolLabCaptureRoute" = 1
        "Disable-ProtocolLabProxy" = 1
        "Repair-ProtocolLabCaptureRoute" = 1
        "Test-ProtocolLabPcapGrowth" = 1
        "Restore-ProtocolLabProxy" = 1
    }
    foreach ($CommandName in $ForwardedCommands.Keys) {
        $Calls = @(Get-CommandAsts -Ast $StartAst -CommandName $CommandName)
        Assert-TestCondition -Condition ($Calls.Count -eq $ForwardedCommands[$CommandName]) -Message "启动器的 $CommandName 调用数量已变化, 请更新超时覆盖测试"
        foreach ($Call in $Calls) {
            Assert-TestCondition -Condition ($Call.Extent.Text -match '(?s)-CommandTimeoutSeconds\s+\$AdbCommandTimeoutSeconds\b') -Message "启动器未将 ADB 超时传给 ${CommandName}: $($Call.Extent.Text)"
        }
    }

    $ServerTimeoutCalls = @{
        "Start-ProtocolLabAdbServer" = 1
        "Stop-ProtocolLabAdbServer" = 2
        "Get-ProtocolLabAdbDevices" = 1
    }
    foreach ($CommandName in $ServerTimeoutCalls.Keys) {
        $Calls = @(Get-CommandAsts -Ast $StartAst -CommandName $CommandName)
        Assert-TestCondition -Condition ($Calls.Count -eq $ServerTimeoutCalls[$CommandName]) -Message "启动器的 $CommandName 调用数量已变化"
        foreach ($Call in $Calls) {
            Assert-TestCondition -Condition ($Call.Extent.Text -match '(?s)-TimeoutSeconds\s+\$AdbCommandTimeoutSeconds\b') -Message "启动器未将 ADB 超时传给 ${CommandName}: $($Call.Extent.Text)"
        }
    }

    $ModuleTokens = $null
    $ModuleErrors = $null
    $ModuleAst = [System.Management.Automation.Language.Parser]::ParseFile($ModulePath, [ref]$ModuleTokens, [ref]$ModuleErrors)
    Assert-TestCondition -Condition ($ModuleErrors.Count -eq 0) -Message "protocol-lab.psm1 AST 解析失败: $($ModuleErrors -join '; ')"

    $HelperNames = @(
        "Wait-ProtocolLabAndroidBoot",
        "Get-ProtocolLabCaptureRouteState",
        "Repair-ProtocolLabCaptureRoute",
        "Enable-ProtocolLabCaptureRoute",
        "Test-ProtocolLabPcapGrowth",
        "Get-ProtocolLabProxy",
        "Disable-ProtocolLabProxy",
        "Restore-ProtocolLabProxy"
    )
    foreach ($HelperName in $HelperNames) {
        $Helper = Get-FunctionAst -Ast $ModuleAst -Name $HelperName
        $TimeoutParameters = @($Helper.Body.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq "CommandTimeoutSeconds" })
        Assert-TestCondition -Condition ($TimeoutParameters.Count -eq 1) -Message "$HelperName 未定义 CommandTimeoutSeconds"
        $AdbCalls = @(Get-CommandAsts -Ast $Helper.Body -CommandName "Invoke-ProtocolLabAdb")
        Assert-TestCondition -Condition ($AdbCalls.Count -gt 0) -Message "$HelperName 未调用 ADB"
        foreach ($Call in $AdbCalls) {
            Assert-TestCondition -Condition ($Call.Extent.Text -match '(?s)-TimeoutSeconds\s+\$CommandTimeoutSeconds\b') -Message "$HelperName 存在未传入 CommandTimeoutSeconds 的 ADB 调用: $($Call.Extent.Text)"
        }
    }

    $NestedForwarders = @{
        "Repair-ProtocolLabCaptureRoute" = @("Get-ProtocolLabCaptureRouteState")
        "Enable-ProtocolLabCaptureRoute" = @("Invoke-ProtocolLabAdbRootWithRetry", "Repair-ProtocolLabCaptureRoute")
        "Disable-ProtocolLabProxy" = @("Get-ProtocolLabProxy")
        "Restore-ProtocolLabProxy" = @("Get-ProtocolLabProxy")
    }
    foreach ($HelperName in $NestedForwarders.Keys) {
        $Helper = Get-FunctionAst -Ast $ModuleAst -Name $HelperName
        foreach ($NestedCommandName in $NestedForwarders[$HelperName]) {
            $NestedCalls = @(Get-CommandAsts -Ast $Helper.Body -CommandName $NestedCommandName)
            Assert-TestCondition -Condition ($NestedCalls.Count -gt 0) -Message "$HelperName 未调用 $NestedCommandName"
            foreach ($Call in $NestedCalls) {
                Assert-TestCondition -Condition ($Call.Extent.Text -match '(?s)-CommandTimeoutSeconds\s+\$CommandTimeoutSeconds\b') -Message "$HelperName 未将 ADB 超时传给 ${NestedCommandName}: $($Call.Extent.Text)"
            }
        }
    }

    $RootRetry = Get-FunctionAst -Ast $ModuleAst -Name "Invoke-ProtocolLabAdbRootWithRetry"
    $RootTimeoutParameters = @($RootRetry.Body.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq "CommandTimeoutSeconds" })
    Assert-TestCondition -Condition ($RootTimeoutParameters.Count -eq 1) -Message "ADB root 重试未定义单次命令超时"
    Assert-TestCondition -Condition ($RootRetry.Extent.Text -match '(?s)\$InvocationTimeoutSeconds\s*=\s*\[Math\]::Min\(\$RemainingCommandTimeoutSeconds,\s*\$CommandTimeoutSeconds\)') -Message "ADB root 重试未同时保留截止时间和单次命令上限"
    $RootAdbCalls = @(Get-CommandAsts -Ast $RootRetry.Body -CommandName "Invoke-ProtocolLabAdb")
    Assert-TestCondition -Condition ($RootAdbCalls.Count -eq 1 -and $RootAdbCalls[0].Extent.Text -match '(?s)-TimeoutSeconds\s+\$InvocationTimeoutSeconds\b') -Message "ADB root 重试未使用有限单次超时"

    $AdbInvoker = Get-FunctionAst -Ast $ModuleAst -Name "Invoke-ProtocolLabAdb"
    Assert-TestCondition -Condition ($AdbInvoker.Extent.Text -match '\[int\]\$TimeoutSeconds\s*=\s*0') -Message "Invoke-ProtocolLabAdb 的兼容默认值不应改变"
}
# //// /验证启动器和模块的 ADB 调用都传入有限超时 ////

# //// 验证 Provider 观察器的 ADB 调用都传入有限超时 [@x380kkm 2026-08-03] ////
function Test-ProviderObserverAdbTimeoutCoverage {
    param(
        [Parameter(Mandatory)]
        [string]$ProbeProviderObserverPath
    )

    $ProbeTokens = $null
    $ProbeErrors = $null
    $ProbeAst = [System.Management.Automation.Language.Parser]::ParseFile($ProbeProviderObserverPath, [ref]$ProbeTokens, [ref]$ProbeErrors)
    Assert-TestCondition -Condition ($ProbeErrors.Count -eq 0) -Message "probe-provider-observer.ps1 AST 解析失败: $($ProbeErrors -join '; ')"

    $TimeoutParameters = @($ProbeAst.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq "AdbCommandTimeoutSeconds" })
    Assert-TestCondition -Condition ($TimeoutParameters.Count -eq 1) -Message "Provider 观察器未定义 AdbCommandTimeoutSeconds"
    Assert-TestCondition -Condition ($TimeoutParameters[0].Extent.Text -match '(?s)\[ValidateRange\(1,\s*30\)\].*\[int\]\$AdbCommandTimeoutSeconds\s*=\s*10') -Message "Provider 观察器 ADB 超时未使用有限默认值"
    $InstallTimeoutParameters = @($ProbeAst.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq "ApkInstallTimeoutSeconds" })
    Assert-TestCondition -Condition ($InstallTimeoutParameters.Count -eq 1) -Message "Provider 观察器未定义 APK 安装超时"
    Assert-TestCondition -Condition ($InstallTimeoutParameters[0].Extent.Text -match '(?s)\[ValidateRange\(10,\s*300\)\].*\[int\]\$ApkInstallTimeoutSeconds\s*=\s*180') -Message "Provider 观察器 APK 安装未使用独立有限超时"

    $AdbCalls = @(Get-CommandAsts -Ast $ProbeAst -CommandName "Invoke-ProtocolLabAdb")
    Assert-TestCondition -Condition ($AdbCalls.Count -eq 7) -Message "Provider 观察器的直接 ADB 调用数量已变化, 请更新超时覆盖测试"
    $InstallCalls = @($AdbCalls | Where-Object { $_.Extent.Text -match "-CommandArguments\s+@\('install'" })
    Assert-TestCondition -Condition ($InstallCalls.Count -eq 1) -Message "Provider 观察器 APK 安装调用位置已变化"
    foreach ($Call in $AdbCalls) {
        $ExpectedTimeout = if ($Call.Extent.Text -match "-CommandArguments\s+@\('install'") { '\$ApkInstallTimeoutSeconds' } else { '\$AdbCommandTimeoutSeconds' }
        Assert-TestCondition -Condition ($Call.Extent.Text -match "(?s)-TimeoutSeconds\s+$ExpectedTimeout\b") -Message "Provider 观察器存在未传入正确 ADB 超时的调用: $($Call.Extent.Text)"
    }

    $ProbeContent = [IO.File]::ReadAllText($ProbeProviderObserverPath)
    Assert-TestCondition -Condition (-not $ProbeContent.Contains('& $State.AdbPath', [StringComparison]::Ordinal)) -Message "Provider 观察器仍直接启动未限时的 ADB 进程"
}
# //// /验证 Provider 观察器的 ADB 调用都传入有限超时 ////

# //// 验证 emulator 入口和共享模块传递同一 ADB server 端口 [@x380kkm 2026-08-18] ////
function Test-AdbServerPortContract {
    param(
        [Parameter(Mandatory)]
        [string]$StartEmulatorPath,
        [Parameter(Mandatory)]
        [string]$InstallClientPath,
        [Parameter(Mandatory)]
        [string]$VerifyEmulatorPath,
        [Parameter(Mandatory)]
        [string]$StopEmulatorPath,
        [Parameter(Mandatory)]
        [string]$ExportClientApksPath,
        [Parameter(Mandatory)]
        [string]$ProbeProviderObserverPath,
        [Parameter(Mandatory)]
        [string]$ModulePath
    )

    $Paths = @($StartEmulatorPath, $InstallClientPath, $VerifyEmulatorPath, $StopEmulatorPath, $ExportClientApksPath, $ProbeProviderObserverPath, $ModulePath)
    $Asts = @{}
    foreach ($Path in $Paths) {
        $Tokens = $null
        $Errors = $null
        $Asts[$Path] = [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref]$Tokens, [ref]$Errors)
        Assert-TestCondition -Condition ($Errors.Count -eq 0) -Message "$Path AST 解析失败: $($Errors -join '; ')"
    }

    $StartAst = $Asts[$StartEmulatorPath]
    $StartPort = @($StartAst.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq "AdbServerPort" })
    Assert-TestCondition -Condition ($StartPort.Count -eq 1 -and $StartPort[0].Extent.Text -match '\[Nullable\[int\]\]\$AdbServerPort\b' -and $StartPort[0].Extent.Text -notmatch '=') -Message "启动器 AdbServerPort 必须可空以便按需分配专用端口"
    $StartContent = [IO.File]::ReadAllText($StartEmulatorPath)
    Assert-TestCondition -Condition ($StartContent -match '(?m)^\s*ANDROID_ADB_SERVER_PORT\s*=\s*\$ResolvedAdbServerPort\.ToString') -Message "Emulator 进程环境未指向已解析端口"
    Assert-TestCondition -Condition ($StartContent -notmatch '(?s)Start-Process.+-Environment\b') -Message "启动器使用了 PowerShell 7.0 不支持的 Start-Process -Environment"
    Assert-TestCondition -Condition ([regex]::Matches($StartContent, '(?m)^\s*AdbServerPort\s*=\s*\$ResolvedAdbServerPort\s*$').Count -ge 2) -Message "启动器未将已解析端口写入状态和输出"
    Assert-TestCondition -Condition ($StartContent -match '(?m)^\s*Start-ProtocolLabAdbServer\s+-AdbPath\s+\$AdbPath') -Message "启动器未启动专用 ADB server"
    Assert-TestCondition -Condition ($StartContent -match 'Headless\s*=\s*\$true' -and $StartContent -match 'AdbIsolation\s*=\s*"one-device"') -Message "启动器状态未记录无界面隔离标记"
    $StartResolver = @(Get-CommandAsts -Ast $StartAst -CommandName "Resolve-ProtocolLabAdbServerPort")
    Assert-TestCondition -Condition ($StartResolver.Count -eq 1 -and $StartResolver[0].Extent.Text -match '-RequestedPort\s+\$AdbServerPort\b.+-State\s+\$null\b') -Message "启动器未在首次 ADB 调用前解析端口"
    foreach ($CommandName in @("Invoke-ProtocolLabAdb", "Wait-ProtocolLabAndroidBoot", "Enable-ProtocolLabCaptureRoute", "Disable-ProtocolLabProxy", "Repair-ProtocolLabCaptureRoute", "Test-ProtocolLabPcapGrowth", "Restore-ProtocolLabProxy")) {
        foreach ($Call in @(Get-CommandAsts -Ast $StartAst -CommandName $CommandName)) {
            Assert-TestCondition -Condition ($Call.Extent.Text -match '-AdbServerPort\s+\$ResolvedAdbServerPort\b') -Message "启动器未将已解析端口传给 ${CommandName}"
        }
    }

    foreach ($Path in @($InstallClientPath, $VerifyEmulatorPath, $StopEmulatorPath, $ExportClientApksPath, $ProbeProviderObserverPath)) {
        $Ast = $Asts[$Path]
        $Port = @($Ast.ParamBlock.Parameters | Where-Object { $_.Name.VariablePath.UserPath -eq "AdbServerPort" })
        Assert-TestCondition -Condition ($Port.Count -eq 1) -Message "$Path 未定义 AdbServerPort"
        $Resolver = @(Get-CommandAsts -Ast $Ast -CommandName "Resolve-ProtocolLabAdbServerPort")
        Assert-TestCondition -Condition ($Resolver.Count -eq 1 -and $Resolver[0].Extent.Text -match '-RequestedPort\s+\$AdbServerPort\b.+-State\s+\$State\b') -Message "$Path 未从参数或状态解析端口"
        foreach ($Call in @(Get-CommandAsts -Ast $Ast -CommandName "Invoke-ProtocolLabAdb")) {
            Assert-TestCondition -Condition ($Call.Extent.Text -match '-AdbServerPort\s+\$ResolvedAdbServerPort\b') -Message "$Path 存在未使用已解析端口的 ADB 调用"
        }
        $Content = [IO.File]::ReadAllText($Path)
        Assert-TestCondition -Condition ($Content -notmatch '&\s+\$(?:State\.)?AdbPath\b') -Message "$Path 存在绕过共享封装的 ADB 调用"
    }

    $ModuleAst = $Asts[$ModulePath]
    $ModuleContent = [IO.File]::ReadAllText($ModulePath)
    Assert-TestCondition -Condition ($ModuleContent -match '@\("-P",\s*\$AdbServerPort') -Message "有超时 ADB 调用未使用 -P 端口"
    Assert-TestCondition -Condition ($ModuleContent -match '&\s*\$AdbPath\s+-P\s+\$AdbServerPort\s+-s\s+\$Serial') -Message "普通 ADB 调用未使用 -P 端口"
    Assert-TestCondition -Condition ($ModuleContent -match 'CommandArguments\s+@\("--one-device",\s*\$Serial,\s*"start-server"\)') -Message "共享模块未使用 --one-device 启动专用 ADB server"
    foreach ($Call in @(Get-CommandAsts -Ast $ModuleAst -CommandName "Invoke-ProtocolLabAdb")) {
        Assert-TestCondition -Condition ($Call.Extent.Text -match '-AdbServerPort\s+\$AdbServerPort\b') -Message "共享模块存在未传递端口的 ADB 调用"
    }

    $Module = Import-Module -Name $ModulePath -Force -PassThru
    $OriginalEnvironmentPort = [Environment]::GetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", [EnvironmentVariableTarget]::Process)
    try {
        [Environment]::SetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", $null, [EnvironmentVariableTarget]::Process)
        $DefaultPort = Resolve-ProtocolLabAdbServerPort -State ([pscustomobject]@{})
        [Environment]::SetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", "5038", [EnvironmentVariableTarget]::Process)
        $EnvironmentPort = Resolve-ProtocolLabAdbServerPort -State ([pscustomobject]@{})
        $StoredPort = Resolve-ProtocolLabAdbServerPort -State ([pscustomobject]@{ AdbServerPort = 5039 })
        $RequestedPort = Resolve-ProtocolLabAdbServerPort -RequestedPort 5040 -State ([pscustomobject]@{ AdbServerPort = 5039 })
        Assert-TestCondition -Condition ($DefaultPort -ne 5037 -and $DefaultPort -ne $EnvironmentPort) -Message "缺少参数时未为不同运行分配专用端口"
        Assert-TestCondition -Condition ($EnvironmentPort -ne 5038 -and $EnvironmentPort -ne 5037) -Message "端口解析不应读取全局 ANDROID_ADB_SERVER_PORT"
        Assert-TestCondition -Condition ($StoredPort -eq 5039) -Message "状态端口未覆盖环境端口"
        Assert-TestCondition -Condition ($RequestedPort -eq 5040) -Message "显式 ADB server 端口未覆盖状态端口"

        [Environment]::SetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", "5038", [EnvironmentVariableTarget]::Process)
        $InvalidStateFailed = $false
        try {
            Resolve-ProtocolLabAdbServerPort -State ([pscustomobject]@{ AdbServerPort = "invalid" }) | Out-Null
        } catch {
            $InvalidStateFailed = $_.Exception.Message -match "模拟器状态中的 ADB server 端口无效"
        }
        Assert-TestCondition -Condition $InvalidStateFailed -Message "非法状态端口未在使用环境变量前失败"
        $DefaultPortFailed = $false
        try {
            Resolve-ProtocolLabAdbServerPort -RequestedPort 5037 -State ([pscustomobject]@{}) | Out-Null
        } catch {
            $DefaultPortFailed = $true
        }
        Assert-TestCondition -Condition $DefaultPortFailed -Message "显式使用系统默认 ADB server 端口未被拒绝"
    } finally {
        [Environment]::SetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", $OriginalEnvironmentPort, [EnvironmentVariableTarget]::Process)
        Remove-Module -ModuleInfo $Module -Force
    }
}
# //// /验证 emulator 入口和共享模块传递同一 ADB server 端口 ////

# //// 验证 Android 入口只接受隔离 Emulator [@x380kkm 2026-09-01] ////
function Test-AdbIsolationContract {
    param(
        [Parameter(Mandatory)][string]$SetupEmulatorPath,
        [Parameter(Mandatory)][string]$StartEmulatorPath,
        [Parameter(Mandatory)][string]$InstallClientPath,
        [Parameter(Mandatory)][string]$VerifyEmulatorPath,
        [Parameter(Mandatory)][string]$StopEmulatorPath,
        [Parameter(Mandatory)][string]$ExportClientApksPath,
        [Parameter(Mandatory)][string]$ModulePath
    )

    $Entrypoints = @($SetupEmulatorPath, $StartEmulatorPath, $InstallClientPath, $VerifyEmulatorPath, $StopEmulatorPath, $ExportClientApksPath, $ModulePath)
    foreach ($Path in $Entrypoints) {
        $Content = [IO.File]::ReadAllText($Path)
        Assert-TestCondition -Condition ($Content -notmatch '(?i)Get-Command\s+adb(?:\.exe)?') -Message "$Path 存在 PATH ADB 回退"
        Assert-TestCondition -Condition ($Content -notmatch '\$AdbServerPort\s*=\s*5037') -Message "$Path 存在系统默认 ADB server 端口回退"
    }

    $ModuleContent = [IO.File]::ReadAllText($ModulePath)
    Assert-TestCondition -Condition ($ModuleContent -match 'Assert-ProtocolLabEmulatorSerial') -Message "共享模块未限制 emulator-* serial"
    Assert-TestCondition -Condition ($ModuleContent -match 'ro\.kernel\.qemu') -Message "共享模块未验证 ro.kernel.qemu"
    Assert-TestCondition -Condition ($ModuleContent -match 'CommandArguments\s+@\("--one-device",\s*\$Serial,\s*"start-server"\)') -Message "共享模块未用 --one-device 启动 ADB server"
    Assert-TestCondition -Condition ($ModuleContent -match 'New-ProtocolLabAdbServerPort') -Message "共享模块未分配随机专用 ADB server 端口"

    foreach ($Path in @($InstallClientPath, $VerifyEmulatorPath, $StopEmulatorPath, $ExportClientApksPath)) {
        $Content = [IO.File]::ReadAllText($Path)
        Assert-TestCondition -Condition ($Content -match 'Assert-ProtocolLabEmulatorState') -Message "$Path 未校验隔离模拟器状态"
    }
}
# //// /验证 Android 入口只接受隔离 Emulator ////

# //// 以本机替身执行启动器并记录超时透传 [@x380kkm 2026-07-29] ////
function Invoke-StartEmulatorWithMocks {
    param(
        [Parameter(Mandatory)]
        [string]$StartEmulatorPath,
        [Parameter(Mandatory)]
        [int]$AdbCommandTimeoutSeconds,
        [AllowNull()]
        [Nullable[int]]$AdbServerPort,
        [switch]$UseDefaultTimeout,
        [switch]$FailDuringCaptureValidation
    )

    $TestRoot = Join-Path ([IO.Path]::GetTempPath()) "start-emulator-adb-timeouts-$([Guid]::NewGuid().ToString('N'))"
    $RunDirectory = Join-Path $TestRoot "runs"
    New-Item -ItemType Directory -Force -Path $RunDirectory | Out-Null

    $global:StartEmulatorAdbTimeoutMock = [pscustomobject]@{
        Paths = [pscustomobject]@{
            SdkRoot = $TestRoot
            AvdHome = $TestRoot
            EmulatorStatePath = Join-Path $TestRoot "emulator-state.json"
            RunDirectory = $RunDirectory
        }
        TimeoutRecords = [Collections.Generic.List[object]]::new()
        State = $null
        StartProcessAdbServerPort = $null
        FailDuringCaptureValidation = $FailDuringCaptureValidation
    }
    $FailureMessage = $null
    $CapturedTimeoutRecords = @()
    $CapturedState = $null
    $CapturedStartProcessAdbServerPort = $null

    function Add-TimeoutRecord {
        param(
            [Parameter(Mandatory)]
            [string]$Name,
            [Parameter(Mandatory)]
            [int]$TimeoutSeconds,
            [Parameter(Mandatory)]
            [int]$AdbServerPort
        )

        $global:StartEmulatorAdbTimeoutMock.TimeoutRecords.Add([pscustomobject]@{
                Name = $Name
                TimeoutSeconds = $TimeoutSeconds
                AdbServerPort = $AdbServerPort
            })
    }

    function Import-Module {
        param(
            [Parameter(Position = 0)]
            [object]$Name,
            [switch]$Force
        )
    }

    function Resolve-ProtocolLabAdbServerPort {
        param(
            [AllowNull()][Nullable[int]]$RequestedPort,
            [AllowNull()][object]$State
        )

        if ($null -ne $RequestedPort) {
            return [int]$RequestedPort
        }
        $Listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
        try {
            $Listener.Start()
            return ([Net.IPEndPoint]$Listener.LocalEndpoint).Port
        } finally {
            $Listener.Stop()
        }
    }

    function Get-ProtocolLabPaths {
        param([string]$SdkRoot, [string]$AvdHome)

        $global:StartEmulatorAdbTimeoutMock.Paths
    }

    function Assert-ProtocolLabFile {
        param([string]$Path, [string]$Description)
    }

    function Start-ProtocolLabAdbServer {
        param([string]$AdbPath, [string]$Serial, [int]$AdbServerPort, [int]$TimeoutSeconds)

        Add-TimeoutRecord -Name "Start-ProtocolLabAdbServer" -TimeoutSeconds $TimeoutSeconds -AdbServerPort $AdbServerPort
    }

    function Stop-ProtocolLabAdbServer {
        param([string]$AdbPath, [int]$AdbServerPort, [switch]$AllowFailure, [int]$TimeoutSeconds)

        Add-TimeoutRecord -Name "Stop-ProtocolLabAdbServer" -TimeoutSeconds $TimeoutSeconds -AdbServerPort $AdbServerPort
    }

    function Get-ProtocolLabAdbDevices {
        param([string]$AdbPath, [int]$AdbServerPort, [switch]$AllowFailure, [int]$TimeoutSeconds)

        Add-TimeoutRecord -Name "Get-ProtocolLabAdbDevices" -TimeoutSeconds $TimeoutSeconds -AdbServerPort $AdbServerPort
        [pscustomobject]@{ ExitCode = 0; Output = @("List of devices attached") }
    }

    function Start-Process {
        param(
            [string]$FilePath,
            [object[]]$ArgumentList,
            [string]$WorkingDirectory,
            [object]$WindowStyle,
            [switch]$PassThru,
            [string]$RedirectStandardOutput,
            [string]$RedirectStandardError
        )

        $global:StartEmulatorAdbTimeoutMock.StartProcessAdbServerPort = [Environment]::GetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", [EnvironmentVariableTarget]::Process)
        [pscustomobject]@{
            Id = 4321
            StartTime = [DateTime]::ParseExact("2026-07-29T12:00:00", "yyyy-MM-ddTHH:mm:ss", [Globalization.CultureInfo]::InvariantCulture)
            HasExited = $false
        }
    }

    function Start-Sleep {
        param([int]$Seconds = 0, [int]$Milliseconds = 0)
    }

    function Stop-Process {
        param(
            [Parameter(ValueFromPipeline)]
            [object]$InputObject,
            [switch]$Force
        )

        process {
        }
    }

    function Invoke-ProtocolLabAdb {
        param(
            [string]$AdbPath,
            [string]$Serial,
            [string[]]$CommandArguments,
            [int]$AdbServerPort,
            [switch]$AllowFailure,
            [int]$TimeoutSeconds
        )

        $CommandName = $CommandArguments -join " "
        Add-TimeoutRecord -Name "Invoke-ProtocolLabAdb:$CommandName" -TimeoutSeconds $TimeoutSeconds -AdbServerPort $AdbServerPort
        if ($CommandName -eq "get-state") {
            return [pscustomobject]@{ ExitCode = 1; Output = @("offline") }
        }
        if ($CommandName -eq "shell getprop ro.product.cpu.abilist") {
            return [pscustomobject]@{ ExitCode = 0; Output = @("arm64-v8a") }
        }
        if ($CommandName -eq "shell getprop ro.dalvik.vm.native.bridge") {
            return [pscustomobject]@{ ExitCode = 0; Output = @("libndk_translation.so") }
        }
        [pscustomobject]@{ ExitCode = 0; Output = @() }
    }

    function Wait-ProtocolLabAndroidBoot {
        param(
            [string]$AdbPath,
            [string]$Serial,
            [int]$TimeoutSeconds,
            [object]$EmulatorProcess,
            [string]$EmulatorStderrPath,
            [int]$AdbServerPort,
            [int]$CommandTimeoutSeconds
        )

        Add-TimeoutRecord -Name "Wait-ProtocolLabAndroidBoot" -TimeoutSeconds $CommandTimeoutSeconds -AdbServerPort $AdbServerPort
    }

    function Enable-ProtocolLabCaptureRoute {
        param([string]$AdbPath, [string]$Serial, [string]$HostAddress, [int]$AdbServerPort, [int]$CommandTimeoutSeconds)

        Add-TimeoutRecord -Name "Enable-ProtocolLabCaptureRoute" -TimeoutSeconds $CommandTimeoutSeconds -AdbServerPort $AdbServerPort
    }

    function Disable-ProtocolLabProxy {
        param([string]$AdbPath, [string]$Serial, [int]$AdbServerPort, [int]$CommandTimeoutSeconds)

        Add-TimeoutRecord -Name "Disable-ProtocolLabProxy" -TimeoutSeconds $CommandTimeoutSeconds -AdbServerPort $AdbServerPort
        "proxy.example:8080"
    }

    function Repair-ProtocolLabCaptureRoute {
        param([string]$AdbPath, [string]$Serial, [string]$HostAddress, [int]$AdbServerPort, [int]$CommandTimeoutSeconds)

        Add-TimeoutRecord -Name "Repair-ProtocolLabCaptureRoute" -TimeoutSeconds $CommandTimeoutSeconds -AdbServerPort $AdbServerPort
    }

    function Test-ProtocolLabPcapGrowth {
        param([string]$AdbPath, [string]$Serial, [string]$PcapPath, [string]$HostAddress, [int]$AdbServerPort, [int]$CommandTimeoutSeconds)

        Add-TimeoutRecord -Name "Test-ProtocolLabPcapGrowth" -TimeoutSeconds $CommandTimeoutSeconds -AdbServerPort $AdbServerPort
        if ($global:StartEmulatorAdbTimeoutMock.FailDuringCaptureValidation) {
            throw "mock capture validation failure"
        }
        [pscustomobject]@{
            BeforeBytes = 1
            AfterBytes = 2
            AddedBytes = 1
        }
    }

    function Restore-ProtocolLabProxy {
        param([string]$AdbPath, [string]$Serial, [string]$ProxyValue, [int]$AdbServerPort, [int]$CommandTimeoutSeconds)

        Add-TimeoutRecord -Name "Restore-ProtocolLabProxy" -TimeoutSeconds $CommandTimeoutSeconds -AdbServerPort $AdbServerPort
    }

    function Write-ProtocolLabState {
        param([string]$Path, [object]$State)

        $global:StartEmulatorAdbTimeoutMock.State = $State
    }

    try {
        $InvocationParameters = @{
            SdkRoot = $TestRoot
            AvdHome = $TestRoot
        }
        if (-not $UseDefaultTimeout) {
            $InvocationParameters.AdbCommandTimeoutSeconds = $AdbCommandTimeoutSeconds
        }
        if ($null -ne $AdbServerPort) {
            $InvocationParameters.AdbServerPort = $AdbServerPort
        }
        & $StartEmulatorPath @InvocationParameters | Out-Null
    } catch {
        $FailureMessage = $_.Exception.Message
    } finally {
        $CapturedTimeoutRecords = @($global:StartEmulatorAdbTimeoutMock.TimeoutRecords)
        $CapturedState = $global:StartEmulatorAdbTimeoutMock.State
        $CapturedStartProcessAdbServerPort = $global:StartEmulatorAdbTimeoutMock.StartProcessAdbServerPort
        if (Test-Path -LiteralPath $TestRoot) {
            Remove-Item -LiteralPath $TestRoot -Recurse -Force
        }
        Remove-Variable -Name "StartEmulatorAdbTimeoutMock" -Scope Global
    }

    [pscustomobject]@{
        FailureMessage = $FailureMessage
        TimeoutRecords = $CapturedTimeoutRecords
        State = $CapturedState
        StartProcessAdbServerPort = $CapturedStartProcessAdbServerPort
    }
}
# //// /以本机替身执行启动器并记录超时透传 ////

# //// 验证一次启动的预期超时记录 [@x380kkm 2026-07-29] ////
function Assert-TimeoutRecords {
    param(
        [Parameter(Mandatory)]
        [object[]]$Records,
        [Parameter(Mandatory)]
        [string[]]$ExpectedNames,
        [Parameter(Mandatory)]
        [int]$ExpectedTimeoutSeconds
    )

    Assert-TestCondition -Condition ($Records.Count -eq $ExpectedNames.Count) -Message "启动器记录的超时调用数量不匹配: actual=$($Records.Count) expected=$($ExpectedNames.Count)"
    foreach ($ExpectedName in $ExpectedNames) {
        $Matches = @($Records | Where-Object { $_.Name -eq $ExpectedName })
        Assert-TestCondition -Condition ($Matches.Count -eq 1) -Message "启动器未记录唯一超时调用: $ExpectedName"
        Assert-TestCondition -Condition ($Matches[0].TimeoutSeconds -eq $ExpectedTimeoutSeconds) -Message "$ExpectedName 未收到预期 ADB 超时: actual=$($Matches[0].TimeoutSeconds) expected=$ExpectedTimeoutSeconds"
    }
}
# //// /验证一次启动的预期超时记录 ////

# //// 验证启动替身完整传递已解析 ADB server 端口 [@x380kkm 2026-08-18] ////
function Assert-AdbServerPortRun {
    param(
        [Parameter(Mandatory)]
        [object]$Run,
        [Parameter(Mandatory)]
        [int]$ExpectedPort
    )

    Assert-TestCondition -Condition ([string]::IsNullOrEmpty($Run.FailureMessage)) -Message "ADB server 端口启动替身失败: $($Run.FailureMessage)"
    Assert-TestCondition -Condition ($null -ne $Run.State -and $Run.State.AdbServerPort -eq $ExpectedPort) -Message "启动状态未写入预期 ADB server 端口: expected=$ExpectedPort"
    Assert-TestCondition -Condition ([int]$Run.StartProcessAdbServerPort -eq $ExpectedPort) -Message "Emulator 进程未继承预期 ADB server 端口: expected=$ExpectedPort actual=$($Run.StartProcessAdbServerPort)"
    $UnexpectedRecords = @($Run.TimeoutRecords | Where-Object { $_.AdbServerPort -ne $ExpectedPort })
    Assert-TestCondition -Condition ($UnexpectedRecords.Count -eq 0) -Message "启动器存在未传递预期 ADB server 端口的调用: expected=$ExpectedPort"
}
# //// /验证启动替身完整传递已解析 ADB server 端口 ////

# //// 验证启动器使用默认, 环境和显式 ADB server 端口 [@x380kkm 2026-08-18] ////
function Test-StartEmulatorAdbServerPortForwarding {
    param([Parameter(Mandatory)][string]$StartEmulatorPath)

    $OriginalEnvironmentPort = [Environment]::GetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", [EnvironmentVariableTarget]::Process)
    try {
        [Environment]::SetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", $null, [EnvironmentVariableTarget]::Process)
        $DefaultRun = Invoke-StartEmulatorWithMocks -StartEmulatorPath $StartEmulatorPath -AdbCommandTimeoutSeconds 5 -UseDefaultTimeout
        Assert-TestCondition -Condition ($DefaultRun.State.AdbServerPort -ne 5037) -Message "启动器默认使用了系统 ADB server 端口"
        Assert-AdbServerPortRun -Run $DefaultRun -ExpectedPort $DefaultRun.State.AdbServerPort
        Assert-TestCondition -Condition ([string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", [EnvironmentVariableTarget]::Process))) -Message "启动器未恢复空 ADB server 环境变量"

        [Environment]::SetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", "5038", [EnvironmentVariableTarget]::Process)
        $EnvironmentRun = Invoke-StartEmulatorWithMocks -StartEmulatorPath $StartEmulatorPath -AdbCommandTimeoutSeconds 5 -UseDefaultTimeout
        Assert-TestCondition -Condition ($EnvironmentRun.State.AdbServerPort -notin @(5037, 5038)) -Message "启动器读取了全局 ADB server 端口"
        Assert-AdbServerPortRun -Run $EnvironmentRun -ExpectedPort $EnvironmentRun.State.AdbServerPort
        Assert-TestCondition -Condition ([Environment]::GetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", [EnvironmentVariableTarget]::Process) -eq "5038") -Message "启动器未恢复原 ADB server 环境变量"

        $ExplicitRun = Invoke-StartEmulatorWithMocks -StartEmulatorPath $StartEmulatorPath -AdbCommandTimeoutSeconds 5 -AdbServerPort 5039 -UseDefaultTimeout
        Assert-AdbServerPortRun -Run $ExplicitRun -ExpectedPort 5039
        Assert-TestCondition -Condition ([Environment]::GetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", [EnvironmentVariableTarget]::Process) -eq "5038") -Message "显式端口启动后未恢复原环境变量"
    } finally {
        [Environment]::SetEnvironmentVariable("ANDROID_ADB_SERVER_PORT", $OriginalEnvironmentPort, [EnvironmentVariableTarget]::Process)
    }
}
# //// /验证启动器使用默认, 环境和显式 ADB server 端口 ////

# //// 验证默认值、自定义值和异常清理都透传 ADB 超时 [@x380kkm 2026-07-29] ////
function Test-StartEmulatorTimeoutForwarding {
    param([Parameter(Mandatory)][string]$StartEmulatorPath)

    $StartupRecords = @(
        "Start-ProtocolLabAdbServer",
        "Get-ProtocolLabAdbDevices",
        "Wait-ProtocolLabAndroidBoot",
        "Enable-ProtocolLabCaptureRoute",
        "Invoke-ProtocolLabAdb:shell getprop ro.product.cpu.abilist",
        "Invoke-ProtocolLabAdb:shell getprop ro.dalvik.vm.native.bridge",
        "Disable-ProtocolLabProxy",
        "Repair-ProtocolLabCaptureRoute",
        "Test-ProtocolLabPcapGrowth"
    )

    $DefaultRun = Invoke-StartEmulatorWithMocks -StartEmulatorPath $StartEmulatorPath -AdbCommandTimeoutSeconds 5 -AdbServerPort 5041 -UseDefaultTimeout
    Assert-TestCondition -Condition ([string]::IsNullOrEmpty($DefaultRun.FailureMessage)) -Message "默认 ADB 超时启动替身失败: $($DefaultRun.FailureMessage)"
    Assert-TimeoutRecords -Records $DefaultRun.TimeoutRecords -ExpectedNames $StartupRecords -ExpectedTimeoutSeconds 5

    $CustomRun = Invoke-StartEmulatorWithMocks -StartEmulatorPath $StartEmulatorPath -AdbCommandTimeoutSeconds 9 -AdbServerPort 5041
    Assert-TestCondition -Condition ([string]::IsNullOrEmpty($CustomRun.FailureMessage)) -Message "自定义 ADB 超时启动替身失败: $($CustomRun.FailureMessage)"
    Assert-TimeoutRecords -Records $CustomRun.TimeoutRecords -ExpectedNames $StartupRecords -ExpectedTimeoutSeconds 9

    $CleanupRun = Invoke-StartEmulatorWithMocks -StartEmulatorPath $StartEmulatorPath -AdbCommandTimeoutSeconds 7 -AdbServerPort 5041 -FailDuringCaptureValidation
    Assert-TestCondition -Condition ($CleanupRun.FailureMessage -match "mock capture validation failure") -Message "替身未进入启动器异常清理路径: $($CleanupRun.FailureMessage)"
    Assert-TimeoutRecords -Records $CleanupRun.TimeoutRecords -ExpectedNames ($StartupRecords + @("Restore-ProtocolLabProxy", "Invoke-ProtocolLabAdb:emu kill", "Stop-ProtocolLabAdbServer")) -ExpectedTimeoutSeconds 7
}
# //// /验证默认值、自定义值和异常清理都透传 ADB 超时 ////

# //// 运行启动器 ADB 超时测试 [@x380kkm 2026-07-29] ////
$StartEmulatorPath = Join-Path $PSScriptRoot "start-emulator.ps1"
$InstallClientPath = Join-Path $PSScriptRoot "install-client.ps1"
$VerifyEmulatorPath = Join-Path $PSScriptRoot "verify-emulator.ps1"
$StopEmulatorPath = Join-Path $PSScriptRoot "stop-emulator.ps1"
$ExportClientApksPath = Join-Path $PSScriptRoot "export-client-apks.ps1"
$ModulePath = Join-Path $PSScriptRoot "protocol-lab.psm1"
$SetupEmulatorPath = Join-Path $PSScriptRoot "setup-emulator.ps1"
$ProbeProviderObserverPath = Join-Path $PSScriptRoot "probe-provider-observer.ps1"
Test-StaticAdbTimeoutCoverage -StartEmulatorPath $StartEmulatorPath -ModulePath $ModulePath
Test-ProviderObserverAdbTimeoutCoverage -ProbeProviderObserverPath $ProbeProviderObserverPath
Test-AdbServerPortContract -StartEmulatorPath $StartEmulatorPath -InstallClientPath $InstallClientPath -VerifyEmulatorPath $VerifyEmulatorPath -StopEmulatorPath $StopEmulatorPath -ExportClientApksPath $ExportClientApksPath -ProbeProviderObserverPath $ProbeProviderObserverPath -ModulePath $ModulePath
Test-AdbIsolationContract -SetupEmulatorPath $SetupEmulatorPath -StartEmulatorPath $StartEmulatorPath -InstallClientPath $InstallClientPath -VerifyEmulatorPath $VerifyEmulatorPath -StopEmulatorPath $StopEmulatorPath -ExportClientApksPath $ExportClientApksPath -ModulePath $ModulePath
Test-StartEmulatorAdbServerPortForwarding -StartEmulatorPath $StartEmulatorPath
Test-StartEmulatorTimeoutForwarding -StartEmulatorPath $StartEmulatorPath
# //// /运行启动器 ADB 超时测试 ////
