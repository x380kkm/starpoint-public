// audience: internal
// # story-quest-routes
//
// 该模块处理 CN 剧情任务的普通结算和跳过结算. 请求要求有效 viewer 会话和非战斗剧情任务.
// 两个入口共享奖励和进度持久化行为. 重复结算不重复发放奖励.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import {
    getAccountPlayers,
    getPlayerSingleQuestProgressSync,
    getPlayerSync,
    getSession,
    insertPlayerQuestProgressSync,
    runPlayerDataTransactionSync,
    updatePlayerQuestProgressSync,
} from "../../data/wdfpData"
import { getQuestFromCategorySync } from "../../lib/assets"
import { givePlayerRewardSync } from "../../lib/quest"
import { QuestCategory } from "../../lib/types"
import { generateDataHeaders } from "../../utils"

interface FinishBody {
    party_id: number
    quest_id: number
    viewer_id: number
    category: number
    api_count?: number
    retry_count?: number
}

// //// 验证可选请求计数器 [@x380kkm 2026-08-07] ////
function isOptionalNonNegativeInteger(value: unknown) {
    return value === undefined || (
        typeof value === "number"
        && Number.isInteger(value)
        && value >= 0
    )
}
// //// /验证可选请求计数器 ////

// //// 结算 CN 剧情任务并持久化首次奖励 [@x380kkm 2026-08-04] ////
async function finishStoryQuest(request: FastifyRequest, reply: FastifyReply) {
    const body = (request.body ?? {}) as Partial<FinishBody>
    const viewerId = body.viewer_id
    const questSection = body.category
    const questId = body.quest_id
    const partyId = body.party_id
    const apiCount = body.api_count
    const retryCount = body.retry_count
    if (
        typeof viewerId !== "number"
        || !Number.isInteger(viewerId)
        || viewerId <= 0
        || typeof questSection !== "number"
        || !Number.isInteger(questSection)
        || questSection <= 0
        || typeof questId !== "number"
        || !Number.isInteger(questId)
        || questId <= 0
        || typeof partyId !== "number"
        || !Number.isInteger(partyId)
        || partyId < 0
        || !isOptionalNonNegativeInteger(apiCount)
        || !isOptionalNonNegativeInteger(retryCount)
    ) {
        return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body.",
        })
    }

    const viewerIdSession = await getSession(viewerId.toString())
    if (!viewerIdSession) {
        return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id.",
        })
    }

    const playerIds = await getAccountPlayers(viewerIdSession.accountId)
    const playerId = playerIds[0]
    const playerData = Number.isInteger(playerId) ? getPlayerSync(playerId) : null

    if (playerData === null) {
        return reply.status(500).send({
            "error": "Internal Server Error",
            "message": "No player bound to account.",
        })
    }

    const questData = getQuestFromCategorySync(questSection as QuestCategory, questId)
    if (questData === null || ("sPlusReward" in questData)) {
        return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid quest ID provided.",
        })
    }

    const settlement = runPlayerDataTransactionSync(() => {
        const questProgress = getPlayerSingleQuestProgressSync(playerId, questSection, questId)
        const finished = questProgress?.finished ?? false
        const rewardResult = !finished && questData.clearReward !== undefined
            ? givePlayerRewardSync(playerId, questData.clearReward)
            : null

        if (!finished) {
            const progress = { questId, finished: true }
            if (questProgress === null) {
                insertPlayerQuestProgressSync(playerId, questSection, progress)
            } else {
                updatePlayerQuestProgressSync(playerId, questSection, progress)
            }
        }
        return { finished, rewardResult }
    })

    reply.header("content-type", "application/x-msgpack")
    return reply.status(200).send({
        "data_headers": generateDataHeaders({ viewer_id: viewerId }),
        "data": !settlement.finished ? {
            "user_info": {
                "free_vmoney": playerData.freeVmoney + (settlement.rewardResult?.user_info.free_vmoney || 0),
                "free_mana": playerData.freeMana + (settlement.rewardResult?.user_info.free_mana || 0),
            },
            "character_list": settlement.rewardResult?.character_list || [],
            "joined_character_id_list": settlement.rewardResult?.joined_character_id_list || [],
            "equipment_list": settlement.rewardResult?.equipment_list || [],
            "items": settlement.rewardResult?.items || {},
        } : [],
    })
}
// //// /结算 CN 剧情任务并持久化首次奖励 ////

// //// 注册 CN 剧情任务结算入口 [@x380kkm 2026-08-04] ////
const routes = async (fastify: FastifyInstance) => {
    fastify.post("/finish", finishStoryQuest)
    fastify.post("/finish_with_skip", finishStoryQuest)
}
// //// /注册 CN 剧情任务结算入口 ////

export default routes
