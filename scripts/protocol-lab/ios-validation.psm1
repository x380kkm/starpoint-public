# audience: internal
# # ios-validation
# 该模块编排本地预检, 临时远端构建和单 Simulator 验证, 并只保存脱敏报告与回传产物.

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$script:IosValidationStageOrder = @(
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
    "persistence",
    "cleanup"
)
$script:IosValidationStageDependencies = @{
    ARCHIVE = @("LOCAL_REPOSITORY", "LOCAL_CONTRACTS", "REMOTE_TOOLCHAIN")
    DEVICE_BUILD = @("ARCHIVE", "REMOTE_TOOLCHAIN")
    simulator_build = @("ARCHIVE", "REMOTE_TOOLCHAIN")
    simulator_boot = @("simulator_build")
    app_install = @("simulator_build", "simulator_boot")
    app_launch = @("app_install")
    internal_diagnostic = @("app_launch")
    loopback = @("internal_diagnostic")
    background_checkpoint = @("loopback")
    foreground_resume = @("background_checkpoint")
    protocol_chain = @("foreground_resume")
    http_observations = @("foreground_resume")
    relaunch = @("protocol_chain", "http_observations")
    persistence = @("relaunch")
}

# //// 返回阶段的直接依赖 [@x380kkm 2026-08-18] ////
function Get-IosValidationStageDependencies {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Stage)

    if ($script:IosValidationStageDependencies.ContainsKey($Stage)) {
        return @($script:IosValidationStageDependencies[$Stage])
    }
    @()
}
# //// /返回阶段的直接依赖 ////

# //// 返回阶段的固定排序位置 [@x380kkm 2026-08-18] ////
function Get-IosValidationStageOrderIndex {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Stage)

    $index = [Array]::IndexOf($script:IosValidationStageOrder, $Stage)
    if ($index -lt 0) { return [int]::MaxValue }
    $index
}
# //// /返回阶段的固定排序位置 ////

# //// 构造稳定的阶段结果 [@x380kkm 2026-08-18] ////
function New-IosValidationStage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Stage,
        [Parameter(Mandatory)][ValidateSet("passed", "failed", "blocked", "skipped")][string]$Status,
        [AllowNull()][string]$ErrorCode,
        [AllowNull()][string]$Detail,
        [datetime]$StartedAtUtc = [datetime]::UtcNow,
        [datetime]$EndedAtUtc = [datetime]::UtcNow,
        [AllowNull()][Nullable[int]]$ExitCode,
        [AllowNull()][string[]]$DependsOn,
        [AllowNull()][string]$ArtifactPath,
        [AllowNull()][string]$Sha256
    )

    if ($null -eq $DependsOn) {
        $DependsOn = @(Get-IosValidationStageDependencies -Stage $Stage)
    }
    [pscustomobject][ordered]@{
        stage = $Stage
        status = $Status
        error_code = if ([string]::IsNullOrWhiteSpace($ErrorCode)) { $null } else { $ErrorCode }
        exit_code = if ($null -eq $ExitCode) { $null } else { [int]$ExitCode }
        detail = Protect-IosValidationText -Text $Detail
        started_at = $StartedAtUtc.ToUniversalTime().ToString("o")
        ended_at = $EndedAtUtc.ToUniversalTime().ToString("o")
        depends_on = @($DependsOn)
        artifact_path = if ([string]::IsNullOrWhiteSpace($ArtifactPath)) { $null } else { $ArtifactPath }
        sha256 = if ([string]::IsNullOrWhiteSpace($Sha256)) { $null } else { $Sha256.ToLowerInvariant() }
    }
}
# //// /构造稳定的阶段结果 ////

# //// 通过 stdin 调用共享诊断脱敏器 [@x380kkm 2026-08-18] ////
function Invoke-IosValidationSanitizer {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Text)

    $repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
    $sanitizerPath = Join-Path $repositoryRoot "platforms\ios\sanitize-diagnostic-detail.py"
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = "uv"
    $startInfo.ArgumentList.Add("run")
    $startInfo.ArgumentList.Add("--python")
    $startInfo.ArgumentList.Add("3.12")
    $startInfo.ArgumentList.Add("python")
    $startInfo.ArgumentList.Add($sanitizerPath)
    $startInfo.WorkingDirectory = $repositoryRoot
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $utf8 = [Text.UTF8Encoding]::new($false)
    $startInfo.StandardInputEncoding = $utf8
    $startInfo.StandardOutputEncoding = $utf8
    $startInfo.StandardErrorEncoding = $utf8

    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) { return "[detail redaction failed]" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $process.StandardInput.Write($Text)
        $process.StandardInput.Close()
        if (-not $process.WaitForExit(10000)) {
            try { $process.Kill($true) } catch {}
            [void]$process.WaitForExit(5000)
            return "[detail redaction failed]"
        }
        $sanitized = $stdoutTask.GetAwaiter().GetResult()
        [void]$stderrTask.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { return "[detail redaction failed]" }
        $sanitized
    } catch {
        "[detail redaction failed]"
    } finally {
        $process.Dispose()
    }
}
# //// /通过 stdin 调用共享诊断脱敏器 ////

