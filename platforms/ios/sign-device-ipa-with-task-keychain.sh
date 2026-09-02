# audience: external
# # sign-device-ipa-with-task-keychain
# 此脚本验证证书, profile, Team ID 和目标 UDID, 临时加入 task keychain 后调用现有 IPA 签名器.
# 退出时恢复用户 keychain search list, 锁定 task keychain 并删除本轮临时文件.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
INPUT_IPA=""
OUTPUT_IPA=""
BUNDLE_ID=""
DEVICE_UDID=""
TEAM_ID=""
CERTIFICATE=""
PROVISIONING_PROFILE=""
TASK_KEYCHAIN=""
KEYCHAIN_PASSWORD_FILE=""
EXPECTED_PUBLIC_KEY_SHA256=""
OUTPUT_REPORT=""
WORK_ROOT=""
SEARCH_LIST_CHANGED=0
ORIGINAL_SEARCH_LIST=()

# //// 输出签名参数错误 [@x380kkm 2026-08-21] ////
fail() {
    printf '%s\n' "$1" >&2
    exit 1
}
# //// /输出签名参数错误 ////

# //// 解析 task keychain 签名参数 [@x380kkm 2026-08-21] ////
parse_arguments() {
    while [ "$#" -gt 0 ]; do
        [ "$#" -ge 2 ] || fail "Missing value for $1."
        case "$1" in
            --input) INPUT_IPA="$2" ;;
            --output) OUTPUT_IPA="$2" ;;
            --bundle-id) BUNDLE_ID="$2" ;;
            --device-udid) DEVICE_UDID="$2" ;;
            --team-id) TEAM_ID="$2" ;;
            --certificate) CERTIFICATE="$2" ;;
            --profile) PROVISIONING_PROFILE="$2" ;;
            --keychain) TASK_KEYCHAIN="$2" ;;
            --password-file) KEYCHAIN_PASSWORD_FILE="$2" ;;
            --expected-public-key-sha256) EXPECTED_PUBLIC_KEY_SHA256="$2" ;;
            --report) OUTPUT_REPORT="$2" ;;
            *) fail "Unknown argument: $1" ;;
        esac
        shift 2
    done

    [ -f "$INPUT_IPA" ] || fail "Input IPA does not exist."
    [ -n "$OUTPUT_IPA" ] || fail "Output IPA is required."
    [ "$INPUT_IPA" != "$OUTPUT_IPA" ] || fail "Input and output IPA must differ."
    [ -n "$BUNDLE_ID" ] || fail "Bundle identifier is required."
    [ -n "$DEVICE_UDID" ] || fail "Device UDID is required."
    case "$TEAM_ID" in
        [A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9][A-Z0-9]) ;;
        *) fail "Team ID must contain 10 uppercase letters or digits." ;;
    esac
    [ -f "$CERTIFICATE" ] || fail "Apple Development certificate does not exist."
    [ -f "$PROVISIONING_PROFILE" ] || fail "Development provisioning profile does not exist."
    [ -f "$TASK_KEYCHAIN" ] || fail "Task keychain does not exist."
    [ -f "$KEYCHAIN_PASSWORD_FILE" ] || fail "Task keychain password file does not exist."
    [ "$(stat -f '%Lp' "$KEYCHAIN_PASSWORD_FILE")" = "600" ] ||
        fail "Task keychain password file mode must be 600."
    case "$EXPECTED_PUBLIC_KEY_SHA256" in
        *[!0-9a-fA-F]*|'') fail "Expected public-key SHA-256 is invalid." ;;
    esac
    [ "${#EXPECTED_PUBLIC_KEY_SHA256}" -eq 64 ] ||
        fail "Expected public-key SHA-256 must contain 64 hexadecimal characters."
    [ -n "$OUTPUT_REPORT" ] || fail "Signing report is required."
}
# //// /解析 task keychain 签名参数 ////

# //// 恢复 search list 并锁定 task keychain [@x380kkm 2026-08-21] ////
cleanup() {
    status=$?
    cleanup_status=0
    set +e
    if [ "$SEARCH_LIST_CHANGED" -eq 1 ]; then
        security list-keychains -d user -s "${ORIGINAL_SEARCH_LIST[@]}" || cleanup_status=1
    fi
    if [ -n "$TASK_KEYCHAIN" ] && [ -e "$TASK_KEYCHAIN" ]; then
        security lock-keychain "$TASK_KEYCHAIN" || cleanup_status=1
    fi
    if [ -n "$WORK_ROOT" ] && [ -d "$WORK_ROOT" ]; then
        rm -rf -- "$WORK_ROOT" || cleanup_status=1
    fi
    trap - EXIT HUP INT TERM
    if [ "$status" -eq 0 ] && [ "$cleanup_status" -ne 0 ]; then
        exit 1
    fi
    exit "$status"
}
# //// /恢复 search list 并锁定 task keychain ////

