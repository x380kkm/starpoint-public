$ErrorActionPreference = 'Stop'
# audience: internal
# # test-ios-cn-candidate-runner
# 此脚本使用进程边界替身验证 iOS CN 候选构建编排、失败清理和报告脱敏.

Set-StrictMode -Version Latest

$runnerPath = Join-Path $PSScriptRoot 'build-ios-cn-candidate.ps1'
. $runnerPath

# //// 断言候选构建测试条件 [@x380kkm 2026-08-19] ////
function Assert-CandidateCondition {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][bool]$Condition,
        [Parameter(Mandatory)][string]$Message
    )

    if (-not $Condition) { throw $Message }
}
# //// /断言候选构建测试条件 ////

# //// 创建不访问远端设备的候选构建进程替身 [@x380kkm 2026-08-19] ////
function New-IosCandidateProcessMock {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Root,
        [string]$FailKind,
        [string]$DirtyStatus,
        [string]$RequestedCommit
    )

    $headCommit = '1234567890abcdef1234567890abcdef12345678'
    if ([string]::IsNullOrWhiteSpace($RequestedCommit)) { $RequestedCommit = $headCommit }
    $state = [pscustomobject]@{
        Root = $Root
        FailKind = $FailKind
        DirtyStatus = $DirtyStatus
        HeadCommit = $headCommit
        RequestedCommit = $RequestedCommit
        Calls = [Collections.Generic.List[object]]::new()
        CleanupInvoked = $false
        ArchivePath = $null
        RemoteBuildScriptSource = $null
    }
    $invoker = {
        param($FilePath, $Arguments, $WorkingDirectory, $TimeoutSeconds)

        $argumentCopy = @($Arguments | ForEach-Object { [string]$_ })
        $state.Calls.Add([pscustomobject]@{
            FilePath = [string]$FilePath
            Arguments = $argumentCopy
            WorkingDirectory = [string]$WorkingDirectory
            TimeoutSeconds = [int]$TimeoutSeconds
        })
        $success = { param($Stdout = '') [pscustomobject]@{ ExitCode = 0; Stdout = $Stdout; Stderr = '' } }
        $failure = {
            [pscustomobject]@{
                ExitCode = 29
                Stdout = ''
                Stderr = 'Authorization: Bearer secret-value token=other-secret password="hidden phrase"'
            }
        }

        if ($FilePath -eq 'git') {
            if ($argumentCopy -contains 'archive') {
                if ($state.FailKind -eq 'ARCHIVE') { return & $failure }
                $outputArgument = $argumentCopy | Where-Object { $_.StartsWith('--output=', [StringComparison]::Ordinal) } | Select-Object -First 1
                $state.ArchivePath = $outputArgument.Substring('--output='.Length)
                [IO.File]::WriteAllBytes($state.ArchivePath, [Text.Encoding]::UTF8.GetBytes('mock archive'))
                return & $success
            }
            if ($argumentCopy -contains '--verify') { return & $success "$($state.RequestedCommit)`n" }
            if ($argumentCopy -contains 'status') { return & $success $state.DirtyStatus }
            return & $success "$($state.HeadCommit)`n"
        }

        if ($FilePath -eq 'ssh') {
            $remoteCommand = $argumentCopy[-1]
            if ($remoteCommand.Contains('rm -rf --', [StringComparison]::Ordinal)) {
                $state.CleanupInvoked = $true
                if ($state.FailKind -eq 'CLEANUP') { return & $failure }
                return & $success
            }
            if ($remoteCommand.Contains('mkdir -p', [StringComparison]::Ordinal) -and $state.FailKind -eq 'REMOTE_PREPARE') {
                return & $failure
            }
            if ($remoteCommand.Contains('tar -xzf', [StringComparison]::Ordinal) -and $state.FailKind -eq 'SOURCE_EXTRACT') {
                return & $failure
            }
            if ($remoteCommand.Contains('candidate-build.sh', [StringComparison]::Ordinal) -and $state.FailKind -eq 'FRAMEWORK_BUILD') {
                return & $failure
            }
            return & $success
        }

        if ($FilePath -eq 'scp') {
            $isDownload = @($argumentCopy | Where-Object { $_ -match ':/.+PersonalServiceBootstrap\.framework\.zip' }).Count -gt 0
            if (-not $isDownload) {
                if ($state.FailKind -eq 'SOURCE_UPLOAD') { return & $failure }
                $buildScriptPath = $argumentCopy |
                    Where-Object { $_.EndsWith('candidate-build.sh', [StringComparison]::OrdinalIgnoreCase) } |
                    Select-Object -First 1
                if ($null -ne $buildScriptPath) {
                    $state.RemoteBuildScriptSource = Get-Content -LiteralPath $buildScriptPath -Raw -Encoding UTF8
                }
                return & $success
            }
            if ($state.FailKind -eq 'FRAMEWORK_DOWNLOAD') { return & $failure }

            $destination = $argumentCopy[-1]
            $sourceRoot = Join-Path $state.Root "framework-source-$([Guid]::NewGuid().ToString('N'))"
            $frameworkRoot = Join-Path $sourceRoot 'PersonalServiceBootstrap.framework'
            New-Item -ItemType Directory -Force -Path $frameworkRoot | Out-Null
            [IO.File]::WriteAllBytes((Join-Path $frameworkRoot 'PersonalServiceBootstrap'), [byte[]](1, 2, 3, 4))
            '<plist version="1.0"><dict/></plist>' | Set-Content -LiteralPath (Join-Path $frameworkRoot 'Info.plist') -Encoding UTF8
            $zipPath = Join-Path $destination 'PersonalServiceBootstrap.framework.zip'
            Compress-Archive -LiteralPath $frameworkRoot -DestinationPath $zipPath -CompressionLevel Fastest
            $hash = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
            "$hash  PersonalServiceBootstrap.framework.zip" |
                Set-Content -LiteralPath "$zipPath.sha256" -Encoding UTF8
            Remove-Item -LiteralPath $sourceRoot -Recurse -Force
            return & $success
        }

        if ($FilePath -eq 'pwsh') {
            if ($state.FailKind -eq 'PACKAGE') { return & $failure }
            $outputIpa = $null
            $packageReport = $null
            for ($index = 0; $index -lt $argumentCopy.Count - 1; $index += 1) {
                if ($argumentCopy[$index] -eq '-OutputIpa') { $outputIpa = $argumentCopy[$index + 1] }
                if ($argumentCopy[$index] -eq '-Report') { $packageReport = $argumentCopy[$index + 1] }
            }
            [IO.File]::WriteAllBytes($outputIpa, [Text.Encoding]::UTF8.GetBytes('mock unsigned ipa'))
            [pscustomobject]@{
                requires_resigning = $true
                installable = $false
                framework_architecture = 'arm64'
            } | ConvertTo-Json | Set-Content -LiteralPath $packageReport -Encoding UTF8
            return & $success '{"status":"passed"}'
        }

        throw "测试进程替身收到未知命令: $FilePath"
    }.GetNewClosure()

    [pscustomobject]@{ State = $state; Invoker = $invoker }
}
# //// /创建不访问远端设备的候选构建进程替身 ////

