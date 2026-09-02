// audience: internal
// # mail-api
// 此模块实现 Global 和 CN 客户端共用的邮件查询与奖励领取协议.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { getAccountPlayers, getPlayerSync, getSession } from "../../data/wdfpData"
import {
    claimAllPlayerMailsSync,
    claimPlayerMailSync,
    getPlayerMailsSync,
    PlayerMail,
    PlayerMailClaimResult,
} from "../../data/playerMails"
import { clientSerializeDate } from "../../data/utils"
import { generateDataHeaders } from "../../utils"

const UNRECEIVED_MAIL_TIME = "0000-00-00 00:00:00"

interface IndexBody {
    viewer_id: number | string
    current_page?: number | string
}

interface ReceiveBody {
    viewer_id: number | string
    mail_id: number | string
}

interface ReceiveAllBody {
    viewer_id: number | string
    mail_ids?: (number | string)[]
}

function parsePositiveInteger(value: unknown, field: string): number {
    const parsed = Number(value)
    if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${field} must be a positive integer.`)
    return parsed
}

async function resolvePlayerId(viewerValue: unknown): Promise<{ viewerId: number; playerId: number } | null> {
    const viewerId = parsePositiveInteger(viewerValue, "viewer_id")
    const session = await getSession(viewerId.toString())
    if (session === null) return null
    const playerId = (await getAccountPlayers(session.accountId))[0]
    return playerId === undefined ? null : { viewerId, playerId }
}

interface MailKindData {
    type: number
    typeId: number | null
    number: number
}

// //// 将本地奖励映射为 CN 邮件索引中的单一奖励类型 [@x380kkm 2026-07-24] ////
function resolveMailKind(mail: PlayerMail): MailKindData {
    const item = Object.entries(mail.rewards.itemList)[0]
    if (item !== undefined) return { type: 1, typeId: Number(item[0]), number: item[1] }
    const equipment = Object.entries(mail.rewards.equipmentList)[0]
    if (equipment !== undefined) return { type: 6, typeId: Number(equipment[0]), number: equipment[1] }
    if (mail.rewards.characterList.length > 0) {
        return { type: 5, typeId: mail.rewards.characterList[0], number: mail.rewards.characterList.length }
    }
    if (mail.rewards.freeVmoney > 0) return { type: 4, typeId: null, number: mail.rewards.freeVmoney }
    if (mail.rewards.vmoney > 0) return { type: 3, typeId: null, number: mail.rewards.vmoney }
    if (mail.rewards.freeMana > 0) return { type: 8, typeId: null, number: mail.rewards.freeMana }
    if (mail.rewards.expPool > 0) return { type: 9, typeId: null, number: mail.rewards.expPool }
    return { type: 8, typeId: null, number: Math.max(1, mail.rewards.paidMana) }
}
// //// /将本地奖励映射为 CN 邮件索引中的单一奖励类型 ////

// //// 序列化 CN 邮件索引对象 [@x380kkm 2026-07-24] ////
function serializeMail(mail: PlayerMail): Record<string, unknown> {
    const kind = resolveMailKind(mail)
    return {
        create_time: clientSerializeDate(new Date(mail.createdAt * 1000)),
        description: mail.body,
        id: mail.id,
        number: kind.number,
        reason_id: 999998,
        receive_time: mail.receivedAt === null ? UNRECEIVED_MAIL_TIME : clientSerializeDate(new Date(mail.receivedAt * 1000)),
        reward_limit_time: mail.expiresAt === null ? null : clientSerializeDate(new Date(mail.expiresAt * 1000)),
        reward_period_limited: mail.expiresAt !== null,
        subject: mail.title,
        type: kind.type,
        type_id: kind.typeId,
    }
}
// //// /序列化 CN 邮件索引对象 ////

// //// 序列化 CN 邮件领取中的玩家资源 [@x380kkm 2026-07-24] ////
function serializeClaimUserInfo(playerId: number): Record<string, unknown> {
    const player = getPlayerSync(playerId)
    if (player === null) throw new Error("player not found.")
    return {
        free_mana: player.freeMana,
        paid_mana: player.paidMana,
        free_vmoney: player.freeVmoney,
        vmoney: player.vmoney,
        exp_pool: player.expPool,
        exp_pooled_time: Math.floor(player.expPooledTime.getTime() / 1000),
    }
}
// //// /序列化 CN 邮件领取中的玩家资源 ////

// //// 序列化单封 CN 邮件领取响应 [@x380kkm 2026-07-24] ////
function serializeReceiveData(playerId: number, result: PlayerMailClaimResult): Record<string, unknown> {
    return {
        user_info: serializeClaimUserInfo(playerId),
        character_list: result.characterList,
        equipment_list: result.equipmentList,
        item_list: result.itemList,
        total_count: result.remainingCount,
        dispose_expired_mail: false,
        auto_sale_expired_mail: false,
        mail_arrived: result.remainingCount > 0,
    }
}
// //// /序列化单封 CN 邮件领取响应 ////

// //// 序列化 CN 邮件批量领取响应 [@x380kkm 2026-07-24] ////
function serializeReceiveAllData(playerId: number, result: PlayerMailClaimResult): Record<string, unknown> {
    return {
        user_info: serializeClaimUserInfo(playerId),
        character_list: result.characterList,
        equipment_list: result.equipmentList,
        item_list: result.itemList,
        mail_ids: result.mailIds,
        already_mail_count: 0,
        auto_sale_expired_mail_count: 0,
        deleted_mail_count: 0,
        dispose_expired_mail_count: 0,
        max_overed_mail_count: 0,
        outdated_mail_count: result.expiredMailCount,
        total_count: result.remainingCount,
        mail_arrived: result.remainingCount > 0,
    }
}
// //// /序列化 CN 邮件批量领取响应 ////

function sendMessagePack(reply: FastifyReply, viewerId: number, data: Record<string, unknown>): FastifyReply {
    reply.header("content-type", "application/x-msgpack")
    return reply.status(200).send({
        data_headers: generateDataHeaders({ viewer_id: viewerId }),
        data,
    })
}

// //// 注册邮件查询和领取接口 [@x380kkm 2026-07-24] ////
const routes = async (fastify: FastifyInstance) => {
    fastify.post("/index", async (request: FastifyRequest, reply: FastifyReply) => {
        try {
            const body = request.body as IndexBody
            const resolved = await resolvePlayerId(body?.viewer_id)
            if (resolved === null) return reply.status(400).send({ error: "Bad Request", message: "Invalid viewer id." })
            const page = Math.max(1, Number(body.current_page ?? 1) || 1)
            const result = getPlayerMailsSync(resolved.playerId, page)
            return sendMessagePack(reply, resolved.viewerId, {
                mail: result.mails.map(serializeMail),
                total_count: result.total,
            })
        } catch (error) {
            return reply.status(400).send({ error: "Bad Request", message: (error as Error).message })
        }
    })

    fastify.post("/receive", async (request: FastifyRequest, reply: FastifyReply) => {
        try {
            const body = request.body as ReceiveBody
            const resolved = await resolvePlayerId(body?.viewer_id)
            if (resolved === null) return reply.status(400).send({ error: "Bad Request", message: "Invalid viewer id." })
            const result = claimPlayerMailSync(resolved.playerId, parsePositiveInteger(body?.mail_id, "mail_id"))
            return sendMessagePack(reply, resolved.viewerId, serializeReceiveData(resolved.playerId, result))
        } catch (error) {
            return reply.status(400).send({ error: "Bad Request", message: (error as Error).message })
        }
    })

    fastify.post("/receive_all", async (request: FastifyRequest, reply: FastifyReply) => {
        try {
            const body = request.body as ReceiveAllBody
            const resolved = await resolvePlayerId(body?.viewer_id)
            if (resolved === null) return reply.status(400).send({ error: "Bad Request", message: "Invalid viewer id." })
            if (body.mail_ids !== undefined && (!Array.isArray(body.mail_ids) || body.mail_ids.length > 100)) {
                return reply.status(400).send({ error: "Bad Request", message: "mail_ids must contain at most 100 entries." })
            }
            const mailIds = body.mail_ids === undefined ? undefined : body.mail_ids.map((mailId) => parsePositiveInteger(mailId, "mail_id"))
            const result = claimAllPlayerMailsSync(resolved.playerId, mailIds)
            return sendMessagePack(reply, resolved.viewerId, serializeReceiveAllData(resolved.playerId, result))
        } catch (error) {
            return reply.status(400).send({ error: "Bad Request", message: (error as Error).message })
        }
    })
}
// //// /注册邮件查询和领取接口 ////

export default routes
