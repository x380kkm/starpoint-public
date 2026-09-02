# audience: external
# # run-ios-simulator-diagnostic
# 该脚本构建并运行 Simulator 诊断 App, 重放个人服务协议, 查询容器请求记录, 并只关闭本次启动的设备.

set -Eeuo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
OUTPUT_ROOT="${1:-$REPOSITORY_ROOT/build/ios-simulator-validation}"
SIMULATOR_NAME="${2:-iPhone 17 Pro}"
REPORT_PATH="${3:-$OUTPUT_ROOT/simulator-diagnostic.json}"
SCENARIO_REPORT_PATH="${STARPOINT_IOS_SCENARIO_REPORT_PATH:-$OUTPUT_ROOT/ios-cn-game-scenario.json}"
OBSERVATIONS_REPORT_PATH="${STARPOINT_IOS_OBSERVATIONS_REPORT_PATH:-$OUTPUT_ROOT/http-observations.json}"
BUNDLE_ID="dev.starpoint.PersonalServiceDiagnostic"
SIMULATOR_RUNTIME_IDENTIFIER="${STARPOINT_IOS_SIMULATOR_RUNTIME_IDENTIFIER:-com.apple.CoreSimulator.SimRuntime.iOS-26-5}"
DIAGNOSTIC_TIMEOUT_SECONDS="${STARPOINT_IOS_DIAGNOSTIC_TIMEOUT_SECONDS:-20}"
BUILD_ROOT="$OUTPUT_ROOT/build"
APP_PATH="$BUILD_ROOT/PersonalServiceDiagnostic.app"
STAGE_PATH="$OUTPUT_ROOT/stages.tsv"
SIMULATOR_OWNER_PATH="${STARPOINT_IOS_SIMULATOR_OWNER_PATH:-$OUTPUT_ROOT/simulator-udid.txt}"
MANAGEMENT_SCREENSHOT_PATH="${STARPOINT_IOS_MANAGEMENT_SCREENSHOT_PATH:-}"

SIMULATOR_UDID=""
BOOTED_BY_SCRIPT=0
APP_PID=""
FIRST_FAILURE=""
FIRST_ERROR_CODE=""
LAST_SUCCESS=""
FIRST_RUN_ID=""
FIRST_GENERATION_BEFORE=""
FIRST_GENERATION_AFTER=""
BACKGROUND_GENERATION_AFTER=""
FOREGROUND_GENERATION_AFTER=""
SECOND_RUN_ID=""
SECOND_GENERATION_BEFORE=""
SECOND_GENERATION_AFTER=""
CLEANUP_FAILED=0
CURRENT_STAGE="simulator_build"
CURRENT_ERROR_CODE="SIMULATOR_BUILD_FAILED"
CURRENT_DEPENDENCIES="ARCHIVE,REMOTE_TOOLCHAIN"
CURRENT_STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

mkdir -p "$OUTPUT_ROOT"
: > "$STAGE_PATH"

# //// 加载诊断状态和报告函数 [@x380kkm 2026-08-18] ////
. "$SCRIPT_DIR/ios-simulator-diagnostic-lib.sh"
# //// /加载诊断状态和报告函数 ////