# //// 创建候选构建测试输入 [@x380kkm 2026-08-19] ////
function New-IosCandidateTestInput {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Root)

    New-Item -ItemType Directory -Force -Path $Root | Out-Null
    $ipaPath = Join-Path $Root 'input.ipa'
    $cdnPath = Join-Path $Root 'cn-cdn'
    [IO.File]::WriteAllBytes($ipaPath, [byte[]](80, 75, 3, 4))
    New-Item -ItemType Directory -Force -Path $cdnPath | Out-Null
    'fixture' | Set-Content -LiteralPath (Join-Path $cdnPath 'fixture.dat') -Encoding UTF8
    [pscustomobject]@{ IpaPath = $ipaPath; CdnPath = $cdnPath }
}
# //// /创建候选构建测试输入 ////

# //// 验证命令行入口解析完整参数并拒绝重复项 [@x380kkm 2026-08-19] ////
$parsedCommandLine = ConvertFrom-IosCandidateCommandLine -Arguments @(
    '-InputIpa', 'input.ipa',
    '-CnCdnBundle', 'cdn',
    '-BundleId', 'dev.starpoint.CN',
    '-DisplayName', 'Starpoint CN',
    '-SshHost', 'starpoint-mac',
    '-OutputDirectory', 'output'
)
Assert-CandidateCondition ($parsedCommandLine.InputIpa -eq 'input.ipa') '命令行入口没有解析 InputIpa.'
Assert-CandidateCondition ($parsedCommandLine.DisplayName -eq 'Starpoint CN') '命令行入口没有保留带空格的 DisplayName.'
$duplicateArgumentRejected = $false
try {
    ConvertFrom-IosCandidateCommandLine -Arguments @(
        '-InputIpa', 'first.ipa', '-InputIpa', 'second.ipa',
        '-CnCdnBundle', 'cdn', '-BundleId', 'dev.starpoint.CN', '-DisplayName', 'Starpoint CN'
    ) | Out-Null
} catch {
    $duplicateArgumentRejected = $_.Exception.Message.Contains('命令行参数重复', [StringComparison]::Ordinal)
}
Assert-CandidateCondition $duplicateArgumentRejected '命令行入口没有拒绝重复参数.'
# //// /验证命令行入口解析完整参数并拒绝重复项 ////

