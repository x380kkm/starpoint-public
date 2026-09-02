# audience: external
# # build-ios-device-diagnostic
# 该脚本构建只包含个人服务 Framework 的 unsigned iPhone 诊断 IPA. 输出必须由开发者身份重新签名.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
HARNESS_ROOT="$SCRIPT_DIR/DiagnosticHarness"
OUTPUT_ROOT="${1:-$REPOSITORY_ROOT/build/ios-device-diagnostic}"
FRAMEWORK_OUTPUT="$OUTPUT_ROOT/framework"
FRAMEWORK_SOURCE="$FRAMEWORK_OUTPUT/PersonalServiceBootstrap.framework"
APP_ROOT="$OUTPUT_ROOT/PersonalServiceDiagnostic.app"
PAYLOAD_ROOT="$OUTPUT_ROOT/Payload"
IPA_PATH="$OUTPUT_ROOT/PersonalServiceDiagnostic-unsigned.ipa"
BUNDLE_ID="${STARPOINT_IOS_BUNDLE_ID:-dev.starpoint.PersonalServiceDiagnostic}"

# //// 验证 iPhone 构建环境 [@x380kkm 2026-07-23] ////
if [ "$(uname -s)" != "Darwin" ]; then
    echo "This build requires macOS and Xcode." >&2
    exit 1
fi
command -v codesign >/dev/null
command -v ditto >/dev/null
command -v shasum >/dev/null
command -v xcrun >/dev/null
test -x /usr/libexec/PlistBuddy
SDK_PATH="$(xcrun --sdk iphoneos --show-sdk-path)"
# //// /验证 iPhone 构建环境 ////

# //// 构建 iPhone Framework [@x380kkm 2026-07-23] ////
STARPOINT_IOS_SDK=iphoneos bash "$SCRIPT_DIR/build-framework.sh" "$FRAMEWORK_OUTPUT"
# //// /构建 iPhone Framework ////

# //// 链接 unsigned iPhone 诊断 App [@x380kkm 2026-07-23] ////
rm -rf "$APP_ROOT" "$PAYLOAD_ROOT"
mkdir -p "$APP_ROOT/Frameworks"
xcrun --sdk iphoneos clang \
    -DSTARPOINT_SELF_DIAGNOSTIC=1 \
    -fobjc-arc \
    -Wall \
    -Wextra \
    -Werror \
    -target arm64-apple-ios12.0 \
    -isysroot "$SDK_PATH" \
    -F "$FRAMEWORK_OUTPUT" \
    "$HARNESS_ROOT/main.m" \
    -framework CoreGraphics \
    -framework Foundation \
    -framework UIKit \
    -framework WebKit \
    -framework PersonalServiceBootstrap \
    -Wl,-rpath,@executable_path/Frameworks \
    -o "$APP_ROOT/PersonalServiceDiagnostic"
cp "$HARNESS_ROOT/Info.plist" "$APP_ROOT/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" "$APP_ROOT/Info.plist"
cp -R "$FRAMEWORK_SOURCE" "$APP_ROOT/Frameworks/"
plutil -lint "$APP_ROOT/Info.plist"
xcrun lipo "$APP_ROOT/PersonalServiceDiagnostic" -verify_arch arm64
xcrun lipo "$APP_ROOT/Frameworks/PersonalServiceBootstrap.framework/PersonalServiceBootstrap" \
    -verify_arch arm64
xcrun otool -L "$APP_ROOT/PersonalServiceDiagnostic" | \
    grep -F '@executable_path/Frameworks/PersonalServiceBootstrap.framework/PersonalServiceBootstrap'
test ! -e "$APP_ROOT/_CodeSignature"
test ! -e "$APP_ROOT/embedded.mobileprovision"
if codesign --verify --strict "$APP_ROOT" >/dev/null 2>&1; then
    echo "Device diagnostic App must remain unsigned." >&2
    exit 1
fi
# //// /链接 unsigned iPhone 诊断 App ////

# //// 打包 unsigned iPhone 诊断 IPA [@x380kkm 2026-07-23] ////
mkdir -p "$PAYLOAD_ROOT"
cp -R "$APP_ROOT" "$PAYLOAD_ROOT/"
rm -f "$IPA_PATH" "$IPA_PATH.sha256"
(
    cd "$OUTPUT_ROOT"
    ditto -c -k --sequesterRsrc --keepParent Payload "$(basename "$IPA_PATH")"
)
shasum -a 256 "$IPA_PATH" > "$IPA_PATH.sha256"
printf '%s\n' "$IPA_PATH"
# //// /打包 unsigned iPhone 诊断 IPA ////
