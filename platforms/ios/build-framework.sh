# audience: external
# # build-ios-framework
# 该脚本在 macOS 和 Xcode 环境中构建 iPhone 或 iOS Simulator Framework. 输出不包含签名身份或设备资料.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
CORE_ROOT="$REPOSITORY_ROOT/core/personal-service"
BOOTSTRAP_ROOT="$SCRIPT_DIR/PersonalServiceBootstrap"
OUTPUT_ROOT="${1:-$REPOSITORY_ROOT/build/ios}"
FRAMEWORK_ROOT="$OUTPUT_ROOT/PersonalServiceBootstrap.framework"
SDK_NAME="${STARPOINT_IOS_SDK:-iphoneos}"
MINIMUM_IOS_VERSION="12.0"

# //// 验证 Apple 构建环境 [@x380kkm 2026-07-22] ////
if [ "$(uname -s)" != "Darwin" ]; then
    echo "This build requires macOS and Xcode." >&2
    exit 1
fi
command -v cargo >/dev/null
command -v rustup >/dev/null
command -v xcrun >/dev/null
case "$SDK_NAME" in
    iphoneos)
        ARCHITECTURE="arm64"
        CLANG_TARGET="arm64-apple-ios${MINIMUM_IOS_VERSION}"
        RUST_TARGET="aarch64-apple-ios"
        ;;
    iphonesimulator)
        if [ "$(uname -m)" = "arm64" ]; then
            ARCHITECTURE="arm64"
            CLANG_TARGET="arm64-apple-ios${MINIMUM_IOS_VERSION}-simulator"
            RUST_TARGET="aarch64-apple-ios-sim"
        else
            ARCHITECTURE="x86_64"
            CLANG_TARGET="x86_64-apple-ios${MINIMUM_IOS_VERSION}-simulator"
            RUST_TARGET="x86_64-apple-ios"
        fi
        ;;
    *)
        echo "Unsupported Apple SDK: $SDK_NAME" >&2
        exit 1
        ;;
esac
SDK_PATH="$(xcrun --sdk "$SDK_NAME" --show-sdk-path)"
CARGO_TARGET_ROOT="${CARGO_TARGET_DIR:-$CORE_ROOT/target}"
RUST_LIBRARY="$CARGO_TARGET_ROOT/$RUST_TARGET/release/libstarpoint_personal_service.a"
# //// /验证 Apple 构建环境 ////

# //// 构建 iPhone arm64 原生核心 [@x380kkm 2026-07-22] ////
export IPHONEOS_DEPLOYMENT_TARGET="$MINIMUM_IOS_VERSION"
rustup target add "$RUST_TARGET"
cargo build --manifest-path "$CORE_ROOT/Cargo.toml" --target "$RUST_TARGET" --release --lib
# //// /构建 iPhone arm64 原生核心 ////

# //// 链接可注入的动态 Framework [@x380kkm 2026-07-24] ////
rm -rf "$FRAMEWORK_ROOT"
mkdir -p "$FRAMEWORK_ROOT/Headers"
xcrun --sdk "$SDK_NAME" clang \
    -fobjc-arc \
    -Wall \
    -Wextra \
    -Werror \
    -target "$CLANG_TARGET" \
    -isysroot "$SDK_PATH" \
    -I "$CORE_ROOT/include" \
    -c "$BOOTSTRAP_ROOT/StarpointPersonalServiceBootstrap.m" \
    -o "$OUTPUT_ROOT/StarpointPersonalServiceBootstrap.o"
xcrun --sdk "$SDK_NAME" clang \
    -fobjc-arc \
    -Wall \
    -Wextra \
    -Werror \
    -target "$CLANG_TARGET" \
    -isysroot "$SDK_PATH" \
    -c "$BOOTSTRAP_ROOT/StarpointCNTitleSceneHook.m" \
    -o "$OUTPUT_ROOT/StarpointCNTitleSceneHook.o"
xcrun --sdk "$SDK_NAME" clang \
    -dynamiclib \
    -target "$CLANG_TARGET" \
    -isysroot "$SDK_PATH" \
    -Wl,-install_name,@executable_path/Frameworks/PersonalServiceBootstrap.framework/PersonalServiceBootstrap \
    "$OUTPUT_ROOT/StarpointPersonalServiceBootstrap.o" \
    "$OUTPUT_ROOT/StarpointCNTitleSceneHook.o" \
    "$RUST_LIBRARY" \
    -framework Foundation \
    -framework UIKit \
    -framework SafariServices \
    -framework Security \
    -framework SystemConfiguration \
    -liconv \
    -lresolv \
    -lsqlite3 \
    -lz \
    -o "$FRAMEWORK_ROOT/PersonalServiceBootstrap"
cp "$BOOTSTRAP_ROOT/Info.plist" "$FRAMEWORK_ROOT/Info.plist"
cp "$CORE_ROOT/include/starpoint_personal_service.h" "$FRAMEWORK_ROOT/Headers/"
cp "$BOOTSTRAP_ROOT/StarpointPersonalServiceBootstrap.h" "$FRAMEWORK_ROOT/Headers/"
chmod 755 "$FRAMEWORK_ROOT/PersonalServiceBootstrap"
xcrun --sdk "$SDK_NAME" lipo "$FRAMEWORK_ROOT/PersonalServiceBootstrap" -verify_arch "$ARCHITECTURE"
printf '%s\n' "$FRAMEWORK_ROOT"
# //// /链接可注入的动态 Framework ////
