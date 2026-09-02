// audience: internal | external
// # cn-tool
// CN 注册接口把一个设备绑定到一个账号, 并签发 viewer session.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import {
    generateViewerIdSession,
    getAccountFromIdpIdSync,
    getPlayerFromAccountIdSync,
    insertAccount,
    insertDefaultPlayerSync,
    updateAccount,
} from "../../data/wdfpData"
import { generateDataHeaders, generateIdpAlias } from "../../utils"

interface CnSignupBody {
    device_id: number
    channelNo?: string
}

interface GetHeaderResponseBody {
    viewer_id: number
}

function generateLoginToken(): string {
    const chars = "abcdefghijklmnopqrstuvwxyz0123456789"
    let token = ""
    for (let index = 0; index < 32; index += 1) token += chars[Math.floor(Math.random() * chars.length)]
    return token
}

function sendMsgpack(reply: FastifyReply, data: unknown): void {
    reply.header("content-type", "application/x-msgpack").status(200).send({
        data_headers: generateDataHeaders(),
        data,
    })
}

const routes = async (fastify: FastifyInstance) => {
    fastify.post("/get_header_response", (request: FastifyRequest, reply: FastifyReply) => {
        const body = (request.body ?? {}) as Partial<GetHeaderResponseBody>
        return reply.header("content-type", "application/x-msgpack").send({
            data_headers: generateDataHeaders({ viewer_id: body.viewer_id }),
            data: [],
        })
    })

    fastify.post("/auth", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, {})
    })

    fastify.post("/signup", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = (request.body ?? {}) as Partial<CnSignupBody>
        const deviceId = Number(body.device_id)
        if (!Number.isInteger(deviceId) || deviceId <= 0) {
            return reply.status(400).send({ error: "invalid_device_id" })
        }

        const idpId = `cn:${deviceId}`
        const idpAlias = generateIdpAlias("wf_cn", String(deviceId), "android")
        let account = getAccountFromIdpIdSync(idpId)
        const newAccount = account === null
        if (account === null) {
            account = await insertAccount({
                appId: "wf_cn",
                idpAlias,
                idpCode: "leiting",
                idpId,
                status: "normal",
            })
            insertDefaultPlayerSync(account.id)
        } else {
            if (account.appId !== "wf_cn" || account.idpAlias !== idpAlias) {
                return reply.status(409).send({ error: "device_binding_conflict" })
            }
            if (getPlayerFromAccountIdSync(account.id) === null) insertDefaultPlayerSync(account.id)
            account = await updateAccount({ id: account.id, lastLoginTime: new Date() })
        }

        const viewerSession = await generateViewerIdSession(account.id)
        const viewerId = Number(viewerSession.token)
        const udid = typeof request.headers.udid === "string" ? request.headers.udid : "unknown"
        return reply.header("content-type", "application/x-msgpack").send({
            data_headers: generateDataHeaders({ viewer_id: viewerId, udid }),
            data: {
                login_token: generateLoginToken(),
                newAccount: newAccount ? 1 : 0,
                roleName: `Player${account.id}`,
                accountName: `Player${account.id}`,
                sign: "dummy_sign",
                createDate: account.regTime.toISOString(),
                serverName: "StarPoint CN",
                serverId: "1",
            },
        })
    })

    fastify.post("/check_social_link_enable", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, { enable: false })
    })

    fastify.post("/check_enable_gift", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, { enable_gift: true })
    })

    fastify.post("/contact_active", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, { enable_customer_service: false })
    })

    fastify.post("/custom_notify", async (_request: FastifyRequest, reply: FastifyReply) => {
        sendMsgpack(reply, {})
    })
}

export default routes