# //// 等待同一诊断运行进入指定生命周期阶段 [@x380kkm 2026-08-18] ////
wait_for_lifecycle_stage() {
    local data_path="$1"
    local run_id="$2"
    local expected_stage="$3"
    local preferences_path="$data_path/Library/Preferences/$BUNDLE_ID.plist"
    python3 - "$preferences_path" "$run_id" "$expected_stage" "$DIAGNOSTIC_TIMEOUT_SECONDS" <<'PY'
import json
import subprocess
import sys
import time
from pathlib import Path

preferences_path = Path(sys.argv[1])
run_id = sys.argv[2]
expected_stage = sys.argv[3]
deadline = time.monotonic() + int(sys.argv[4])
last_result = None
while time.monotonic() < deadline:
    try:
        converted = subprocess.run(
            ["plutil", "-convert", "json", "-o", "-", str(preferences_path)],
            check=True,
            capture_output=True,
        )
        values = json.loads(converted.stdout)
    except (
        FileNotFoundError,
        json.JSONDecodeError,
        OSError,
        subprocess.CalledProcessError,
    ):
        time.sleep(0.25)
        continue
    if values.get("run_id") == run_id and values.get("stage") == expected_stage:
        last_result = {
            "run_id": run_id,
            "state": values.get("state"),
            "stage": expected_stage,
            "error_code": values.get("error_code"),
            "generation_before": values.get("generation_before"),
            "generation_after": values.get("generation_after"),
        }
        if last_result["state"] in {"passed", "failed"}:
            print(json.dumps(last_result, ensure_ascii=True, separators=(",", ":")))
            raise SystemExit(0)
    time.sleep(0.25)
if last_result is not None:
    print(json.dumps(last_result, ensure_ascii=True, separators=(",", ":")))
raise SystemExit(2)
PY
}
# //// /等待同一诊断运行进入指定生命周期阶段 ////

# //// 等待管理页面完成数据渲染 [@x380kkm 2026-08-20] ////
wait_for_management_page() {
    local preferences_path="$1/Library/Preferences/$BUNDLE_ID.plist"
    python3 - "$preferences_path" "$DIAGNOSTIC_TIMEOUT_SECONDS" <<'PY'
import plistlib
import sys
import time
from pathlib import Path

preferences_path = Path(sys.argv[1])
deadline = time.monotonic() + int(sys.argv[2])
while time.monotonic() < deadline:
    try:
        with preferences_path.open("rb") as preferences:
            state = plistlib.load(preferences).get("management_page_state")
    except (FileNotFoundError, OSError, plistlib.InvalidFileException):
        state = None
    if state == "loaded":
        raise SystemExit(0)
    if state == "failed":
        raise SystemExit(2)
    time.sleep(0.25)
raise SystemExit(3)
PY
}
# //// /等待管理页面完成数据渲染 ////

# //// 注册诊断和清理 trap [@x380kkm 2026-08-18] ////
trap on_exit EXIT
trap 'on_error $? $LINENO' ERR
trap 'exit 130' INT TERM
# //// /注册诊断和清理 trap ////

# //// 构建并验证 Simulator App [@x380kkm 2026-08-18] ////
build_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="simulator_build"
CURRENT_ERROR_CODE="SIMULATOR_BUILD_FAILED"
CURRENT_DEPENDENCIES="ARCHIVE,REMOTE_TOOLCHAIN"
CURRENT_STARTED_AT="$build_started"
if build_output="$(bash "$SCRIPT_DIR/build-simulator-harness.sh" "$BUILD_ROOT" 2>&1)"; then
    build_exit=0
else
    build_exit=$?
fi
if [ "$build_exit" -ne 0 ]; then
    fail_stage "simulator_build" "SIMULATOR_BUILD_FAILED" "$build_output" "$build_started" \
        "$build_exit" "$CURRENT_DEPENDENCIES"
fi
main_binary="$APP_PATH/PersonalServiceDiagnostic"
framework_binary="$APP_PATH/Frameworks/PersonalServiceBootstrap.framework/PersonalServiceBootstrap"
if [ ! -d "$APP_PATH" ] || \
   ! codesign --verify --deep --strict "$APP_PATH" >/dev/null 2>&1 || \
   ! has_adhoc_signature "$APP_PATH" || \
   ! has_adhoc_signature "$(dirname "$framework_binary")" || \
   ! xcrun lipo "$main_binary" -verify_arch arm64 >/dev/null 2>&1 || \
   ! xcrun lipo "$framework_binary" -verify_arch arm64 >/dev/null 2>&1; then
    fail_stage "simulator_build" "SIMULATOR_BUILD_FAILED" \
        "Simulator App 或 Framework 的架构和 ad-hoc 签名无效." "$build_started" "1" "$CURRENT_DEPENDENCIES"
