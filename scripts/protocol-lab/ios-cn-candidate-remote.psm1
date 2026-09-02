$ErrorActionPreference = 'Stop'
# audience: internal
# # ios-cn-candidate-remote
# 该模块生成带本轮路径标记和有界进程清理的 iOS CN 远端脚本.

Set-StrictMode -Version Latest

$remoteProcessModule = Join-Path $PSScriptRoot 'ios-cn-remote-process.sh'
$script:RemoteProcessFunctions = Get-Content -LiteralPath $remoteProcessModule -Raw -Encoding UTF8
$script:RemoteProcessFunctions = $script:RemoteProcessFunctions.Replace("`r`n", "`n")

# //// 生成远端 iPhone Framework 构建脚本 [@x380kkm 2026-08-20] ////
function New-IosCandidateRemoteBuildScript {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][ValidatePattern('^/tmp/starpoint-ios-cn-[0-9a-f]{32}$')][string]$RemoteRoot,
        [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9._-]+$')][string]$FrameworkFileName
    )

    $script = @'
set -euo pipefail
__REMOTE_PROCESS_FUNCTIONS__
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
export CARGO_TARGET_DIR='__REMOTE_ROOT__/cargo-device'
remote_root="$1"
cd '__REMOTE_ROOT__/source'
runner_pid=$$
runner_group="$(ps -o pgid= -p "$runner_pid" | tr -d '[:space:]')"
runner_started_at="$(read_process_start_time "$runner_pid")"
if [ -z "$runner_started_at" ]; then
    printf '%s\n' 'Build runner start time is unavailable.' >&2
    exit 1
fi
printf '%s\n' "$runner_pid" > '__REMOTE_ROOT__/build-runner.pid'
printf '%s\n' "$runner_started_at" > '__REMOTE_ROOT__/build-runner.start'
build_pid=''
build_group=''
stop_build_group() {
    if [ -n "$build_group" ]; then
        terminate_process_group "$build_group" || true
    elif [ -n "$build_pid" ] && process_contains_root "$build_pid" "$remote_root"; then
        terminate_process "$build_pid" || true
    fi
}
trap stop_build_group EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
set -m
(
    STARPOINT_IOS_SDK=iphoneos bash platforms/ios/build-framework.sh '__REMOTE_ROOT__/framework'
    framework_binary='__REMOTE_ROOT__/framework/PersonalServiceBootstrap.framework/PersonalServiceBootstrap'
    xcrun lipo "$framework_binary" -verify_arch arm64
    if xcrun otool -l "$framework_binary" | grep -q LC_CODE_SIGNATURE; then
        printf '%s\n' 'Framework must remain unsigned.' >&2
        exit 1
    fi
    ditto -c -k --keepParent '__REMOTE_ROOT__/framework/PersonalServiceBootstrap.framework' '__REMOTE_ROOT__/__FRAMEWORK_FILE_NAME__'
    shasum -a 256 '__REMOTE_ROOT__/__FRAMEWORK_FILE_NAME__' > '__REMOTE_ROOT__/__FRAMEWORK_FILE_NAME__.sha256'
) &
build_pid=$!
printf '%s\n' "$build_pid" > '__REMOTE_ROOT__/build-child.pid'
build_started_at="$(read_process_start_time "$build_pid")"
if [ -z "$build_started_at" ]; then
    printf '%s\n' 'Build process start time is unavailable.' >&2
    exit 1
fi
printf '%s\n' "$build_started_at" > '__REMOTE_ROOT__/build-child.start'
build_group="$(ps -o pgid= -p "$build_pid" | tr -d '[:space:]')"
case "$build_group" in
    ''|*[!0-9]*|0|1) printf '%s\n' 'Build process group is invalid.' >&2; exit 1 ;;
esac
if [ "$build_group" = "$runner_group" ]; then
    printf '%s\n' 'Build process group is not isolated.' >&2
    exit 1
fi
if ! process_group_contains_root "$build_group" "$remote_root"; then
    printf '%s\n' 'Build process group does not contain the run marker.' >&2
    exit 1
fi
printf '%s\n' "$build_group" > '__REMOTE_ROOT__/build-group.pid'
printf '%s\n' "$build_started_at" > '__REMOTE_ROOT__/build-group.start'
set +e
wait "$build_pid"
build_status=$?
set -e
set +m
build_pid=''
build_group=''
trap - EXIT HUP INT TERM
exit "$build_status"
'@
    $script = $script.Replace('__REMOTE_PROCESS_FUNCTIONS__', $script:RemoteProcessFunctions)
    $script = $script.Replace('__REMOTE_ROOT__', $RemoteRoot)
    $script = $script.Replace('__FRAMEWORK_FILE_NAME__', $FrameworkFileName)
    $script.Replace("`r`n", "`n")
}
# //// /生成远端 iPhone Framework 构建脚本 ////

