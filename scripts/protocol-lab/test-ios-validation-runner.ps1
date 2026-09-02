# audience: internal
# # test-ios-validation-runner
# 此脚本验证 iOS 单次运行器的汇总, 脱敏, IPA 结构和清理约束.

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "ios-validation.psm1") -Force

# //// 断言测试条件成立 [@x380kkm 2026-08-18] ////
function Assert-TestCondition {
    param([Parameter(Mandatory)][bool]$Condition, [Parameter(Mandatory)][string]$Message)
    if (-not $Condition) { throw $Message }
}
# //// /断言测试条件成立 ////

# //// 构造可选择嵌入代码签名命令的 arm64 测试 IPA [@x380kkm 2026-08-18] ////
function New-TestIosIpa {
    param(
        [Parameter(Mandatory)][string]$IpaPath,
        [switch]$IncludeCodeSignature
    )

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::Open($IpaPath, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($binary in @(
            [pscustomobject]@{ Path = "Payload/Test.app/PersonalServiceDiagnostic"; FileType = 2; Signed = $IncludeCodeSignature.IsPresent },
            [pscustomobject]@{ Path = "Payload/Test.app/Frameworks/PersonalServiceBootstrap.framework/PersonalServiceBootstrap"; FileType = 6; Signed = $false }
        )) {
            $entry = $archive.CreateEntry($binary.Path)
            $stream = $entry.Open()
            try {
                $machOMagic = [uint32]::Parse("FEEDFACF", [Globalization.NumberStyles]::HexNumber)
                $arm64CpuType = [uint32]::Parse("0100000C", [Globalization.NumberStyles]::HexNumber)
                $commandCount = if ($binary.Signed) { [uint32]1 } else { [uint32]0 }
                $commandBytes = if ($binary.Signed) { [uint32]16 } else { [uint32]0 }
                foreach ($value in @(
                    $machOMagic,
                    $arm64CpuType,
                    [uint32]0,
                    [uint32]$binary.FileType,
                    $commandCount,
                    $commandBytes,
                    [uint32]0,
                    [uint32]0
                )) {
                    $bytes = [BitConverter]::GetBytes([uint32]$value)
                    $stream.Write($bytes, 0, $bytes.Length)
                }
                if ($binary.Signed) {
                    foreach ($value in @([uint32]0x1D, [uint32]16, [uint32]0, [uint32]0)) {
                        $bytes = [BitConverter]::GetBytes([uint32]$value)
                        $stream.Write($bytes, 0, $bytes.Length)
                    }
                }
            } finally {
                $stream.Dispose()
            }
        }
    } finally {
        $archive.Dispose()
    }
}
# //// /构造可选择嵌入代码签名命令的 arm64 测试 IPA ////