fi
main_sha256="$(shasum -a 256 "$main_binary" | awk '{ print $1 }')"
framework_sha256="$(shasum -a 256 "$framework_binary" | awk '{ print $1 }')"
record_stage "simulator_build" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "Framework SHA-256: $framework_sha256." "$main_binary" "$main_sha256" "$build_started"
# //// /构建并验证 Simulator App ////

# //// 选择并启动唯一 Simulator [@x380kkm 2026-08-18] ////
boot_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="simulator_boot"
CURRENT_ERROR_CODE="SIMULATOR_BOOT_FAILED"
CURRENT_DEPENDENCIES="simulator_build"
CURRENT_STARTED_AT="$boot_started"
device_selection="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
name = sys.argv[1]
runtime_id = sys.argv[2]
all_devices = json.load(sys.stdin)["devices"]
devices = [device for runtime in all_devices.values() for device in runtime]
booted = [device for device in devices if device.get("state") == "Booted"]
matches = [
    device
    for device in all_devices.get(runtime_id, [])
    if device.get("name") == name and device.get("isAvailable", True)
]
if booted:
    print("BOOTED")
    raise SystemExit(3)
if not matches:
    raise SystemExit(4)
print(matches[0]["udid"])
' "$SIMULATOR_NAME" "$SIMULATOR_RUNTIME_IDENTIFIER" 2>/dev/null || true)"
if [ -z "$device_selection" ] || [ "$device_selection" = "BOOTED" ]; then
    fail_stage "simulator_boot" "SIMULATOR_BOOT_FAILED" \
        "没有可独占使用的 iOS 26.5 目标 Simulator." "$boot_started" "1" "$CURRENT_DEPENDENCIES"
fi
SIMULATOR_UDID="$device_selection"
printf '%s\n' "$SIMULATOR_UDID" > "$SIMULATOR_OWNER_PATH"
if ! xcrun simctl boot "$SIMULATOR_UDID" >/dev/null 2>&1; then
    fail_stage "simulator_boot" "SIMULATOR_BOOT_FAILED" "Simulator 启动失败." \
        "$boot_started" "1" "$CURRENT_DEPENDENCIES"
fi
BOOTED_BY_SCRIPT=1
if ! xcrun simctl bootstatus "$SIMULATOR_UDID" -b >/dev/null 2>&1; then
    fail_stage "simulator_boot" "SIMULATOR_BOOT_FAILED" "Simulator 未完成启动." \
        "$boot_started" "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "simulator_boot" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "$SIMULATOR_UDID" "" "" "$boot_started"
# //// /选择并启动唯一 Simulator ////

# //// 安装一个没有历史数据的诊断 App [@x380kkm 2026-08-18] ////
install_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="app_install"
CURRENT_ERROR_CODE="APP_INSTALL_FAILED"
CURRENT_DEPENDENCIES="simulator_build,simulator_boot"
CURRENT_STARTED_AT="$install_started"
if xcrun simctl get_app_container "$SIMULATOR_UDID" "$BUNDLE_ID" data >/dev/null 2>&1; then
    existing_container_exit=0
else
    existing_container_exit=$?
fi
if [ "$existing_container_exit" -eq 0 ]; then
    if uninstall_output="$(xcrun simctl uninstall "$SIMULATOR_UDID" "$BUNDLE_ID" 2>&1)"; then
        uninstall_exit=0
    else
        uninstall_exit=$?
    fi
    if [ "$uninstall_exit" -ne 0 ]; then
        fail_stage "app_install" "APP_INSTALL_FAILED" "$uninstall_output" "$install_started" \
            "$uninstall_exit" "$CURRENT_DEPENDENCIES"
    fi
    if xcrun simctl get_app_container "$SIMULATOR_UDID" "$BUNDLE_ID" data >/dev/null 2>&1; then
        stale_container_exit=0
    else
        stale_container_exit=$?
    fi
    if [ "$stale_container_exit" -eq 0 ]; then
        fail_stage "app_install" "APP_INSTALL_FAILED" \
            "Simulator 仍保留旧诊断 App 数据容器." "$install_started" "1" "$CURRENT_DEPENDENCIES"
    fi
