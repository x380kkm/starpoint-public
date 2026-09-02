// audience: internal | external
// # cn-leiting-payment
// CN 雷霆支付兼容接口返回未完成订单状态, 并且只接受有效 viewer session.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { SessionType } from "../../data/types"
import { getSession } from "../../data/wdfpData"
import { generateDataHeaders } from "../../utils"

interface CnQueryUnfinishedOrderBody {
    viewer_id?: number
}

const routes = async (fastify: FastifyInstance) => {
    // //// 返回当前账号的未完成订单状态 [@x380kkm 2026-07-22] ////
    fastify.post("/query_unfinish_order", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = (request.body ?? {}) as CnQueryUnfinishedOrderBody
        const viewerId = Number(body.viewer_id)
        if (!Number.isInteger(viewerId) || viewerId <= 0) {
            return reply.status(400).send({ error: "invalid_viewer_id" })
        }

        const session = await getSession(String(viewerId))
        if (session === null || session.type !== SessionType.VIEWER) {
            return reply.status(400).send({ error: "invalid_viewer_session" })
        }

        return reply.header("content-type", "application/x-msgpack").send({
            data_headers: generateDataHeaders({ viewer_id: viewerId }),
            data: { order_id: "" },
        })
    })
    // //// /返回当前账号的未完成订单状态 ////
}

export default routes
