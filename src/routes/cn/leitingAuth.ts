// audience: internal | external
// # cn-leiting-auth
// CN 防沉迷接口返回停运服务使用的兼容数据结构. CN_LEITING_LOGIN_HEADERS 的实验值只用于客户端响应解析实验.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { generateDataHeaders } from "../../utils"

interface LoginBody {
    userId: string
}

//// 选择 CN 雷霆登录响应头 [@x380kkm 2026-08-03] ////
function createLeitingLoginHeaders(): Record<string, unknown> {
    if (["minimal", "minimal-transport"].includes(process.env.CN_LEITING_LOGIN_HEADERS ?? "")) {
        return { result_code: 1 }
    }
    return generateDataHeaders()
}
//// /选择 CN 雷霆登录响应头 ////

//// 设置 CN 雷霆登录传输头实验值 [@x380kkm 2026-08-03] ////
function setLeitingLoginTransportHeaders(reply: FastifyReply): void {
    if (!["transport", "minimal-transport"].includes(process.env.CN_LEITING_LOGIN_HEADERS ?? "")) return
    reply.header("connection", "keep-alive")
    reply.header("x-result-code", "1")
    reply.header("param", process.env.CN_LEITING_LOGIN_RESPONSE_PARAM ?? "probe-param")
}
//// /设置 CN 雷霆登录传输头实验值 ////

function sendMsgpack(
    reply: FastifyReply,
    data: unknown,
    dataHeaders: Record<string, unknown> = generateDataHeaders(),
): void {
    reply.header("content-type", "application/x-msgpack").status(200).send({
        data_headers: dataHeaders,
        data,
    })
}

const routes = async (fastify: FastifyInstance) => {
    fastify.post("/channels/channel_leiting/leiting_login", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = (request.body ?? {}) as Partial<LoginBody>
        setLeitingLoginTransportHeaders(reply)
        sendMsgpack(
            reply,
            {
                status: "success",
                userId: body.userId ?? "",
                data: { idCard: "123456", age: 18, isGuest: 0, auth: 1 },
                online_server_check: true,
                heart_beat_interval: 240,
            },
            createLeitingLoginHeaders(),
        )
    })

    fastify.post("/channels/channel_leiting/leiting_antiaddiction_login", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, {
            status: 0,
            message: "success",
            data: { onlineTime: 0, limitTime: 999999, usableTime: 999999 },
        })
    })

    fastify.post("/channels/channel_leiting/leiting_antiaddiction_logout", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, {})
    })

    fastify.post("/channels/channel_leiting/leiting_update", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, {})
    })
}

export default routes
