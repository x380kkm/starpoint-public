# audience: internal
# # ios-validation-run
# 该脚本在模块作用域传递提交与诊断 CDN, 执行 device 产物验证和单 Simulator 协议诊断.
# //// 执行单次 iOS 多判定流水线 [@x380kkm 2026-08-18] ////
function Invoke-IosValidation {
    [CmdletBinding()]
    param(
        [string]$RepositoryRoot,
        [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._:-]*$')][string]$SshHost = "starpoint-mac",
        [ValidatePattern('^[A-Za-z0-9 .()_-]+$')][string]$SimulatorName = "iPhone 17 Pro",
        [string]$Commit = "HEAD",
        [string]$OutputRoot,
        [string]$DeviceIpaPath,
        [string]$DiagnosticCdnRoot,
        [ValidateRange(1, 3)][int]$ThrottleLimit = 3,
        [switch]$RebuildDeviceIpa
    )

    if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
        $RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
    } else {
        $RepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
    }
    $commitHash = (& git -C $RepositoryRoot rev-parse --verify "$Commit`^{commit}").Trim()
    if ($LASTEXITCODE -ne 0) { throw "无法解析 commit: $Commit" }
    $shortCommit = (& git -C $RepositoryRoot rev-parse --short $commitHash).Trim()
    $runId = [Guid]::NewGuid().ToString("N")
    $workspaceRoot = Split-Path -Parent $RepositoryRoot
    if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
        $OutputRoot = Join-Path $workspaceRoot "artifacts\ios-validation\$shortCommit-$runId"
    }
    $OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
    if ([string]::IsNullOrWhiteSpace($DeviceIpaPath)) {
        $DeviceIpaPath = Join-Path $workspaceRoot "artifacts\ios-device-diagnostic\$shortCommit\PersonalServiceDiagnostic-unsigned.ipa"
    }
    if ([string]::IsNullOrWhiteSpace($DiagnosticCdnRoot)) {
        $DiagnosticCdnRoot = Join-Path $RepositoryRoot ".cdn\cn"
    }
    $DiagnosticCdnRoot = [IO.Path]::GetFullPath($DiagnosticCdnRoot)

    $stages = [Collections.Generic.List[object]]::new()
    $localArchive = Join-Path ([IO.Path]::GetTempPath()) "starpoint-ios-$runId.tar.gz"
    $localCdnArchive = Join-Path ([IO.Path]::GetTempPath()) "starpoint-ios-$runId-cdn.tar.gz"
    $remoteRoot = "/tmp/starpoint-ios-$runId"
    $remoteMayExist = $false
    $remoteCleanupError = $null
    $preflightBlocked = $false
    $recordedFailure = $false
    $scpOptions = @(
        "-O",
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=10",
        "-o", "ServerAliveInterval=30",
        "-o", "ServerAliveCountMax=4"
    )
    try {
        foreach ($stage in @(Invoke-IosValidationPreflight -RepositoryRoot $RepositoryRoot -Commit $commitHash -SshHost $SshHost -SimulatorName $SimulatorName -DeviceIpaPath $DeviceIpaPath -ThrottleLimit $ThrottleLimit)) {
            $stages.Add($stage)
        }
        $hardPreflightFailure = @($stages | Where-Object { $_.status -eq "failed" }).Count -gt 0
        if ($hardPreflightFailure) {
            $preflightBlocked = $true
            throw "PREFLIGHT_BLOCKED"
        }

        $archiveStarted = [datetime]::UtcNow
        $archiveResult = Invoke-IosValidationProcess -FilePath "git" -Arguments @("-C", $RepositoryRoot, "archive", "--format=tar.gz", "--output=$localArchive", $commitHash) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 120
        if ($archiveResult.ExitCode -ne 0) {
            $stages.Add((New-IosValidationStage -Stage "ARCHIVE" -Status failed -ErrorCode "ARCHIVE_FAILED" -ExitCode $archiveResult.ExitCode -Detail $archiveResult.Stderr -StartedAtUtc $archiveStarted))
            $recordedFailure = $true
            throw "RECORDED_FAILURE"
        }
        $cdnGenerator = Join-Path $PSScriptRoot "prepare-ios-simulator-diagnostic-cdn.py"
        $cdnGeneratorArguments = @(
            "run", "--python", "3.12", "python", $cdnGenerator,
            "--output", $localCdnArchive
        )
        if (Test-Path -LiteralPath $DiagnosticCdnRoot -PathType Container) {
            $cdnGeneratorArguments += @("--source-root", $DiagnosticCdnRoot)
        }
        $cdnArchiveResult = Invoke-IosValidationProcess -FilePath "uv" -Arguments $cdnGeneratorArguments -WorkingDirectory $RepositoryRoot -TimeoutSeconds 120
        if ($cdnArchiveResult.ExitCode -ne 0) {
            $stages.Add((New-IosValidationStage -Stage "ARCHIVE" -Status failed -ErrorCode "ARCHIVE_FAILED" -ExitCode $cdnArchiveResult.ExitCode -Detail $cdnArchiveResult.Stderr -StartedAtUtc $archiveStarted))
            $recordedFailure = $true
            throw "RECORDED_FAILURE"
        }
        $remoteMayExist = $true
        $mkdirResult = Invoke-IosValidationProcess -FilePath "ssh" -Arguments @($SshHost, "mkdir -p '$remoteRoot/source' '$remoteRoot/cdn'") -WorkingDirectory $RepositoryRoot -TimeoutSeconds 30
        if ($mkdirResult.ExitCode -ne 0) {
            $stages.Add((New-IosValidationStage -Stage "ARCHIVE" -Status failed -ErrorCode "ARCHIVE_FAILED" -ExitCode $mkdirResult.ExitCode -Detail $mkdirResult.Stderr -StartedAtUtc $archiveStarted))
            $recordedFailure = $true
            throw "RECORDED_FAILURE"
        }
        $copyResult = Invoke-IosValidationProcess -FilePath "scp" -Arguments @(
            $scpOptions + @($localArchive, "$($SshHost):$remoteRoot/source.tar.gz")
        ) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 600
        if ($copyResult.ExitCode -ne 0) {
            $stages.Add((New-IosValidationStage -Stage "ARCHIVE" -Status failed -ErrorCode "ARCHIVE_FAILED" -ExitCode $copyResult.ExitCode -Detail $copyResult.Stderr -StartedAtUtc $archiveStarted))
            $recordedFailure = $true
            throw "RECORDED_FAILURE"
        }
        $cdnCopyResult = Invoke-IosValidationProcess -FilePath "scp" -Arguments @(
            $scpOptions + @($localCdnArchive, "$($SshHost):$remoteRoot/cdn.tar.gz")
        ) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 600
        if ($cdnCopyResult.ExitCode -ne 0) {
            $stages.Add((New-IosValidationStage -Stage "ARCHIVE" -Status failed -ErrorCode "ARCHIVE_FAILED" -ExitCode $cdnCopyResult.ExitCode -Detail $cdnCopyResult.Stderr -StartedAtUtc $archiveStarted))
            $recordedFailure = $true
            throw "RECORDED_FAILURE"
        }
        $extractResult = Invoke-IosValidationProcess -FilePath "ssh" -Arguments @($SshHost, "tar -xzf '$remoteRoot/source.tar.gz' -C '$remoteRoot/source' && tar -xzf '$remoteRoot/cdn.tar.gz' -C '$remoteRoot/cdn'") -WorkingDirectory $RepositoryRoot -TimeoutSeconds 120
        if ($extractResult.ExitCode -ne 0) {
            $stages.Add((New-IosValidationStage -Stage "ARCHIVE" -Status failed -ErrorCode "ARCHIVE_FAILED" -ExitCode $extractResult.ExitCode -Detail $extractResult.Stderr -StartedAtUtc $archiveStarted))
            $recordedFailure = $true
            throw "RECORDED_FAILURE"
        }
        $stages.Add((New-IosValidationStage -Stage "ARCHIVE" -Status passed -ExitCode 0 -Detail "Committed source and a self-consistent diagnostic CDN were uploaded to a temporary directory." -StartedAtUtc $archiveStarted))

        $deviceStage = $stages | Where-Object { $_.stage -eq "DEVICE_ARTIFACT" } | Select-Object -First 1
        if ($RebuildDeviceIpa -or $deviceStage.status -ne "passed") {
            $deviceStarted = [datetime]::UtcNow
            $remoteDeviceCommand = 'export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer; export CARGO_TARGET_DIR=' + "'$remoteRoot/cargo-device'" + '; echo $$ > ' + "'$remoteRoot/device-runner.pid'" + "; cd '$remoteRoot/source'; " + "bash platforms/ios/build-device-harness.sh '$remoteRoot/device'" + ' & child_pid=$!; echo $child_pid > ' + "'$remoteRoot/device-child.pid'" + '; wait $child_pid'
            $deviceBuild = Invoke-IosValidationProcess -FilePath "ssh" -Arguments @($SshHost, $remoteDeviceCommand) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 1800
            if ($deviceBuild.ExitCode -ne 0) {
                $stages.Add((New-IosValidationStage -Stage "DEVICE_BUILD" -Status failed -ErrorCode "DEVICE_BUILD_FAILED" -ExitCode $deviceBuild.ExitCode -Detail $deviceBuild.Stderr -StartedAtUtc $deviceStarted))
                try {
                    $deviceStop = Stop-IosRemoteValidationRunner -RepositoryRoot $RepositoryRoot -SshHost $SshHost -RemoteRoot $remoteRoot -RunId $runId -RunnerName device
                    $deviceStopDetail = ($deviceStop.Stderr, $deviceStop.Stdout) -join " "
                    $deviceStopExitCode = $deviceStop.ExitCode
                } catch {
                    $deviceStopDetail = $_.Exception.Message
                    $deviceStopExitCode = 1
                }
                if ($deviceStopExitCode -ne 0) {
                    $stages.Add((New-IosValidationStage -Stage "simulator_build" -Status failed -ErrorCode "CLEANUP_FAILED" -ExitCode $deviceStopExitCode -Detail $deviceStopDetail))
                    $recordedFailure = $true
                    throw "RECORDED_FAILURE"
                }
            } else {
                $deviceOutput = Join-Path $OutputRoot "device"
                New-Item -ItemType Directory -Force -Path $deviceOutput | Out-Null
                $deviceCopy = Invoke-IosValidationProcess -FilePath "scp" -Arguments @(
                    $scpOptions + @(
                        "$($SshHost):$remoteRoot/device/PersonalServiceDiagnostic-unsigned.ipa",
                        "$($SshHost):$remoteRoot/device/PersonalServiceDiagnostic-unsigned.ipa.sha256",
                        $deviceOutput
                    )
                ) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 900
                if ($deviceCopy.ExitCode -ne 0) {
                    $stages.Add((New-IosValidationStage -Stage "DEVICE_BUILD" -Status failed -ErrorCode "DEVICE_BUILD_FAILED" -ExitCode $deviceCopy.ExitCode -Detail $deviceCopy.Stderr -StartedAtUtc $deviceStarted))
                } else {
                    try {
                        $artifact = Test-IosDeviceArtifact -IpaPath (Join-Path $deviceOutput "PersonalServiceDiagnostic-unsigned.ipa")
                        $stages.Add((New-IosValidationStage -Stage "DEVICE_BUILD" -Status passed -ExitCode 0 -Detail "Unsigned device IPA rebuilt." -ArtifactPath $artifact.Path -Sha256 $artifact.Sha256 -StartedAtUtc $deviceStarted))
                    } catch {
                        $stages.Add((New-IosValidationStage -Stage "DEVICE_BUILD" -Status failed -ErrorCode "DEVICE_BUILD_FAILED" -ExitCode 1 -Detail $_.Exception.Message -StartedAtUtc $deviceStarted))
                    }
                }
            }
        } else {
            $stages.Add((New-IosValidationStage -Stage "DEVICE_BUILD" -Status skipped -Detail "Verified device IPA was reused."))
        }

        $simulatorStarted = [datetime]::UtcNow
        $remoteReport = "$remoteRoot/simulator-report.json"
        $remoteScenarioReport = "$remoteRoot/simulator/ios-cn-game-scenario.json"
        $remoteObservationsReport = "$remoteRoot/simulator/http-observations.json"
        $remoteSimulatorCommand = 'export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer; export CARGO_TARGET_DIR=' + "'$remoteRoot/cargo-simulator'" + '; echo $$ > ' + "'$remoteRoot/simulator-runner.pid'" + "; export STARPOINT_IOS_SIMULATOR_OWNER_PATH='$remoteRoot/simulator-udid.txt'; export STARPOINT_IOS_DIAGNOSTIC_CDN_ROOT='$remoteRoot/cdn'; cd '$remoteRoot/source'; " + "bash platforms/ios/run-simulator-diagnostic.sh '$remoteRoot/simulator' '$SimulatorName' '$remoteReport'" + ' & child_pid=$!; echo $child_pid > ' + "'$remoteRoot/simulator-child.pid'" + '; wait $child_pid'
        $simulatorResult = Invoke-IosValidationProcess -FilePath "ssh" -Arguments @($SshHost, $remoteSimulatorCommand) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 2400
        $localSimulatorReport = Join-Path $OutputRoot "simulator-diagnostic.json"
        $reportCopy = Invoke-IosValidationProcess -FilePath "scp" -Arguments @(
            $scpOptions + @("$($SshHost):$remoteReport", $localSimulatorReport)
        ) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 120
        if ($reportCopy.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $localSimulatorReport)) {
            $failureExitCode = if ($simulatorResult.ExitCode -ne 0) { $simulatorResult.ExitCode } else { $reportCopy.ExitCode }
            $stages.Add((New-IosValidationStage -Stage "simulator_build" -Status failed -ErrorCode "SIMULATOR_BUILD_FAILED" -ExitCode $failureExitCode -Detail ($simulatorResult.Stderr + " " + $reportCopy.Stderr) -StartedAtUtc $simulatorStarted))
        } else {
            $remoteResult = Get-Content -LiteralPath $localSimulatorReport -Raw -Encoding UTF8 | ConvertFrom-Json
            $artifactMappings = @{
                protocol_chain = [pscustomobject]@{
                    Remote = $remoteScenarioReport
                    Local = (Join-Path $OutputRoot "ios-cn-game-scenario.json")
                }
                http_observations = [pscustomobject]@{
                    Remote = $remoteObservationsReport
                    Local = (Join-Path $OutputRoot "http-observations.json")
                }
            }
            foreach ($remoteStage in @($remoteResult.stages)) {
                if ($remoteStage.stage -eq "cleanup") {
                    if ($remoteStage.status -eq "failed") { $remoteCleanupError = [string]$remoteStage.detail }
                    continue
                }
                $stageStatus = [string]$remoteStage.status
                $stageErrorCode = $remoteStage.error_code
                $stageExitCode = $remoteStage.exit_code
                $stageDetail = [string]$remoteStage.detail
                $stageArtifactPath = $remoteStage.artifact_path
                if ($artifactMappings.ContainsKey([string]$remoteStage.stage)) {
                    $mapping = $artifactMappings[[string]$remoteStage.stage]
                    if (-not [string]::IsNullOrWhiteSpace([string]$remoteStage.sha256)) {
                        $artifactCopy = Invoke-IosValidationProcess -FilePath "scp" -Arguments @(
                            $scpOptions + @("$($SshHost):$($mapping.Remote)", $mapping.Local)
                        ) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 120
                        if ($artifactCopy.ExitCode -eq 0 -and (Test-Path -LiteralPath $mapping.Local)) {
                            $stageArtifactPath = $mapping.Local
                        } else {
                            $stageStatus = "failed"
                            $stageErrorCode = "SIMULATOR_ARTIFACT_COPY_FAILED"
                            $stageExitCode = $artifactCopy.ExitCode
                            $stageDetail = $artifactCopy.Stderr
                            $stageArtifactPath = $null
                        }
                    } else {
                        $stageArtifactPath = $null
                    }
                }
                $stages.Add([pscustomobject]@{
                    stage = [string]$remoteStage.stage
                    status = $stageStatus
                    error_code = $stageErrorCode
                    exit_code = $stageExitCode
                    detail = Protect-IosValidationText -Text $stageDetail
                    started_at = [string]$remoteStage.started_at
                    ended_at = [string]$remoteStage.ended_at
                    depends_on = @($remoteStage.depends_on)
                    artifact_path = $stageArtifactPath
                    sha256 = $remoteStage.sha256
                })
            }
            if ($simulatorResult.ExitCode -ne 0 -and [string]$remoteResult.status -eq "passed") {
                $stages.Add((New-IosValidationStage -Stage "simulator_build" -Status failed -ErrorCode "SIMULATOR_BUILD_FAILED" -ExitCode $simulatorResult.ExitCode -Detail $simulatorResult.Stderr -StartedAtUtc $simulatorStarted))
            }
        }
    } catch {
        $message = $_.Exception.Message
        $knownStop = ($preflightBlocked -and $message -eq "PREFLIGHT_BLOCKED") -or ($recordedFailure -and $message -eq "RECORDED_FAILURE")
        if (-not $knownStop) {
            $stages.Add((New-IosValidationStage -Stage "simulator_build" -Status failed -ErrorCode "SIMULATOR_BUILD_FAILED" -ExitCode 1 -Detail $message))
        }
    } finally {
        $cleanupStarted = [datetime]::UtcNow
        $cleanupErrors = [Collections.Generic.List[string]]::new()
        if ($remoteMayExist) {
            try {
                $cleanupResult = Invoke-IosRemoteCleanup -RepositoryRoot $RepositoryRoot -SshHost $SshHost -RemoteRoot $remoteRoot -RunId $runId
                if ($cleanupResult.ExitCode -ne 0) {
                    $cleanupErrors.Add(($cleanupResult.Stderr, $cleanupResult.Stdout) -join " ")
                }
            } catch {
                $cleanupErrors.Add($_.Exception.Message)
            }
        }
        if (-not [string]::IsNullOrWhiteSpace($remoteCleanupError)) { $cleanupErrors.Add($remoteCleanupError) }
        try {
            foreach ($temporaryArchive in @($localArchive, $localCdnArchive)) {
                if (Test-Path -LiteralPath $temporaryArchive) {
                    Remove-Item -LiteralPath $temporaryArchive -Force
                }
            }
        } catch {
            $cleanupErrors.Add($_.Exception.Message)
        }
        if ($cleanupErrors.Count -eq 0) {
            $stages.Add((New-IosValidationStage -Stage "cleanup" -Status passed -ExitCode 0 -Detail "Temporary local and remote inputs were removed." -StartedAtUtc $cleanupStarted))
        } else {
            $stages.Add((New-IosValidationStage -Stage "cleanup" -Status failed -ErrorCode "CLEANUP_FAILED" -ExitCode 1 -Detail ($cleanupErrors -join " ") -StartedAtUtc $cleanupStarted))
        }
    }

    Complete-IosValidationRun -OutputRoot $OutputRoot -Commit $commitHash -RunId $runId -Stages $stages
}
# //// /执行单次 iOS 多判定流水线 ////
