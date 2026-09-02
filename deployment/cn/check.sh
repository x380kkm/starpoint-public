#!/usr/bin/env sh
# audience: external
# # cn-health-check
# 此脚本读取 CN 服务的公开健康端点, 用于 Linux 本机部署后的就绪检查.

set -eu

BASE_URL=http://127.0.0.1:8001
TIMEOUT_SECONDS=5

require_option_value() {
    option_name=$1
    option_value=${2-}
    case "$option_value" in
        ""|--*)
            printf '%s requires a non-empty value.\n' "$option_name" >&2
            exit 2
            ;;
    esac
}

# //// 解析 CN 健康检查参数 [@x380kkm 2026-07-24] ////
while [ "$#" -gt 0 ]; do
    case "$1" in
        --base-url)
            require_option_value "$1" "${2-}"
            BASE_URL=$2
            shift 2
            ;;
        --timeout)
            require_option_value "$1" "${2-}"
            TIMEOUT_SECONDS=$2
            shift 2
            ;;
        *)
            printf 'Unknown option: %s\n' "$1" >&2
            exit 2
            ;;
    esac
done

case "$TIMEOUT_SECONDS" in
    ''|*[!0-9]*) printf '%s\n' '--timeout must be an integer.' >&2; exit 2 ;;
esac
if [ "$TIMEOUT_SECONDS" -lt 1 ] || [ "$TIMEOUT_SECONDS" -gt 120 ]; then
    printf '%s\n' '--timeout must be between 1 and 120.' >&2
    exit 2
fi
# //// /解析 CN 健康检查参数 ////

# //// 检查 CN HTTP 和多人端口 [@x380kkm 2026-07-24] ////
HEALTH_URL="${BASE_URL%/}/healthz"
node - "$HEALTH_URL" "$TIMEOUT_SECONDS" <<'NODE'
const [url, timeoutText] = process.argv.slice(2)
const timeout = Number(timeoutText) * 1000
const controller = new AbortController()
const timer = setTimeout(() => controller.abort(), timeout);
(async () => {
  try {
    const response = await fetch(url, { signal: controller.signal })
    if (!response.ok) throw new Error(`health endpoint returned HTTP ${response.status}`)
    const health = await response.json()
    if (health.status !== "ok" || health.service !== "starpoint") throw new Error("invalid health response")
    if (!Number.isInteger(health.httpPort) || !Number.isInteger(health.sessionPort)) throw new Error("health response has invalid ports")
    process.stdout.write(`${JSON.stringify(health)}\n`)
  } finally {
    clearTimeout(timer)
  }
})().catch((error) => {
  console.error(error.message)
  process.exitCode = 1
})
NODE
# //// /检查 CN HTTP 和多人端口 ////