fi
if install_output="$(xcrun simctl install "$SIMULATOR_UDID" "$APP_PATH" 2>&1)"; then
    install_exit=0
else
    install_exit=$?
fi
if [ "$install_exit" -ne 0 ]; then
    fail_stage "app_install" "APP_INSTALL_FAILED" "$install_output" "$install_started" \
        "$install_exit" "$CURRENT_DEPENDENCIES"
fi
record_stage "app_install" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "App installed with a fresh data container." "" "" "$install_started"
# //// /安装一个没有历史数据的诊断 App ////

# //// 启动 App 并验证内部自检 [@x380kkm 2026-08-18] ////
launch_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="app_launch"
CURRENT_ERROR_CODE="APP_INSTALL_FAILED"
CURRENT_DEPENDENCIES="app_install"
CURRENT_STARTED_AT="$launch_started"
if [ -n "$MANAGEMENT_SCREENSHOT_PATH" ]; then
    if launch_output="$(SIMCTL_CHILD_STARPOINT_OPEN_MANAGEMENT=1 \
        xcrun simctl launch --terminate-running-process "$SIMULATOR_UDID" "$BUNDLE_ID" 2>&1)"; then
        launch_exit=0
    else
        launch_exit=$?
    fi
elif launch_output="$(xcrun simctl launch --terminate-running-process "$SIMULATOR_UDID" "$BUNDLE_ID" 2>&1)"; then
    launch_exit=0
else
    launch_exit=$?
fi
if [ "$launch_exit" -ne 0 ]; then
    fail_stage "app_launch" "APP_INSTALL_FAILED" "$launch_output" "$launch_started" \
        "$launch_exit" "$CURRENT_DEPENDENCIES"
fi
APP_PID="$(printf '%s' "$launch_output" | sed -nE 's/.*: ([0-9]+)$/\1/p' | tail -n 1)"
if [ -z "$APP_PID" ]; then
    fail_stage "app_launch" "APP_INSTALL_FAILED" "simctl 未返回 App PID." \
        "$launch_started" "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "app_launch" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "App PID $APP_PID." "" "" "$launch_started"

diagnostic_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="internal_diagnostic"
CURRENT_ERROR_CODE="SERVICE_START_TIMEOUT"
CURRENT_DEPENDENCIES="app_launch"
CURRENT_STARTED_AT="$diagnostic_started"
if data_path="$(xcrun simctl get_app_container "$SIMULATOR_UDID" "$BUNDLE_ID" data 2>/dev/null)"; then
    container_exit=0
else
    container_exit=$?
fi
if [ "$container_exit" -ne 0 ] || [ -z "$data_path" ]; then
    fail_stage "internal_diagnostic" "SERVICE_START_TIMEOUT" \
        "无法读取诊断 App 数据容器." "$diagnostic_started" "$container_exit" "$CURRENT_DEPENDENCIES"
fi
if first_diagnostic="$(wait_for_diagnostic "$data_path" "")"; then
    diagnostic_exit=0
else
    diagnostic_exit=$?
fi
if [ "$diagnostic_exit" -ne 0 ]; then
    stalled_stage="service_start"
    if [ -n "$first_diagnostic" ]; then
        stalled_stage="$(json_field "$first_diagnostic" "stage")"
    fi
    stalled_error="$(diagnostic_error_code "$stalled_stage")"
    fail_stage "internal_diagnostic" "$stalled_error" \
        "诊断状态停留在 $stalled_stage." "$diagnostic_started" "$diagnostic_exit" "$CURRENT_DEPENDENCIES"
