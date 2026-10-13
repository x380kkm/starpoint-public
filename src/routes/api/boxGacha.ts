// audience: internal
// # box-gacha-routes
// 箱池接口使用持久化抽取事务, 关闭和查询沿用同一箱状态序列化.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { getAccountPlayers, getPlayerBoxGachaSync, getPlayerSync, getSession, updatePlayerBoxGachaSync } from "../../data/wdfpData"
import { BoxGachaError, BoxGachaRequest, executeBoxGachaSync, getAllBoxListSync } from "../../data/boxGacha"
import { generateDataHeaders } from "../../utils"
import { getBoxGachaSync } from "../../lib/assets"

interface BoxRequest {
    box_gacha_id: number
    box_id: number
    viewer_id: number
}

// //// 解析客户端会话对应的玩家 [@x380kkm 2026-09-28] ////
async function playerForViewer(viewerId: number): Promise<number | null> {
    const session = await getSession(viewerId.toString())
    if (!session) return null
    const ids = await getAccountPlayers(session.accountId)
    return Number.isSafeInteger(ids[0]) && getPlayerSync(ids[0]) !== null ? ids[0] : null
}

// //// 返回客户端箱池请求参数错误 [@x380kkm 2026-09-28] ////
function invalidRequest(reply: FastifyReply, message: string) {
    return reply.status(400).send({ error: "Bad Request", message })
}

// //// 检查箱池与玩家标识 [@x380kkm 2026-09-28] ////
function validIds(...ids: number[]): boolean {
    return ids.every(id => Number.isSafeInteger(id) && id > 0)
}

const routes = async (fastify: FastifyInstance) => {
    // //// 关闭指定箱并返回当前箱池状态 [@x380kkm 2026-09-28] ////
    fastify.post("/close", async (request: FastifyRequest, reply: FastifyReply) => {
        const { viewer_id: viewerId, box_gacha_id: poolId, box_id: boxId } = request.body as BoxRequest
        if (!validIds(viewerId, poolId, boxId)) return invalidRequest(reply, "Invalid request body.")
        const playerId = await playerForViewer(viewerId)
        if (playerId === null) return invalidRequest(reply, "Invalid viewer id.")
        const definition = getBoxGachaSync(poolId)
        if (definition === null || definition.boxes[boxId] === undefined) return invalidRequest(reply, "Invalid box gacha id.")
        const state = getPlayerBoxGachaSync(playerId, poolId, boxId)
        if (state === null) return invalidRequest(reply, "Box doesn't exist.")
        if (state.isClosed) return invalidRequest(reply, "Box is already closed.")
        updatePlayerBoxGachaSync(playerId, poolId, { boxId, isClosed: true })
        const current = getAllBoxListSync(playerId, poolId, { [boxId]: definition.boxes[boxId] })
        reply.header("content-type", "application/x-msgpack")
        return reply.send({
            data_headers: generateDataHeaders({ viewer_id: viewerId }),
            data: { all_box_info: [...getAllBoxListSync(playerId, poolId, definition.boxes, boxId), ...current] },
        })
    })

    // //// 验证批量抽取请求并返回已提交的结算响应 [@x380kkm 2026-09-28] ////
    fastify.post("/exec", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as BoxGachaRequest
        if (!validIds(body.viewer_id, body.box_gacha_id, body.box_id)
            || !Number.isSafeInteger(body.number) || body.number < 1 || body.number > 9999
            || typeof body.stop_on_featured_rewards !== "boolean"
            || (body.api_count !== undefined && (!Number.isSafeInteger(body.api_count) || body.api_count < 0))) {
            return invalidRequest(reply, "Invalid request body.")
        }
        const playerId = await playerForViewer(body.viewer_id)
        if (playerId === null) return invalidRequest(reply, "Invalid viewer id.")
        try {
            const result = executeBoxGachaSync(playerId, body)
            reply.header("content-type", "application/x-msgpack")
            return reply.send(result)
        } catch (error) {
            if (error instanceof BoxGachaError) return invalidRequest(reply, error.message)
            throw error
        }
    })

    // //// 返回指定箱池的全部箱状态 [@x380kkm 2026-09-28] ////
    fastify.post("/get_box_list", async (request: FastifyRequest, reply: FastifyReply) => {
        const { viewer_id: viewerId, box_gacha_id: poolId } = request.body as BoxRequest
        if (!validIds(viewerId, poolId)) return invalidRequest(reply, "Invalid request body.")
        const playerId = await playerForViewer(viewerId)
        if (playerId === null) return invalidRequest(reply, "Invalid viewer id.")
        const definition = getBoxGachaSync(poolId)
        if (definition === null) return invalidRequest(reply, "Invalid box gacha id.")
        reply.header("content-type", "application/x-msgpack")
        return reply.send({
            data_headers: generateDataHeaders({ viewer_id: viewerId }),
            data: { all_box_info: getAllBoxListSync(playerId, poolId, definition.boxes) },
        })
    })
}

export default routes
