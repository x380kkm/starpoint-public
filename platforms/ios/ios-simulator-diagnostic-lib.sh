# audience: internal
# # ios-simulator-diagnostic-lib
# 该脚本由 Simulator 诊断入口加载, 并使用入口定义的阶段状态和输出路径.

# //// 删除阶段详情中的凭据形状并保留尾部 [@x380kkm 2026-08-18] ////
sanitize_stage_detail() {
    if ! python3 "$SCRIPT_DIR/sanitize-diagnostic-detail.py"; then
        printf '%s' "[detail redaction failed]"
    fi
}
# //// /删除阶段详情中的凭据形状并保留尾部 ////

# //// 记录一个不包含凭据的阶段结果 [@x380kkm 2026-08-18] ////
record_stage() {
    local stage="$1"
    local status="$2"
    local error_code="$3"
    local stage_exit_code="$4"
    local dependencies="$5"
    local detail="$6"
    local artifact_path="$7"
    local sha256="$8"
    local started_at="$9"
    local ended_at
    local sanitized_detail
    ended_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    sanitized_detail="$(printf '%s' "$detail" | sanitize_stage_detail)"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$stage" "$status" "$error_code" "$stage_exit_code" "$started_at" "$ended_at" \
        "$dependencies" "$artifact_path" "$sha256" "$sanitized_detail" >> "$STAGE_PATH"
    if [ "$status" = "passed" ] && [ "$stage" != "cleanup" ]; then
        LAST_SUCCESS="$stage"
    elif [ "$status" = "failed" ] && [ -z "$FIRST_FAILURE" ]; then
        FIRST_FAILURE="$stage"
        FIRST_ERROR_CODE="$error_code"
    fi
}
# //// /记录一个不包含凭据的阶段结果 ////

# //// 记录失败阶段 [@x380kkm 2026-08-18] ////
fail_stage() {
    local stage="$1"
    local error_code="$2"
    local detail="$3"
    local started_at="$4"
    local stage_exit_code="${5:-1}"
    local dependencies="${6:-}"
    record_stage "$stage" "failed" "$error_code" "$stage_exit_code" "$dependencies" "$detail" "" "" "$started_at"
    return 1
}
# //// /记录失败阶段 ////

# //// 将诊断阶段转换为稳定错误代码 [@x380kkm 2026-08-18] ////
diagnostic_error_code() {
    case "$1" in
        management_auth) printf '%s\n' "MANAGEMENT_AUTH_FAILED" ;;
        management_features) printf '%s\n' "MANAGEMENT_FEATURES_FAILED" ;;
        state_increment) printf '%s\n' "PERSISTENCE_REGRESSION" ;;
        checkpoint) printf '%s\n' "CHECKPOINT_FAILED" ;;
        *) printf '%s\n' "SERVICE_START_TIMEOUT" ;;
    esac
}
# //// /将诊断阶段转换为稳定错误代码 ////

# //// 读取 App 写入的机器诊断状态 [@x380kkm 2026-08-18] ////
wait_for_diagnostic() {
    local data_path="$1"
    local previous_run_id="$2"
    local preferences_path="$data_path/Library/Preferences/$BUNDLE_ID.plist"
    python3 - "$preferences_path" "$previous_run_id" "$DIAGNOSTIC_TIMEOUT_SECONDS" <<'PY'
import json
import subprocess
import sys
import time
from pathlib import Path

preferences_path = Path(sys.argv[1])
previous_run_id = sys.argv[2]
deadline = time.monotonic() + int(sys.argv[3])
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
    run_id = values.get("run_id")
    state = values.get("state")
    if run_id and run_id != previous_run_id:
        last_result = {
            "run_id": run_id,
            "state": state,
            "stage": values.get("stage"),
            "error_code": values.get("error_code"),
            "generation_before": values.get("generation_before"),
            "generation_after": values.get("generation_after"),
        }
    if run_id and run_id != previous_run_id and state in {"passed", "failed"}:
        print(json.dumps(last_result, ensure_ascii=True, separators=(",", ":")))
        raise SystemExit(0)
    time.sleep(0.25)
if last_result is not None:
    print(json.dumps(last_result, ensure_ascii=True, separators=(",", ":")))
raise SystemExit(2)
PY
}
# //// /读取 App 写入的机器诊断状态 ////

# //// 读取诊断 JSON 的一个字段 [@x380kkm 2026-08-18] ////
json_field() {
    local json_text="$1"
    local field="$2"
    python3 -c 'import json, sys; value=json.loads(sys.argv[1]).get(sys.argv[2]); print("" if value is None else value)' \
        "$json_text" "$field"
}
# //// /读取诊断 JSON 的一个字段 ////

# //// 从 App 进程的监听端口验证 loopback 健康接口 [@x380kkm 2026-08-18] ////
find_healthy_loopback_port() {
    local process_id="$1"
    local candidate_ports
    local candidate_port
    local health
    candidate_ports="$(
        lsof -Pan -p "$process_id" -a -iTCP -sTCP:LISTEN 2>/dev/null |
            awk 'NR > 1 { print $9 }' |
            sed -nE 's/.*:([0-9]+)$/\1/p' |
            sort -nu
    )"
    for candidate_port in $candidate_ports; do
        health="$(curl --silent --show-error --fail --max-time 2 "http://127.0.0.1:$candidate_port/health" 2>/dev/null || true)"
        if [ -n "$health" ] && python3 -c 'import json, sys; value=json.loads(sys.argv[1]); assert isinstance(value.get("generation"), int)' "$health" 2>/dev/null; then
            printf '%s\n' "$candidate_port"
            return 0
        fi
    done
    return 1
}
# //// /从 App 进程的监听端口验证 loopback 健康接口 ////

