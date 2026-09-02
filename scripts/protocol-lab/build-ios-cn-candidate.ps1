$ErrorActionPreference = 'Stop'
# audience: external
# # build-ios-cn-candidate
# 此脚本从当前干净提交远端构建 iPhone Framework, 回传归档, 并在 Windows 生成 unsigned CN 候选 IPA.

Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot 'ios-validation.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'ios-cn-candidate-remote.psm1') -Force

# //// 解析 PowerShell 7 命令行参数 [@x380kkm 2026-08-19] ////
function ConvertFrom-IosCandidateCommandLine {
    [CmdletBinding()]
    param([Parameter(Mandatory)][object[]]$Arguments)

    $canonicalNames = @{
        inputipa = 'InputIpa'
        cncdnbundle = 'CnCdnBundle'
        bundleid = 'BundleId'
        displayname = 'DisplayName'
        sshhost = 'SshHost'
        outputdirectory = 'OutputDirectory'
        commit = 'Commit'
    }
    $parameters = @{}
    for ($index = 0; $index -lt $Arguments.Count; $index += 2) {
        $nameToken = [string]$Arguments[$index]
        if ($nameToken -notmatch '^--?([A-Za-z][A-Za-z0-9]*)$') {
            throw "未知命令行参数: $nameToken"
        }
        $lookupName = $Matches[1].ToLowerInvariant()
        if (-not $canonicalNames.ContainsKey($lookupName)) {
            throw "未知命令行参数: $nameToken"
        }
        if ($index + 1 -ge $Arguments.Count) {
            throw "命令行参数缺少值: $nameToken"
        }
        $canonicalName = $canonicalNames[$lookupName]
        if ($parameters.ContainsKey($canonicalName)) {
            throw "命令行参数重复: $nameToken"
        }
        $parameters[$canonicalName] = [string]$Arguments[$index + 1]
    }
    foreach ($requiredName in @('InputIpa', 'CnCdnBundle', 'BundleId', 'DisplayName')) {
        if (-not $parameters.ContainsKey($requiredName) -or
            [string]::IsNullOrWhiteSpace([string]$parameters[$requiredName])) {
            throw "缺少必要命令行参数: -$requiredName"
        }
    }
    $parameters
}
# //// /解析 PowerShell 7 命令行参数 ////

# //// 判断候选输出是否位于仓库内 [@x380kkm 2026-08-19] ////
function Test-IosCandidatePathInRepository {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$CandidatePath
    )

    $relativePath = [IO.Path]::GetRelativePath($RepositoryRoot, $CandidatePath)
    if ([IO.Path]::IsPathRooted($relativePath)) { return $false }
    if ($relativePath -eq '.') { return $true }
    return $relativePath -ne '..' -and
        -not $relativePath.StartsWith("..$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::Ordinal)
}
# //// /判断候选输出是否位于仓库内 ////

# //// 创建脱敏候选构建阶段 [@x380kkm 2026-08-19] ////
function New-IosCandidateStage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Stage,
        [Parameter(Mandatory)][ValidateSet('passed', 'failed', 'skipped')][string]$Status,
        [AllowNull()][string]$ErrorCode,
        [int]$ExitCode = 0,
        [AllowNull()][string]$Detail,
        [AllowNull()][string]$ArtifactPath,
        [AllowNull()][string]$Sha256,
        [datetime]$StartedAtUtc = [datetime]::UtcNow
    )

    [pscustomobject]@{
        stage = $Stage
        status = $Status
        error_code = $ErrorCode
        exit_code = $ExitCode
        detail = Protect-IosValidationText -Text $Detail
        artifact_path = $ArtifactPath
        sha256 = $Sha256
        started_at_utc = $StartedAtUtc.ToString('o')
        ended_at_utc = [datetime]::UtcNow.ToString('o')
    }
}
# //// /创建脱敏候选构建阶段 ////

