// audience: internal
// # start-raw-cn-server
// 此入口只为协议实验启动原始 MessagePack 响应模式, 不改变默认服务编码.

process.env.LISTEN_HOST = "0.0.0.0"
process.env.LISTEN_PORT = "8001"
process.env.CN_MSGPACK_RESPONSE_ENCODING = "raw"

await import("../../out/start.js")