# //// 验证首个失败和依赖阻断汇总 [@x380kkm 2026-08-18] ////
$summary = Get-IosValidationSummary -Stages @(
    (New-IosValidationStage -Stage "LOCAL_REPOSITORY" -Status passed),
    (New-IosValidationStage -Stage "LOCAL_CONTRACTS" -Status passed),
    (New-IosValidationStage -Stage "REMOTE_TOOLCHAIN" -Status failed -ErrorCode "SSH_UNREACHABLE"),
    (New-IosValidationStage -Stage "DEVICE_ARTIFACT" -Status passed),
    (New-IosValidationStage -Stage "ARCHIVE" -Status blocked),
    (New-IosValidationStage -Stage "cleanup" -Status passed)
)
Assert-TestCondition ($summary.status -eq "failed") "失败流水线未标记 failed."
Assert-TestCondition ($summary.first_failure -eq "REMOTE_TOOLCHAIN") "首个失败阶段不正确."
Assert-TestCondition ($summary.root_blocker -eq "SSH_UNREACHABLE") "根阻断代码不正确."
Assert-TestCondition ($summary.last_successful_stage -eq "LOCAL_CONTRACTS") "最后成功阶段不正确."
Assert-TestCondition ($summary.blocked_stages -contains "ARCHIVE") "依赖阶段未标记 blocked."
Assert-TestCondition (($summary.independent_failures -join ",") -eq "REMOTE_TOOLCHAIN") "独立失败阶段不正确."
$diagnosticStage = New-IosValidationStage -Stage "internal_diagnostic" -Status passed -ExitCode 0
Assert-TestCondition ($diagnosticStage.exit_code -eq 0) "阶段退出码未保留."
Assert-TestCondition (($diagnosticStage.depends_on -join ",") -eq "app_launch") "阶段依赖未保留."
$backgroundStage = New-IosValidationStage -Stage "background_checkpoint" -Status passed -ExitCode 0
$foregroundStage = New-IosValidationStage -Stage "foreground_resume" -Status passed -ExitCode 0
$protocolStage = New-IosValidationStage -Stage "protocol_chain" -Status passed -ExitCode 0
$observationsStage = New-IosValidationStage -Stage "http_observations" -Status passed -ExitCode 0
$relaunchStage = New-IosValidationStage -Stage "relaunch" -Status passed -ExitCode 0
Assert-TestCondition (($backgroundStage.depends_on -join ",") -eq "loopback") "后台 checkpoint 阶段依赖不正确."
Assert-TestCondition (($foregroundStage.depends_on -join ",") -eq "background_checkpoint") "前台恢复阶段依赖不正确."
Assert-TestCondition (($protocolStage.depends_on -join ",") -eq "foreground_resume") "协议链未依赖前台恢复."
Assert-TestCondition (($observationsStage.depends_on -join ",") -eq "foreground_resume") "请求记录未依赖前台恢复."
Assert-TestCondition (($relaunchStage.depends_on -join ",") -eq "protocol_chain,http_observations") "进程重启未依赖协议链判定."
# //// /验证首个失败和依赖阻断汇总 ////

# //// 拒绝会进入远端 shell 的不安全参数 [@x380kkm 2026-08-18] ////
$unsafeSimulatorRejected = $false
try {
    Invoke-IosValidation -SimulatorName "iPhone'; touch /tmp/unsafe; '" | Out-Null
} catch {
    $unsafeSimulatorRejected = $_.FullyQualifiedErrorId -match "ParameterArgumentValidationError"
}
Assert-TestCondition $unsafeSimulatorRejected "Simulator 名称未拒绝 shell 元字符."
# //// /拒绝会进入远端 shell 的不安全参数 ////