# //// 删除报告中的凭据形状并限制错误长度 [@x380kkm 2026-08-18] ////
function Protect-IosValidationText {
    [CmdletBinding()]
    param([AllowNull()][string]$Text)

    if ([string]::IsNullOrEmpty($Text)) { return "" }
    Invoke-IosValidationSanitizer -Text $Text
}
# //// /删除报告中的凭据形状并限制错误长度 ////

# //// 执行外部进程并收集有限输出 [@x380kkm 2026-08-18] ////
function Invoke-IosValidationProcess {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [ValidateRange(1, 7200)][int]$TimeoutSeconds = 600
    )

    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }

    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "进程未启动: $FilePath"
        }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill($true)
            $process.WaitForExit(5000) | Out-Null
            return [pscustomobject]@{ ExitCode = 124; Stdout = $stdoutTask.Result; Stderr = "进程超时." }
        }
        [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdoutTask, $stderrTask), 5000) | Out-Null
        [pscustomobject]@{
            ExitCode = $process.ExitCode
            Stdout = $stdoutTask.Result
            Stderr = $stderrTask.Result
        }
    } finally {
        $process.Dispose()
    }
}
# //// /执行外部进程并收集有限输出 ////

# //// 验证一个 arm64 Mach-O 条目没有嵌入代码签名 [@x380kkm 2026-08-18] ////
function Test-IosUnsignedMachOEntry {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][IO.Compression.ZipArchiveEntry]$Entry,
        [Parameter(Mandatory)][uint32]$ExpectedFileType
    )

    $stream = $Entry.Open()
    $reader = [IO.BinaryReader]::new($stream)
    try {
        $header = $reader.ReadBytes(32)
        if ($header.Length -ne 32) {
            throw "Mach-O 头长度不足: $($Entry.FullName)"
        }
        $machOMagic = [uint32]::Parse("FEEDFACF", [Globalization.NumberStyles]::HexNumber)
        $arm64CpuType = [uint32]::Parse("0100000C", [Globalization.NumberStyles]::HexNumber)
        if ([BitConverter]::ToUInt32($header, 0) -ne $machOMagic -or
            [BitConverter]::ToUInt32($header, 4) -ne $arm64CpuType -or
            [BitConverter]::ToUInt32($header, 12) -ne $ExpectedFileType) {
            throw "Mach-O 不是预期的 arm64 类型: $($Entry.FullName)"
        }

        $commandCount = [BitConverter]::ToUInt32($header, 16)
        $commandBytesLength = [BitConverter]::ToUInt32($header, 20)
        if ($commandBytesLength -gt 16MB) {
            throw "Mach-O load commands 长度异常: $($Entry.FullName)"
        }
        $commandBytes = $reader.ReadBytes([int]$commandBytesLength)
        if ($commandBytes.Length -ne $commandBytesLength) {
            throw "Mach-O load commands 不完整: $($Entry.FullName)"
        }

        $offset = 0
        for ($index = 0; $index -lt $commandCount; $index++) {
            if ($offset + 8 -gt $commandBytes.Length) {
                throw "Mach-O load command 边界无效: $($Entry.FullName)"
            }
            $command = [BitConverter]::ToUInt32($commandBytes, $offset)
            $commandSize = [BitConverter]::ToUInt32($commandBytes, $offset + 4)
            if ($commandSize -lt 8 -or $offset + $commandSize -gt $commandBytes.Length) {
                throw "Mach-O load command 长度无效: $($Entry.FullName)"
            }
            if ($command -eq 0x1D) {
                throw "device Mach-O 包含 LC_CODE_SIGNATURE: $($Entry.FullName)"
            }
            $offset += $commandSize
        }
    } finally {
        $reader.Dispose()
        $stream.Dispose()
    }
}
# //// /验证一个 arm64 Mach-O 条目没有嵌入代码签名 ////

