# audience: external
# # sign-ios-device-ipa
# 该脚本在临时目录中签名 IPA 的嵌套 Framework 和主 App. 开发者模式要求本机 keychain 身份和设备 profile.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
INPUT_IPA=""
OUTPUT_IPA=""
BUNDLE_ID=""
SIGNING_MODE=""
SIGNING_IDENTITY=""
PROVISIONING_PROFILE=""
KEYCHAIN_PATH=""
DEVICE_UDID=""
APP_ROOT=""
PROFILE_PLIST=""
ENTITLEMENTS_PLIST=""
SIGNING_SUMMARY=""

# //// 输出签名命令说明和错误 [@x380kkm 2026-07-23] ////
usage() {
    printf '%s\n' \
        "Usage:" \
        "  $0 --adhoc --input INPUT --output OUTPUT --bundle-id ID" \
        "  $0 --development --input INPUT --output OUTPUT --bundle-id ID \\" \
        "     --identity IDENTITY --profile PROFILE [--keychain KEYCHAIN] [--device-udid UDID]"
}

fail() {
    printf '%s\n' "$1" >&2
    exit 1
}
# //// /输出签名命令说明和错误 ////

# //// 解析设备签名参数 [@x380kkm 2026-07-23] ////
parse_arguments() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --adhoc)
                [ -z "$SIGNING_MODE" ] || fail "Choose exactly one signing mode."
                SIGNING_MODE="adhoc"
                shift
                ;;
            --development)
                [ -z "$SIGNING_MODE" ] || fail "Choose exactly one signing mode."
                SIGNING_MODE="development"
                shift
                ;;
            --input | --output | --bundle-id | --identity | --profile | --keychain | --device-udid)
                [ "$#" -ge 2 ] || fail "Missing value for $1."
                case "$1" in
                    --input) INPUT_IPA="$2" ;;
                    --output) OUTPUT_IPA="$2" ;;
                    --bundle-id) BUNDLE_ID="$2" ;;
                    --identity) SIGNING_IDENTITY="$2" ;;
                    --profile) PROVISIONING_PROFILE="$2" ;;
                    --keychain) KEYCHAIN_PATH="$2" ;;
                    --device-udid) DEVICE_UDID="$2" ;;
                esac
                shift 2
                ;;
            --help)
                usage
                exit 0
                ;;
            *)
                fail "Unknown argument: $1"
                ;;
        esac
    done

    [ -n "$SIGNING_MODE" ] || fail "Choose a signing mode."
    [ -f "$INPUT_IPA" ] || fail "Input IPA does not exist."
    [ -n "$OUTPUT_IPA" ] || fail "Output IPA is required."
    [ "$INPUT_IPA" != "$OUTPUT_IPA" ] || fail "Input and output IPA must differ."
    [ -n "$BUNDLE_ID" ] || fail "Bundle identifier is required."
    if [ "$SIGNING_MODE" = "development" ]; then
        [ -n "$SIGNING_IDENTITY" ] || fail "Development signing identity is required."
        [ -f "$PROVISIONING_PROFILE" ] || fail "Provisioning profile does not exist."
        if [ -n "$KEYCHAIN_PATH" ]; then
            [ -f "$KEYCHAIN_PATH" ] || fail "Signing keychain does not exist."
        fi
    fi
}
# //// /解析设备签名参数 ////

# //// 调用 codesign 签名代码 [@x380kkm 2026-07-23] ////
sign_code_path() {
    local code_path="$1"
    local arguments=(--force --sign "$SIGNING_IDENTITY" --timestamp=none)
    if [ -n "$KEYCHAIN_PATH" ]; then
        arguments+=(--keychain "$KEYCHAIN_PATH")
    fi
    codesign "${arguments[@]}" "$code_path"
}

sign_application() {
    local arguments=(--force --sign "$SIGNING_IDENTITY" --timestamp=none)
    if [ -n "$KEYCHAIN_PATH" ]; then
        arguments+=(--keychain "$KEYCHAIN_PATH")
    fi
    if [ "$SIGNING_MODE" = "development" ]; then
        arguments+=(--entitlements "$ENTITLEMENTS_PLIST" --generate-entitlement-der)
    fi
    codesign "${arguments[@]}" "$APP_ROOT"
}
# //// /调用 codesign 签名代码 ////

# //// 准备 Apple 开发者签名输入 [@x380kkm 2026-07-23] ////
prepare_development_signing() {
    SIGNING_SUMMARY="$WORK_ROOT/signing-summary.json"
    security cms -D -i "$PROVISIONING_PROFILE" > "$PROFILE_PLIST"
    local arguments=(
        --profile-plist "$PROFILE_PLIST"
        --bundle-id "$BUNDLE_ID"
        --output "$ENTITLEMENTS_PLIST"
    )
    if [ -n "$DEVICE_UDID" ]; then
        arguments+=(--device-udid "$DEVICE_UDID")
    fi
    python3 "$REPOSITORY_ROOT/scripts/protocol-lab/prepare_ios_signing.py" \
        "${arguments[@]}" > "$SIGNING_SUMMARY"
    cp "$PROVISIONING_PROFILE" "$APP_ROOT/embedded.mobileprovision"
}
# //// /准备 Apple 开发者签名输入 ////