fi
FIRST_RUN_ID="$(json_field "$first_diagnostic" "run_id")"
first_state="$(json_field "$first_diagnostic" "state")"
first_stage="$(json_field "$first_diagnostic" "stage")"
first_error="$(json_field "$first_diagnostic" "error_code")"
FIRST_GENERATION_BEFORE="$(json_field "$first_diagnostic" "generation_before")"
FIRST_GENERATION_AFTER="$(json_field "$first_diagnostic" "generation_after")"
if [ "$first_state" != "passed" ]; then
    fail_stage "internal_diagnostic" "${first_error:-$(diagnostic_error_code "$first_stage")}" \
        "内部自检失败于 $first_stage." "$diagnostic_started" "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "internal_diagnostic" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "Internal diagnostic passed." "" "" "$diagnostic_started"
# //// /启动 App 并验证内部自检 ////

# //// 从 macOS 宿主验证 App loopback [@x380kkm 2026-08-18] ////
loopback_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="loopback"
CURRENT_ERROR_CODE="LOOPBACK_UNREACHABLE"
CURRENT_DEPENDENCIES="internal_diagnostic"
CURRENT_STARTED_AT="$loopback_started"
if ! loopback_port="$(find_healthy_loopback_port "$APP_PID")"; then
    fail_stage "loopback" "LOOPBACK_UNREACHABLE" "宿主无法访问 App loopback 健康接口." \
        "$loopback_started" "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "loopback" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "Health endpoint responded on port $loopback_port." "" "" "$loopback_started"
if [ -n "$MANAGEMENT_SCREENSHOT_PATH" ]; then
    if ! wait_for_management_page "$data_path"; then
        fail_stage "management_screenshot" "MANAGEMENT_PAGE_CAPTURE_FAILED" \
            "本地管理页面没有在诊断时限内完成数据渲染." "$loopback_started" "1" "loopback"
    fi
    mkdir -p "$(dirname "$MANAGEMENT_SCREENSHOT_PATH")"
    if ! xcrun simctl io "$SIMULATOR_UDID" screenshot "$MANAGEMENT_SCREENSHOT_PATH" >/dev/null; then
        fail_stage "management_screenshot" "MANAGEMENT_PAGE_CAPTURE_FAILED" \
            "无法截取本地管理页面." "$loopback_started" "1" "loopback"
    fi
fi
# //// /从 macOS 宿主验证 App loopback ////

# //// 将 App 置于后台并确认真实 checkpoint [@x380kkm 2026-08-18] ////
background_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="background_checkpoint"
CURRENT_ERROR_CODE="BACKGROUND_CHECKPOINT_FAILED"
CURRENT_DEPENDENCIES="loopback"
CURRENT_STARTED_AT="$background_started"
if ! settings_output="$(xcrun simctl launch "$SIMULATOR_UDID" "com.apple.Preferences" 2>&1)"; then
    fail_stage "background_checkpoint" "BACKGROUND_CHECKPOINT_FAILED" \
        "无法通过 Simulator Home 操作将诊断 App 置于后台: $settings_output." \
        "$background_started" "1" "$CURRENT_DEPENDENCIES"
fi
if background_diagnostic="$(wait_for_lifecycle_stage "$data_path" "$FIRST_RUN_ID" "background_checkpoint")"; then
    background_diagnostic_exit=0
else
    background_diagnostic_exit=$?
fi
if [ "$background_diagnostic_exit" -ne 0 ]; then
    background_error="$(json_field "${background_diagnostic:-{}}" "error_code")"
    fail_stage "background_checkpoint" "${background_error:-BACKGROUND_CHECKPOINT_FAILED}" \
        "后台 checkpoint 没有在诊断时限内成功完成." "$background_started" \
        "$background_diagnostic_exit" "$CURRENT_DEPENDENCIES"
fi
background_state="$(json_field "$background_diagnostic" "state")"
background_generation_before="$(json_field "$background_diagnostic" "generation_before")"
BACKGROUND_GENERATION_AFTER="$(json_field "$background_diagnostic" "generation_after")"
if [ "$background_state" != "passed" ]; then
    background_error="$(json_field "$background_diagnostic" "error_code")"
    fail_stage "background_checkpoint" "${background_error:-BACKGROUND_CHECKPOINT_FAILED}" \
        "后台 checkpoint 阶段返回失败状态." "$background_started" "1" "$CURRENT_DEPENDENCIES"