# //// 注入关键失败并验证报告仍包含 cleanup [@x380kkm 2026-08-18] ////
$mockRoot = Join-Path ([IO.Path]::GetTempPath()) "ios-validation-mocks-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $mockRoot | Out-Null
try {
    $orderedStages = @(
        "LOCAL_REPOSITORY",
        "LOCAL_CONTRACTS",
        "REMOTE_TOOLCHAIN",
        "DEVICE_ARTIFACT",
        "ARCHIVE",
        "DEVICE_BUILD",
        "simulator_build",
        "simulator_boot",
        "app_install",
        "app_launch",
        "internal_diagnostic",
        "loopback",
        "background_checkpoint",
        "foreground_resume",
        "protocol_chain",
        "http_observations",
        "relaunch",
        "persistence"
    )
    $failureCases = @(
        [pscustomobject]@{ Name = "ssh"; Stage = "REMOTE_TOOLCHAIN"; Code = "SSH_UNREACHABLE" },
        [pscustomobject]@{ Name = "xcode"; Stage = "REMOTE_TOOLCHAIN"; Code = "XCODE_UNAVAILABLE" },
        [pscustomobject]@{ Name = "build"; Stage = "simulator_build"; Code = "SIMULATOR_BUILD_FAILED" },
        [pscustomobject]@{ Name = "launch"; Stage = "app_launch"; Code = "APP_INSTALL_FAILED" },
        [pscustomobject]@{ Name = "management"; Stage = "internal_diagnostic"; Code = "MANAGEMENT_AUTH_FAILED" },
        [pscustomobject]@{ Name = "management-features"; Stage = "internal_diagnostic"; Code = "MANAGEMENT_FEATURES_FAILED" },
        [pscustomobject]@{ Name = "background"; Stage = "background_checkpoint"; Code = "BACKGROUND_CHECKPOINT_FAILED" },
        [pscustomobject]@{ Name = "foreground"; Stage = "foreground_resume"; Code = "FOREGROUND_RESUME_FAILED" },
        [pscustomobject]@{ Name = "protocol"; Stage = "protocol_chain"; Code = "PROTOCOL_CHAIN_FAILED" },
        [pscustomobject]@{ Name = "protocol-http"; Stage = "protocol_chain"; Code = "JSON_HTTP_404" },
        [pscustomobject]@{ Name = "observations"; Stage = "http_observations"; Code = "HTTP_OBSERVATIONS_FAILED" },
        [pscustomobject]@{ Name = "persistence"; Stage = "persistence"; Code = "PERSISTENCE_REGRESSION" }
    )
    foreach ($failureCase in $failureCases) {
        $mockStages = [Collections.Generic.List[object]]::new()
        foreach ($stageName in $orderedStages) {
            if ($stageName -eq $failureCase.Stage) {
                $mockStages.Add((New-IosValidationStage -Stage $stageName -Status failed -ErrorCode $failureCase.Code -ExitCode 1))
                break
            }
            $mockStages.Add((New-IosValidationStage -Stage $stageName -Status passed -ExitCode 0))
        }
        $mockStages.Add((New-IosValidationStage -Stage "cleanup" -Status passed -ExitCode 0))
        $caseRoot = Join-Path $mockRoot $failureCase.Name
        New-Item -ItemType Directory -Force -Path $caseRoot | Out-Null
        $mockResult = Complete-IosValidationRun -OutputRoot $caseRoot -Commit "mock" -Stages $mockStages
        $mockReport = Get-Content -LiteralPath $mockResult.JsonReport -Raw -Encoding UTF8 | ConvertFrom-Json
        Assert-TestCondition ($mockReport.run_id -eq $failureCase.Name) "报告未记录 run ID."
        Assert-TestCondition ($mockResult.FirstFailure -eq $failureCase.Stage) "失败注入未定位到 $($failureCase.Stage)."
        Assert-TestCondition ($mockResult.RootBlocker -eq $failureCase.Code) "失败注入未保留 $($failureCase.Code)."
        Assert-TestCondition (@($mockReport.stages | Where-Object { $_.stage -eq $failureCase.Stage }).Count -eq 1) "失败阶段 $($failureCase.Stage) 未保持单一结果."
        Assert-TestCondition (@($mockReport.stages | Where-Object { $_.stage -eq "cleanup" -and $_.status -eq "passed" }).Count -eq 1) "失败注入未执行 cleanup."
    }
} finally {
    Remove-Item -LiteralPath $mockRoot -Recurse -Force
}
# //// /注入关键失败并验证报告仍包含 cleanup ////

# //// 拒绝缺失或重复的阶段结果 [@x380kkm 2026-08-18] ////
$integrityRoot = Join-Path ([IO.Path]::GetTempPath()) "ios-validation-integrity-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $integrityRoot | Out-Null
try {
    $completeStageNames = @(
        "LOCAL_REPOSITORY",
        "LOCAL_CONTRACTS",
        "REMOTE_TOOLCHAIN",
        "DEVICE_ARTIFACT",
        "ARCHIVE",
        "DEVICE_BUILD",
        "simulator_build",
        "simulator_boot",
        "app_install",
        "app_launch",
        "internal_diagnostic",
        "loopback",
        "background_checkpoint",
        "foreground_resume",
        "protocol_chain",
        "http_observations",
        "relaunch"
    )
    $missingPersistenceStages = @(
        $completeStageNames | ForEach-Object { New-IosValidationStage -Stage $_ -Status passed -ExitCode 0 }
    ) + @(New-IosValidationStage -Stage "cleanup" -Status passed -ExitCode 0)
    $missingResult = Complete-IosValidationRun -OutputRoot (Join-Path $integrityRoot "missing") -Commit "mock" -Stages $missingPersistenceStages
    Assert-TestCondition ($missingResult.FirstFailure -eq "persistence") "缺失 persistence 阶段未失败."
    Assert-TestCondition ($missingResult.RootBlocker -eq "VALIDATION_REPORT_INVALID") "缺失阶段未标记报告无效."

    $duplicateStages = @(
        $completeStageNames | ForEach-Object { New-IosValidationStage -Stage $_ -Status passed -ExitCode 0 }
    ) + @(
        (New-IosValidationStage -Stage "persistence" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "persistence" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "cleanup" -Status passed -ExitCode 0)
    )
    $duplicateResult = Complete-IosValidationRun -OutputRoot (Join-Path $integrityRoot "duplicate") -Commit "mock" -Stages $duplicateStages
    Assert-TestCondition ($duplicateResult.FirstFailure -eq "persistence") "重复 persistence 阶段未失败."
    Assert-TestCondition ($duplicateResult.RootBlocker -eq "VALIDATION_REPORT_INVALID") "重复阶段未标记报告无效."
} finally {
    Remove-Item -LiteralPath $integrityRoot -Recurse -Force
}
# //// /拒绝缺失或重复的阶段结果 ////

