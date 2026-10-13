# audience: internal
# # android-adb-entrypoints-tests
# AST 提取的安装分支使用本地替身验证状态端口, 所属进程和共享调用边界.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'protocol-lab.psm1') -Force

# //// 定位安装入口并拒绝直接 ADB 调用 [@x380kkm 2026-09-20] ////
$EntrypointAsts = @{}
foreach ($Name in @('install-android-cn-distribution.ps1', 'test-android-apk-bundle.ps1')) {
    $ParseErrors = $null
    $Ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $Name), [ref]$null, [ref]$ParseErrors)
    if ($ParseErrors.Count -gt 0) { throw ($ParseErrors -join [Environment]::NewLine) }
    $RawCalls = $Ast.FindAll({
        param($Node)
        $Node -is [Management.Automation.Language.CommandAst] -and
        ($Node.GetCommandName() -in @('adb', 'adb.exe') -or
         ($Node.InvocationOperator -eq 'Ampersand' -and $Node.CommandElements[0].Extent.Text -match 'AdbPath$'))
    }, $true)
    if ($RawCalls.Count -gt 0) { throw "$Name 存在直接 ADB 调用" }
    $EntrypointAsts[$Name] = $Ast
}
$InstallBlock = $EntrypointAsts['test-android-apk-bundle.ps1'].Find({
    param($Node)
    $Node -is [Management.Automation.Language.IfStatementAst] -and $Node.Clauses[0].Item1.Extent.Text -ceq '$Install'
}, $true).Clauses[0].Item2
$InstallBody = [ScriptBlock]::Create($InstallBlock.Extent.Text.Trim().Substring(1).TrimEnd().TrimEnd('}'))

# //// 用替身执行 bundle 安装分支并记录边界调用 [@x380kkm 2026-09-20] ////
function Invoke-BundleBranchWithMocks {
    param([string]$TargetSerial, [bool]$OwnerRunning = $true, [string]$AbiList = 'x86_64,arm64-v8a')
    $Events = [Collections.Generic.List[object]]::new()
    $Serial = $TargetSerial
    $RepositoryRoot = $PSScriptRoot
    $ProjectAdbPath = Join-Path $PSScriptRoot 'mock-adb.exe'
    $AdbPath = $ProjectAdbPath
    $BundleApkPath = 'mock.apk'
    $InspectorReport = @{}
    $State = [pscustomobject]@{
        Headless=$true; AdbIsolation='one-device'; Serial='emulator-5562'; AdbServerPort=51991
        AdbPath=$ProjectAdbPath; EmulatorProcessId=12345; EmulatorPath='owned-emulator.exe'
        EmulatorStartTimeUtc='2026-09-20T00:00:00Z'
    }
    function Get-ProtocolLabPaths { param($RepositoryRoot) [pscustomobject]@{EmulatorStatePath='mock-state.json'} }
    function Read-ProtocolLabState { param($Path) $State }
    function Get-OwnedProtocolLabProcess {
        param($ProcessId, $ExecutablePath, $StartTimeUtc)
        if ($ProcessId -ne 12345 -or $ExecutablePath -cne 'owned-emulator.exe') { throw '所属进程参数不一致' }
        if ($OwnerRunning) { [pscustomobject]@{Id=$ProcessId} }
    }
    function Start-ProtocolLabAdbServer {
        param($AdbPath, $Serial, $AdbServerPort)
        $Events.Add([pscustomobject]@{Kind='server'; Serial=$Serial; Port=$AdbServerPort})
    }
    function Invoke-ProtocolLabAdb {
        param($AdbPath, $Serial, $AdbServerPort, $CommandArguments)
        $Events.Add([pscustomobject]@{Kind='command'; Serial=$Serial; Port=$AdbServerPort; Arguments=$CommandArguments})
        $Output = if ($CommandArguments[0] -eq 'shell') { @($AbiList) } else { @('Success') }
        [pscustomobject]@{ExitCode=0; Output=$Output}
    }
    $Failure = $null
    $Output = $null
    try { $Output = & $InstallBody } catch { $Failure = $_.Exception.Message }
    [pscustomobject]@{Events=@($Events.ToArray()); Failure=$Failure; Output=$Output}
}

# //// 验证状态推导和安装前拒绝条件 [@x380kkm 2026-09-20] ////
$Accepted = Invoke-BundleBranchWithMocks
if ($null -ne $Accepted.Failure) { throw $Accepted.Failure }
if ($Accepted.Events.Count -ne 3) { throw 'bundle 分支未通过共享服务和两条目标命令' }
foreach ($Event in $Accepted.Events) {
    if ($Event.Serial -cne 'emulator-5562' -or $Event.Port -ne 51991) { throw 'bundle 分支改变了状态目标或端口' }
}
$Report = ($Accepted.Output -join '') | ConvertFrom-Json
if ($Report.Install.adb_port -ne 51991) { throw 'bundle 回执端口与项目状态不一致' }
$Mismatch = Invoke-BundleBranchWithMocks -TargetSerial 'emulator-5598'
if ($null -eq $Mismatch.Failure -or $Mismatch.Events.Count -ne 0) { throw '目标不一致时仍启动了设备操作' }
$Stopped = Invoke-BundleBranchWithMocks -OwnerRunning $false
if ($null -eq $Stopped.Failure -or $Stopped.Events.Count -ne 0) { throw '所属进程停止时仍启动了设备操作' }
$Unsupported = Invoke-BundleBranchWithMocks -AbiList 'x86_64'
if ($null -eq $Unsupported.Failure -or $Unsupported.Events.Count -ne 2) { throw 'ABI 不匹配时仍提交了安装' }

# //// 验证分发入口的命令委托保留目标和端口 [@x380kkm 2026-09-20] ////
$DistributionFunctions = $EntrypointAsts['install-android-cn-distribution.ps1'].FindAll({
    param($Node)
    $Node -is [Management.Automation.Language.FunctionDefinitionAst] -and
    $Node.Name -in @('Start-DistributionAdbServer', 'Assert-DistributionEmulator', 'Invoke-DistributionEmulatorAdb')
}, $true)
& {
    $script:AdbPath = 'mock-adb.exe'
    $script:AdbServerPort = 51991
    $Serial = 'emulator-5562'
    $Events = [Collections.Generic.List[object]]::new()
    function Start-ProtocolLabAdbServer {
        param($AdbPath, $Serial, $AdbServerPort)
        $Events.Add([pscustomobject]@{Serial=$Serial; Port=$AdbServerPort})
    }
    function Invoke-ProtocolLabAdb {
        param($AdbPath, $Serial, $AdbServerPort, $CommandArguments)
        $Events.Add([pscustomobject]@{Serial=$Serial; Port=$AdbServerPort})
        $Output = if ($CommandArguments[0] -eq 'shell') { @('x86_64,arm64-v8a') } else { @('Success') }
        [pscustomobject]@{ExitCode=0; Output=$Output}
    }
    foreach ($Function in $DistributionFunctions) { . ([ScriptBlock]::Create($Function.Extent.Text)) }
    Start-DistributionAdbServer
    Assert-DistributionEmulator
    $Output = @(Invoke-DistributionEmulatorAdb -ArgumentList @('install', '-r', 'mock.apk'))
    if ($Events.Count -ne 3 -or $Output[0] -cne 'Success') { throw '分发命令委托不完整' }
    foreach ($Event in $Events) {
        if ($Event.Serial -cne 'emulator-5562' -or $Event.Port -ne 51991) { throw '分发入口改变了状态目标或端口' }
    }
}
Write-Output '两个入口的目标推导, 进程保护, ABI 拒绝和共享 ADB 委托验证通过.'