# //// 生成远端候选构建清理命令 [@x380kkm 2026-08-20] ////
function New-IosCandidateRemoteCleanupCommand {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][ValidatePattern('^/tmp/starpoint-ios-cn-[0-9a-f]{32}$')][string]$RemoteRoot
    )

    $command = @'
bash -s -- '__REMOTE_ROOT__' <<'STARPOINT_REMOTE_CLEANUP'
set -euo pipefail
__REMOTE_PROCESS_FUNCTIONS__
remote_root="$1"
cleanup_failed=0

if [ -f '__REMOTE_ROOT__/build-group.pid' ]; then
    build_group="$(cat '__REMOTE_ROOT__/build-group.pid' 2>/dev/null || true)"
    recorded_group_start="$(cat '__REMOTE_ROOT__/build-group.start' 2>/dev/null || true)"
    case "$build_group" in
        ''|*[!0-9]*|0|1) build_group='' ;;
    esac
    if [ -n "$build_group" ] && process_group_exists "$build_group"; then
        current_group_start="$(read_process_start_time "$build_group")"
        group_belongs_to_run=false
        if process_group_contains_root "$build_group" "$remote_root"; then
            if [ -z "$current_group_start" ] || [ "$current_group_start" = "$recorded_group_start" ]; then
                group_belongs_to_run=true
            fi
        fi
        if [ "$group_belongs_to_run" = true ]; then
            terminate_process_group "$build_group" || true
        fi
    fi
    if [ -n "$build_group" ] && process_group_contains_root "$build_group" "$remote_root"; then
        printf '%s\n' 'Build process group is still running.' >&2
        cleanup_failed=1
    fi
fi

if [ -f '__REMOTE_ROOT__/build-child.pid' ]; then
    build_pid="$(cat '__REMOTE_ROOT__/build-child.pid' 2>/dev/null || true)"
    recorded_build_start="$(cat '__REMOTE_ROOT__/build-child.start' 2>/dev/null || true)"
    case "$build_pid" in
        ''|*[!0-9]*|0|1) build_pid='' ;;
    esac
    if [ -n "$build_pid" ]; then
        current_build_start="$(read_process_start_time "$build_pid")"
        if [ -n "$current_build_start" ] &&
            [ "$current_build_start" = "$recorded_build_start" ] &&
            process_contains_root "$build_pid" "$remote_root"; then
            derived_group="$(ps -o pgid= -p "$build_pid" 2>/dev/null | tr -d '[:space:]' || true)"
            case "$derived_group" in
                ''|*[!0-9]*|0|1) derived_group='' ;;
            esac
            if [ -n "$derived_group" ] && process_group_contains_root "$derived_group" "$remote_root"; then
                terminate_process_group "$derived_group" || true
            else
                terminate_process "$build_pid" || true
            fi
        fi
        current_build_start="$(read_process_start_time "$build_pid")"
        if [ -n "$current_build_start" ] &&
            [ "$current_build_start" = "$recorded_build_start" ] &&
            process_contains_root "$build_pid" "$remote_root"; then
            printf '%s\n' 'Build child is still running.' >&2
            cleanup_failed=1
        fi
    fi
fi

if [ -f '__REMOTE_ROOT__/build-runner.pid' ]; then
    runner_pid="$(cat '__REMOTE_ROOT__/build-runner.pid' 2>/dev/null || true)"
    recorded_runner_start="$(cat '__REMOTE_ROOT__/build-runner.start' 2>/dev/null || true)"
    case "$runner_pid" in
        ''|*[!0-9]*|0|1) runner_pid='' ;;
    esac
    if [ -n "$runner_pid" ] && [ "$runner_pid" != "$$" ]; then
        current_runner_start="$(read_process_start_time "$runner_pid")"
        if [ -n "$current_runner_start" ] &&
            [ "$current_runner_start" = "$recorded_runner_start" ] &&
            process_contains_root "$runner_pid" "$remote_root"; then
            if ! wait_for_process_exit "$runner_pid" 5; then
                terminate_process "$runner_pid" || true
            fi
        fi
        current_runner_start="$(read_process_start_time "$runner_pid")"
        if [ -n "$current_runner_start" ] &&
            [ "$current_runner_start" = "$recorded_runner_start" ] &&
            process_contains_root "$runner_pid" "$remote_root"; then
            printf '%s\n' 'Build runner is still running.' >&2
            cleanup_failed=1
        fi
    fi
fi

if [ "$cleanup_failed" -ne 0 ]; then
    exit 1
fi
rm -rf -- '__REMOTE_ROOT__'
test ! -e '__REMOTE_ROOT__'
STARPOINT_REMOTE_CLEANUP
'@
    $command = $command.Replace('__REMOTE_PROCESS_FUNCTIONS__', $script:RemoteProcessFunctions)
    $command = $command.Replace('__REMOTE_ROOT__', $RemoteRoot)
    $command.Replace("`r`n", "`n")
}
# //// /生成远端候选构建清理命令 ////

Export-ModuleMember -Function New-IosCandidateRemoteBuildScript, New-IosCandidateRemoteCleanupCommand