# //// 保持 device 构建与 Simulator 验证相互独立 [@x380kkm 2026-08-18] ////
$independentRoot = Join-Path ([IO.Path]::GetTempPath()) "ios-validation-independent-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $independentRoot | Out-Null
try {
    $independentStages = @(
        (New-IosValidationStage -Stage "LOCAL_REPOSITORY" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "LOCAL_CONTRACTS" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "REMOTE_TOOLCHAIN" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "DEVICE_ARTIFACT" -Status skipped),
        (New-IosValidationStage -Stage "ARCHIVE" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "DEVICE_BUILD" -Status failed -ErrorCode "DEVICE_BUILD_FAILED" -ExitCode 1),
        (New-IosValidationStage -Stage "simulator_build" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "simulator_boot" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "app_install" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "app_launch" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "internal_diagnostic" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "loopback" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "background_checkpoint" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "foreground_resume" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "protocol_chain" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "http_observations" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "relaunch" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "persistence" -Status passed -ExitCode 0),
        (New-IosValidationStage -Stage "cleanup" -Status passed -ExitCode 0)
    )
    $independentResult = Complete-IosValidationRun -OutputRoot $independentRoot -Commit "mock" -Stages $independentStages
    $independentReport = Get-Content -LiteralPath $independentResult.JsonReport -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-TestCondition ($independentResult.FirstFailure -eq "DEVICE_BUILD") "device 构建失败未保留为独立失败."
    Assert-TestCondition (@($independentReport.stages | Where-Object { $_.stage -eq "persistence" -and $_.status -eq "passed" }).Count -eq 1) "device 构建失败错误地阻断 Simulator."
} finally {
    Remove-Item -LiteralPath $independentRoot -Recurse -Force
}
# //// /保持 device 构建与 Simulator 验证相互独立 ////

# //// 验证报告文本删除凭据 [@x380kkm 2026-08-18] ////
$protected = Protect-IosValidationText -Text (
    "discarded-prefix " + ("x" * 900) +
    ' Authorization: Bearer abc token=token-value password="alpha beta gamma" ' +
    '"password":"delta\"epsilon zeta" ' +
    "secret='eta\'theta iota' " +
    '{"private_key":"json-private-value","token":"json-token-value"} ' +
    '-----BEGIN PRIVATE KEY-----private-value-----END PRIVATE KEY----- tail-marker'
)
Assert-TestCondition (-not $protected.Contains("abc")) "Bearer token 未脱敏."
Assert-TestCondition (-not $protected.Contains("token-value")) "token 字段未脱敏."
Assert-TestCondition (-not $protected.Contains("alpha beta gamma")) "带空格的 password 字段未脱敏."
Assert-TestCondition (-not $protected.Contains("epsilon zeta")) "转义双引号 password 字段未脱敏."
Assert-TestCondition (-not $protected.Contains("theta iota")) "转义单引号 secret 字段未脱敏."
Assert-TestCondition (-not $protected.Contains("json-private-value")) "JSON private_key 字段未脱敏."
Assert-TestCondition (-not $protected.Contains("json-token-value")) "JSON token 字段未脱敏."
Assert-TestCondition (-not $protected.Contains("private-value")) "PEM 正文未脱敏."
Assert-TestCondition (-not $protected.Contains("discarded-prefix")) "报告未保留错误尾部."
Assert-TestCondition ($protected.EndsWith("tail-marker")) "报告没有保留错误尾部."
$truncatedPem = Protect-IosValidationText -Text 'prefix -----BEGIN PRIVATE KEY-----truncated-value'
Assert-TestCondition ($truncatedPem -eq 'prefix [redacted-pem]') "截断 PEM 未脱敏."
# //// /验证报告文本删除凭据 ////

