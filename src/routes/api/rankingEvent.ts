// audience: internal
// # ranking-event
// 客户端以空 best_record 表示尚未参赛, 非空成绩引用实际存在的角色.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify";
import { getAccountPlayers, getPlayerCharactersSync, getPlayerPartyGroupListSync, getPlayerSingleQuestProgressSync, getSession } from "../../data/wdfpData";
import { generateDataHeaders } from "../../utils";
import { QuestCategory } from "../../lib/types";
import { getCharacterDataSync } from "../../lib/assets";
import { PlayerQuestProgress } from "../../data/types";
import rankingEventSource from "../../../core/personal-service/assets/cn-ranking-rewards.json";

interface ReceiveRewardBody {
    api_count: number,
    viewer_id: number,
    ranking_event_id: number
}

interface GetSummaryBody {
    viewer_id: number,
    ranking_event_id: number,
    quest_kind: number
}

const rankingEvents: Record<string, { quest_ids: number[], reference_time_ms: number }> = rankingEventSource

// //// 从共享竞速目录读取最佳成绩和客户端百分数 [@x380kkm 2026-09-28] ////
function getRankingSummary(
    playerId: number,
    eventId: number
): Object | null {
    const definition = rankingEvents[eventId]
    if (definition === undefined) return null
    const playerQuestData = definition.quest_ids
        .map(questId => getPlayerSingleQuestProgressSync(playerId, QuestCategory.RANKING_EVENT_SINGLE, questId))
        .filter((record): record is PlayerQuestProgress & { bestElapsedTimeMs: number } =>
            record !== null && typeof record.bestElapsedTimeMs === "number" &&
            Number.isFinite(record.bestElapsedTimeMs) && record.bestElapsedTimeMs > 0)
        .sort((left, right) => left.bestElapsedTimeMs - right.bestElapsedTimeMs)[0]
    if (playerQuestData === undefined) return { best_record: null }
    const topTime = definition.reference_time_ms
    const characters = getPlayerCharactersSync(playerId)
    const parties = Object.values(getPlayerPartyGroupListSync(playerId)).flatMap(group => Object.values(group.list))
    const leaderId = [...parties.flatMap(party => party.characterIds), ...Object.keys(characters).map(Number)]
        .find(id => id !== null && id > 0 && getCharacterDataSync(id) !== null) ?? 1

    return {
        "best_record": {
            "elapsed_time_ms": playerQuestData.bestElapsedTimeMs,
            "is_accomplished": true,
            "score": playerQuestData.highScore ?? 0
        },
        "leader_character_evolution_img_level": characters[leaderId]?.evolutionLevel ?? 0,
        "leader_character_id": leaderId,
        "rank_border_top": {
            "elapsed_time_ms": topTime,
            "is_accomplished": true,
            "score": 0
        },
        "rank_percentage": Math.max(0, Math.min(100, 100 * (1 - topTime / playerQuestData.bestElapsedTimeMs)))
    }
}

const routes = async (fastify: FastifyInstance) => {
    fastify.post("/get_summary", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as GetSummaryBody

        const viewerId = body.viewer_id
        const eventId = body.ranking_event_id
        if (isNaN(viewerId) || isNaN(eventId)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body."
        })

        const viewerIdSession = await getSession(viewerId.toString())
        if (!viewerIdSession) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id."
        })

        // get player
        const playerIds = await getAccountPlayers(viewerIdSession.accountId)
        const playerId = playerIds[0]

        if (isNaN(playerId)) return reply.status(500).send({
            "error": "Internal Server Error",
            "message": "No player bound to account."
        })

        // get summary
        const summary = getRankingSummary(playerId, eventId)
        if (summary === null) return reply.status(400).send({
            "error": "Bad Request",
            "message": `Summary could not be generated for '${eventId}' and PlayerId '${playerId}'.`
        })

        reply.header("content-type", "application/x-msgpack")
        return reply.status(200).send({
            "data_headers": generateDataHeaders({
                viewer_id: viewerId
            }),
            "data": summary
        })
    })

    fastify.post("/receive_reward", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as ReceiveRewardBody

        const viewerId = body.viewer_id
        const eventId = body.ranking_event_id
        if (isNaN(viewerId) || isNaN(eventId)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body."
        })

        const viewerIdSession = await getSession(viewerId.toString())
        if (!viewerIdSession) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id."
        })

        // get player id
        const playerIds = await getAccountPlayers(viewerIdSession.accountId)
        const playerId = playerIds[0]

        if (isNaN(playerId)) return reply.status(500).send({
            "error": "Internal Server Error",
            "message": "No player bound to account."
        })

        // get summary
        const summary = getRankingSummary(playerId, eventId)
        if (summary === null) return reply.status(400).send({
            "error": "Bad Request",
            "message": `Summary could not be generated for '${eventId}' and PlayerId '${playerId}'.`
        })

        reply.header("content-type", "application/x-msgpack")
        return reply.status(200).send({
            "data_headers": generateDataHeaders({
                viewer_id: viewerId
            }),
            "data": {
                "status": 1,
                ...summary
            }
        })
    })
}

export default routes;
