# audience: external
# # create-apple-development-csr
# 此脚本通过 macOS certtool 在独立 task keychain 创建 RSA 2048 私钥和 CSR, 并输出不含私钥的审计报告.

set -euo pipefail

TEAM_ID=""
COMMON_NAME=""
KEY_LABEL=""
OUTPUT_CSR=""
OUTPUT_REPORT=""
TASK_KEYCHAIN=""
KEYCHAIN_PASSWORD_FILE=""
COMPLETED=0

# //// 输出参数错误 [@x380kkm 2026-08-21] ////
fail() {
    printf '%s\n' "$1" >&2
    exit 1
}
# //// /输出参数错误 ////

# //// 解析 CSR 参数 [@x380kkm 2026-08-21] ////
parse_arguments() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --team-id) TEAM_ID="$2"; shift 2 ;;
            --common-name) COMMON_NAME="$2"; shift 2 ;;
            --key-label) KEY_LABEL="$2"; shift 2 ;;
            --keychain) TASK_KEYCHAIN="$2"; shift 2 ;;
            --password-file) KEYCHAIN_PASSWORD_FILE="$2"; shift 2 ;;
            --output) OUTPUT_CSR="$2"; shift 2 ;;
            --report) OUTPUT_REPORT="$2"; shift 2 ;;
            *) fail "Unknown argument: $1" ;;
        esac
    done
    case "$TEAM_ID" in
        [A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9]) ;;
        *) fail "Team ID must contain 10 uppercase letters or digits." ;;
    esac
    [ -n "$COMMON_NAME" ] || fail "Common name is required."
    [ -n "$KEY_LABEL" ] || fail "Key label is required."
    [ -n "$TASK_KEYCHAIN" ] || fail "Task keychain path is required."
    [ -n "$KEYCHAIN_PASSWORD_FILE" ] || fail "Keychain password file is required."
    [ -n "$OUTPUT_CSR" ] || fail "CSR output path is required."
    [ -n "$OUTPUT_REPORT" ] || fail "Report output path is required."
    [ "$OUTPUT_CSR" != "$OUTPUT_REPORT" ] || fail "CSR and report paths must differ."
}
# //// /解析 CSR 参数 ////

# //// 回滚失败的钥匙串条目 [@x380kkm 2026-08-21] ////
cleanup() {
    if [ "$COMPLETED" -eq 0 ]; then
        if [ -n "$TASK_KEYCHAIN" ] && [ -e "$TASK_KEYCHAIN" ]; then
            security delete-keychain "$TASK_KEYCHAIN" >/dev/null 2>&1 || true
        fi
        if [ -n "$KEYCHAIN_PASSWORD_FILE" ] && [ -f "$KEYCHAIN_PASSWORD_FILE" ]; then
            rm -f -- "$KEYCHAIN_PASSWORD_FILE"
        fi
    fi
}
# //// /回滚失败的钥匙串条目 ////

# //// 在登录钥匙串创建私钥和 CSR [@x380kkm 2026-08-21] ////
main() {
    parse_arguments "$@"
    [ "$(uname -s)" = "Darwin" ] || fail "This operation requires macOS."
    command -v certtool >/dev/null
    command -v openssl >/dev/null
    command -v security >/dev/null
    command -v shasum >/dev/null
    command -v python3 >/dev/null

    [ ! -e "$TASK_KEYCHAIN" ] || fail "The task keychain already exists."
    [ ! -e "$KEYCHAIN_PASSWORD_FILE" ] || fail "The keychain password file already exists."

    mkdir -p \
        "$(dirname "$OUTPUT_CSR")" \
        "$(dirname "$OUTPUT_REPORT")" \
        "$(dirname "$TASK_KEYCHAIN")" \
        "$(dirname "$KEYCHAIN_PASSWORD_FILE")"
    umask 077
    trap cleanup EXIT HUP INT TERM
    openssl rand -base64 48 > "$KEYCHAIN_PASSWORD_FILE"
    chmod 600 "$KEYCHAIN_PASSWORD_FILE"
    KEYCHAIN_PASSWORD="$(tr -d '\r\n' < "$KEYCHAIN_PASSWORD_FILE")"
    security create-keychain -p "$KEYCHAIN_PASSWORD" "$TASK_KEYCHAIN"
    security set-keychain-settings -lut 21600 "$TASK_KEYCHAIN"
    security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$TASK_KEYCHAIN"

    python3 - "$OUTPUT_CSR" "$TASK_KEYCHAIN" "$KEY_LABEL" "$COMMON_NAME" "$TEAM_ID" <<'PY'
import subprocess
import sys

output, keychain, label, common_name, team_id = sys.argv[1:]
answers = "\n".join(
    [
        label,
        "r",
        "2048",
        "y",
        "s",
        "2",
        "y",
        "starpoint-development",
        common_name,
        "",
        "Starpoint",
        team_id,
        "",
        "",
        "y",
    ]
) + "\n"
try:
    result = subprocess.run(
        ["certtool", "r", output, f"k={keychain}", "a"],
        input=answers,
        text=True,
        capture_output=True,
        timeout=60,
        check=False,
    )
except subprocess.TimeoutExpired as error:
    raise SystemExit("certtool timed out") from error
if result.returncode != 0:
    detail = (result.stderr or result.stdout).strip()
    raise SystemExit(f"certtool failed: {detail}")
PY
    certtool V "$OUTPUT_CSR" >/dev/null
    security find-key -t private -l "$KEY_LABEL" "$TASK_KEYCHAIN" >/dev/null
    security set-key-partition-list \
        -S apple-tool:,apple:,codesign: \
        -s \
        -k "$KEYCHAIN_PASSWORD" \
        "$TASK_KEYCHAIN" >/dev/null

    CSR_SHA256="$(shasum -a 256 "$OUTPUT_CSR" | awk '{print $1}')"
    PUBLIC_KEY_SHA256="$(
        openssl req -in "$OUTPUT_CSR" -noout -pubkey |
            openssl pkey -pubin -outform DER 2>/dev/null |
            shasum -a 256 |
            awk '{print $1}'
    )"
    python3 - "$OUTPUT_REPORT" "$TEAM_ID" "$COMMON_NAME" "$KEY_LABEL" \
        "$TASK_KEYCHAIN" "$KEYCHAIN_PASSWORD_FILE" "$CSR_SHA256" "$PUBLIC_KEY_SHA256" <<'PY'
import json
import pathlib
import sys

report = {
    "schema_version": 1,
    "status": "passed",
    "team_id": sys.argv[2],
    "common_name": sys.argv[3],
    "private_key_label": sys.argv[4],
    "keychain": sys.argv[5],
    "keychain_password_file": sys.argv[6],
    "private_key_exported": False,
    "private_key_origin": "macos-certtool-task-keychain",
    "csr_sha256": sys.argv[7],
    "public_key_sha256": sys.argv[8],
    "algorithm": "RSA",
    "key_size": 2048,
}
path = pathlib.Path(sys.argv[1])
temporary = path.with_suffix(path.suffix + ".tmp")
temporary.write_text(
    json.dumps(report, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
temporary.replace(path)
PY
    security lock-keychain "$TASK_KEYCHAIN"
    COMPLETED=1
    trap - EXIT HUP INT TERM
    printf '%s\n' "$OUTPUT_CSR"
}
# //// /在独立 task keychain 创建私钥和 CSR ////

main "$@"