# //// 调用可替换的外部进程边界 [@x380kkm 2026-08-19] ////
function Invoke-IosCandidateProcess {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][scriptblock]$ProcessInvoker,
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [ValidateRange(1, 7200)][int]$TimeoutSeconds
    )

    $result = & $ProcessInvoker -FilePath $FilePath -Arguments $Arguments `
        -WorkingDirectory $WorkingDirectory -TimeoutSeconds $TimeoutSeconds
    if ($null -eq $result -or
        $result.PSObject.Properties.Name -notcontains 'ExitCode' -or
        $result.PSObject.Properties.Name -notcontains 'Stdout' -or
        $result.PSObject.Properties.Name -notcontains 'Stderr') {
        throw "外部进程边界没有返回完整结果: $FilePath"
    }
    $result
}
# //// /调用可替换的外部进程边界 ////

# //// 把命令失败转换为有限脱敏错误 [@x380kkm 2026-08-19] ////
function Assert-IosCandidateProcessPassed {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][pscustomobject]$Result,
        [Parameter(Mandatory)][string]$Operation
    )

    if ([int]$Result.ExitCode -eq 0) { return }
    $detail = Protect-IosValidationText -Text (([string]$Result.Stderr, [string]$Result.Stdout) -join "`n")
    $exception = [InvalidOperationException]::new("$Operation 失败, 退出码为 $($Result.ExitCode). $detail")
    $exception.Data['ios_candidate_exit_code'] = [int]$Result.ExitCode
    throw $exception
}
# //// /把命令失败转换为有限脱敏错误 ////

# //// 安全展开远端 Framework ZIP [@x380kkm 2026-08-19] ////
function Expand-IosCandidateFrameworkArchive {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ArchivePath,
        [Parameter(Mandatory)][string]$DestinationPath
    )

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        foreach ($entry in $archive.Entries) {
            $normalizedName = $entry.FullName.Replace('\', '/')
            $segments = @($normalizedName.Split('/', [StringSplitOptions]::RemoveEmptyEntries))
            if ($normalizedName.StartsWith('/', [StringComparison]::Ordinal) -or
                $normalizedName.Contains(':') -or
                $segments -contains '..') {
                throw "Framework ZIP 包含越界路径: $($entry.FullName)"
            }
        }
    } finally {
        $archive.Dispose()
    }

    New-Item -ItemType Directory -Force -Path $DestinationPath | Out-Null
    [IO.Compression.ZipFile]::ExtractToDirectory($ArchivePath, $DestinationPath)
    $frameworks = @(Get-ChildItem -LiteralPath $DestinationPath -Recurse -Directory |
        Where-Object { $_.Name -eq 'PersonalServiceBootstrap.framework' })
    if ($frameworks.Count -ne 1) {
        throw "Framework ZIP 必须只包含一个 PersonalServiceBootstrap.framework, 实际为 $($frameworks.Count)."
    }
    $frameworkBinary = Join-Path $frameworks[0].FullName 'PersonalServiceBootstrap'
    $frameworkPlist = Join-Path $frameworks[0].FullName 'Info.plist'
    if (-not (Test-Path -LiteralPath $frameworkBinary -PathType Leaf) -or
        -not (Test-Path -LiteralPath $frameworkPlist -PathType Leaf)) {
        throw 'Framework ZIP 缺少主二进制或 Info.plist.'
    }
    $frameworks[0].FullName
}
# //// /安全展开远端 Framework ZIP ////

