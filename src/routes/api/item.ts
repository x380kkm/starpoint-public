// audience: internal
// # item-routes
// 道具接口验证 viewer 会话并返回库存, 体力和邮件变更.

import { FastifyInstance } from "fastify"
import { SessionType } from "../../data/types"
import { getAccountPlayers, getSession } from "../../data/wdfpData"
import { ItemUseError, useItemsSync } from "../../data/itemUse"
import { getServerTime } from "../../utils"

// //// 从已验证的玩家会话使用体力道具和自选素材箱 [@x380kkm 2026-09-28] ////
export default async function routes(fastify: FastifyInstance) {
    fastify.post("/use_item", async (request, reply) => {
        const body = request.body as { viewer_id?: number, api_count?: number, items?: unknown } | null
        const viewerId = body?.viewer_id
        if (!Number.isSafeInteger(viewerId) || (viewerId ?? 0) <= 0 ||
            (body?.api_count !== undefined && !Number.isSafeInteger(body.api_count))) {
            return reply.status(400).send({ error: "invalid_request_body" })
        }
        const session = await getSession(String(viewerId))
        if (session === null || session.type !== SessionType.VIEWER) return reply.status(400).send({ error: "invalid_viewer_session" })
        const [playerId] = await getAccountPlayers(session.accountId)
        if (playerId === undefined) return reply.status(400).send({ error: "missing_player" })
        try {
            const response = useItemsSync(playerId, viewerId!, body!.api_count, body!.items, getServerTime())
            return reply.header("content-type", "application/x-msgpack").send(response)
        } catch (error) {
            if (error instanceof ItemUseError) return reply.status(400).send({ error: error.message })
            throw error
        }
    })
}
// //// /从已验证的玩家会话使用体力道具和自选素材箱 ////