# //// 验证 unsigned device IPA 的结构和 arm64 Mach-O [@x380kkm 2026-08-18] ////
function Test-IosDeviceArtifact {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$IpaPath)

    $resolvedPath = [IO.Path]::GetFullPath($IpaPath)
    if (-not (Test-Path -LiteralPath $resolvedPath -PathType Leaf)) {
        throw "device IPA 不存在: $resolvedPath"
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($resolvedPath)
    try {
        $entries = @($archive.Entries)
        $names = @($entries.FullName)
        if ($names -match '(^|/)_CodeSignature/' -or $names -match '(^|/)embedded\.mobileprovision$') {
            throw "device IPA 包含签名或 provisioning profile."
        }
        $mainEntry = $entries | Where-Object { $_.FullName -match '^Payload/[^/]+\.app/PersonalServiceDiagnostic$' } | Select-Object -First 1
        $frameworkEntry = $entries | Where-Object { $_.FullName -match '^Payload/[^/]+\.app/Frameworks/PersonalServiceBootstrap\.framework/PersonalServiceBootstrap$' } | Select-Object -First 1
        if ($null -eq $mainEntry -or $null -eq $frameworkEntry) {
            throw "device IPA 缺少诊断主程序或个人服务 Framework."
        }
        Test-IosUnsignedMachOEntry -Entry $mainEntry -ExpectedFileType 2
        Test-IosUnsignedMachOEntry -Entry $frameworkEntry -ExpectedFileType 6
    } finally {
        $archive.Dispose()
    }

    $hash = (Get-FileHash -LiteralPath $resolvedPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $shaPath = "$resolvedPath.sha256"
    if (Test-Path -LiteralPath $shaPath -PathType Leaf) {
        $declaredHash = ((Get-Content -LiteralPath $shaPath -Raw -Encoding UTF8).Trim() -split '\s+')[0].ToLowerInvariant()
        if ($declaredHash -ne $hash) {
            throw "device IPA 的 SHA-256 与 sidecar 不一致."
        }
    }
    [pscustomobject]@{ Path = $resolvedPath; Sha256 = $hash; Bytes = (Get-Item -LiteralPath $resolvedPath).Length }
}
# //// /验证 unsigned device IPA 的结构和 arm64 Mach-O ////

# //// 汇总首个阻断项和可复跑阶段 [@x380kkm 2026-08-18] ////
function Get-IosValidationSummary {
    [CmdletBinding()]
    param([Parameter(Mandatory)][object[]]$Stages)

    $ordered = @($Stages | Sort-Object { Get-IosValidationStageOrderIndex -Stage ([string]$_.stage) })
    $failed = @($ordered | Where-Object { $_.status -eq "failed" })
    $blocked = @($ordered | Where-Object { $_.status -eq "blocked" })
    $firstFailure = $failed | Select-Object -First 1
    $lastSuccessful = $null
    foreach ($stage in $ordered) {
        if ($null -ne $firstFailure -and $stage.stage -eq $firstFailure.stage) { break }
        if ($stage.status -eq "passed" -and $stage.stage -ne "cleanup") { $lastSuccessful = $stage.stage }
    }
    [pscustomobject][ordered]@{
        status = if ($failed.Count -eq 0) { "passed" } else { "failed" }
        first_failure = if ($null -eq $firstFailure) { $null } else { $firstFailure.stage }
        root_blocker = if ($null -eq $firstFailure) { $null } else { $firstFailure.error_code }
        independent_failures = @($failed | ForEach-Object { $_.stage })
        blocked_stages = @($blocked | ForEach-Object { $_.stage })
        last_successful_stage = $lastSuccessful
        rerun_stage = if ($null -eq $firstFailure) { $null } else { $firstFailure.stage }
    }
}
# //// /汇总首个阻断项和可复跑阶段 ////

# //// 写入 JSON 和 Markdown 结果 [@x380kkm 2026-08-18] ////
function Write-IosValidationReports {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$OutputDirectory,
        [Parameter(Mandatory)][string]$Commit,
        [Parameter(Mandatory)][string]$RunId,
        [Parameter(Mandatory)][object[]]$Stages,
        [Parameter(Mandatory)][object]$Summary
    )

    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    $report = [ordered]@{
        schema_version = 1
        run_id = $RunId
        commit = $Commit
        generated_at = [datetime]::UtcNow.ToString("o")
        status = $Summary.status
        first_failure = $Summary.first_failure
        root_blocker = $Summary.root_blocker
        independent_failures = $Summary.independent_failures
        blocked_stages = $Summary.blocked_stages
        last_successful_stage = $Summary.last_successful_stage
        rerun_stage = $Summary.rerun_stage
        stages = @($Stages)
    }
    $jsonPath = Join-Path $OutputDirectory "ios-validation.json"
    $markdownPath = Join-Path $OutputDirectory "ios-validation.md"
    $report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $jsonPath -Encoding UTF8
    $lines = [Collections.Generic.List[string]]::new()
    $lines.Add("# iOS validation")
    $lines.Add("")
    $lines.Add("- Run ID: $RunId")
    $lines.Add("- Commit: $Commit")
    $lines.Add("- Status: $($Summary.status)")
    $lines.Add("- First failure: $(if ($Summary.first_failure) { $Summary.first_failure } else { 'none' })")
    $lines.Add("- Root blocker: $(if ($Summary.root_blocker) { $Summary.root_blocker } else { 'none' })")
    $lines.Add("- Last successful stage: $(if ($Summary.last_successful_stage) { $Summary.last_successful_stage } else { 'none' })")
    $lines.Add("")
    $lines.Add("| Stage | Status | Exit | Error | Depends on | Artifact | SHA-256 | Detail |")
    $lines.Add("|---|---|---:|---|---|---|---|---|")
    foreach ($stage in $Stages) {
        $detail = ([string]$stage.detail).Replace("|", "\\|")
        $dependencies = (@($stage.depends_on) -join ", ").Replace("|", "\\|")
        $artifact = ([string]$stage.artifact_path).Replace("|", "\\|")
        $lines.Add("| $($stage.stage) | $($stage.status) | $($stage.exit_code) | $($stage.error_code) | $dependencies | $artifact | $($stage.sha256) | $detail |")
    }
    $lines | Set-Content -LiteralPath $markdownPath -Encoding UTF8
    [pscustomobject]@{ JsonPath = $jsonPath; MarkdownPath = $markdownPath }
}
# //// /写入 JSON 和 Markdown 结果 ////