fi
if ! python3 - "$FIRST_GENERATION_AFTER" "$background_generation_before" \
    "$BACKGROUND_GENERATION_AFTER" <<'PY'
import sys

baseline, before, after = map(int, sys.argv[1:])
assert before >= baseline
assert after >= before
PY
then
    fail_stage "background_checkpoint" "BACKGROUND_CHECKPOINT_FAILED" \
        "后台 checkpoint 后 generation 回退." "$background_started" "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "background_checkpoint" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "Background checkpoint completed without generation rollback." "" "" "$background_started"
# //// /将 App 置于后台并确认真实 checkpoint ////

# //// 恢复 App 前台并确认服务恢复 [@x380kkm 2026-08-18] ////
foreground_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="foreground_resume"
CURRENT_ERROR_CODE="FOREGROUND_RESUME_FAILED"
CURRENT_DEPENDENCIES="background_checkpoint"
CURRENT_STARTED_AT="$foreground_started"
if foreground_output="$(xcrun simctl launch "$SIMULATOR_UDID" "$BUNDLE_ID" 2>&1)"; then
    foreground_launch_exit=0
else
    foreground_launch_exit=$?
fi
if [ "$foreground_launch_exit" -ne 0 ]; then
    fail_stage "foreground_resume" "FOREGROUND_RESUME_FAILED" "$foreground_output" \
        "$foreground_started" "$foreground_launch_exit" "$CURRENT_DEPENDENCIES"
fi
foreground_pid="$(printf '%s' "$foreground_output" | sed -nE 's/.*: ([0-9]+)$/\1/p' | tail -n 1)"
if [ -z "$foreground_pid" ] || [ "$foreground_pid" != "$APP_PID" ]; then
    fail_stage "foreground_resume" "FOREGROUND_RESUME_FAILED" \
        "前台恢复没有保持原诊断 App 进程." "$foreground_started" "1" "$CURRENT_DEPENDENCIES"
fi
APP_PID="$foreground_pid"
if foreground_diagnostic="$(wait_for_lifecycle_stage "$data_path" "$FIRST_RUN_ID" "foreground_resume")"; then
    foreground_diagnostic_exit=0
else
    foreground_diagnostic_exit=$?
fi
if [ "$foreground_diagnostic_exit" -ne 0 ]; then
    foreground_error="$(json_field "${foreground_diagnostic:-{}}" "error_code")"
    fail_stage "foreground_resume" "${foreground_error:-FOREGROUND_RESUME_FAILED}" \
        "前台恢复没有在诊断时限内完成." "$foreground_started" \
        "$foreground_diagnostic_exit" "$CURRENT_DEPENDENCIES"
fi
foreground_state="$(json_field "$foreground_diagnostic" "state")"
foreground_generation_before="$(json_field "$foreground_diagnostic" "generation_before")"
FOREGROUND_GENERATION_AFTER="$(json_field "$foreground_diagnostic" "generation_after")"
if [ "$foreground_state" != "passed" ]; then
    foreground_error="$(json_field "$foreground_diagnostic" "error_code")"
    fail_stage "foreground_resume" "${foreground_error:-FOREGROUND_RESUME_FAILED}" \
        "前台恢复阶段返回失败状态." "$foreground_started" "1" "$CURRENT_DEPENDENCIES"
fi
if ! python3 - "$BACKGROUND_GENERATION_AFTER" "$foreground_generation_before" \
    "$FOREGROUND_GENERATION_AFTER" <<'PY'
import sys

baseline, before, after = map(int, sys.argv[1:])
assert before >= baseline
assert after >= before
PY
then
    fail_stage "foreground_resume" "PERSISTENCE_REGRESSION" \
        "前台恢复后 generation 回退." "$foreground_started" "1" "$CURRENT_DEPENDENCIES"