# //// 构造并验证一个 unsigned arm64 device IPA [@x380kkm 2026-08-18] ////
$testRoot = Join-Path ([IO.Path]::GetTempPath()) "ios-validation-test-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
try {
    $ipaPath = Join-Path $testRoot "diagnostic.ipa"
    New-TestIosIpa -IpaPath $ipaPath
    $hash = (Get-FileHash -LiteralPath $ipaPath -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  diagnostic.ipa" | Set-Content -LiteralPath "$ipaPath.sha256" -Encoding UTF8
    $artifact = Test-IosDeviceArtifact -IpaPath $ipaPath
    Assert-TestCondition ($artifact.Sha256 -eq $hash) "device IPA hash 验证失败."

    $signedIpaPath = Join-Path $testRoot "signed-diagnostic.ipa"
    New-TestIosIpa -IpaPath $signedIpaPath -IncludeCodeSignature
    $signedRejected = $false
    try {
        Test-IosDeviceArtifact -IpaPath $signedIpaPath | Out-Null
    } catch {
        $signedRejected = $_.Exception.Message.Contains("LC_CODE_SIGNATURE")
    }
    Assert-TestCondition $signedRejected "包含 LC_CODE_SIGNATURE 的 device IPA 未被拒绝."
} finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}
# //// /构造并验证一个 unsigned arm64 device IPA ////

