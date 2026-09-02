# audience: internal
# # ios-validation-preflight
# 该脚本在模块作用域执行并行预检和远端有界清理.
# //// 验证远端 iOS 工具链并返回精确错误代码 [@x380kkm 2026-08-18] ////
function Test-IosRemoteToolchain {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._:-]*$')][string]$SshHost,
        [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9 .()_-]+$')][string]$SimulatorName,
        [datetime]$StartedAtUtc = [datetime]::UtcNow
    )

    $remoteScript = @'
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
# //// 返回远端工具链错误 [@x380kkm 2026-08-18] ////
fail_stage() {
    printf 'STARPOINT_ERROR_CODE=%s\n' "$1"
    exit 1
}
# //// /返回远端工具链错误 ////
test "$(uname -m)" = arm64 || fail_stage XCODE_UNAVAILABLE
command -v xcodebuild >/dev/null 2>&1 || fail_stage XCODE_UNAVAILABLE
xcodebuild -version >/dev/null 2>&1 || fail_stage XCODE_UNAVAILABLE
for tool in codesign curl lsof plutil python3 rustup shasum tar; do
    command -v "$tool" >/dev/null 2>&1 || fail_stage XCODE_UNAVAILABLE
done
sdk_path="$(xcrun --sdk iphoneos --show-sdk-path 2>/dev/null)" || fail_stage IOS_SDK_UNAVAILABLE
runtime_line="$(xcrun simctl list runtimes 2>/dev/null | grep -F 'iOS 26.5' | grep -v -F 'unavailable' | head -n 1)"
test -n "$runtime_line" || fail_stage IOS_SDK_UNAVAILABLE
xcrun simctl list devices available -j 2>/dev/null | python3 -c 'import json, sys; devices = json.load(sys.stdin)["devices"].get("com.apple.CoreSimulator.SimRuntime.iOS-26-5", []); raise SystemExit(0 if any(device.get("name") == sys.argv[1] and device.get("isAvailable", True) for device in devices) else 1)' '__SIMULATOR_NAME__' || fail_stage IOS_SDK_UNAVAILABLE
rustup target list --installed | grep -Fx aarch64-apple-ios >/dev/null || fail_stage RUST_TARGET_MISSING
rustup target list --installed | grep -Fx aarch64-apple-ios-sim >/dev/null || fail_stage RUST_TARGET_MISSING
free_kb="$(df -Pk /tmp | awk 'NR == 2 { print $4 }')"
test -n "$free_kb" || fail_stage REMOTE_DISK_INSUFFICIENT
test "$free_kb" -ge 5242880 || fail_stage REMOTE_DISK_INSUFFICIENT
test -z "$(find -H /tmp -maxdepth 1 -type d -name 'starpoint-ios-*' -print -quit 2>/dev/null)" || fail_stage CLEANUP_FAILED
stale_pid="$(pgrep -f '[s]tarpoint-ios-' 2>/dev/null | while IFS= read -r pid; do
    if [ "$pid" != "$$" ] && [ "$pid" != "$PPID" ]; then
        printf '%s\n' "$pid"
        break
    fi
done)"
test -z "$stale_pid" || fail_stage CLEANUP_FAILED
booted_count="$(xcrun simctl list devices 2>/dev/null | grep -c '(Booted)' || true)"
test "$booted_count" -eq 0 || fail_stage CLEANUP_FAILED
xcode_version="$(xcodebuild -version | tr '\n' ' ' | sed 's/[[:space:]]*$//')"
printf 'STARPOINT_OK=xcode:%s sdk:%s runtime:iOS-26.5 free_kb:%s\n' "$xcode_version" "$(basename "$sdk_path")" "$free_kb"
'@
    $remoteScript = $remoteScript.Replace("`r`n", "`n")
    $remoteScript = $remoteScript.Replace("__SIMULATOR_NAME__", $SimulatorName.Replace("'", ""))
    $remoteCommand = @"
