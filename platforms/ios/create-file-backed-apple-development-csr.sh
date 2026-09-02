# audience: external
# # create-file-backed-apple-development-csr
# 此脚本创建加密 RSA 2048 私钥和 CSR, 私钥及其随机口令只保存在 Mac mode 600 文件中.
# CSR 和审计报告不包含私钥, 可回传 Apple Developer 签发新的 Apple Development 证书.

set -euo pipefail

TEAM_ID=""
COMMON_NAME=""
PRIVATE_KEY=""
PRIVATE_KEY_PASSWORD_FILE=""
OUTPUT_CSR=""
OUTPUT_REPORT=""
COMPLETED=0

# //// 输出 file-backed CSR 参数错误 [@x380kkm 2026-08-21] ////
fail() {
    printf '%s\n' "$1" >&2
    exit 1
}
# //// /输出 file-backed CSR 参数错误 ////

# //// 解析 file-backed CSR 参数 [@x380kkm 2026-08-21] ////
parse_arguments() {
    while [ "$#" -gt 0 ]; do
        [ "$#" -ge 2 ] || fail "Missing value for $1."
        case "$1" in
            --team-id) TEAM_ID="$2" ;;
            --common-name) COMMON_NAME="$2" ;;
            --private-key) PRIVATE_KEY="$2" ;;
            --password-file) PRIVATE_KEY_PASSWORD_FILE="$2" ;;
            --output) OUTPUT_CSR="$2" ;;
            --report) OUTPUT_REPORT="$2" ;;
            *) fail "Unknown argument: $1" ;;
        esac
        shift 2
    done
    case "$TEAM_ID" in
        [A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9]) ;;
        *) fail "Team ID must contain 10 uppercase letters or digits." ;;
    esac
    [ -n "$COMMON_NAME" ] || fail "Common name is required."
    [ -n "$PRIVATE_KEY" ] || fail "Private-key path is required."
    [ -n "$PRIVATE_KEY_PASSWORD_FILE" ] || fail "Private-key password file is required."
    [ -n "$OUTPUT_CSR" ] || fail "CSR output path is required."
    [ -n "$OUTPUT_REPORT" ] || fail "Report output path is required."
    for path in "$PRIVATE_KEY" "$PRIVATE_KEY_PASSWORD_FILE" "$OUTPUT_CSR" "$OUTPUT_REPORT"; do
        [ ! -e "$path" ] || fail "Output path already exists: $path"
    done
}
# //// /解析 file-backed CSR 参数 ////

# //// 删除失败轮次的加密私钥和输出 [@x380kkm 2026-08-21] ////
cleanup() {
    if [ "$COMPLETED" -eq 0 ]; then
        rm -f -- \
            "$PRIVATE_KEY" \
            "$PRIVATE_KEY_PASSWORD_FILE" \
            "$OUTPUT_CSR" \
            "$OUTPUT_REPORT" \
            "$OUTPUT_REPORT.tmp"
    fi
}
# //// /删除失败轮次的加密私钥和输出 ////

# //// 创建加密私钥, CSR 和审计报告 [@x380kkm 2026-08-21] ////
main() {
    parse_arguments "$@"
    [ "$(uname -s)" = "Darwin" ] || fail "This operation requires macOS."
    command -v openssl >/dev/null
    command -v shasum >/dev/null
    command -v python3 >/dev/null

    mkdir -p \
        "$(dirname "$PRIVATE_KEY")" \
        "$(dirname "$PRIVATE_KEY_PASSWORD_FILE")" \
        "$(dirname "$OUTPUT_CSR")" \
        "$(dirname "$OUTPUT_REPORT")"
    umask 077
    trap cleanup EXIT HUP INT TERM
    openssl rand -base64 48 > "$PRIVATE_KEY_PASSWORD_FILE"
    chmod 600 "$PRIVATE_KEY_PASSWORD_FILE"
    openssl genpkey \
        -algorithm RSA \
        -aes-256-cbc \
        -pass "file:$PRIVATE_KEY_PASSWORD_FILE" \
        -pkeyopt rsa_keygen_bits:2048 \
        -out "$PRIVATE_KEY" >/dev/null 2>&1
    chmod 600 "$PRIVATE_KEY"
    openssl req \
        -new \
        -sha256 \
        -key "$PRIVATE_KEY" \
        -passin "file:$PRIVATE_KEY_PASSWORD_FILE" \
        -subj "/CN=$COMMON_NAME/OU=$TEAM_ID/O=Starpoint" \
        -out "$OUTPUT_CSR"
    openssl req -in "$OUTPUT_CSR" -noout -verify >/dev/null 2>&1

    PRIVATE_KEY_SHA256="$(shasum -a 256 "$PRIVATE_KEY" | awk '{print $1}')"
    CSR_SHA256="$(shasum -a 256 "$OUTPUT_CSR" | awk '{print $1}')"
    PUBLIC_KEY_SHA256="$(
        openssl req -in "$OUTPUT_CSR" -noout -pubkey |
            openssl pkey -pubin -outform DER 2>/dev/null |
            shasum -a 256 |
            awk '{print $1}'
    )"
    python3 - "$OUTPUT_REPORT" "$TEAM_ID" "$COMMON_NAME" "$PRIVATE_KEY" \
        "$PRIVATE_KEY_PASSWORD_FILE" "$PRIVATE_KEY_SHA256" "$CSR_SHA256" \
        "$PUBLIC_KEY_SHA256" <<'PY'
import json
import pathlib
import sys

report = {
    "schema_version": 1,
    "status": "passed",
    "team_id": sys.argv[2],
    "common_name": sys.argv[3],
    "private_key": sys.argv[4],
    "private_key_password_file": sys.argv[5],
    "private_key_sha256": sys.argv[6],
    "private_key_encrypted": True,
    "private_key_exported_to_windows": False,
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
    COMPLETED=1
    trap - EXIT HUP INT TERM
    printf '%s\n' "$OUTPUT_CSR"
}
# //// /创建加密私钥, CSR 和审计报告 ////

main "$@"
