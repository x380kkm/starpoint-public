#!/usr/bin/env sh
# audience: external
# # personal-service-linux-runner
# 此脚本构建并启动 loopback 个人服务. 服务收到标准输入 stop 或 quit 后提交状态并退出.

set -eu

SCRIPT_DIRECTORY=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPOSITORY_ROOT=$(CDPATH= cd -- "$SCRIPT_DIRECTORY/../.." && pwd)
SERVICE_ROOT="${STARPOINT_PERSONAL_SERVICE_ROOT:-$REPOSITORY_ROOT/data/personal-service}"
CDN_ROOT="${STARPOINT_PERSONAL_SERVICE_CDN_ROOT:-$SERVICE_ROOT/cdn/cn}"
PORT="${PERSONAL_SERVICE_PORT:-17171}"
SKIP_BUILD=0
SHOW_MANAGEMENT_TOKEN=0
LOG_HTTP_ACCESS=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --root)
            [ "$#" -ge 2 ] || { printf '%s\n' '--root requires a path.' >&2; exit 2; }
            SERVICE_ROOT=$2
            shift 2
            ;;
        --port)
            [ "$#" -ge 2 ] || { printf '%s\n' '--port requires a value.' >&2; exit 2; }
            PORT=$2
            shift 2
            ;;
        --cdn-root)
            [ "$#" -ge 2 ] || { printf '%s\n' '--cdn-root requires a path.' >&2; exit 2; }
            CDN_ROOT=$2
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --show-management-token)
            SHOW_MANAGEMENT_TOKEN=1
            shift
            ;;
        --log-http-access)
            LOG_HTTP_ACCESS=1
            shift
            ;;
        *)
            printf 'Unknown option: %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

command -v cargo >/dev/null 2>&1 || { printf '%s\n' 'cargo is required.' >&2; exit 1; }
case "$PORT" in
    ''|*[!0-9]*) printf '%s\n' 'PORT must be an integer.' >&2; exit 2 ;;
esac
if [ "$PORT" -gt 65535 ]; then
    printf '%s\n' 'PORT must be between 0 and 65535.' >&2
    exit 2
fi
mkdir -p "$SERVICE_ROOT"
mkdir -p "$CDN_ROOT"

cd "$REPOSITORY_ROOT"
if [ "$SKIP_BUILD" -eq 0 ]; then
    cargo build --locked --release --manifest-path core/personal-service/Cargo.toml --bin personal-service
fi
BINARY_PATH="$REPOSITORY_ROOT/core/personal-service/target/release/personal-service"
[ -x "$BINARY_PATH" ] || { printf 'Missing personal-service binary: %s\n' "$BINARY_PATH" >&2; exit 1; }
set -- --root "$SERVICE_ROOT" --cdn-root "$CDN_ROOT" --port "$PORT"
if [ "$SHOW_MANAGEMENT_TOKEN" -eq 1 ]; then
    set -- "$@" --show-management-token
fi
if [ "$LOG_HTTP_ACCESS" -eq 1 ]; then
    set -- "$@" --log-http-access
fi
exec "$BINARY_PATH" "$@"