fi
if ! loopback_port="$(find_healthy_loopback_port "$APP_PID")"; then
    fail_stage "foreground_resume" "FOREGROUND_RESUME_FAILED" \
        "前台恢复后宿主无法访问 App loopback 健康接口." "$foreground_started" \
        "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "foreground_resume" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "Foreground resume restored the service on port $loopback_port." "" "" "$foreground_started"
# //// /恢复 App 前台并确认服务恢复 ////

# //// 重放完整协议链并读取容器请求记录 [@x380kkm 2026-08-21] ////
protocol_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="protocol_chain"
CURRENT_ERROR_CODE="PROTOCOL_CHAIN_FAILED"
CURRENT_DEPENDENCIES="foreground_resume"
CURRENT_STARTED_AT="$protocol_started"
if protocol_output="$(
    trap - ERR
    python3 "$REPOSITORY_ROOT/scripts/protocol-lab/run-ios-cn-game-scenarios.py" \
        --base-url "http://127.0.0.1:$loopback_port" \
        --output "$SCENARIO_REPORT_PATH" \
        --timeout-ms "$((DIAGNOSTIC_TIMEOUT_SECONDS * 1000))" 2>&1
)"; then
    protocol_exit=0
else
    protocol_exit=$?
fi
if [ -f "$SCENARIO_REPORT_PATH" ]; then
    scenario_sha256="$(shasum -a 256 "$SCENARIO_REPORT_PATH" | awk '{ print $1 }')"
else
    scenario_sha256=""
fi
protocol_error_code="PROTOCOL_CHAIN_FAILED"
if [ "$protocol_exit" -ne 0 ] && [ -f "$SCENARIO_REPORT_PATH" ]; then
    if scenario_error_code="$(
        trap - ERR
        python3 -c 'import json, sys; failure=json.load(open(sys.argv[1], encoding="utf-8")).get("first_failure") or {}; print(failure.get("error_code") or "")' \
            "$SCENARIO_REPORT_PATH"
    )"; then
        if [ -n "$scenario_error_code" ]; then
            protocol_error_code="$scenario_error_code"
        fi
    fi
fi
if [ "$protocol_exit" -eq 0 ]; then
    record_stage "protocol_chain" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
        "Complete CN game scenario passed on loopback." "$SCENARIO_REPORT_PATH" \
        "$scenario_sha256" "$protocol_started"
else
    record_stage "protocol_chain" "failed" "$protocol_error_code" "$protocol_exit" \
        "$CURRENT_DEPENDENCIES" "$protocol_output" "$SCENARIO_REPORT_PATH" \
        "$scenario_sha256" "$protocol_started"
fi

observations_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="http_observations"
CURRENT_ERROR_CODE="HTTP_OBSERVATIONS_FAILED"
CURRENT_DEPENDENCIES="foreground_resume"
CURRENT_STARTED_AT="$observations_started"
if observations_output="$(
    trap - ERR
    python3 "$REPOSITORY_ROOT/scripts/protocol-lab/export-ios-simulator-http-observations.py" \
        --data-container "$data_path" \
        --scenario-report "$SCENARIO_REPORT_PATH" \
        --output "$OBSERVATIONS_REPORT_PATH" 2>&1
)"; then
    observations_exit=0
else
    observations_exit=$?
fi
if [ -f "$OBSERVATIONS_REPORT_PATH" ]; then
    observations_sha256="$(shasum -a 256 "$OBSERVATIONS_REPORT_PATH" | awk '{ print $1 }')"
else
    observations_sha256=""
fi
if [ "$observations_exit" -eq 0 ]; then
    record_stage "http_observations" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
        "Simulator data container contains no core HTTP failures for the scenario." \
        "$OBSERVATIONS_REPORT_PATH" "$observations_sha256" "$observations_started"
else
    record_stage "http_observations" "failed" "HTTP_OBSERVATIONS_FAILED" \
        "$observations_exit" "$CURRENT_DEPENDENCIES" "$observations_output" \
        "$OBSERVATIONS_REPORT_PATH" "$observations_sha256" "$observations_started"
