# audience: external
# # build-android-device-diagnostic
# 该脚本在 Linux 或 macOS Android SDK/NDK 环境中构建 arm64 个人服务诊断 APK.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPOSITORY_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
CORE_ROOT="$REPOSITORY_ROOT/core/personal-service"
HARNESS_ROOT="$SCRIPT_DIR/DiagnosticHarness"
BOOTSTRAP_ROOT="$SCRIPT_DIR/PersonalServiceBootstrap"
OUTPUT_ROOT="${1:-$REPOSITORY_ROOT/build/android-device-diagnostic}"
SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
BUILD_TOOLS_VERSION="${STARPOINT_ANDROID_BUILD_TOOLS_VERSION:-35.0.0}"
NDK_VERSION="${STARPOINT_ANDROID_NDK_VERSION:-27.2.12479018}"
NDK_ROOT="${ANDROID_NDK_ROOT:-$SDK_ROOT/ndk/$NDK_VERSION}"
PLATFORM_API="${STARPOINT_ANDROID_PLATFORM_API:-35}"
MINIMUM_API="${STARPOINT_ANDROID_MINIMUM_API:-26}"
RUST_TARGET="aarch64-linux-android"
PACKAGE_NAME="dev.starpoint.personalservice"

# //// 验证 Android 构建环境 [@x380kkm 2026-07-23] ////
case "$(uname -s)" in
    Linux) NDK_HOST="linux-x86_64" ;;
    Darwin) NDK_HOST="darwin-x86_64" ;;
    *)
        echo "Android diagnostic build requires Linux or macOS." >&2
        exit 1
        ;;
esac
if [ -z "$SDK_ROOT" ]; then
    echo "ANDROID_SDK_ROOT or ANDROID_HOME is required." >&2
    exit 1
fi
BUILD_TOOLS="$SDK_ROOT/build-tools/$BUILD_TOOLS_VERSION"
ANDROID_JAR="$SDK_ROOT/platforms/android-$PLATFORM_API/android.jar"
TOOLCHAIN="$NDK_ROOT/toolchains/llvm/prebuilt/$NDK_HOST"
CLANG="$TOOLCHAIN/bin/aarch64-linux-android${MINIMUM_API}-clang"
LLVM_AR="$TOOLCHAIN/bin/llvm-ar"
LLVM_NM="$TOOLCHAIN/bin/llvm-nm"
LLVM_READELF="$TOOLCHAIN/bin/llvm-readelf"
AAPT2="$BUILD_TOOLS/aapt2"
D8="$BUILD_TOOLS/d8"
ZIPALIGN="$BUILD_TOOLS/zipalign"
APKSIGNER="$BUILD_TOOLS/apksigner"
for command_name in cargo javac keytool python3 rustup; do
    command -v "$command_name" >/dev/null
done
for required_file in \
    "$ANDROID_JAR" "$CLANG" "$LLVM_AR" "$LLVM_NM" "$LLVM_READELF" \
    "$AAPT2" "$D8" "$ZIPALIGN" "$APKSIGNER"; do
    test -x "$required_file" || test -f "$required_file"
done
# //// /验证 Android 构建环境 ////

# //// 构建 Android arm64 Rust 核心和 JNI 库 [@x380kkm 2026-07-23] ////
rm -rf "$OUTPUT_ROOT"
mkdir -p "$OUTPUT_ROOT/native" "$OUTPUT_ROOT/classes" "$OUTPUT_ROOT/dex"
export CARGO_TARGET_DIR="$OUTPUT_ROOT/cargo-target"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$LLVM_AR"
export CC_aarch64_linux_android="$CLANG"
export AR_aarch64_linux_android="$LLVM_AR"
rustup target add "$RUST_TARGET"
cargo build \
    --locked \
    --release \
    --manifest-path "$CORE_ROOT/Cargo.toml" \
    --target "$RUST_TARGET"
RUST_LIBRARY="$CARGO_TARGET_DIR/$RUST_TARGET/release/libstarpoint_personal_service.a"
JNI_LIBRARY="$OUTPUT_ROOT/native/libstarpoint_android_bridge.so"
"$CLANG" \
    -fPIC \
    -shared \
    -Wall \
    -Wextra \
    -Werror \
    -Wl,--build-id=sha1 \
    -Wl,--gc-sections \
    -Wl,--exclude-libs,ALL \
    -Wl,-z,noexecstack \
    -Wl,-z,relro \
    -Wl,-z,now \
    -I "$CORE_ROOT/include" \
    "$HARNESS_ROOT/native/starpoint_android_bridge.c" \
    "$RUST_LIBRARY" \
    -landroid \
    -latomic \
    -ldl \
    -llog \
    -lm \
    -lunwind \
    -o "$JNI_LIBRARY"
"$LLVM_READELF" -h "$JNI_LIBRARY" | grep -F 'AArch64'
JNI_SYMBOLS="$("$LLVM_NM" -D --defined-only "$JNI_LIBRARY" | awk '{print $NF}')"
for required_symbol in \
    JNI_OnLoad \
    Java_dev_starpoint_personalservice_DiagnosticActivity_nativeStart \
    Java_dev_starpoint_personalservice_DiagnosticActivity_nativeGetPort \
    Java_dev_starpoint_personalservice_DiagnosticActivity_nativeIsRunning \
    Java_dev_starpoint_personalservice_DiagnosticActivity_nativeCopyManagementToken \
    Java_dev_starpoint_personalservice_DiagnosticActivity_nativeFlush \
    Java_dev_starpoint_personalservice_DiagnosticActivity_nativeStop \
    Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeStart \
    Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeGetPort \
    Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeIsRunning \
    Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeCopyManagementToken \
    Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeFlush \
    Java_dev_starpoint_personalservice_PersonalServiceBootstrap_nativeStop; do
    grep -Fx "$required_symbol" <<< "$JNI_SYMBOLS"
