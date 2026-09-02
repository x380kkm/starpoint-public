// audience: external
// # multiplayer-session-config
// 此模块提供 CN 多人 TCP 监听地址和客户端可见端点.
// SESSION_PORT 同时用于监听和 HTTP 房间响应, 取值必须是有效 TCP 端口.

interface MultiplayerSessionEndpoint {
    publicHost: string
    port: number
}

// //// 读取多人 TCP 监听配置 [@x380kkm 2026-07-22] ////
export function getMultiplayerSessionPort(): number {
    const port = Number(process.env.SESSION_PORT ?? "8003")
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("SESSION_PORT must be an integer from 1 to 65535.")
    return port
}

export function getMultiplayerSessionListenHost(): string {
    return process.env.SESSION_HOST ?? process.env.LISTEN_HOST ?? "localhost"
}
// //// /读取多人 TCP 监听配置 ////

// //// 生成返回给客户端的多人 TCP 端点 [@x380kkm 2026-07-22] ////
export function getMultiplayerSessionEndpoint(requestHostname: string): MultiplayerSessionEndpoint {
    return {
        publicHost: process.env.SESSION_PUBLIC_HOST ?? requestHostname,
        port: getMultiplayerSessionPort(),
    }
}
// //// /生成返回给客户端的多人 TCP 端点 ////
