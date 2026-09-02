#!/usr/bin/env sh
# audience: external
# # cn-linux-runner
# 此脚本准备 CN CDN, 检查 Linux 运行环境, 安装依赖, 构建项目, 按请求探测本地服务并以前台进程启动 CN 服务.

set -eu

SCRIPT_DIRECTORY=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPOSITORY_ROOT=$(CDPATH= cd -- "$SCRIPT_DIRECTORY/../.." && pwd)
ENVIRONMENT_FILE="$REPOSITORY_ROOT/.env.cn"
CDN_DIRECTORY="$REPOSITORY_ROOT/.cdn"
DOWNLOAD_DIRECTORY=""
SKIP_INSTALL=0
SKIP_CDN_DOWNLOAD=0
KEEP_CDN_PARTS=0
ADOPT_EXISTING_CDN=0
VALIDATE_ONLY=0
HEALTH_CHECK=0

require_option_value() {
    option_name=$1
    option_value=${2-}
    case "$option_value" in
        ""|--*)
            printf '%s requires a non-empty value.\n' "$option_name" >&2
            exit 2
            ;;
        *[![:space:]]*)
            ;;
        *)
            printf '%s requires a non-empty value.\n' "$option_name" >&2
            exit 2
            ;;
    esac
}

# //// 同步部署环境中的 CN CDN 配置 [@x380kkm 2026-07-27] ////
sync_cn_cdn_environment() {
    temporary_file=$(mktemp "$ENVIRONMENT_FILE.tmp.XXXXXX")
    if ! {
        if [ -f "$ENVIRONMENT_FILE" ]; then
            while IFS= read -r line || [ -n "$line" ]; do
                case "$line" in
                    CDN_DIR=*|CN_RES_VERSION=*)
                        ;;
                    *)
                        printf '%s\n' "$line" >> "$temporary_file"
                        ;;
                esac
            done < "$ENVIRONMENT_FILE"
        fi
        printf 'CDN_DIR=%s\n' "$CDN_DIRECTORY" >> "$temporary_file"
        printf 'CN_RES_VERSION=%s\n' "$CN_RES_VERSION" >> "$temporary_file"
        chmod 600 "$temporary_file"
        mv "$temporary_file" "$ENVIRONMENT_FILE"
    }; then
        rm -f "$temporary_file"
        return 1
    fi
}
# //// /同步部署环境中的 CN CDN 配置 ////

# //// 探测本地 CN HTTP 服务健康状态 [@x380kkm 2026-07-24] ////
probe_local_health() {
    node --env-file="$ENVIRONMENT_FILE" -e '
const configuredPort = Number.parseInt(process.env.LISTEN_PORT ?? "", 10);
const port = Number.isNaN(configuredPort) ? 8000 : configuredPort;
const healthUrl = `http://127.0.0.1:${port}/healthz`;
const healthProbeTimeoutMilliseconds = 5000;

(async () => {
    const response = await fetch(healthUrl, { signal: AbortSignal.timeout(healthProbeTimeoutMilliseconds) });
    const body = await response.json();
    const isExpectedService = response.ok
        && body.status === "ok"
        && body.service === "starpoint"
        && body.httpPort === port
        && Number.isInteger(body.sessionPort);
    if (!isExpectedService) {
        throw new Error(`unexpected health response (HTTP ${response.status})`);
    }
    process.stdout.write(healthUrl);
})().catch((error) => {
    console.error(`CN health probe failed for ${healthUrl}: ${error.message}`);
    process.exit(1);
});
'
}
# //// /探测本地 CN HTTP 服务健康状态 ////

while [ "$#" -gt 0 ]; do
    case "$1" in
        --cdn-dir)
            require_option_value "$1" "${2-}"
            CDN_DIRECTORY=$2
            shift 2
            ;;
        --download-dir)
            require_option_value "$1" "${2-}"
            DOWNLOAD_DIRECTORY=$2
            shift 2
            ;;
        --skip-install)
            SKIP_INSTALL=1
            shift
            ;;
        --skip-cdn-download)
            SKIP_CDN_DOWNLOAD=1
            shift
            ;;
        --keep-cdn-parts)
            KEEP_CDN_PARTS=1
            shift
            ;;
        --adopt-existing-cdn)
            ADOPT_EXISTING_CDN=1
            shift
            ;;
        --validate-only)
            VALIDATE_ONLY=1
            shift
            ;;
        --health-check)
            HEALTH_CHECK=1
            shift
            ;;
        *)
            printf 'Unknown option: %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

# //// 检查 Linux CN 服务运行条件 [@x380kkm 2026-07-22] ////
command -v node >/dev/null 2>&1 || { printf 'Node.js 20.6.0 or newer is required.\n' >&2; exit 1; }
command -v npm >/dev/null 2>&1 || { printf 'npm is required.\n' >&2; exit 1; }
command -v tar >/dev/null 2>&1 || { printf 'tar is required.\n' >&2; exit 1; }
command -v mktemp >/dev/null 2>&1 || { printf 'mktemp is required.\n' >&2; exit 1; }
node -e "const [major, minor] = process.versions.node.split('.').map(Number); process.exit(major > 20 || (major === 20 && minor >= 6) ? 0 : 1)" \
    || { printf 'Node.js 20.6.0 or newer is required. Current version: %s\n' "$(node -p process.versions.node)" >&2; exit 1; }