$testRoot = Join-Path ([IO.Path]::GetTempPath()) "ios-cn-candidate-test-$([Guid]::NewGuid().ToString('N'))"
$input = New-IosCandidateTestInput -Root (Join-Path $testRoot 'input')
try {
    # //// 验证成功编排和产物报告 [@x380kkm 2026-08-19] ////
    $successOutput = Join-Path $testRoot 'success-output'
    $successMock = New-IosCandidateProcessMock -Root (Join-Path $testRoot 'success-mock')
    $successResult = Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
        -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' -SshHost 'starpoint-mac' `
        -OutputDirectory $successOutput -ProcessInvoker $successMock.Invoker
    Assert-CandidateCondition ($successResult.status -eq 'passed') '成功候选构建没有返回 passed.'
    Assert-CandidateCondition $successMock.State.CleanupInvoked '成功候选构建没有执行远端清理.'
    Assert-CandidateCondition (Test-Path -LiteralPath $successResult.output_ipa -PathType Leaf) '成功候选构建缺少 IPA.'
    Assert-CandidateCondition (Test-Path -LiteralPath $successResult.framework_zip -PathType Leaf) '成功候选构建缺少 Framework ZIP.'
    Assert-CandidateCondition (Test-Path -LiteralPath $successResult.candidate_report -PathType Leaf) '成功候选构建缺少 JSON 报告.'
    Assert-CandidateCondition ($successResult.remote_cleanup -eq 'passed') '成功候选构建没有确认远端清理.'

    $scpCalls = @($successMock.State.Calls | Where-Object { $_.FilePath -eq 'scp' })
    Assert-CandidateCondition ($scpCalls.Count -eq 2) '成功候选构建没有执行一次上传和一次回传.'
    foreach ($scpCall in $scpCalls) {
        Assert-CandidateCondition ($scpCall.Arguments -contains '-O') 'SCP 没有使用 legacy 模式.'
        Assert-CandidateCondition ($scpCall.Arguments -contains 'BatchMode=yes') 'SCP 没有启用 BatchMode.'
        Assert-CandidateCondition ($scpCall.Arguments -contains 'ServerAliveInterval=30') 'SCP 没有配置 keepalive.'
        Assert-CandidateCondition ($scpCall.Arguments -contains 'ServerAliveCountMax=4') 'SCP 没有限制失活次数.'
    }
    $sshCalls = @($successMock.State.Calls | Where-Object { $_.FilePath -eq 'ssh' })
    foreach ($sshCall in $sshCalls) {
        Assert-CandidateCondition ($sshCall.Arguments -contains 'BatchMode=yes') 'SSH 没有启用 BatchMode.'
        Assert-CandidateCondition ($sshCall.Arguments -contains 'ConnectTimeout=10') 'SSH 没有限制连接超时.'
    }
    $buildCall = $sshCalls | Where-Object { $_.Arguments[-1].Contains('candidate-build.sh', [StringComparison]::Ordinal) } | Select-Object -First 1
    Assert-CandidateCondition ($null -ne $buildCall) '成功候选构建没有调用 Framework 构建脚本.'
    Assert-CandidateCondition ($buildCall.Arguments[-1].StartsWith("/bin/bash '/tmp/starpoint-ios-cn-", [StringComparison]::Ordinal)) 'Framework 构建没有执行上传的 runner.'
    Assert-CandidateCondition ($buildCall.Arguments[-1].EndsWith("' '/tmp/starpoint-ios-cn-$($successResult.run_id)'", [StringComparison]::Ordinal)) 'Framework runner 没有接收本轮路径参数.'
    $buildScriptSource = [string]$successMock.State.RemoteBuildScriptSource
    Assert-CandidateCondition (-not [string]::IsNullOrWhiteSpace($buildScriptSource)) 'Framework runner 没有随源码归档上传.'
    Assert-CandidateCondition ($buildScriptSource.Contains('STARPOINT_IOS_SDK=iphoneos', [StringComparison]::Ordinal)) 'Framework 构建没有固定 iphoneos.'
    Assert-CandidateCondition ($buildScriptSource.Contains('ditto -c -k --keepParent', [StringComparison]::Ordinal)) 'Framework 没有使用 ZIP 回传.'
    Assert-CandidateCondition ($buildScriptSource.Contains('LC_CODE_SIGNATURE', [StringComparison]::Ordinal)) 'Framework 构建没有拒绝预签名字节码.'
    Assert-CandidateCondition ($buildScriptSource.Contains('build-runner.pid', [StringComparison]::Ordinal)) 'Framework 构建没有记录本轮 runner PID.'
    Assert-CandidateCondition ($buildScriptSource.Contains('build-group.pid', [StringComparison]::Ordinal)) 'Framework 构建没有记录独立进程组.'
    Assert-CandidateCondition ($buildScriptSource.Contains('set -m', [StringComparison]::Ordinal)) 'Framework 构建没有启用独立后台进程组.'
    Assert-CandidateCondition ($buildScriptSource.Contains('kill -KILL --', [StringComparison]::Ordinal)) 'Framework 构建没有在 TERM 超时后强制终止进程组.'
    Assert-CandidateCondition ($buildScriptSource.Contains("cd '/tmp/starpoint-ios-cn-", [StringComparison]::Ordinal)) 'Framework runner 没有把本轮路径写入构建目录.'
    $childPidRecordIndex = $buildScriptSource.IndexOf("build-child.pid'", [StringComparison]::Ordinal)
    $childGroupProbeIndex = $buildScriptSource.IndexOf('ps -o pgid= -p "$build_pid"', [StringComparison]::Ordinal)
    Assert-CandidateCondition ($childPidRecordIndex -ge 0 -and $childPidRecordIndex -lt $childGroupProbeIndex) 'Framework 构建没有在探测 PGID 前记录 child PID.'
    Assert-CandidateCondition ($buildScriptSource.Contains('elif [ -n "$build_pid" ]', [StringComparison]::Ordinal)) 'Framework EXIT trap 没有 child PID 回退.'
    $cleanupCall = $sshCalls | Where-Object { $_.Arguments[-1].Contains('rm -rf --', [StringComparison]::Ordinal) } | Select-Object -First 1
    Assert-CandidateCondition ($cleanupCall.Arguments[-1].Contains('process_group_contains_root', [StringComparison]::Ordinal)) '远端清理没有核对本轮进程组.'
    Assert-CandidateCondition ($cleanupCall.Arguments[-1].Contains('build-group.start', [StringComparison]::Ordinal)) '远端清理没有核对进程组启动时间.'
    Assert-CandidateCondition ($cleanupCall.Arguments[-1].Contains('wait_for_process_group_exit', [StringComparison]::Ordinal)) '远端清理没有等待进程组退出.'
    Assert-CandidateCondition ($cleanupCall.Arguments[-1].Contains('build-child.start', [StringComparison]::Ordinal)) '远端清理没有使用 child PID 回退记录.'
    Assert-CandidateCondition ($cleanupCall.Arguments[-1].Contains('derived_group=', [StringComparison]::Ordinal)) '远端清理没有从 child PID 恢复进程组.'
    $packageCall = $successMock.State.Calls | Where-Object { $_.FilePath -eq 'pwsh' } | Select-Object -First 1
    $packageScriptArgument = $packageCall.Arguments |
        Where-Object { $_.EndsWith('package-ios-cn-personal-service.ps1', [StringComparison]::OrdinalIgnoreCase) } |
        Select-Object -First 1
    Assert-CandidateCondition ($null -ne $packageScriptArgument) '候选构建没有调用现有 CN 包装器.'
    Assert-CandidateCondition ($packageCall.Arguments -contains '-CnCdnBundle') '候选构建没有传入 CN CDN.'
    Assert-CandidateCondition ($packageCall.Arguments -contains '-BundleId') '候选构建没有传入 bundle ID.'
    # //// /验证成功编排和产物报告 ////

    # //// 验证所有远端失败分支执行有界清理 [@x380kkm 2026-08-19] ////
    $failureCases = @(
        [pscustomobject]@{ Kind = 'REMOTE_PREPARE'; Stage = 'REMOTE_PREPARE'; Code = 'REMOTE_PREPARE_FAILED' },
        [pscustomobject]@{ Kind = 'SOURCE_UPLOAD'; Stage = 'SOURCE_UPLOAD'; Code = 'SOURCE_UPLOAD_FAILED' },
        [pscustomobject]@{ Kind = 'SOURCE_EXTRACT'; Stage = 'SOURCE_UPLOAD'; Code = 'SOURCE_UPLOAD_FAILED' },
        [pscustomobject]@{ Kind = 'FRAMEWORK_BUILD'; Stage = 'FRAMEWORK_BUILD'; Code = 'FRAMEWORK_BUILD_FAILED' },
        [pscustomobject]@{ Kind = 'FRAMEWORK_DOWNLOAD'; Stage = 'FRAMEWORK_DOWNLOAD'; Code = 'FRAMEWORK_DOWNLOAD_FAILED' },
        [pscustomobject]@{ Kind = 'PACKAGE'; Stage = 'PACKAGE'; Code = 'PACKAGE_FAILED' }
    )
    foreach ($failureCase in $failureCases) {
        $caseRoot = Join-Path $testRoot "failure-$($failureCase.Kind)"
        $failureMock = New-IosCandidateProcessMock -Root (Join-Path $caseRoot 'mock') -FailKind $failureCase.Kind
        $failureResult = Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
            -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' `
            -OutputDirectory (Join-Path $caseRoot 'output') -ProcessInvoker $failureMock.Invoker
        Assert-CandidateCondition ($failureResult.status -eq 'failed') "$($failureCase.Kind) 没有返回 failed."
        Assert-CandidateCondition ($failureResult.first_failure -eq $failureCase.Stage) "$($failureCase.Kind) 的失败阶段不正确."
        Assert-CandidateCondition ($failureResult.root_error_code -eq $failureCase.Code) "$($failureCase.Kind) 的错误代码不正确."
        $failedStage = $failureResult.stages | Where-Object {
            $_.stage -eq $failureCase.Stage -and $_.status -eq 'failed'
        } | Select-Object -First 1
        Assert-CandidateCondition ($failedStage.exit_code -eq 29) "$($failureCase.Kind) 没有保留真实退出码."
        Assert-CandidateCondition $failureMock.State.CleanupInvoked "$($failureCase.Kind) 没有执行远端清理."
        Assert-CandidateCondition ($failureResult.remote_cleanup -eq 'passed') "$($failureCase.Kind) 没有确认远端清理."
        $failureReportText = Get-Content -LiteralPath $failureResult.candidate_report -Raw -Encoding UTF8
        Assert-CandidateCondition (-not $failureReportText.Contains('secret-value', [StringComparison]::Ordinal)) "$($failureCase.Kind) 报告泄漏 Bearer token."
        Assert-CandidateCondition (-not $failureReportText.Contains('other-secret', [StringComparison]::Ordinal)) "$($failureCase.Kind) 报告泄漏 token 字段."
        Assert-CandidateCondition (-not $failureReportText.Contains('hidden phrase', [StringComparison]::Ordinal)) "$($failureCase.Kind) 报告泄漏 password 字段."
    }
    # //// /验证所有远端失败分支执行有界清理 ////

    # //// 失败报告不暴露同一提交的旧产物 [@x380kkm 2026-08-20] ////
    $staleOutput = Join-Path $testRoot 'stale-output'
    New-Item -ItemType Directory -Force -Path $staleOutput | Out-Null
    $staleIpa = Join-Path $staleOutput 'StarpointCN-1234567-unsigned.ipa'
    $staleFramework = Join-Path $staleOutput 'PersonalServiceBootstrap-1234567.framework.zip'
    $stalePackageReport = "$staleIpa.package.json"
    'old ipa' | Set-Content -LiteralPath $staleIpa -Encoding UTF8
    'old framework' | Set-Content -LiteralPath $staleFramework -Encoding UTF8
    '{}' | Set-Content -LiteralPath $stalePackageReport -Encoding UTF8
    $staleMock = New-IosCandidateProcessMock -Root (Join-Path $testRoot 'stale-mock') -FailKind 'REMOTE_PREPARE'
    $staleResult = Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
        -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' `
        -OutputDirectory $staleOutput -ProcessInvoker $staleMock.Invoker
    Assert-CandidateCondition ($null -eq $staleResult.output_ipa) '失败报告暴露了旧 IPA.'
    Assert-CandidateCondition ($null -eq $staleResult.framework_zip) '失败报告暴露了旧 Framework.'
    Assert-CandidateCondition ($null -eq $staleResult.package_report) '失败报告暴露了旧包装报告.'
    Assert-CandidateCondition ((Get-Content -LiteralPath $staleIpa -Raw -Encoding UTF8).Contains('old ipa', [StringComparison]::Ordinal)) '远端失败改写了旧 IPA.'
    # //// /失败报告不暴露同一提交的旧产物 ////

    # //// 验证本机归档失败仍删除本轮临时输入 [@x380kkm 2026-08-19] ////
    $archiveMock = New-IosCandidateProcessMock -Root (Join-Path $testRoot 'archive-mock') -FailKind 'ARCHIVE'
    $archiveResult = Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
        -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' `
        -OutputDirectory (Join-Path $testRoot 'archive-output') -ProcessInvoker $archiveMock.Invoker
    Assert-CandidateCondition ($archiveResult.root_error_code -eq 'ARCHIVE_FAILED') '归档失败没有返回 ARCHIVE_FAILED.'
    Assert-CandidateCondition (-not $archiveMock.State.CleanupInvoked) '远端目录创建前错误执行了远端清理.'
    if (-not [string]::IsNullOrWhiteSpace([string]$archiveMock.State.ArchivePath)) {
        Assert-CandidateCondition (-not (Test-Path -LiteralPath $archiveMock.State.ArchivePath)) '归档失败没有删除本轮本机临时目录.'
    }
    # //// /验证本机归档失败仍删除本轮临时输入 ////

    # //// 拒绝脏工作树和不一致提交 [@x380kkm 2026-08-19] ////
    $dirtyMock = New-IosCandidateProcessMock -Root (Join-Path $testRoot 'dirty-mock') -DirtyStatus ' M tracked.txt'
    $dirtyResult = Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
        -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' `
        -OutputDirectory (Join-Path $testRoot 'dirty-output') -ProcessInvoker $dirtyMock.Invoker
    Assert-CandidateCondition ($dirtyResult.root_error_code -eq 'DIRTY_WORKTREE') '脏工作树没有被拒绝.'
    Assert-CandidateCondition (@($dirtyMock.State.Calls | Where-Object { $_.FilePath -in @('ssh', 'scp') }).Count -eq 0) '脏工作树触发了远端命令.'

    $mismatchCommit = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    $mismatchMock = New-IosCandidateProcessMock -Root (Join-Path $testRoot 'mismatch-mock') -RequestedCommit $mismatchCommit
    $mismatchResult = Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
        -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' -Commit $mismatchCommit `
        -OutputDirectory (Join-Path $testRoot 'mismatch-output') -ProcessInvoker $mismatchMock.Invoker
    Assert-CandidateCondition ($mismatchResult.root_error_code -eq 'COMMIT_MISMATCH') '不一致提交没有被拒绝.'
    Assert-CandidateCondition (@($mismatchMock.State.Calls | Where-Object { $_.FilePath -in @('ssh', 'scp') }).Count -eq 0) '不一致提交触发了远端命令.'
    # //// /拒绝脏工作树和不一致提交 ////

    # //// 拒绝仓库内候选输出 [@x380kkm 2026-08-19] ////
    $insideRepositoryRejected = $false
    $insideMock = New-IosCandidateProcessMock -Root (Join-Path $testRoot 'inside-mock')
    try {
        Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
            -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' `
            -OutputDirectory (Join-Path $PSScriptRoot '..\..\build\ios-cn-candidate-test') `
            -ProcessInvoker $insideMock.Invoker | Out-Null
    } catch {
        $insideRepositoryRejected = $_.Exception.Message.Contains('不能位于 Git 仓库内', [StringComparison]::Ordinal)
    }
    Assert-CandidateCondition $insideRepositoryRejected '仓库内候选输出没有被拒绝.'
    # //// /拒绝仓库内候选输出 ////

    # //// 验证清理失败覆盖成功状态 [@x380kkm 2026-08-19] ////
    $cleanupMock = New-IosCandidateProcessMock -Root (Join-Path $testRoot 'cleanup-mock') -FailKind 'CLEANUP'
    $cleanupResult = Invoke-IosCnCandidateBuild -InputIpa $input.IpaPath -CnCdnBundle $input.CdnPath `
        -BundleId 'dev.starpoint.CNTest' -DisplayName 'Starpoint CN Test' `
        -OutputDirectory (Join-Path $testRoot 'cleanup-output') -ProcessInvoker $cleanupMock.Invoker
    Assert-CandidateCondition ($cleanupResult.status -eq 'failed') '清理失败没有覆盖成功状态.'
    Assert-CandidateCondition ($cleanupResult.root_error_code -eq 'CLEANUP_FAILED') '清理失败没有返回 CLEANUP_FAILED.'
    Assert-CandidateCondition ($cleanupResult.remote_cleanup -eq 'failed') '清理失败没有保留远端状态.'
    $cleanupStage = $cleanupResult.stages | Where-Object { $_.stage -eq 'CLEANUP' } | Select-Object -First 1
    Assert-CandidateCondition ($cleanupStage.exit_code -eq 29) '清理失败没有保留真实退出码.'
    # //// /验证清理失败覆盖成功状态 ////

    'PASS'
} finally {
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