# //// 加载预检和执行边界 [@x380kkm 2026-08-18] ////
. (Join-Path $PSScriptRoot "ios-validation-preflight.ps1")
. (Join-Path $PSScriptRoot "ios-validation-run.ps1")
# //// /加载预检和执行边界 ////

# //// 将一个阶段替换为报告完整性失败 [@x380kkm 2026-08-18] ////
function Set-IosValidationStageFailure {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][Collections.Generic.List[object]]$Stages,
        [Parameter(Mandatory)][string]$Stage,
        [Parameter(Mandatory)][string]$ErrorCode,
        [Parameter(Mandatory)][string]$Detail
    )

    foreach ($existing in @($Stages | Where-Object { $_.stage -eq $Stage })) {
        $Stages.Remove($existing) | Out-Null
    }
    $Stages.Add((New-IosValidationStage -Stage $Stage -Status failed -ErrorCode $ErrorCode -ExitCode 1 -Detail $Detail))
}
# //// /将一个阶段替换为报告完整性失败 ////

# //// 补齐并验证所有必需阶段 [@x380kkm 2026-08-18] ////
function Repair-IosValidationStageResults {
    [CmdletBinding()]
    param([Parameter(Mandatory)][Collections.Generic.List[object]]$Stages)

    $allowedStatuses = @("passed", "failed", "blocked")
    foreach ($name in $script:IosValidationStageOrder) {
        $matches = @($Stages | Where-Object { $_.stage -eq $name })
        if ($matches.Count -gt 1) {
            $errorCode = if ($name -eq "cleanup") { "CLEANUP_FAILED" } else { "VALIDATION_REPORT_INVALID" }
            Set-IosValidationStageFailure -Stages $Stages -Stage $name -ErrorCode $errorCode -Detail "Stage returned more than one result."
            continue
        }
        if ($matches.Count -eq 1) {
            $status = [string]$matches[0].status
            $allowsSkipped = $name -in @("DEVICE_ARTIFACT", "DEVICE_BUILD")
            if ($status -notin $allowedStatuses -and -not ($allowsSkipped -and $status -eq "skipped")) {
                $errorCode = if ($name -eq "cleanup") { "CLEANUP_FAILED" } else { "VALIDATION_REPORT_INVALID" }
                Set-IosValidationStageFailure -Stages $Stages -Stage $name -ErrorCode $errorCode -Detail "Stage returned an invalid status."
            }
        }
    }

    foreach ($name in $script:IosValidationStageOrder) {
        if ($name -eq "cleanup") { continue }
        $stage = $Stages | Where-Object { $_.stage -eq $name } | Select-Object -First 1
        $dependencies = @(Get-IosValidationStageDependencies -Stage $name)
        $blockingDependencies = @(
            foreach ($dependency in $dependencies) {
                $dependencyStage = $Stages | Where-Object { $_.stage -eq $dependency } | Select-Object -First 1
                if ($null -eq $dependencyStage -or $dependencyStage.status -in @("failed", "blocked")) {
                    $dependency
                }
            }
        )
        if ($null -eq $stage) {
            if ($blockingDependencies.Count -gt 0) {
                $Stages.Add((New-IosValidationStage -Stage $name -Status blocked -Detail "Blocked by: $($blockingDependencies -join ', ')."))
            } else {
                Set-IosValidationStageFailure -Stages $Stages -Stage $name -ErrorCode "VALIDATION_REPORT_INVALID" -Detail "Required stage result is missing."
            }
            continue
        }
        if ($stage.status -eq "blocked" -and $blockingDependencies.Count -eq 0) {
            Set-IosValidationStageFailure -Stages $Stages -Stage $name -ErrorCode "VALIDATION_REPORT_INVALID" -Detail "Stage is blocked without a blocking dependency."
        } elseif ($stage.status -in @("passed", "skipped") -and $blockingDependencies.Count -gt 0) {
            Set-IosValidationStageFailure -Stages $Stages -Stage $name -ErrorCode "VALIDATION_REPORT_INVALID" -Detail "Stage completed despite a blocking dependency."
        }
    }

    $cleanupStages = @($Stages | Where-Object { $_.stage -eq "cleanup" })
    if ($cleanupStages.Count -eq 0) {
        $Stages.Add((New-IosValidationStage -Stage "cleanup" -Status failed -ErrorCode "CLEANUP_FAILED" -ExitCode 1 -Detail "Cleanup did not return a stage."))
    } elseif ($cleanupStages[0].status -notin @("passed", "failed")) {
        Set-IosValidationStageFailure -Stages $Stages -Stage "cleanup" -ErrorCode "CLEANUP_FAILED" -Detail "Cleanup returned an invalid status."
    }
}
# //// /补齐并验证所有必需阶段 ////

