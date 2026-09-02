// audience: internal | external
// # cn-version-check
// CN 客户端先读取版本描述, 再连接游戏 API.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"

const CN_API_HOST = process.env.CN_API_HOST
const CN_API_SCHEME = process.env.CN_API_SCHEME ?? "http"

//// 根据请求主机生成 CN 版本描述 [@x380kkm 2026-07-21] ////
function getVersionData(requestHost: string): string {
    const apiPath = CN_API_HOST ?? requestHost
    return [
        "// StarPoint CN compatibility endpoint",
        JSON.stringify({ default: { apiScheme: CN_API_SCHEME, apiPath } }),
    ].join("\r\n")
}
//// /根据请求主机生成 CN 版本描述 ////

const routes = async (fastify: FastifyInstance) => {
    fastify.get("/shijtswy/version/client_release_android.dis", async (request: FastifyRequest, reply: FastifyReply) => {
        return reply.type("text/plain; charset=utf-8").send(getVersionData(request.headers.host ?? "localhost:8001"))
    })

    fastify.get("/shijtswy/version/client_release_ios.dis", async (request: FastifyRequest, reply: FastifyReply) => {
        return reply.type("text/plain; charset=utf-8").send(getVersionData(request.headers.host ?? "localhost:8001"))
    })
}

export default routes
