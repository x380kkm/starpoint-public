# audience: internal
# # cleanup-ios-validation
# 该脚本只终止指定验证运行创建的进程和 Simulator, 然后删除对应的 /tmp 目录.

set -u

REMOTE_ROOT="${1:?remote root is required}"
RUN_ID="${2:?run id is required}"
BUNDLE_ID="${3:-dev.starpoint.PersonalServiceDiagnostic}"
EXPECTED_ROOT="/tmp/starpoint-ios-$RUN_ID"
CLEANUP_FAILED=0
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
STOP_PROCESS_SCRIPT="$SCRIPT_DIR/stop-ios-validation-process.sh"

if [ "$REMOTE_ROOT" != "$EXPECTED_ROOT" ]; then
    printf '%s\n' "Cleanup root does not match the validation run." >&2
    exit 2
fi

# //// 终止本次远端构建和 Simulator runner [@x380kkm 2026-08-18] ////
for runner_name in device simulator; do
    if ! bash "$STOP_PROCESS_SCRIPT" "$REMOTE_ROOT" "$RUN_ID" "$runner_name"; then
        CLEANUP_FAILED=1
    fi
done
# //// /终止本次远端构建和 Simulator runner ////

# //// 只关闭本次 runner 记录的 Simulator [@x380kkm 2026-08-18] ////
owner_path="$REMOTE_ROOT/simulator-udid.txt"
simulator_udid=""
if [ -f "$owner_path" ]; then
    simulator_udid="$(tr -cd 'A-Fa-f0-9-' < "$owner_path")"
fi
if [ -n "$simulator_udid" ]; then
    xcrun simctl terminate "$simulator_udid" "$BUNDLE_ID" >/dev/null 2>&1 || true
    xcrun simctl shutdown "$simulator_udid" >/dev/null 2>&1 || true
    simulator_line="$(xcrun simctl list devices | grep -F "$simulator_udid" || true)"
    case "$simulator_line" in
        *"(Booted)"*)
            printf '%s\n' "Validation Simulator remains booted." >&2
            CLEANUP_FAILED=1
            ;;
    esac
fi
# //// /只关闭本次 runner 记录的 Simulator ////

# //// 删除本次远端临时目录并检查残留 [@x380kkm 2026-08-18] ////
if ! rm -rf "$REMOTE_ROOT"; then
    printf '%s\n' "Validation temporary directory could not be removed." >&2
    CLEANUP_FAILED=1
fi

for process_id in $(pgrep -f "[s]tarpoint-ios-$RUN_ID" 2>/dev/null || true); do
    if [ "$process_id" != "$$" ] && [ "$process_id" != "$PPID" ]; then
        printf '%s\n' "Validation process remains after cleanup." >&2
        CLEANUP_FAILED=1
        break
    fi
done

if [ -e "$REMOTE_ROOT" ]; then
    CLEANUP_FAILED=1
fi
exit "$CLEANUP_FAILED"
# //// /删除本次远端临时目录并检查残留 ////