# //// 验证 runner 使用 commit archive 和有界清理 [@x380kkm 2026-08-18] ////
$runnerPath = Join-Path $PSScriptRoot "run-ios-validation.ps1"
$preflightPath = Join-Path $PSScriptRoot "ios-validation-preflight.ps1"
$runModulePath = Join-Path $PSScriptRoot "ios-validation-run.ps1"
$coreModulePath = Join-Path $PSScriptRoot "ios-validation.psm1"
$simulatorPath = Join-Path $PSScriptRoot "..\..\platforms\ios\run-simulator-diagnostic.sh"
$simulatorLibraryPath = Join-Path $PSScriptRoot "..\..\platforms\ios\ios-simulator-diagnostic-lib.sh"
$scenarioPath = Join-Path $PSScriptRoot "run-ios-cn-game-scenarios.py"
$observationsPath = Join-Path $PSScriptRoot "export-ios-simulator-http-observations.py"
$cleanupPath = Join-Path $PSScriptRoot "..\..\platforms\ios\cleanup-ios-validation.sh"
$processStopPath = Join-Path $PSScriptRoot "..\..\platforms\ios\stop-ios-validation-process.sh"
$runnerSource = Get-Content -LiteralPath $runnerPath -Raw -Encoding UTF8
$preflightSource = Get-Content -LiteralPath $preflightPath -Raw -Encoding UTF8
$runModuleSource = Get-Content -LiteralPath $runModulePath -Raw -Encoding UTF8
$coreModuleSource = Get-Content -LiteralPath $coreModulePath -Raw -Encoding UTF8
$simulatorSource = Get-Content -LiteralPath $simulatorPath -Raw -Encoding UTF8
$simulatorLibrarySource = Get-Content -LiteralPath $simulatorLibraryPath -Raw -Encoding UTF8
$scenarioSource = Get-Content -LiteralPath $scenarioPath -Raw -Encoding UTF8
$observationsSource = Get-Content -LiteralPath $observationsPath -Raw -Encoding UTF8
$cleanupSource = Get-Content -LiteralPath $cleanupPath -Raw -Encoding UTF8
$processStopSource = Get-Content -LiteralPath $processStopPath -Raw -Encoding UTF8
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$invalidSshHostRejected = $false
try {
    Test-IosRemoteToolchain -RepositoryRoot $repositoryRoot -SshHost "-V" `
        -SimulatorName "iPhone 17 Pro" | Out-Null
} catch {
    $invalidSshHostRejected = $_.FullyQualifiedErrorId -like "ParameterArgumentValidationError*"
}
Assert-TestCondition $invalidSshHostRejected "SSH host 仍可被解释为命令行选项."
Assert-TestCondition ($runnerSource.Contains('ThrottleLimit = 3')) "runner 未保留并发上限 3."
Assert-TestCondition ($preflightSource.Contains('Start-ThreadJob')) "预检未并行执行."
Assert-TestCondition ($preflightSource.Contains('Microsoft.PowerShell.ThreadJob')) "预检未导入 PowerShell 7 ThreadJob 模块."
Assert-TestCondition ($preflightSource.Contains('$result.ExitCode -eq 0 -and $detailMatch.Success')) "远端工具链未要求成功标记."
Assert-TestCondition ($preflightSource.Contains('find -H /tmp')) "远端残留检查未遍历 macOS /tmp 符号链接."
Assert-TestCondition ($runModuleSource.Contains('archive", "--format=tar.gz"')) "runner 未从 commit 创建归档."
Assert-TestCondition ($runModuleSource.Contains('"-O"')) "runner 未使用已验证的 legacy SCP 传输模式."
Assert-TestCondition ($runModuleSource.Contains('ServerAliveInterval=30')) "runner 未为低速 SCP 保持连接."
Assert-TestCondition ($runModuleSource.Contains('ServerAliveCountMax=4')) "runner 未限制 SCP 心跳失败次数."
Assert-TestCondition ($runModuleSource.Contains('-TimeoutSeconds 600')) "源码归档上传超时不足."
Assert-TestCondition ($runModuleSource.Contains('/tmp/starpoint-ios-$runId')) "runner 未使用唯一远端临时目录."
Assert-TestCondition (-not $runModuleSource.Contains('shutdown all')) "runner 包含 shutdown all."
Assert-TestCondition ($runModuleSource.Contains('Stop-IosRemoteValidationRunner')) "device 失败后未停止远端构建进程."
Assert-TestCondition ($runModuleSource.Contains('cargo-device')) "device 构建未隔离 Cargo target."
Assert-TestCondition ($runModuleSource.Contains('cargo-simulator')) "Simulator 构建未隔离 Cargo target."
Assert-TestCondition ($runnerSource.Contains('DiagnosticCdnRoot')) "runner 未接受诊断 CDN 根目录."
Assert-TestCondition ($runModuleSource.Contains('Test-Path -LiteralPath $DiagnosticCdnRoot -PathType Container')) "runner 未将有效诊断 CDN 根目录传给生成器."
Assert-TestCondition ($runModuleSource.Contains('STARPOINT_IOS_DIAGNOSTIC_CDN_ROOT')) "Simulator 构建未嵌入诊断活动目录."
Assert-TestCondition ($coreModuleSource.Contains('sanitize-diagnostic-detail.py')) "Windows 汇总层未复用共享诊断脱敏器."
Assert-TestCondition ($simulatorSource.Contains('BOOTED_BY_SCRIPT')) "Simulator 脚本未跟踪设备所有权."
Assert-TestCondition ($simulatorSource.Contains('com.apple.CoreSimulator.SimRuntime.iOS-26-5')) "Simulator 脚本未固定 iOS 26.5 runtime."
Assert-TestCondition ($simulatorSource.Contains('trap on_exit EXIT')) "Simulator 脚本未注册清理 trap."
Assert-TestCondition ($simulatorSource.Contains('background_checkpoint')) "Simulator 脚本缺少后台 checkpoint 阶段."
Assert-TestCondition ($simulatorSource.Contains('foreground_resume')) "Simulator 脚本缺少前台恢复阶段."
Assert-TestCondition ($simulatorSource.Contains('com.apple.Preferences')) "后台阶段未通过系统 Settings 触发真实 Home 操作."
Assert-TestCondition ($simulatorSource.Contains('wait_for_lifecycle_stage')) "生命周期阶段未读取诊断宿主状态."
Assert-TestCondition ($simulatorSource.Contains('"$foreground_pid" != "$APP_PID"')) "前台恢复未验证原诊断 App 进程保持不变."
Assert-TestCondition ($simulatorSource.Contains('run-ios-cn-game-scenarios.py')) "Simulator runner 未重放完整游戏协议链."
Assert-TestCondition ($simulatorSource.Contains('export-ios-simulator-http-observations.py')) "Simulator runner 未从数据容器导出请求记录."
$protocolCapture = [regex]::Match(
    $simulatorSource,
    'if protocol_output="\$\((?<command>.*?)\)"; then',
    [Text.RegularExpressions.RegexOptions]::Singleline
)
$observationsCapture = [regex]::Match(
    $simulatorSource,
    'if observations_output="\$\((?<command>.*?)\)"; then',
    [Text.RegularExpressions.RegexOptions]::Singleline
)
Assert-TestCondition ($protocolCapture.Success -and $protocolCapture.Groups['command'].Value.TrimStart().StartsWith('trap - ERR')) "协议链失败仍会触发意外错误 trap."
Assert-TestCondition ($observationsCapture.Success -and $observationsCapture.Groups['command'].Value.TrimStart().StartsWith('trap - ERR')) "请求记录失败仍会触发意外错误 trap."
Assert-TestCondition ($simulatorSource.Contains('protocol_error_code="$scenario_error_code"')) "协议链阶段未保留场景报告的具体错误代码."
Assert-TestCondition ($runModuleSource.Contains('ios-cn-game-scenario.json')) "Windows runner 未回传协议链报告."
Assert-TestCondition ($runModuleSource.Contains('http-observations.json')) "Windows runner 未回传请求记录报告."
Assert-TestCondition (-not $scenarioSource.Contains('import msgpack')) "协议链 runner 仍依赖远端安装 MessagePack 包."
Assert-TestCondition ($scenarioSource.Contains('NON_LOOPBACK_BASE_URL')) "协议链 runner 未拒绝非 loopback 地址."
Assert-TestCondition ($observationsSource.Contains('personal-service.sqlite3')) "请求记录导出未查询 App 数据容器数据库."
Assert-TestCondition (-not $simulatorSource.Contains('starpoint_personal_service_bootstrap_flush')) "Simulator runner 直接调用 Framework flush, 绕过真实生命周期."
Assert-TestCondition (-not $simulatorSource.Contains('set +e')) "Simulator 脚本用 set +e 绕过 ERR trap."
Assert-TestCondition (-not $simulatorSource.Contains('token=')) "Simulator 报告脚本包含 token 值输出."
Assert-TestCondition ($simulatorLibrarySource.Contains('lsof -Pan -p "$process_id" -a -iTCP')) "loopback 检查未同时约束 App PID 和监听端口."
Assert-TestCondition ($simulatorLibrarySource.Contains('local main_exit_code=$?')) "cleanup 仍可能覆盖主退出码."
Assert-TestCondition ($simulatorLibrarySource.Contains('has_adhoc_signature')) "Simulator 签名检查未读取完整 codesign 输出."
Assert-TestCondition (-not $simulatorSource.Contains('grep -Fq "Signature=adhoc"')) "Simulator 签名检查仍会在 pipefail 下触发 SIGPIPE 误判."
Assert-TestCondition ($processStopSource.Contains('collect_reparented_build_processes')) "进程停止脚本未重新扫描脱离 runner 的构建进程."
Assert-TestCondition (([regex]::Matches($processStopSource, 'collect_validation_processes')).Count -ge 3) "进程停止脚本未在终止期间重复刷新进程集合."
Assert-TestCondition ($processStopSource.Contains('signal_validation_processes KILL')) "进程停止脚本缺少有界强制终止阶段."
Assert-TestCondition ($processStopSource.Contains('sleep 0.25')) "进程停止脚本未等待进程退出."
Assert-TestCondition (-not $simulatorSource.Contains('simctl uninstall "$SIMULATOR_UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true')) "Simulator 脚本仍忽略 uninstall 失败."
Assert-TestCondition ($cleanupSource.Contains('simulator-udid.txt')) "远端清理未读取本轮 Simulator 所有权."
Assert-TestCondition (-not $cleanupSource.Contains('shutdown all')) "远端清理包含 shutdown all."
Assert-TestCondition (-not $runModuleSource.Contains('grep -c Booted)')) "Windows 清理错误地依赖全局 Booted 数量."
# //// /验证 runner 使用 commit archive 和有界清理 ////

Write-Output "test-ios-validation-runner: PASS"
