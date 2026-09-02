# audience: external
# # build-ios-simulator-harness
# 该脚本构建只包含个人服务 Framework 的 iOS Simulator App. 输出使用临时 ad-hoc 签名.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
HARNESS_ROOT="$SCRIPT_DIR/DiagnosticHarness"
OUTPUT_ROOT="${1:-$REPOSITORY_ROOT/build/ios-simulator}"
FRAMEWORK_OUTPUT="$OUTPUT_ROOT/framework"
FRAMEWORK_SOURCE="$FRAMEWORK_OUTPUT/PersonalServiceBootstrap.framework"
APP_ROOT="$OUTPUT_ROOT/PersonalServiceDiagnostic.app"
DIAGNOSTIC_CDN_ROOT="${STARPOINT_IOS_DIAGNOSTIC_CDN_ROOT:-}"

# //// 验证 Simulator 构建环境 [@x380kkm 2026-07-22] ////
if [ "$(uname -s)" != "Darwin" ]; then
    echo "This build requires macOS and Xcode." >&2
    exit 1
fi
command -v codesign >/dev/null
command -v xcrun >/dev/null
SDK_PATH="$(xcrun --sdk iphonesimulator --show-sdk-path)"
# //// /验证 Simulator 构建环境 ////

if [ "$(uname -m)" = "arm64" ]; then
    CLANG_TARGET="arm64-apple-ios12.0-simulator"
else
    CLANG_TARGET="x86_64-apple-ios12.0-simulator"
fi

# //// 构建 Simulator Framework [@x380kkm 2026-07-22] ////
STARPOINT_IOS_SDK=iphonesimulator bash "$SCRIPT_DIR/build-framework.sh" "$FRAMEWORK_OUTPUT"
# //// /构建 Simulator Framework ////

# //// 链接并签名最小 Simulator App [@x380kkm 2026-07-22] ////
rm -rf "$APP_ROOT"
mkdir -p "$APP_ROOT/Frameworks"
xcrun --sdk iphonesimulator clang \
    -DSTARPOINT_SELF_DIAGNOSTIC=1 \
    -fobjc-arc \
    -Wall \
    -Wextra \
    -Werror \
    -target "$CLANG_TARGET" \
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
cp -R "$FRAMEWORK_SOURCE" "$APP_ROOT/Frameworks/"
if [ -n "$DIAGNOSTIC_CDN_ROOT" ]; then
    if [ ! -d "$DIAGNOSTIC_CDN_ROOT" ] || \
       [ -z "$(find "$DIAGNOSTIC_CDN_ROOT" -type f -print -quit)" ]; then
        echo "STARPOINT_IOS_DIAGNOSTIC_CDN_ROOT must be a non-empty directory." >&2
        exit 1
    fi
    cp -R "$DIAGNOSTIC_CDN_ROOT" "$APP_ROOT/StarpointCNCDN"
    /usr/libexec/PlistBuddy -c "Add :StarpointCNCDNBundlePath string StarpointCNCDN" \
        "$APP_ROOT/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :StarpointCNCDNBundleMode string direct" \
        "$APP_ROOT/Info.plist"
fi
codesign --force --sign - --timestamp=none "$APP_ROOT/Frameworks/PersonalServiceBootstrap.framework"
codesign --force --sign - --timestamp=none "$APP_ROOT"
xcrun --sdk iphonesimulator otool -L "$APP_ROOT/PersonalServiceDiagnostic" | \
    grep -F '@executable_path/Frameworks/PersonalServiceBootstrap.framework/PersonalServiceBootstrap'
printf '%s\n' "$APP_ROOT"
# //// /链接并签名最小 Simulator App ////