# //// 读取并临时扩展用户 keychain search list [@x380kkm 2026-08-21] ////
add_task_keychain_to_search_list() {
    while IFS= read -r line; do
        normalized="$(printf '%s' "$line" | sed 's/^[[:space:]]*"//;s/"[[:space:]]*$//')"
        [ -n "$normalized" ] && ORIGINAL_SEARCH_LIST+=("$normalized")
    done < <(security list-keychains -d user)

    temporary_search_list=("$TASK_KEYCHAIN")
    for keychain in "${ORIGINAL_SEARCH_LIST[@]}"; do
        [ "$keychain" = "$TASK_KEYCHAIN" ] || temporary_search_list+=("$keychain")
    done
    security list-keychains -d user -s "${temporary_search_list[@]}"
    SEARCH_LIST_CHANGED=1
}
# //// /读取并临时扩展用户 keychain search list ////

# //// 把 DER 或 PEM 证书规范化为 PEM [@x380kkm 2026-08-21] ////
normalize_certificate() {
    output="$1"
    if openssl x509 -in "$CERTIFICATE" -noout >/dev/null 2>&1; then
        openssl x509 -in "$CERTIFICATE" -out "$output"
        return
    fi
    openssl x509 -inform DER -in "$CERTIFICATE" -out "$output"
}
# //// /把 DER 或 PEM 证书规范化为 PEM ////

# //// 验证证书和 profile 后签名候选 IPA [@x380kkm 2026-08-21] ////
main() {
    parse_arguments "$@"
    [ "$(uname -s)" = "Darwin" ] || fail "This operation requires macOS."
    command -v codesign >/dev/null
    command -v openssl >/dev/null
    command -v security >/dev/null
    command -v python3 >/dev/null
    command -v shasum >/dev/null

    WORK_ROOT="$(mktemp -d /tmp/starpoint-ios-task-sign.XXXXXX)"
    trap cleanup EXIT HUP INT TERM
    CERTIFICATE_PEM="$WORK_ROOT/apple-development.pem"
    CERTIFICATE_DER="$WORK_ROOT/apple-development.der"
    PROFILE_PLIST="$WORK_ROOT/profile.plist"
    ENTITLEMENTS_PLIST="$WORK_ROOT/entitlements.plist"
    PROFILE_SUMMARY="$WORK_ROOT/profile-summary.json"
    normalize_certificate "$CERTIFICATE_PEM"
    openssl x509 -in "$CERTIFICATE_PEM" -outform DER -out "$CERTIFICATE_DER"

    ACTUAL_PUBLIC_KEY_SHA256="$(
        openssl x509 -in "$CERTIFICATE_PEM" -noout -pubkey |
            openssl pkey -pubin -outform DER 2>/dev/null |
            shasum -a 256 |
            awk '{print $1}'
    )"
    NORMALIZED_EXPECTED_PUBLIC_KEY_SHA256="$(
        printf '%s' "$EXPECTED_PUBLIC_KEY_SHA256" | tr '[:upper:]' '[:lower:]'
    )"
    [ "$ACTUAL_PUBLIC_KEY_SHA256" = "$NORMALIZED_EXPECTED_PUBLIC_KEY_SHA256" ] ||
        fail "Certificate public key does not match the generated CSR."
    CERTIFICATE_SUBJECT="$(openssl x509 -in "$CERTIFICATE_PEM" -noout -subject -nameopt RFC2253)"
    case ",$CERTIFICATE_SUBJECT," in
        *,OU="$TEAM_ID",*|*,OU=$TEAM_ID,*) ;;
        *) fail "Certificate subject does not contain the expected Team ID." ;;
    esac

    security cms -D -i "$PROVISIONING_PROFILE" > "$PROFILE_PLIST"
    python3 - "$PROFILE_PLIST" "$CERTIFICATE_DER" <<'PY'
import plistlib
import sys

profile = plistlib.load(open(sys.argv[1], "rb"))
certificate = open(sys.argv[2], "rb").read()
developer_certificates = profile.get("DeveloperCertificates")
if not isinstance(developer_certificates, list) or certificate not in developer_certificates:
    raise SystemExit("profile does not contain the signing certificate")