bash -s <<'STARPOINT_REMOTE_PREFLIGHT'
$remoteScript
STARPOINT_REMOTE_PREFLIGHT
"@
    $remoteCommand = $remoteCommand.Replace("`r`n", "`n")
    $result = Invoke-IosValidationProcess -FilePath "ssh" -Arguments @(
        "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", $SshHost, $remoteCommand
    ) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 60
    $detailMatch = [regex]::Match($result.Stdout, 'STARPOINT_OK=(.+)')
    if ($result.ExitCode -eq 0 -and $detailMatch.Success) {
        $detail = $detailMatch.Groups[1].Value
        return New-IosValidationStage -Stage "REMOTE_TOOLCHAIN" -Status passed -ExitCode 0 `
            -Detail $detail -StartedAtUtc $StartedAtUtc
    }

    $errorMatch = [regex]::Match($result.Stdout, 'STARPOINT_ERROR_CODE=([A-Z_]+)')
    if ($errorMatch.Success) {
        $errorCode = $errorMatch.Groups[1].Value
    } elseif ($result.ExitCode -in @(0, 124, 255)) {
        $errorCode = "SSH_UNREACHABLE"
    } else {
        $errorCode = "XCODE_UNAVAILABLE"
    }
    $detail = (($result.Stderr, $result.Stdout) -join " ").Trim()
    New-IosValidationStage -Stage "REMOTE_TOOLCHAIN" -Status failed -ErrorCode $errorCode `
        -ExitCode $result.ExitCode -Detail $detail -StartedAtUtc $StartedAtUtc
}
# //// /验证远端 iOS 工具链并返回精确错误代码 ////

# //// 运行一组最多三个并发的轻量预检 [@x380kkm 2026-08-18] ////
function Invoke-IosValidationPreflight {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$Commit,
        [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._:-]*$')][string]$SshHost,
        [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9 .()_-]+$')][string]$SimulatorName,
        [Parameter(Mandatory)][string]$DeviceIpaPath,
        [ValidateRange(1, 3)][int]$ThrottleLimit
    )

    Import-Module Microsoft.PowerShell.ThreadJob -ErrorAction Stop
    $tasks = @(
        [pscustomobject]@{ Stage = "LOCAL_REPOSITORY"; Kind = "repository" },
        [pscustomobject]@{ Stage = "LOCAL_CONTRACTS"; Kind = "contracts" },
        [pscustomobject]@{ Stage = "REMOTE_TOOLCHAIN"; Kind = "remote" },
        [pscustomobject]@{ Stage = "DEVICE_ARTIFACT"; Kind = "device" }
    )
    $modulePath = Join-Path $PSScriptRoot "ios-validation.psm1"
    $jobs = foreach ($task in $tasks) {
        Start-ThreadJob -Name $task.Stage -ThrottleLimit $ThrottleLimit -ArgumentList @(
            $task, $RepositoryRoot, $Commit, $SshHost, $SimulatorName, $DeviceIpaPath, $modulePath
        ) -ScriptBlock {
            param($Task, $RepositoryRoot, $Commit, $SshHost, $SimulatorName, $DeviceIpaPath, $ModulePath)
            $ErrorActionPreference = "Stop"
            Import-Module $ModulePath -Force -DisableNameChecking
            $started = [datetime]::UtcNow
            try {
                switch ($Task.Kind) {
                    "repository" {
                        $resolved = (& git -C $RepositoryRoot rev-parse --verify "$Commit`^{commit}").Trim()
                        if ($LASTEXITCODE -ne 0) { throw "Git commit 不存在." }
                        $branch = (& git -C $RepositoryRoot branch --show-current).Trim()
                        if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($branch)) { throw "Git branch 不可用." }
                        $dirty = & git -C $RepositoryRoot status --porcelain
                        if ($LASTEXITCODE -ne 0 -or $dirty) { throw "工作树不干净." }
                        New-IosValidationStage -Stage $Task.Stage -Status passed -ExitCode 0 `
                            -Detail "Branch $branch at commit $resolved is clean." -StartedAtUtc $started
                    }
                    "contracts" {
                        Push-Location -LiteralPath $RepositoryRoot
                        try {
                            & npm run test:ios-personal-service-bootstrap | Out-Null
                            if ($LASTEXITCODE -ne 0) { throw "iOS bootstrap contract failed." }
                            & uv run --python 3.12 python scripts/protocol-lab/test_package_ios_personal_service.py | Out-Null
                            if ($LASTEXITCODE -ne 0) { throw "iOS package contract failed." }
                            & uv run --python 3.12 python -m unittest platforms.android.tests.test_mobile_personal_service_contract | Out-Null
                            if ($LASTEXITCODE -ne 0) { throw "mobile lifecycle contract failed." }
                        } finally {
                            Pop-Location
                        }
                        New-IosValidationStage -Stage $Task.Stage -Status passed -ExitCode 0 `
                            -Detail "iOS contracts passed." -StartedAtUtc $started
                    }
                    "remote" {
                        Test-IosRemoteToolchain -RepositoryRoot $RepositoryRoot -SshHost $SshHost `
                            -SimulatorName $SimulatorName -StartedAtUtc $started
                    }
                    "device" {
                        try {
                            $artifact = Test-IosDeviceArtifact -IpaPath $DeviceIpaPath
                            New-IosValidationStage -Stage $Task.Stage -Status passed -ExitCode 0 `
                                -Detail "Verified unsigned device IPA." -ArtifactPath $artifact.Path `
                                -Sha256 $artifact.Sha256 -StartedAtUtc $started
                        } catch {
                            New-IosValidationStage -Stage $Task.Stage -Status skipped -ErrorCode "DEVICE_BUILD_REQUIRED" `
                                -Detail $_.Exception.Message -StartedAtUtc $started
                        }
                    }
                }
            } catch {
                $errorCode = switch ($Task.Kind) {
                    "repository" { "LOCAL_REPOSITORY_INVALID" }
                    "contracts" { "LOCAL_CONTRACTS_FAILED" }
                    "remote" { "SSH_UNREACHABLE" }
                    default { "DEVICE_BUILD_FAILED" }
                }
                New-IosValidationStage -Stage $Task.Stage -Status failed -ErrorCode $errorCode `
                    -ExitCode 1 -Detail $_.Exception.Message -StartedAtUtc $started
            }
        }
    }
    try {
        Wait-Job -Job $jobs -Timeout 1200 | Out-Null
        $results = [Collections.Generic.List[object]]::new()
        foreach ($job in $jobs) {
            $received = @($job | Receive-Job -ErrorAction SilentlyContinue)
            foreach ($item in $received) {
                if ($null -ne $item -and $item.PSObject.Properties.Name -contains "stage") {
                    $results.Add($item)
                }
            }
            if ($received.Count -gt 0 -and @($results | Where-Object { $_.stage -eq $job.Name }).Count -gt 0) {
                continue
            }
            Stop-Job -Job $job -ErrorAction SilentlyContinue
            $errorCode = switch ($job.Name) {
                "LOCAL_REPOSITORY" { "LOCAL_REPOSITORY_INVALID" }
                "LOCAL_CONTRACTS" { "LOCAL_CONTRACTS_FAILED" }
                "REMOTE_TOOLCHAIN" { "SSH_UNREACHABLE" }
                default { "DEVICE_BUILD_FAILED" }
            }
            $results.Add((New-IosValidationStage -Stage $job.Name -Status failed -ErrorCode $errorCode `
                -ExitCode 124 -Detail "Preflight job timed out or failed before returning a stage."))
        }
        @($results)
    } finally {
        $jobs | Remove-Job -Force -ErrorAction SilentlyContinue
    }
}
# //// /运行一组最多三个并发的轻量预检 ////