CN_RES_VERSION=$(node -e "const fs = require('node:fs'); process.stdout.write(JSON.parse(fs.readFileSync(process.argv[1])).resVersion)" "$SCRIPT_DIRECTORY/cdn-manifest.json")
CDN_DIRECTORY=$(node -e "process.stdout.write(require('node:path').resolve(process.argv[1]))" "$CDN_DIRECTORY")
if [ -n "$DOWNLOAD_DIRECTORY" ]; then
    DOWNLOAD_DIRECTORY=$(node -e "process.stdout.write(require('node:path').resolve(process.argv[1]))" "$DOWNLOAD_DIRECTORY")
fi
# //// /检查 Linux CN 服务运行条件 ////

# //// 准备 Linux CN CDN [@x380kkm 2026-07-22] ////
set -- "$SCRIPT_DIRECTORY/prepare-cdn.mjs" --cdn-dir "$CDN_DIRECTORY"
if [ -n "$DOWNLOAD_DIRECTORY" ]; then
    set -- "$@" --download-dir "$DOWNLOAD_DIRECTORY"
fi
if [ "$SKIP_CDN_DOWNLOAD" -eq 1 ]; then
    set -- "$@" --skip-download
fi
if [ "$KEEP_CDN_PARTS" -eq 1 ]; then
    set -- "$@" --keep-parts
fi
if [ "$ADOPT_EXISTING_CDN" -eq 1 ]; then
    set -- "$@" --adopt-existing
fi
if [ "$VALIDATE_ONLY" -eq 1 ]; then
    set -- "$@" --validate-existing
fi
node "$@"
if [ "$VALIDATE_ONLY" -eq 1 ]; then
    printf 'CN CDN integrity preflight passed: %s\n' "$CDN_DIRECTORY"
    exit 0
fi
# //// /准备 Linux CN CDN ////

# //// 创建 Linux CN 私有环境文件 [@x380kkm 2026-07-22] ////
if [ ! -f "$ENVIRONMENT_FILE" ]; then
    ADMIN_TOKEN=$(node -e "process.stdout.write(require('node:crypto').randomBytes(32).toString('hex'))")
    ADMIN_PASSWORD=$(node -e "process.stdout.write(require('node:crypto').randomBytes(24).toString('hex'))")
    cat > "$ENVIRONMENT_FILE" <<EOF
LISTEN_HOST=0.0.0.0
LISTEN_PORT=8001
CDN_DIR=$CDN_DIRECTORY
CN_RES_VERSION=$CN_RES_VERSION
MANAGEMENT_ADMIN_TOKEN=$ADMIN_TOKEN
MANAGEMENT_ADMIN_USERNAME=admin
MANAGEMENT_ADMIN_PASSWORD=$ADMIN_PASSWORD
EOF
elif ! grep -q '^MANAGEMENT_ADMIN_PASSWORD=.' "$ENVIRONMENT_FILE"; then
    ADMIN_PASSWORD=$(node -e "process.stdout.write(require('node:crypto').randomBytes(24).toString('hex'))")
    if ! grep -q '^MANAGEMENT_ADMIN_USERNAME=' "$ENVIRONMENT_FILE"; then
        printf 'MANAGEMENT_ADMIN_USERNAME=admin\n' >> "$ENVIRONMENT_FILE"
    fi
    printf 'MANAGEMENT_ADMIN_PASSWORD=%s\n' "$ADMIN_PASSWORD" >> "$ENVIRONMENT_FILE"
fi
sync_cn_cdn_environment
chmod 600 "$ENVIRONMENT_FILE"
# //// /创建 Linux CN 私有环境文件 ////

# //// 构建, 探测并启动 Linux CN 服务 [@x380kkm 2026-07-24] ////
cd "$REPOSITORY_ROOT"
if [ "$SKIP_INSTALL" -eq 0 ]; then
    npm ci
fi
npm run build

if [ "$HEALTH_CHECK" -eq 1 ]; then
    HEALTH_URL=$(probe_local_health)
    printf 'CN local health probe passed: %s\n' "$HEALTH_URL"
    exit 0
fi

if [ "$VALIDATE_ONLY" -eq 1 ]; then
    printf 'CN deployment validation passed.\n'
    exit 0
fi

printf 'CN server: http://127.0.0.1:8001\n'
printf 'Management: http://127.0.0.1:8001/manage\n'
printf 'Management credentials and environment: %s\n' "$ENVIRONMENT_FILE"
CDN_DIR="$CDN_DIRECTORY" CN_RES_VERSION="$CN_RES_VERSION" exec node --env-file="$ENVIRONMENT_FILE" out/start.js
# //// /构建, 探测并启动 Linux CN 服务 ////
