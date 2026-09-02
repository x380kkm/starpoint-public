import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify";
import { SessionType } from "../../data/types";
import {
    addPlayerMissionCountersSync,
    getPlayerClearedRegularMissionListSync,
    getPlayerFromAccountIdSync,
    getPlayerMissionCountersSync,
    getSession,
} from "../../data/wdfpData";
import { getPlayerMailsSync } from "../../data/playerMails";
import { generateDataHeaders } from "../../utils";

interface GetMissionProgressBody {
    api_count: number,
    viewer_id: number,
    category_list: {
        category: number
    }[]
}

interface UpdateMissionProgressBody {
    viewer_id: number,
    api_count: number,
    mission_param_list: {
        progress_value: number,
        mission_pattern: string
    }[]
}

const knownMissionDefinitions: Record<string, { mission_category_id: number, mission_id: number, mission_reward_id: number }> = {
    home_tap_town_character_count: {
        mission_category_id: 5,
        mission_id: 49000,
        mission_reward_id: 49000001,
    },
}

function getPlayerForViewer(viewerId: number) {
    return getSession(viewerId.toString()).then(session => {
        if (session === null || session.type !== SessionType.VIEWER) return null
        return getPlayerFromAccountIdSync(session.accountId)
    })
}

function getMissionProgressList(
    playerId: number,
    categories: number[] | undefined,
) {
    const requestedCategories = new Set((categories ?? []).filter(Number.isInteger))
    const counters = getPlayerMissionCountersSync(playerId)
    const progress = [] as { mission_category: number, mission_id: number, progress_value: number, stage: number }[]

    for (const definition of Object.values(knownMissionDefinitions)) {
        if (requestedCategories.size > 0 && !requestedCategories.has(definition.mission_category_id)) continue
        progress.push({
            mission_category: definition.mission_category_id,
            mission_id: definition.mission_id,
            progress_value: counters.home_tap_town_character_count ?? 0,
            stage: 1,
        })
    }

    if (requestedCategories.size === 0 || requestedCategories.has(1)) {
        const cleared = getPlayerClearedRegularMissionListSync(playerId)
        for (const [missionId, stage] of Object.entries(cleared)) {
            if (progress.some(item => item.mission_category === 1 && item.mission_id === Number(missionId))) continue
            progress.push({
                mission_category: 1,
                mission_id: Number(missionId),
                progress_value: 0,
                stage: Math.max(1, Number(stage)),
            })
        }
    }

    return progress
}

const routes = async (fastify: FastifyInstance) => {
    fastify.post("/get_mission_progress", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as GetMissionProgressBody

        const viewerId = body.viewer_id
        if (!viewerId || isNaN(viewerId)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body."
        })

        const player = await getPlayerForViewer(viewerId)
        if (player === null) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id."
        })

        if (body.category_list !== undefined && !Array.isArray(body.category_list)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid mission category list."
        })

        const categories = Array.isArray(body.category_list)
            ? body.category_list.map(item => Number(item.category))
            : undefined

        reply.header("content-type", "application/x-msgpack")
        return reply.status(200).send({
            "data_headers": generateDataHeaders({
                viewer_id: viewerId
            }),
            "data": {
                "mission_progress_list": getMissionProgressList(player.id, categories),
                "mail_arrived": getPlayerMailsSync(player.id, 1, 1).total > 0,
            }
        })
    })

    fastify.post("/update_mission_progress", async (request: FastifyRequest, reply: FastifyReply) => {
        const body = request.body as UpdateMissionProgressBody

        const viewerId = body.viewer_id
        if (!viewerId || isNaN(viewerId)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid request body."
        })

        const player = await getPlayerForViewer(viewerId)
        if (player === null) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid viewer id."
        })

        if (!Array.isArray(body.mission_param_list)) return reply.status(400).send({
            "error": "Bad Request",
            "message": "Invalid mission parameter list."
        })

        const counters: Record<string, number> = {}
        const completed = [] as { mission_category_id: number, mission_id: number, mission_reward_id: number }[]
        const previousCounters = getPlayerMissionCountersSync(player.id)
        for (const item of body.mission_param_list) {
            const pattern = typeof item?.mission_pattern === "string" ? item.mission_pattern.trim() : ""
            const value = Number(item?.progress_value)
            if (pattern.length === 0 || !Number.isSafeInteger(value) || value < 0) return reply.status(400).send({
                "error": "Bad Request",
                "message": "Invalid mission parameter."
            })
            counters[pattern] = (counters[pattern] ?? 0) + value
        }
        for (const [pattern, value] of Object.entries(counters)) {
            const definition = knownMissionDefinitions[pattern]
            if (definition && (previousCounters[pattern] ?? 0) === 0 && value > 0) completed.push(definition)
        }
        addPlayerMissionCountersSync(player.id, counters)

        reply.header("content-type", "application/x-msgpack")
        return reply.status(200).send({
            "data_headers": generateDataHeaders({
                viewer_id: viewerId
            }),
            "data": {
                "mission_info": completed,
                "degree_list": completed.map(mission => ({ viewer_id: viewerId, degree_id: mission.mission_id })),
                "mail_arrived": getPlayerMailsSync(player.id, 1, 1).total > 0,
            }
        })
    })
}

export default routes;