fi
if [ "$protocol_exit" -ne 0 ] || [ "$observations_exit" -ne 0 ]; then
    exit 1
fi
# //// /重放完整协议链并读取容器请求记录 ////

# //// 重新启动 App 并验证 generation 不回退 [@x380kkm 2026-08-18] ////
relaunch_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="relaunch"
CURRENT_ERROR_CODE="PERSISTENCE_REGRESSION"
CURRENT_DEPENDENCIES="protocol_chain,http_observations"
CURRENT_STARTED_AT="$relaunch_started"
xcrun simctl terminate "$SIMULATOR_UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true
if relaunch_output="$(xcrun simctl launch "$SIMULATOR_UDID" "$BUNDLE_ID" 2>&1)"; then
    relaunch_exit=0
else
    relaunch_exit=$?
fi
if [ "$relaunch_exit" -ne 0 ]; then
    fail_stage "relaunch" "APP_INSTALL_FAILED" "$relaunch_output" "$relaunch_started" \
        "$relaunch_exit" "$CURRENT_DEPENDENCIES"
fi
APP_PID="$(printf '%s' "$relaunch_output" | sed -nE 's/.*: ([0-9]+)$/\1/p' | tail -n 1)"
if [ -z "$APP_PID" ]; then
    fail_stage "relaunch" "APP_INSTALL_FAILED" "simctl 未返回重新启动的 App PID." \
        "$relaunch_started" "1" "$CURRENT_DEPENDENCIES"
fi
if second_diagnostic="$(wait_for_diagnostic "$data_path" "$FIRST_RUN_ID")"; then
    relaunch_diagnostic_exit=0
else
    relaunch_diagnostic_exit=$?
fi
if [ "$relaunch_diagnostic_exit" -ne 0 ]; then
    stalled_stage="service_start"
    if [ -n "$second_diagnostic" ]; then
        stalled_stage="$(json_field "$second_diagnostic" "stage")"
    fi
    stalled_error="$(diagnostic_error_code "$stalled_stage")"
    fail_stage "relaunch" "$stalled_error" "第二次诊断停留在 $stalled_stage." \
        "$relaunch_started" "$relaunch_diagnostic_exit" "$CURRENT_DEPENDENCIES"
fi
SECOND_RUN_ID="$(json_field "$second_diagnostic" "run_id")"
second_state="$(json_field "$second_diagnostic" "state")"
second_stage="$(json_field "$second_diagnostic" "stage")"
second_error="$(json_field "$second_diagnostic" "error_code")"
SECOND_GENERATION_BEFORE="$(json_field "$second_diagnostic" "generation_before")"
SECOND_GENERATION_AFTER="$(json_field "$second_diagnostic" "generation_after")"
if [ "$second_state" != "passed" ]; then
    fail_stage "relaunch" "${second_error:-$(diagnostic_error_code "$second_stage")}" \
        "第二次自检失败于 $second_stage." "$relaunch_started" "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "relaunch" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "Second diagnostic passed." "" "" "$relaunch_started"

persistence_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
CURRENT_STAGE="persistence"
CURRENT_ERROR_CODE="PERSISTENCE_REGRESSION"
CURRENT_DEPENDENCIES="relaunch"
CURRENT_STARTED_AT="$persistence_started"
if ! python3 - "$FOREGROUND_GENERATION_AFTER" "$SECOND_GENERATION_BEFORE" "$SECOND_GENERATION_AFTER" <<'PY'
import sys
foreground_after, second_before, second_after = map(int, sys.argv[1:])
assert second_before >= foreground_after
assert second_after == second_before + 1
PY
then
    fail_stage "persistence" "PERSISTENCE_REGRESSION" "重新启动后的 generation 关系无效." \
        "$persistence_started" "1" "$CURRENT_DEPENDENCIES"
fi
record_stage "persistence" "passed" "" "0" "$CURRENT_DEPENDENCIES" \
    "Generation persisted across relaunch." "" "" "$persistence_started"
# //// /重新启动 App 并验证 generation 不回退 ////