# //// 判断目标使用 ad-hoc 代码签名 [@x380kkm 2026-08-18] ////
has_adhoc_signature() {
    local target_path="$1"
    local signature_details
    if ! signature_details="$(codesign -dv --verbose=4 "$target_path" 2>&1)"; then
        return 1
    fi
    case "$signature_details" in
        *"Signature=adhoc"*) return 0 ;;
        *) return 1 ;;
    esac
}
# //// /判断目标使用 ad-hoc 代码签名 ////

# //// 只清理本次创建的 Simulator 状态 [@x380kkm 2026-08-18] ////
cleanup() {
    local cleanup_started
    local cleanup_errors=""
    cleanup_started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    if [ -n "$SIMULATOR_UDID" ]; then
        xcrun simctl terminate "$SIMULATOR_UDID" "$BUNDLE_ID" >/dev/null 2>&1 || true
    fi
    if [ "$BOOTED_BY_SCRIPT" -eq 1 ] && [ -n "$SIMULATOR_UDID" ]; then
        if ! xcrun simctl shutdown "$SIMULATOR_UDID" >/dev/null 2>&1; then
            cleanup_errors="无法关闭本次启动的 Simulator."
        fi
    fi
    if [ -n "$cleanup_errors" ]; then
        CLEANUP_FAILED=1
        record_stage "cleanup" "failed" "CLEANUP_FAILED" "1" "" "$cleanup_errors" "" "" "$cleanup_started"
    else
        record_stage "cleanup" "passed" "" "0" "" "Simulator cleanup completed." "" "" "$cleanup_started"
    fi
}
# //// /只清理本次创建的 Simulator 状态 ////

# //// 生成稳定的 Simulator 诊断报告 [@x380kkm 2026-08-18] ////
write_report() {
    python3 - "$STAGE_PATH" "$REPORT_PATH" "$SIMULATOR_NAME" "$SIMULATOR_UDID" "$BUNDLE_ID" \
        "$FIRST_FAILURE" "$FIRST_ERROR_CODE" "$LAST_SUCCESS" \
        "$FIRST_RUN_ID" "$FIRST_GENERATION_BEFORE" "$FIRST_GENERATION_AFTER" \
        "$SECOND_RUN_ID" "$SECOND_GENERATION_BEFORE" "$SECOND_GENERATION_AFTER" <<'PY'
import json
import sys
from pathlib import Path

(
    stage_path,
    report_path,
    simulator_name,
    simulator_udid,
    bundle_id,
    first_failure,
    first_error_code,
    last_success,
    first_run_id,
    first_generation_before,
    first_generation_after,
    second_run_id,
    second_generation_before,
    second_generation_after,
) = sys.argv[1:]

stages = []
with Path(stage_path).open("r", encoding="utf-8") as stage_file:
    for line in stage_file:
        (
            stage,
            status,
            error_code,
            exit_code,
            started_at,
            ended_at,
            dependencies,
            artifact_path,
            sha256,
            detail,
        ) = line.rstrip("\n").split("\t", 9)
        stages.append(
            {
                "stage": stage,
                "status": status,
                "error_code": error_code or None,
                "exit_code": int(exit_code) if exit_code else None,
                "started_at": started_at,
                "ended_at": ended_at,
                "depends_on": [value for value in dependencies.split(",") if value],
                "artifact_path": artifact_path or None,
                "sha256": sha256 or None,
                "detail": detail,
            }
        )

def number(value):
    return int(value) if value else None

report = {
    "schema_version": 1,
    "status": "failed" if first_failure else "passed",
    "first_failure": first_failure or None,
    "root_blocker": first_error_code or None,
    "last_successful_stage": last_success or None,
    "simulator": {"name": simulator_name, "udid": simulator_udid or None},
    "bundle_id": bundle_id,
    "runs": [
        {
            "run_id": first_run_id or None,
            "generation_before": number(first_generation_before),
            "generation_after": number(first_generation_after),
        },
        {
            "run_id": second_run_id or None,
            "generation_before": number(second_generation_before),
            "generation_after": number(second_generation_after),
        },
    ],
    "stages": stages,
}
Path(report_path).write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY
}
# //// /生成稳定的 Simulator 诊断报告 ////

# //// 将意外 shell 错误归入当前阶段 [@x380kkm 2026-08-18] ////
on_error() {
    local error_exit_code="$1"
    local line_number="$2"
    if [ -z "$FIRST_FAILURE" ]; then
        record_stage "$CURRENT_STAGE" "failed" "$CURRENT_ERROR_CODE" "$error_exit_code" \
            "$CURRENT_DEPENDENCIES" "Unexpected command failure at line $line_number." "" "" "$CURRENT_STARTED_AT"
    fi
}
# //// /将意外 shell 错误归入当前阶段 ////

# //// 清理本轮资源并保留主退出码 [@x380kkm 2026-08-18] ////
on_exit() {
    local main_exit_code=$?
    trap - EXIT ERR INT TERM
    cleanup
    write_report
    if [ "$CLEANUP_FAILED" -eq 1 ] && [ "$main_exit_code" -eq 0 ]; then
        main_exit_code=1
    fi
    printf '%s\n' "$REPORT_PATH"
    exit "$main_exit_code"
}
# //// /清理本轮资源并保留主退出码 ////
