// audience: internal
// # start-cn-login-transport-server
// 此入口只为 CN 雷霆登录传输头实验启用, 默认服务不受影响.

process.env.LISTEN_HOST = "0.0.0.0"
process.env.LISTEN_PORT = "8001"
process.env.CN_MSGPACK_RESPONSE_ENCODING = ""
process.env.CN_LEITING_LOGIN_HEADERS = "transport"
process.env.CN_LEITING_LOGIN_RESPONSE_PARAM ??= "probe-param"

await import("../../out/start.js")