# //// 准备 CI 使用的 ad-hoc 签名输入 [@x380kkm 2026-07-23] ////
prepare_adhoc_signing() {
    SIGNING_IDENTITY="-"
    rm -f "$APP_ROOT/embedded.mobileprovision"
}
# //// /准备 CI 使用的 ad-hoc 签名输入 ////

# //// 签名嵌套代码和主 App [@x380kkm 2026-07-23] ////
sign_application_tree() {
    if [ -n "$(find "$APP_ROOT" -type d -name '*.appex' -print -quit)" ]; then
        fail "App extensions require their own provisioning profiles."
    fi
    find "$APP_ROOT" -type d -name _CodeSignature -prune -exec rm -rf {} \;
    if [ -d "$APP_ROOT/Frameworks" ]; then
        while IFS= read -r -d '' dynamic_library; do
            sign_code_path "$dynamic_library"
        done < <(find "$APP_ROOT/Frameworks" -type f -name '*.dylib' -print0)
        while IFS= read -r -d '' framework; do
            sign_code_path "$framework"
        done < <(find "$APP_ROOT/Frameworks" -type d -name '*.framework' -prune -print0)
    fi
    sign_application
    codesign --verify --deep --strict --verbose=2 "$APP_ROOT"
    if [ "$SIGNING_MODE" = "development" ]; then
        EXPECTED_TEAM_IDENTIFIER="$(python3 -c \
            'import json,sys; print(json.load(open(sys.argv[1], encoding="utf-8"))["team_identifier"])' \
            "$SIGNING_SUMMARY")"
        ACTUAL_TEAM_IDENTIFIER="$(codesign -dvvv "$APP_ROOT" 2>&1 | sed -n 's/^TeamIdentifier=//p' | head -n 1)"
        [ -n "$ACTUAL_TEAM_IDENTIFIER" ] || fail "Signed App contains no TeamIdentifier."
        [ "$ACTUAL_TEAM_IDENTIFIER" = "$EXPECTED_TEAM_IDENTIFIER" ] || \
            fail "Signing certificate and provisioning profile use different teams."
    fi
}
# //// /签名嵌套代码和主 App ////

# //// 在临时目录签名并重新打包 IPA [@x380kkm 2026-07-23] ////
main() {
    parse_arguments "$@"
    if [ "$(uname -s)" != "Darwin" ]; then
        fail "This operation requires macOS and Xcode command line tools."
    fi
    command -v codesign >/dev/null
    command -v ditto >/dev/null
    command -v security >/dev/null
    command -v shasum >/dev/null
    command -v python3 >/dev/null
    test -x /usr/libexec/PlistBuddy

    WORK_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/starpoint-ios-sign.XXXXXX")"
    trap 'test -n "${WORK_ROOT:-}" && test -d "$WORK_ROOT" && rm -rf "$WORK_ROOT"' EXIT
    ARCHIVE_ROOT="$WORK_ROOT/archive"
    PROFILE_PLIST="$WORK_ROOT/profile.plist"
    ENTITLEMENTS_PLIST="$WORK_ROOT/entitlements.plist"
    mkdir -p "$ARCHIVE_ROOT"
    ditto -x -k "$INPUT_IPA" "$ARCHIVE_ROOT"

    shopt -s nullglob
    applications=("$ARCHIVE_ROOT"/Payload/*.app)
    shopt -u nullglob
    [ "${#applications[@]}" -eq 1 ] || fail "IPA must contain exactly one root App."
    APP_ROOT="${applications[0]}"
    /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" "$APP_ROOT/Info.plist"

    if [ "$SIGNING_MODE" = "development" ]; then
        prepare_development_signing
    else
        prepare_adhoc_signing
    fi
    sign_application_tree

    SIGNED_IPA="$WORK_ROOT/signed.ipa"
    (
        cd "$ARCHIVE_ROOT"
        ditto -c -k --sequesterRsrc --keepParent Payload "$SIGNED_IPA"
    )
    mkdir -p "$(dirname "$OUTPUT_IPA")"
    rm -f "$OUTPUT_IPA" "$OUTPUT_IPA.sha256" "$OUTPUT_IPA.signing.json"
    cp "$SIGNED_IPA" "$OUTPUT_IPA"
    shasum -a 256 "$OUTPUT_IPA" > "$OUTPUT_IPA.sha256"
    if [ -n "$SIGNING_SUMMARY" ]; then
        cp "$SIGNING_SUMMARY" "$OUTPUT_IPA.signing.json"
    fi
    printf '%s\n' "$OUTPUT_IPA"
}

main "$@"
# //// /在临时目录签名并重新打包 IPA ////