PY
    python3 "$REPOSITORY_ROOT/scripts/protocol-lab/prepare_ios_signing.py" \
        --profile-plist "$PROFILE_PLIST" \
        --bundle-id "$BUNDLE_ID" \
        --device-udid "$DEVICE_UDID" \
        --output "$ENTITLEMENTS_PLIST" > "$PROFILE_SUMMARY"
    python3 - "$PROFILE_SUMMARY" "$TEAM_ID" <<'PY'
import json
import sys

summary = json.load(open(sys.argv[1], encoding="utf-8"))
if summary.get("team_identifier") != sys.argv[2]:
    raise SystemExit("profile Team ID does not match")
if summary.get("required_device_registered") is not True:
    raise SystemExit("profile does not contain the target UDID")
PY

    KEYCHAIN_PASSWORD="$(tr -d '\r\n' < "$KEYCHAIN_PASSWORD_FILE")"
    security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$TASK_KEYCHAIN"
    add_task_keychain_to_search_list
    import_output=""
    if ! import_output="$(security import "$CERTIFICATE_PEM" -k "$TASK_KEYCHAIN" -t cert -f x509 2>&1)"; then
        case "$import_output" in
            *"The specified item already exists"*) ;;
            *) fail "Certificate import failed: $import_output" ;;
        esac
    fi
    security set-key-partition-list \
        -S apple-tool:,apple:,codesign: \
        -s \
        -k "$KEYCHAIN_PASSWORD" \
        "$TASK_KEYCHAIN" >/dev/null

    IDENTITY_LIST="$WORK_ROOT/identities.txt"
    security find-identity -v -p codesigning "$TASK_KEYCHAIN" > "$IDENTITY_LIST"
    SIGNING_IDENTITIES=()
    while IFS= read -r identity; do
        [ -n "$identity" ] && SIGNING_IDENTITIES+=("$identity")
    done < <(sed -n 's/.*) \([0-9A-F][0-9A-F]*\) ".*/\1/p' "$IDENTITY_LIST")
    [ "${#SIGNING_IDENTITIES[@]}" -eq 1 ] ||
        fail "Task keychain must contain exactly one valid code-signing identity."
    SIGNING_IDENTITY="${SIGNING_IDENTITIES[0]}"
    [ "${#SIGNING_IDENTITY}" -eq 40 ] || fail "Code-signing identity SHA-1 is invalid."

    bash "$SCRIPT_DIR/sign-device-ipa.sh" \
        --development \
        --input "$INPUT_IPA" \
        --output "$OUTPUT_IPA" \
        --bundle-id "$BUNDLE_ID" \
        --identity "$SIGNING_IDENTITY" \
        --profile "$PROVISIONING_PROFILE" \
        --keychain "$TASK_KEYCHAIN" \
        --device-udid "$DEVICE_UDID"

    SIGNING_SUMMARY="$OUTPUT_IPA.signing.json"
    [ -f "$OUTPUT_IPA" ] || fail "Signed IPA was not created."
    [ -f "$SIGNING_SUMMARY" ] || fail "Signing summary was not created."
    OUTPUT_SHA256="$(shasum -a 256 "$OUTPUT_IPA" | awk '{print $1}')"
    CERTIFICATE_SHA256="$(shasum -a 256 "$CERTIFICATE" | awk '{print $1}')"
    PROFILE_SHA256="$(shasum -a 256 "$PROVISIONING_PROFILE" | awk '{print $1}')"
    python3 - "$OUTPUT_REPORT" "$SIGNING_SUMMARY" "$TEAM_ID" "$DEVICE_UDID" \
        "$BUNDLE_ID" "$SIGNING_IDENTITY" "$OUTPUT_SHA256" "$CERTIFICATE_SHA256" \
        "$PROFILE_SHA256" "$ACTUAL_PUBLIC_KEY_SHA256" <<'PY'
import json
import pathlib
import sys

summary = json.load(open(sys.argv[2], encoding="utf-8"))
report = {
    "schema_version": 1,
    "status": "passed",
    "team_id": sys.argv[3],
    "device_udid": sys.argv[4],
    "bundle_id": sys.argv[5],
    "signing_identity_sha1": sys.argv[6],
    "output_ipa_sha256": sys.argv[7],
    "certificate_sha256": sys.argv[8],
    "profile_sha256": sys.argv[9],
    "public_key_sha256": sys.argv[10],
    "profile": summary,
    "search_list_restored_on_exit": True,
    "task_keychain_locked_on_exit": True,
}
path = pathlib.Path(sys.argv[1])
temporary = path.with_suffix(path.suffix + ".tmp")
temporary.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
temporary.replace(path)
PY
    printf '%s\n' "$OUTPUT_IPA"
}
# //// /验证证书和 profile 后签名候选 IPA ////

main "$@"