# //// 从干净提交构建一个 unsigned CN iPhone 候选包 [@x380kkm 2026-08-19] ////
function Invoke-IosCnCandidateBuild {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$InputIpa,
        [Parameter(Mandatory)][string]$CnCdnBundle,
        [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$')][string]$BundleId,
        [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$DisplayName,
        [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._:-]*$')][string]$SshHost = 'starpoint-mac',
        [string]$OutputDirectory,
        [string]$Commit = 'HEAD',
        [scriptblock]$ProcessInvoker
    )

    $repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
    if ($null -eq $ProcessInvoker) {
        $ProcessInvoker = {
            param($FilePath, $Arguments, $WorkingDirectory, $TimeoutSeconds)
            Invoke-IosValidationProcess -FilePath $FilePath -Arguments $Arguments `
                -WorkingDirectory $WorkingDirectory -TimeoutSeconds $TimeoutSeconds
        }
    }

    $headResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'git' `
        -Arguments @('-C', $repositoryRoot, 'rev-parse', 'HEAD') -WorkingDirectory $repositoryRoot -TimeoutSeconds 30
    Assert-IosCandidateProcessPassed -Result $headResult -Operation '读取当前提交'
    $headCommit = ([string]$headResult.Stdout).Trim().ToLowerInvariant()
    if ($headCommit -notmatch '^[0-9a-f]{40,64}$') { throw '当前提交摘要格式无效.' }
    $shortCommit = $headCommit.Substring(0, 7)

    $workspaceRoot = Split-Path -Parent $repositoryRoot
    if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
        $OutputDirectory = Join-Path $workspaceRoot "artifacts\ios-cn-device\$shortCommit"
    }
    $outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
    if (Test-IosCandidatePathInRepository -RepositoryRoot $repositoryRoot -CandidatePath $outputRoot) {
        throw 'OutputDirectory 不能位于 Git 仓库内, 候选产物必须保存在仓库外.'
    }
    New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null

    $runId = [Guid]::NewGuid().ToString('N')
    $remoteRoot = "/tmp/starpoint-ios-cn-$runId"
    $localTemporaryRoot = Join-Path ([IO.Path]::GetTempPath()) "starpoint-ios-cn-$runId"
    $localArchive = Join-Path $localTemporaryRoot 'source.tar.gz'
    $remoteBuildScriptName = 'candidate-build.sh'
    $localRemoteBuildScript = Join-Path $localTemporaryRoot $remoteBuildScriptName
    $downloadRoot = Join-Path $localTemporaryRoot 'download'
    $expandedRoot = Join-Path $localTemporaryRoot 'expanded'
    $frameworkFileName = 'PersonalServiceBootstrap.framework.zip'
    $downloadedFrameworkZip = Join-Path $downloadRoot $frameworkFileName
    $downloadedFrameworkHash = "$downloadedFrameworkZip.sha256"
    $frameworkArtifact = Join-Path $outputRoot "PersonalServiceBootstrap-$shortCommit.framework.zip"
    $frameworkHashArtifact = "$frameworkArtifact.sha256"
    $outputIpa = Join-Path $outputRoot "StarpointCN-$shortCommit-unsigned.ipa"
    $packageReport = "$outputIpa.package.json"
    $candidateReport = Join-Path $outputRoot 'candidate-build.json'
    $stages = [Collections.Generic.List[object]]::new()
    $remoteMayExist = $false
    $activeStage = 'INPUTS'
    $activeErrorCode = 'INPUT_INVALID'
    $frameworkSha256 = $null
    $ipaSha256 = $null
    $frameworkArtifactPublished = $false
    $ipaArtifactPublished = $false
    $packageReportPublished = $false
    $sshOptions = @(
        '-o', 'BatchMode=yes',
        '-o', 'ConnectTimeout=10',
        '-o', 'ServerAliveInterval=30',
        '-o', 'ServerAliveCountMax=4'
    )
    $scpOptions = @('-O') + $sshOptions

    try {
        New-Item -ItemType Directory -Force -Path $localTemporaryRoot, $downloadRoot | Out-Null
        $stageStarted = [datetime]::UtcNow
        $inputPath = (Resolve-Path -LiteralPath $InputIpa).Path
        $cdnBundlePath = (Resolve-Path -LiteralPath $CnCdnBundle).Path
        if (-not (Test-Path -LiteralPath $inputPath -PathType Leaf) -or
            [IO.Path]::GetExtension($inputPath) -ne '.ipa') {
            throw 'InputIpa 必须是现有 .ipa 文件.'
        }
        if (-not (Test-Path -LiteralPath $cdnBundlePath -PathType Container)) {
            throw 'CnCdnBundle 必须是现有目录.'
        }
        if ([IO.Path]::GetFullPath($inputPath) -eq [IO.Path]::GetFullPath($outputIpa)) {
            throw '输入 IPA 和输出 IPA 必须不同.'
        }
        $stages.Add((New-IosCandidateStage -Stage 'INPUTS' -Status passed -Detail '输入 IPA、CN CDN 和输出边界有效.' -StartedAtUtc $stageStarted))

        $activeStage = 'REPOSITORY'
        $activeErrorCode = 'REPOSITORY_INVALID'
        $stageStarted = [datetime]::UtcNow
        $requestedResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'git' `
            -Arguments @('-C', $repositoryRoot, 'rev-parse', '--verify', "$Commit`^{commit}") `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 30
        Assert-IosCandidateProcessPassed -Result $requestedResult -Operation '解析候选提交'
        $requestedCommit = ([string]$requestedResult.Stdout).Trim().ToLowerInvariant()
        if ($requestedCommit -ne $headCommit) {
            $activeErrorCode = 'COMMIT_MISMATCH'
            throw '候选提交与当前 HEAD 不一致.'
        }
        $statusResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'git' `
            -Arguments @('-C', $repositoryRoot, 'status', '--porcelain', '--untracked-files=all') `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 30
        Assert-IosCandidateProcessPassed -Result $statusResult -Operation '检查工作树'
        if (-not [string]::IsNullOrWhiteSpace([string]$statusResult.Stdout)) {
            $activeErrorCode = 'DIRTY_WORKTREE'
            throw '工作树包含 staged、unstaged 或 untracked 改动.'
        }
        $stages.Add((New-IosCandidateStage -Stage 'REPOSITORY' -Status passed -Detail "当前提交 $headCommit 与干净工作树一致." -StartedAtUtc $stageStarted))

        $activeStage = 'ARCHIVE'
        $activeErrorCode = 'ARCHIVE_FAILED'
        $stageStarted = [datetime]::UtcNow
        $archiveResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'git' `
            -Arguments @('-C', $repositoryRoot, 'archive', '--format=tar.gz', "--output=$localArchive", $headCommit) `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 120
        Assert-IosCandidateProcessPassed -Result $archiveResult -Operation '创建提交归档'
        if (-not (Test-Path -LiteralPath $localArchive -PathType Leaf)) { throw 'git archive 没有生成源码归档.' }
        $headAfterArchive = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'git' `
            -Arguments @('-C', $repositoryRoot, 'rev-parse', 'HEAD') -WorkingDirectory $repositoryRoot -TimeoutSeconds 30
        Assert-IosCandidateProcessPassed -Result $headAfterArchive -Operation '复核当前提交'
        if (([string]$headAfterArchive.Stdout).Trim().ToLowerInvariant() -ne $headCommit) {
            $activeErrorCode = 'COMMIT_MISMATCH'
            throw 'git archive 期间 HEAD 发生变化.'
        }
        $statusAfterArchive = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'git' `
            -Arguments @('-C', $repositoryRoot, 'status', '--porcelain', '--untracked-files=all') `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 30
        Assert-IosCandidateProcessPassed -Result $statusAfterArchive -Operation '复核工作树'
        if (-not [string]::IsNullOrWhiteSpace([string]$statusAfterArchive.Stdout)) {
            $activeErrorCode = 'DIRTY_WORKTREE'
            throw 'git archive 期间工作树发生变化.'
        }
        $archiveSha256 = (Get-FileHash -LiteralPath $localArchive -Algorithm SHA256).Hash.ToLowerInvariant()
        $stages.Add((New-IosCandidateStage -Stage 'ARCHIVE' -Status passed -Detail '当前提交已归档.' -Sha256 $archiveSha256 -StartedAtUtc $stageStarted))

        $remoteBuildScript = New-IosCandidateRemoteBuildScript `
            -RemoteRoot $remoteRoot -FrameworkFileName $frameworkFileName
        Set-Content -LiteralPath $localRemoteBuildScript -Value $remoteBuildScript -Encoding UTF8 -NoNewline

        $activeStage = 'REMOTE_PREPARE'
        $activeErrorCode = 'REMOTE_PREPARE_FAILED'
        $stageStarted = [datetime]::UtcNow
        $remoteMayExist = $true
        $prepareResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'ssh' `
            -Arguments @($sshOptions + @($SshHost, "mkdir -p '$remoteRoot/source'")) `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 60
        Assert-IosCandidateProcessPassed -Result $prepareResult -Operation '创建远端临时目录'
        $stages.Add((New-IosCandidateStage -Stage 'REMOTE_PREPARE' -Status passed -Detail '本轮远端临时目录已创建.' -StartedAtUtc $stageStarted))

        $activeStage = 'SOURCE_UPLOAD'
        $activeErrorCode = 'SOURCE_UPLOAD_FAILED'
        $stageStarted = [datetime]::UtcNow
        $uploadResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'scp' `
            -Arguments @($scpOptions + @($localArchive, $localRemoteBuildScript, "$($SshHost):$remoteRoot/")) `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 600
        Assert-IosCandidateProcessPassed -Result $uploadResult -Operation '上传提交归档'
        $extractResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'ssh' `
            -Arguments @($sshOptions + @($SshHost, "tar -xzf '$remoteRoot/source.tar.gz' -C '$remoteRoot/source'")) `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 180
        Assert-IosCandidateProcessPassed -Result $extractResult -Operation '展开远端提交归档'
        $stages.Add((New-IosCandidateStage -Stage 'SOURCE_UPLOAD' -Status passed -Detail '提交归档已上传并展开.' -StartedAtUtc $stageStarted))

        $activeStage = 'FRAMEWORK_BUILD'
        $activeErrorCode = 'FRAMEWORK_BUILD_FAILED'
        $stageStarted = [datetime]::UtcNow
        $buildResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'ssh' `
            -Arguments @($sshOptions + @($SshHost, "/bin/bash '$remoteRoot/$remoteBuildScriptName' '$remoteRoot'")) `
            -WorkingDirectory $repositoryRoot -TimeoutSeconds 2400
        Assert-IosCandidateProcessPassed -Result $buildResult -Operation '构建 iPhone arm64 Framework'
        $stages.Add((New-IosCandidateStage -Stage 'FRAMEWORK_BUILD' -Status passed -Detail '远端 iPhone arm64 Framework 已构建并压缩.' -StartedAtUtc $stageStarted))

        $activeStage = 'FRAMEWORK_DOWNLOAD'
        $activeErrorCode = 'FRAMEWORK_DOWNLOAD_FAILED'
        $stageStarted = [datetime]::UtcNow
        $downloadResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'scp' `
            -Arguments @($scpOptions + @(
                "$($SshHost):$remoteRoot/$frameworkFileName",
                "$($SshHost):$remoteRoot/$frameworkFileName.sha256",
                $downloadRoot
            )) -WorkingDirectory $repositoryRoot -TimeoutSeconds 900
        Assert-IosCandidateProcessPassed -Result $downloadResult -Operation '回传 Framework ZIP'
        if (-not (Test-Path -LiteralPath $downloadedFrameworkZip -PathType Leaf) -or
            -not (Test-Path -LiteralPath $downloadedFrameworkHash -PathType Leaf)) {
            throw 'Framework 回传缺少 ZIP 或 SHA-256 文件.'
        }
        $remoteHashText = Get-Content -LiteralPath $downloadedFrameworkHash -Raw -Encoding UTF8
        $remoteHashMatch = [regex]::Match($remoteHashText, '(?im)^\s*([0-9a-f]{64})\s+')
        if (-not $remoteHashMatch.Success) { throw '远端 Framework SHA-256 文件格式无效.' }
        $frameworkSha256 = (Get-FileHash -LiteralPath $downloadedFrameworkZip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($frameworkSha256 -ne $remoteHashMatch.Groups[1].Value.ToLowerInvariant()) {
            throw 'Framework ZIP 的本机与远端 SHA-256 不一致.'
        }
        $frameworkPath = Expand-IosCandidateFrameworkArchive -ArchivePath $downloadedFrameworkZip -DestinationPath $expandedRoot
        $temporaryFrameworkArtifact = Join-Path $outputRoot ".$([IO.Path]::GetFileName($frameworkArtifact)).$runId.tmp"
        Copy-Item -LiteralPath $downloadedFrameworkZip -Destination $temporaryFrameworkArtifact
        Move-Item -LiteralPath $temporaryFrameworkArtifact -Destination $frameworkArtifact -Force
        "$frameworkSha256  $([IO.Path]::GetFileName($frameworkArtifact))" |
            Set-Content -LiteralPath $frameworkHashArtifact -Encoding UTF8
        $frameworkArtifactPublished = $true
        $stages.Add((New-IosCandidateStage -Stage 'FRAMEWORK_DOWNLOAD' -Status passed -Detail 'Framework ZIP 已校验并展开.' `
            -ArtifactPath $frameworkArtifact -Sha256 $frameworkSha256 -StartedAtUtc $stageStarted))

        $activeStage = 'PACKAGE'
        $activeErrorCode = 'PACKAGE_FAILED'
        $stageStarted = [datetime]::UtcNow
        $packageScript = Join-Path $repositoryRoot 'scripts\protocol-lab\package-ios-cn-personal-service.ps1'
        $packageArguments = @(
            '-NoLogo', '-NoProfile', '-NonInteractive', '-File', $packageScript,
            '-InputIpa', $inputPath,
            '-Framework', $frameworkPath,
            '-OutputIpa', $outputIpa,
            '-BundleId', $BundleId,
            '-DisplayName', $DisplayName,
            '-CnCdnBundle', $cdnBundlePath,
            '-Report', $packageReport
        )
        $packageResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'pwsh' `
            -Arguments $packageArguments -WorkingDirectory $repositoryRoot -TimeoutSeconds 1800
        Assert-IosCandidateProcessPassed -Result $packageResult -Operation '包装 CN iOS 候选 IPA'
        if (-not (Test-Path -LiteralPath $outputIpa -PathType Leaf) -or
            -not (Test-Path -LiteralPath $packageReport -PathType Leaf)) {
            throw 'CN 包装器没有生成 IPA 和报告.'
        }
        $packageData = Get-Content -LiteralPath $packageReport -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($packageData.requires_resigning -ne $true -or
            $packageData.installable -ne $false -or
            [string]$packageData.framework_architecture -ne 'arm64') {
            throw 'CN 包装报告没有证明候选包为 unsigned arm64 产物.'
        }
        $ipaSha256 = (Get-FileHash -LiteralPath $outputIpa -Algorithm SHA256).Hash.ToLowerInvariant()
        $ipaArtifactPublished = $true
        $packageReportPublished = $true
        $stages.Add((New-IosCandidateStage -Stage 'PACKAGE' -Status passed -Detail 'unsigned CN 候选 IPA 已生成并通过包装报告校验.' `
            -ArtifactPath $outputIpa -Sha256 $ipaSha256 -StartedAtUtc $stageStarted))
    } catch {
        $stageExitCode = 1
        if ($_.Exception.Data.Contains('ios_candidate_exit_code')) {
            $stageExitCode = [int]$_.Exception.Data['ios_candidate_exit_code']
        }
        $stages.Add((New-IosCandidateStage -Stage $activeStage -Status failed -ErrorCode $activeErrorCode `
            -ExitCode $stageExitCode -Detail $_.Exception.Message))
    } finally {
        $cleanupStarted = [datetime]::UtcNow
        $cleanupErrors = [Collections.Generic.List[string]]::new()
        $cleanupExitCode = 1
        if ($remoteMayExist) {
            try {
                $cleanupCommand = New-IosCandidateRemoteCleanupCommand -RemoteRoot $remoteRoot
                $cleanupResult = Invoke-IosCandidateProcess -ProcessInvoker $ProcessInvoker -FilePath 'ssh' `
                    -Arguments @($sshOptions + @($SshHost, $cleanupCommand)) `
                    -WorkingDirectory $repositoryRoot -TimeoutSeconds 120
                if ([int]$cleanupResult.ExitCode -ne 0) {
                    $cleanupExitCode = [int]$cleanupResult.ExitCode
                    $cleanupErrors.Add((Protect-IosValidationText -Text (([string]$cleanupResult.Stderr, [string]$cleanupResult.Stdout) -join "`n")))
                }
            } catch {
                $cleanupErrors.Add((Protect-IosValidationText -Text $_.Exception.Message))
            }
        }
        try {
            if (Test-Path -LiteralPath $localTemporaryRoot) {
                Remove-Item -LiteralPath $localTemporaryRoot -Recurse -Force
            }
        } catch {
            $cleanupErrors.Add((Protect-IosValidationText -Text $_.Exception.Message))
        }
        if ($cleanupErrors.Count -eq 0) {
            $stages.Add((New-IosCandidateStage -Stage 'CLEANUP' -Status passed -Detail '本轮本机和远端临时目录已删除.' -StartedAtUtc $cleanupStarted))
        } else {
            $stages.Add((New-IosCandidateStage -Stage 'CLEANUP' -Status failed -ErrorCode 'CLEANUP_FAILED' `
                -ExitCode $cleanupExitCode -Detail ($cleanupErrors -join "`n") -StartedAtUtc $cleanupStarted))
        }
    }

    $cleanupFailure = $stages | Where-Object { $_.stage -eq 'CLEANUP' -and $_.status -eq 'failed' } | Select-Object -First 1
    $firstFailure = $stages | Where-Object { $_.status -eq 'failed' } | Select-Object -First 1
    $status = if ($null -eq $firstFailure) { 'passed' } else { 'failed' }
    $result = [pscustomobject]@{
        schema_version = 1
        status = $status
        run_id = $runId
        commit = $headCommit
        bundle_id = $BundleId
        output_directory = $outputRoot
        output_ipa = if ($ipaArtifactPublished) { $outputIpa } else { $null }
        output_ipa_sha256 = $ipaSha256
        framework_zip = if ($frameworkArtifactPublished) { $frameworkArtifact } else { $null }
        framework_zip_sha256 = $frameworkSha256
        package_report = if ($packageReportPublished) { $packageReport } else { $null }
        candidate_report = $candidateReport
        first_failure = if ($null -eq $firstFailure) { $null } else { [string]$firstFailure.stage }
        root_error_code = if ($null -eq $firstFailure) { $null } else { [string]$firstFailure.error_code }
        remote_cleanup = if ($null -eq $cleanupFailure) { 'passed' } else { 'failed' }
        stages = @($stages)
    }
    $temporaryCandidateReport = Join-Path $outputRoot ".$([IO.Path]::GetFileName($candidateReport)).$runId.tmp"
    $result | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $temporaryCandidateReport -Encoding UTF8
    Move-Item -LiteralPath $temporaryCandidateReport -Destination $candidateReport -Force
    $result
}
# //// /从干净提交构建一个 unsigned CN iPhone 候选包 ////

# //// 作为命令行入口执行并返回进程状态 [@x380kkm 2026-08-19] ////
if ($MyInvocation.InvocationName -ne '.') {
    $commandParameters = ConvertFrom-IosCandidateCommandLine -Arguments @($args)
    $result = Invoke-IosCnCandidateBuild @commandParameters
    $result | ConvertTo-Json -Depth 10
    if ($result.status -ne 'passed') { exit 1 }
}
# //// /作为命令行入口执行并返回进程状态 ////
