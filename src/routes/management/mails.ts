// audience: internal
// # management-mail-routes
// 此模块提供管理员发放和查看玩家邮件的接口.

import { FastifyInstance } from "fastify"
import { createPlayerMailSync, getPlayerMailsSync } from "../../data/playerMails"

interface MailParams {
    playerId: string
}

interface MailPageQuery {
    page?: string
    pageSize?: string
}

interface CreateMailBody {
    playerId?: number
    title?: string
    body?: string
    sender?: string
    rewards?: Record<string, unknown>
    expiresAt?: number | null
}

function parsePositiveInteger(value: string, field: string): number {
    const parsed = Number(value)
    if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error(`${field} must be a positive integer.`)
    return parsed
}

function parsePage(value: string | undefined, fallback: number, maximum: number, field: string): number {
    if (value === undefined) return fallback
    const parsed = Number(value)
    if (!Number.isSafeInteger(parsed) || parsed < 1 || parsed > maximum) throw new Error(`${field} must be between 1 and ${maximum}.`)
    return parsed
}

// //// 注册管理员邮件发放接口 [@x380kkm 2026-07-24] ////
export function registerManagementMailRoutes(fastify: FastifyInstance): void {
    fastify.get<{ Params: MailParams; Querystring: MailPageQuery }>("/mails/:playerId", async (request, reply) => {
        try {
            return getPlayerMailsSync(
                parsePositiveInteger(request.params.playerId, "playerId"),
                parsePage(request.query.page, 1, 1000000, "page"),
                parsePage(request.query.pageSize, 20, 100, "pageSize"),
            )
        } catch (error) {
            return reply.status(400).send({ error: "mail_list_failed", message: (error as Error).message })
        }
    })

    fastify.post("/mails", async (request, reply) => {
        try {
            const body = (request.body ?? {}) as CreateMailBody
            const playerId = body.playerId
            if (typeof playerId !== "number" || !Number.isSafeInteger(playerId) || playerId <= 0 || typeof body.title !== "string" || typeof body.body !== "string" || typeof body.sender !== "string" || body.rewards === undefined) {
                return reply.status(400).send({ error: "invalid_mail" })
            }
            return createPlayerMailSync({
                playerId,
                title: body.title,
                body: body.body,
                sender: body.sender,
                rewards: body.rewards,
                expiresAt: body.expiresAt,
            })
        } catch (error) {
            return reply.status(400).send({ error: "mail_create_failed", message: (error as Error).message })
        }
    })
}
// //// /注册管理员邮件发放接口 ////