# //// 清理一个远端验证运行的进程, Simulator 和临时目录 [@x380kkm 2026-08-18] ////
function Invoke-IosRemoteCleanup {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$SshHost,
        [Parameter(Mandatory)][string]$RemoteRoot,
        [Parameter(Mandatory)][string]$RunId
    )

    $cleanupScript = @'
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
cleanup_status=0
if [ -f '__REMOTE_ROOT__/source/platforms/ios/cleanup-ios-validation.sh' ]; then
    bash '__REMOTE_ROOT__/source/platforms/ios/cleanup-ios-validation.sh' '__REMOTE_ROOT__' '__RUN_ID__' || cleanup_status=$?
else
    rm -rf '__REMOTE_ROOT__' || cleanup_status=$?
fi
test ! -e '__REMOTE_ROOT__' || cleanup_status=1
exit "$cleanup_status"
'@
    $cleanupScript = $cleanupScript.Replace("`r`n", "`n")
    $cleanupScript = $cleanupScript.Replace("__REMOTE_ROOT__", $RemoteRoot).Replace("__RUN_ID__", $RunId)
    $cleanupCommand = @"
bash -s <<'STARPOINT_REMOTE_CLEANUP'
$cleanupScript
STARPOINT_REMOTE_CLEANUP
"@
    $cleanupCommand = $cleanupCommand.Replace("`r`n", "`n")
    Invoke-IosValidationProcess -FilePath "ssh" -Arguments @($SshHost, $cleanupCommand) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 90
}
# //// /清理一个远端验证运行的进程, Simulator 和临时目录 ////

# //// 终止并确认一个远端验证 runner 已退出 [@x380kkm 2026-08-18] ////
function Stop-IosRemoteValidationRunner {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$SshHost,
        [Parameter(Mandatory)][string]$RemoteRoot,
        [Parameter(Mandatory)][string]$RunId,
        [Parameter(Mandatory)][ValidateSet("device", "simulator")][string]$RunnerName
    )

    $stopCommand = "bash '$RemoteRoot/source/platforms/ios/stop-ios-validation-process.sh' '$RemoteRoot' '$RunId' '$RunnerName'"
    Invoke-IosValidationProcess -FilePath "ssh" -Arguments @($SshHost, $stopCommand) -WorkingDirectory $RepositoryRoot -TimeoutSeconds 90
}
# //// /终止并确认一个远端验证 runner 已退出 ////
