# audience: internal
# # ios-cn-remote-process
# 该模块为 iOS CN 远端构建提供进程组终止和有限等待函数.

# //// 检查进程组是否仍有成员 [@x380kkm 2026-08-20] ////
process_group_exists() {
    local group_id="$1"
    ps -axo pgid= | awk -v group_id="$group_id" '
        $1 == group_id { found = 1; exit }
        END { exit found ? 0 : 1 }
    '
}
# //// /检查进程组是否仍有成员 ////

# //// 检查进程组是否仍属于本轮目录 [@x380kkm 2026-08-20] ////
process_group_contains_root() {
    local group_id="$1"
    local remote_root="$2"
    ps -axo pgid=,command= | awk -v group_id="$group_id" -v remote_root="$remote_root" '
        $1 == group_id {
            $1 = ""
            if (index($0, remote_root) > 0) { found = 1 }
        }
        END { exit found ? 0 : 1 }
    '
}
# //// /检查进程组是否仍属于本轮目录 ////

# //// 有限等待进程组退出 [@x380kkm 2026-08-20] ////
wait_for_process_group_exit() {
    local group_id="$1"
    local timeout_seconds="$2"
    local elapsed=0
    while process_group_exists "$group_id"; do
        if [ "$elapsed" -ge "$timeout_seconds" ]; then
            return 1
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
}
# //// /有限等待进程组退出 ////

# //// 先温和后强制终止进程组 [@x380kkm 2026-08-20] ////
terminate_process_group() {
    local group_id="$1"
    case "$group_id" in
        ''|*[!0-9]*|0|1) return 1 ;;
    esac
    if ! process_group_exists "$group_id"; then
        return 0
    fi
    kill -TERM -- "-$group_id" 2>/dev/null || true
    if wait_for_process_group_exit "$group_id" 10; then
        return 0
    fi
    kill -KILL -- "-$group_id" 2>/dev/null || true
    wait_for_process_group_exit "$group_id" 5
}
# //// /先温和后强制终止进程组 ////

# //// 检查进程是否仍属于本轮目录 [@x380kkm 2026-08-20] ////
process_contains_root() {
    local process_id="$1"
    local remote_root="$2"
    local process_command
    process_command="$(ps -p "$process_id" -o command= 2>/dev/null || true)"
    case "$process_command" in
        *"$remote_root"*) return 0 ;;
        *) return 1 ;;
    esac
}
# //// /检查进程是否仍属于本轮目录 ////

# //// 读取进程启动时间 [@x380kkm 2026-08-20] ////
read_process_start_time() {
    local process_id="$1"
    ps -p "$process_id" -o lstart= 2>/dev/null |
        sed 's/^[[:space:]]*//; s/[[:space:]]*$//' || true
}
# //// /读取进程启动时间 ////

# //// 有限等待进程退出 [@x380kkm 2026-08-20] ////
wait_for_process_exit() {
    local process_id="$1"
    local timeout_seconds="$2"
    local elapsed=0
    while kill -0 "$process_id" 2>/dev/null; do
        if [ "$elapsed" -ge "$timeout_seconds" ]; then
            return 1
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
}
# //// /有限等待进程退出 ////

# //// 先温和后强制终止单个进程 [@x380kkm 2026-08-20] ////
terminate_process() {
    local process_id="$1"
    case "$process_id" in
        ''|*[!0-9]*|0|1) return 1 ;;
    esac
    if ! kill -0 "$process_id" 2>/dev/null; then
        return 0
    fi
    kill -TERM "$process_id" 2>/dev/null || true
    if wait_for_process_exit "$process_id" 10; then
        return 0
    fi
    kill -KILL "$process_id" 2>/dev/null || true
    wait_for_process_exit "$process_id" 5
}
# //// /先温和后强制终止单个进程 ////