# //// 补齐未执行的依赖阶段并完成报告 [@x380kkm 2026-08-18] ////
function Complete-IosValidationRun {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$OutputRoot,
        [Parameter(Mandatory)][string]$Commit,
        [string]$RunId = (Split-Path -Leaf $OutputRoot),
        [Parameter(Mandatory)][Collections.IEnumerable]$Stages
    )

    $stageList = [Collections.Generic.List[object]]::new()
    foreach ($stage in $Stages) { $stageList.Add($stage) }
    Repair-IosValidationStageResults -Stages $stageList
    $stageArray = @($stageList.ToArray() | Sort-Object {
        Get-IosValidationStageOrderIndex -Stage ([string]$_.stage)
    })
    $summary = Get-IosValidationSummary -Stages $stageArray
    $paths = Write-IosValidationReports -OutputDirectory $OutputRoot -Commit $Commit -RunId $RunId -Stages $stageArray -Summary $summary
    [pscustomobject][ordered]@{
        Status = $summary.status
        RunId = $RunId
        FirstFailure = $summary.first_failure
        RootBlocker = $summary.root_blocker
        LastSuccessfulStage = $summary.last_successful_stage
        JsonReport = $paths.JsonPath
        MarkdownReport = $paths.MarkdownPath
        OutputDirectory = $OutputRoot
    }
}
# //// /补齐未执行的依赖阶段并完成报告 ////

Export-ModuleMember -Function @(
    "Complete-IosValidationRun",
    "Get-IosValidationSummary",
    "Invoke-IosValidation",
    "Invoke-IosValidationProcess",
    "New-IosValidationStage",
    "Protect-IosValidationText",
    "Test-IosDeviceArtifact",
    "Test-IosRemoteToolchain",
    "Write-IosValidationReports"
)