done
# //// /构建 Android arm64 Rust 核心和 JNI 库 ////

# //// 编译 Java Activity 和 DEX [@x380kkm 2026-07-23] ////
JAVA_SOURCES=()
for java_root in "$HARNESS_ROOT/src" "$BOOTSTRAP_ROOT/src"; do
    while IFS= read -r source_path; do
        JAVA_SOURCES+=("$source_path")
    done < <(find "$java_root" -name '*.java' -type f | sort)
done
test "${#JAVA_SOURCES[@]}" -gt 0
javac \
    --release 8 \
    -classpath "$ANDROID_JAR" \
    -d "$OUTPUT_ROOT/classes" \
    "${JAVA_SOURCES[@]}"
JAVA_CLASSES=()
while IFS= read -r class_path; do
    JAVA_CLASSES+=("$class_path")
done < <(find "$OUTPUT_ROOT/classes" -name '*.class' -type f | sort)
test "${#JAVA_CLASSES[@]}" -gt 0
"$D8" \
    --min-api "$MINIMUM_API" \
    --lib "$ANDROID_JAR" \
    --output "$OUTPUT_ROOT/dex" \
    "${JAVA_CLASSES[@]}"
test -f "$OUTPUT_ROOT/dex/classes.dex"
# //// /编译 Java Activity 和 DEX ////

# //// 组装, 对齐并签名诊断 APK [@x380kkm 2026-07-23] ////
BASE_APK="$OUTPUT_ROOT/diagnostic-base.apk"
COMPILED_RESOURCES="$OUTPUT_ROOT/diagnostic-resources.zip"
UNALIGNED_APK="$OUTPUT_ROOT/PersonalServiceDiagnostic-unsigned-unaligned.apk"
UNSIGNED_APK="$OUTPUT_ROOT/PersonalServiceDiagnostic-unsigned.apk"
SIGNED_APK="$OUTPUT_ROOT/PersonalServiceDiagnostic-debug.apk"
"$AAPT2" compile \
    --dir "$HARNESS_ROOT/res" \
    -o "$COMPILED_RESOURCES"
"$AAPT2" link \
    -I "$ANDROID_JAR" \
    -R "$COMPILED_RESOURCES" \
    --manifest "$HARNESS_ROOT/AndroidManifest.xml" \
    --min-sdk-version "$MINIMUM_API" \
    --target-sdk-version "$PLATFORM_API" \
    --version-code 1 \
    --version-name 0.1.0 \
    -o "$BASE_APK"
python3 "$SCRIPT_DIR/package_diagnostic_apk.py" \
    --base "$BASE_APK" \
    --dex "$OUTPUT_ROOT/dex/classes.dex" \
    --native-library "$JNI_LIBRARY" \
    --output "$UNALIGNED_APK"
"$ZIPALIGN" -f -p 4 "$UNALIGNED_APK" "$UNSIGNED_APK"
if "$APKSIGNER" verify "$UNSIGNED_APK" >/dev/null 2>&1; then
    echo "Unsigned diagnostic APK unexpectedly contains a valid signature." >&2
    exit 1
fi
KEYSTORE="${STARPOINT_ANDROID_KEYSTORE:-$OUTPUT_ROOT/debug.keystore}"
KEYSTORE_PASSWORD="${STARPOINT_ANDROID_KEYSTORE_PASSWORD:-android}"
KEY_ALIAS="${STARPOINT_ANDROID_KEY_ALIAS:-androiddebugkey}"
KEY_PASSWORD="${STARPOINT_ANDROID_KEY_PASSWORD:-$KEYSTORE_PASSWORD}"
if [ ! -f "$KEYSTORE" ]; then
    keytool -genkeypair \
        -noprompt \
        -keystore "$KEYSTORE" \
        -storepass "$KEYSTORE_PASSWORD" \
        -keypass "$KEY_PASSWORD" \
        -alias "$KEY_ALIAS" \
        -keyalg RSA \
        -keysize 2048 \
        -validity 10000 \
        -dname 'CN=Starpoint Android Diagnostic,O=Starpoint,C=CN'
fi
"$APKSIGNER" sign \
    --ks "$KEYSTORE" \
    --ks-pass "pass:$KEYSTORE_PASSWORD" \
    --ks-key-alias "$KEY_ALIAS" \
    --key-pass "pass:$KEY_PASSWORD" \
    --out "$SIGNED_APK" \
    "$UNSIGNED_APK"
"$APKSIGNER" verify --verbose "$SIGNED_APK"
"$ZIPALIGN" -c -p 4 "$SIGNED_APK"
"$AAPT2" dump badging "$SIGNED_APK" | grep -F "package: name='$PACKAGE_NAME'"
"$AAPT2" dump resources "$SIGNED_APK" | grep -F 'xml/network_security_config'
python3 "$SCRIPT_DIR/verify_diagnostic_apk.py" \
    --apk "$SIGNED_APK" \
    --output "$SIGNED_APK.inventory.json"
if command -v sha256sum >/dev/null; then
    sha256sum "$UNSIGNED_APK" > "$UNSIGNED_APK.sha256"
    sha256sum "$SIGNED_APK" > "$SIGNED_APK.sha256"
else
    shasum -a 256 "$UNSIGNED_APK" > "$UNSIGNED_APK.sha256"
    shasum -a 256 "$SIGNED_APK" > "$SIGNED_APK.sha256"
fi
printf '%s\n%s\n' "$UNSIGNED_APK" "$SIGNED_APK"
# //// /组装, 对齐并签名诊断 APK ////
