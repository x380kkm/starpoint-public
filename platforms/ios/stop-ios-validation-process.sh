# audience: internal
# # stop-ios-validation-process
# 该脚本只终止指定验证运行记录的一个远端 runner 进程树.

set -u

if [ "$#" -lt 3 ]; then
    printf '%s\n' "Remote root, run ID and runner name are required." >&2
    exit 2
fi
REMOTE_ROOT="$1"
RUN_ID="$2"
RUNNER_NAME="$3"
EXPECTED_ROOT="/tmp/starpoint-ios-$RUN_ID"
PROCESS_IDS=""

if [ "$REMOTE_ROOT" != "$EXPECTED_ROOT" ]; then
    printf '%s\n' "Process root does not match the validation run." >&2
    exit 2
fi
case "$RUNNER_NAME" in
    device|simulator) ;;
    *)
        printf '%s\n' "Unknown validation runner." >&2
        exit 2
        ;;
esac
case "$RUN_ID" in
    ""|*[!A-Za-z0-9._-]*)
        printf '%s\n' "Validation run ID is invalid." >&2
        exit 2
        ;;
esac

# //// 判断进程仍占用系统资源 [@x380kkm 2026-08-18] ////
process_is_active() {
    local process_id="$1"
    local process_state
    process_state="$(ps -p "$process_id" -o stat= 2>/dev/null | tr -d '[:space:]' || true)"
    case "$process_state" in
        ""|Z*) return 1 ;;
        *) return 0 ;;
    esac
}
# //// /判断进程仍占用系统资源 ////

# //// 收集一个已验证归属的进程树 [@x380kkm 2026-08-18] ////
collect_process_tree() {
    local process_id="$1"
    local child_id
    if ! process_is_active "$process_id"; then
        return 0
    fi
    case " $PROCESS_IDS " in
        *" $process_id "*) return 0 ;;
    esac
    for child_id in $(pgrep -P "$process_id" 2>/dev/null || true); do
        collect_process_tree "$child_id"
    done
    PROCESS_IDS="$PROCESS_IDS $process_id"
}
# //// /收集一个已验证归属的进程树 ////

# //// 验证 pid 文件后收集 runner 和直接构建子进程 [@x380kkm 2026-08-18] ////
collect_recorded_processes() {
    local pid_path
    local process_id
    local process_command
    for pid_path in "$REMOTE_ROOT/$RUNNER_NAME-child.pid" "$REMOTE_ROOT/$RUNNER_NAME-runner.pid"; do
        if [ ! -f "$pid_path" ]; then
            continue
        fi
        process_id="$(tr -cd '0-9' < "$pid_path")"
        if [ -z "$process_id" ]; then
            continue
        fi
        process_command="$(ps -p "$process_id" -o command= 2>/dev/null || true)"
        if [ -z "$process_command" ]; then
            continue
        fi
        if [ "$process_id" = "$$" ] || [ "$process_id" = "$PPID" ]; then
            printf '%s\n' "Recorded validation pid belongs to the cleanup process." >&2
            exit 1
        fi
        case "$process_command" in
            *"$EXPECTED_ROOT"*) collect_process_tree "$process_id" ;;
            *)
                printf '%s\n' "Recorded validation pid belongs to another process." >&2
                exit 1
                ;;
        esac
    done
}
# //// /验证 pid 文件后收集 runner 和直接构建子进程 ////

# //// 收集脱离 runner 后仍属于本轮的构建进程 [@x380kkm 2026-08-18] ////
collect_reparented_build_processes() {
    local process_id
    local process_command
    for process_id in $(pgrep -f "[s]tarpoint-ios-$RUN_ID" 2>/dev/null || true); do
        if [ "$process_id" = "$$" ] || [ "$process_id" = "$PPID" ]; then
            continue
        fi
        process_command="$(ps -p "$process_id" -o command= 2>/dev/null || true)"
        case "$process_command" in
            *"$EXPECTED_ROOT"*)
                case "$process_command" in
                    *cargo*|*rustc*|*clang*|*build-device-harness.sh*|*build-simulator-harness.sh*|*run-simulator-diagnostic.sh*)
                        collect_process_tree "$process_id"
                        ;;
                esac
                ;;
        esac
    done
}
# //// /收集脱离 runner 后仍属于本轮的构建进程 ////

# //// 刷新本轮仍在运行的构建进程集合 [@x380kkm 2026-08-18] ////
collect_validation_processes() {
    PROCESS_IDS=""
    collect_recorded_processes
    collect_reparented_build_processes
}
# //// /刷新本轮仍在运行的构建进程集合 ////

# //// 向当前进程集合发送一个终止信号 [@x380kkm 2026-08-18] ////
signal_validation_processes() {
    local signal_name="$1"
    local process_id
    for process_id in $PROCESS_IDS; do
        kill -s "$signal_name" "$process_id" >/dev/null 2>&1 || true
    done
}
# //// /向当前进程集合发送一个终止信号 ////

# //// 有界终止并确认进程树退出 [@x380kkm 2026-08-18] ////
collect_validation_processes
signal_validation_processes TERM
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    sleep 0.25
    collect_validation_processes
    if [ -z "$PROCESS_IDS" ]; then
        exit 0
    fi
    signal_validation_processes TERM
done

signal_validation_processes KILL
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    sleep 0.25
    collect_validation_processes
    if [ -z "$PROCESS_IDS" ]; then
        exit 0
    fi
    signal_validation_processes KILL
done

printf '%s\n' "Validation runner process remains after bounded termination." >&2
exit 1
# //// /有界终止并确认进程树退出 ////
