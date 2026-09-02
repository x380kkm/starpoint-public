// audience: internal
// # start-minimal-cn-login-server
// 此入口只为 CN 客户端响应解析实验启用最小登录响应头, 默认服务不受影响.

process.env.LISTEN_HOST = "0.0.0.0"
process.env.LISTEN_PORT = "8001"
process.env.CN_MSGPACK_RESPONSE_ENCODING = ""
process.env.CN_LEITING_LOGIN_HEADERS = "minimal"

await import("../../out/start.js")
