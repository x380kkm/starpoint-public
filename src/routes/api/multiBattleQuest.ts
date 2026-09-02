// audience: external
// # multi-battle-quest-routes
// 此模块实现房间创建, 选择, 准备, 开始, 一次性结算状态, 解散和超时后的 COM 队友回填.
// summon 按当前大厅人数返回 HTTP 队友并暂存同一选择; 客户端 lobby Summon 命令发布 Mates 名单.
// select_room 和 prepare 返回同进程 TCP 会话端点, TCP 握手继续验证房间成员身份.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { managementStore } from "../../control/management"
import { getAccountPlayers, getSession } from "../../data/wdfpData"
import { Session } from "../../data/types"
import { MatchmakingRoom, matchmakingStore } from "../../multiplayer/matchmakingStore"
import { selectNpcFillSelections } from "../../multiplayer/npcMate"
import { getMultiplayerSessionEndpoint } from "../../multiplayer/sessionConfig"
import { stageCnLobbyMates } from "../../multiplayer/sessionServer"
import { generateDataHeaders, getServerDate } from "../../utils"
import { deleteActiveQuest, insertActiveQuest } from "./singleBattleQuest"

interface ViewerBody {
    viewer_id?: number
}

interface CreateRoomBody extends ViewerBody {
    quest_id?: number
    party_id?: number
    category?: number
}

interface GetRoomsBody extends ViewerBody {
    category_id?: number
}

interface RoomBody extends ViewerBody {
    room_number?: string
    quest_id?: number
    category?: number
    category_id?: number
    party_id?: number
}

interface StartRoomBody extends RoomBody {
    play_id?: string
}

interface AuthenticatedViewer {
    viewerId: number
    session: Session
}

// //// 验证 viewer session 和正整数请求字段 [@x380kkm 2026-07-22] ////
async function authenticateViewer(request: FastifyRequest, reply: FastifyReply): Promise<AuthenticatedViewer | null> {
    const body = (request.body ?? {}) as ViewerBody
    const viewerId = Number(body.viewer_id)
    if (!Number.isInteger(viewerId) || viewerId <= 0) {
        reply.status(400).send({ error: "invalid_viewer_id" })
        return null
    }
    const session = await getSession(viewerId.toString())
    if (session === null) {
        reply.status(400).send({ error: "invalid_viewer_id" })
        return null
    }
    return { viewerId, session }
}

function requirePositiveInteger(value: unknown, field: string): number {
    const parsed = Number(value)
    if (!Number.isInteger(parsed) || parsed <= 0) throw new Error(`${field} must be a positive integer.`)
    return parsed
}

function requireRoomNumber(value: unknown): string {
    if (typeof value !== "string" || !/^\d{6}$/.test(value)) throw new Error("room_number must contain 6 digits.")
    return value
}
// //// /验证 viewer session 和正整数请求字段 ////

// //// 为房间内真人成员建立一次性结算状态 [@x380kkm 2026-07-24] ////
async function getRoomPlayerIds(room: MatchmakingRoom): Promise<number[]> {
    const playerIds: number[] = []
    for (const participant of room.participants.values()) {
        const playerId = (await getAccountPlayers(participant.accountId))[0]
        if (playerId === undefined) throw new Error(`account ${participant.accountId} has no player.`)
        playerIds.push(playerId)
    }
    return playerIds
}

function stageRoomBattleFinishStates(room: MatchmakingRoom, playerIds: number[], playId: string): void {
    for (const playerId of playerIds) {
        insertActiveQuest(playerId, {
            questId: room.questId,
            playId,
            category: room.categoryId,
            useBossBoostPoint: false,
            useBoostPoint: false,
            isAutoStartMode: false,
        })
    }
}
// //// /为房间内真人成员建立一次性结算状态 ////

// //// 发送客户端使用的 MessagePack 响应 [@x380kkm 2026-07-22] ////
function sendData(reply: FastifyReply, viewerId: number, data: unknown) {
    reply.header("content-type", "application/x-msgpack")
    reply.status(200)
    return { data_headers: generateDataHeaders({ viewer_id: viewerId }), data }
}
// //// /发送客户端使用的 MessagePack 响应 ////

// //// 生成客户端连接多人 TCP 会话所需字段 [@x380kkm 2026-07-22] ////
function serializeRoomConnection(room: MatchmakingRoom, request: FastifyRequest) {
    const endpoint = getMultiplayerSessionEndpoint(request.hostname)
    return {
        application_update_url: "",
        category_id: room.categoryId,
        host_entry_time: Math.floor(room.createdAt / 1000),
        ip_address: endpoint.publicHost,
        port: endpoint.port,
        quest_id: room.questId,
        raising_state: room.raisingState,
        room_number: room.roomNumber,
        room_sequence: room.roomSequence,
        share_room_options: 0,
        is_pickup: null,
    }
}
// //// /生成客户端连接多人 TCP 会话所需字段 ////

// //// 注册多人房间 HTTP 状态机 [@x380kkm 2026-07-22] ////
const registerMultiBattleQuestRoutes = async (fastify: FastifyInstance) => {
    fastify.post("/create_room", async (request, reply) => {
        const viewer = await authenticateViewer(request, reply)
        if (viewer === null) return
        try {
            const body = request.body as CreateRoomBody
            const room = matchmakingStore.createRoom({
                hostAccountId: viewer.session.accountId,
                hostViewerId: viewer.viewerId,
                categoryId: requirePositiveInteger(body.category, "category"),
                questId: requirePositiveInteger(body.quest_id, "quest_id"),
                partyId: requirePositiveInteger(body.party_id, "party_id"),
            })
            const host = request.headers.host ?? "localhost"
            return sendData(reply, viewer.viewerId, {
                room_number: room.roomNumber,
                room_url: `http://${host}/api/index.php/multi_invitation/join?k=${room.invitationToken}`,
            })
        } catch (error) {
            return reply.status(400).send({ error: "invalid_room", message: (error as Error).message })
        }
    })

    fastify.post("/get_rooms", async (request, reply) => {
        const viewer = await authenticateViewer(request, reply)
        if (viewer === null) return
        try {
            const categoryId = requirePositiveInteger((request.body as GetRoomsBody).category_id, "category_id")
            const rooms = matchmakingStore.listRooms(categoryId).map((room) => ({
                room_number: room.roomNumber,
                category_id: room.categoryId,
                quest_id: room.questId,
                host_entry_time: Math.floor(room.createdAt / 1000),
                raising_state: room.raisingState,
                is_pickup: false,
            }))
            return sendData(reply, viewer.viewerId, { rooms })
        } catch (error) {
            return reply.status(400).send({ error: "invalid_room_query", message: (error as Error).message })
        }
    })

    fastify.post("/select_room", async (request, reply) => {
        const viewer = await authenticateViewer(request, reply)
        if (viewer === null) return
        try {
            const body = request.body as RoomBody
            const roomNumber = requireRoomNumber(body.room_number)
            const room = matchmakingStore.getRoom(roomNumber)
            if (room === null) {
                return sendData(reply, viewer.viewerId, {
                    application_update_url: "",
                    category_id: 0,
                    host_entry_time: 0,
                    ip_address: "",
                    port: 0,
                    quest_id: 0,
                    raising_state: 9,
                    room_number: roomNumber,
                    room_sequence: 0,
                    share_room_options: 0,
                    is_pickup: null,
                })
            }
            const questId = requirePositiveInteger(body.quest_id, "quest_id")
            const categoryId = requirePositiveInteger(body.category, "category")
            const partyId = requirePositiveInteger(body.party_id, "party_id")
            if (room.questId !== questId || room.categoryId !== categoryId) {
                return reply.status(409).send({ error: "room_mismatch" })
            }
            const joined = matchmakingStore.joinRoom(roomNumber, {
                accountId: viewer.session.accountId,
                viewerId: viewer.viewerId,
                partyId,
            })
            if (joined === null) return reply.status(409).send({ error: "room_full" })
            return sendData(reply, viewer.viewerId, serializeRoomConnection(joined, request))
        } catch (error) {
            return reply.status(400).send({ error: "invalid_room", message: (error as Error).message })
        }
    })

    fastify.post("/prepare", async (request, reply) => {
        const viewer = await authenticateViewer(request, reply)
        if (viewer === null) return
        try {
            const body = request.body as RoomBody
            const room = matchmakingStore.getRoom(requireRoomNumber(body.room_number))
            if (room === null) return reply.status(404).send({ error: "room_not_found" })
            if (room.questId !== requirePositiveInteger(body.quest_id, "quest_id") || room.categoryId !== requirePositiveInteger(body.category, "category")) {
                return reply.status(409).send({ error: "room_mismatch" })
            }
            const participant = matchmakingStore.getParticipant(room.roomNumber, viewer.viewerId)
            if (participant === null || participant.accountId !== viewer.session.accountId) {
                return reply.status(403).send({ error: "room_access_denied" })
            }
            return sendData(reply, viewer.viewerId, serializeRoomConnection(room, request))
        } catch (error) {
            return reply.status(400).send({ error: "invalid_room", message: (error as Error).message })
        }
    })

    fastify.post("/start", async (request, reply) => {
        const viewer = await authenticateViewer(request, reply)
        if (viewer === null) return
        try {
            const body = request.body as StartRoomBody
            const roomNumber = requireRoomNumber(body.room_number)
            const room = matchmakingStore.getRoom(roomNumber)
            if (room === null) return reply.status(404).send({ error: "room_not_found" })
            const questId = requirePositiveInteger(body.quest_id, "quest_id")
            const categoryId = requirePositiveInteger(body.category, "category")
            requirePositiveInteger(body.party_id, "party_id")
            if (typeof body.play_id !== "string" || body.play_id.length === 0) throw new Error("play_id must be a non-empty string.")
            if (room.questId !== questId || room.categoryId !== categoryId) {
                return reply.status(409).send({ error: "room_mismatch" })
            }
            if (room.hostAccountId !== viewer.session.accountId || room.hostViewerId !== viewer.viewerId) {
                return reply.status(403).send({ error: "room_access_denied" })
            }
            const playerIds = room.battleStarted ? [] : await getRoomPlayerIds(room)
            const startResult = matchmakingStore.startBattle(roomNumber, {
                accountId: viewer.session.accountId,
                viewerId: viewer.viewerId,
            }, body.play_id)
            if (startResult === null) {
                return reply.status(403).send({ error: "room_access_denied" })
            }
            if (startResult.startedNow) stageRoomBattleFinishStates(startResult.room, playerIds, body.play_id)
            return sendData(reply, viewer.viewerId, { is_multi: "multi", play_id: body.play_id })
        } catch (error) {
            return reply.status(400).send({ error: "invalid_battle_start", message: (error as Error).message })
        }
    })

    fastify.post("/summon", async (request, reply) => {
        const viewer = await authenticateViewer(request, reply)
        if (viewer === null) return
        try {
            const body = request.body as RoomBody
            const roomNumber = requireRoomNumber(body.room_number)
            const room = matchmakingStore.getOwnedRoom(roomNumber, viewer.session.accountId)
            if (room === null) return reply.status(404).send({ error: "room_not_found" })
            const categoryId = requirePositiveInteger(body.category_id, "category_id")
            const questId = requirePositiveInteger(body.quest_id, "quest_id")
            if (room.categoryId !== categoryId || room.questId !== questId) {
                return reply.status(409).send({ error: "room_mismatch" })
            }
            const selections = selectNpcFillSelections(await managementStore.load(), {
                categoryId,
                questId,
                roomCreatedAt: room.createdAt,
                currentTime: getServerDate().getTime(),
            })
            const stagedSelections = stageCnLobbyMates(roomNumber, room.roomSequence, selections)
            const data: Record<string, unknown> = {}
            if (stagedSelections[0] !== undefined) data.mate1 = stagedSelections[0].clientMate
            if (stagedSelections[1] !== undefined) data.mate2 = stagedSelections[1].clientMate
            return sendData(reply, viewer.viewerId, data)
        } catch (error) {
            return reply.status(409).send({ error: "npc_fill_failed", message: (error as Error).message })
        }
    })

    fastify.post("/disband_room", async (request, reply) => {
        const viewer = await authenticateViewer(request, reply)
        if (viewer === null) return
        try {
            const roomNumber = requireRoomNumber((request.body as RoomBody).room_number)
            const room = matchmakingStore.getOwnedRoom(roomNumber, viewer.session.accountId)
            if (room === null) return reply.status(404).send({ error: "room_not_found" })
            const playerIds = await getRoomPlayerIds(room)
            if (!matchmakingStore.disbandRoom(roomNumber, viewer.session.accountId)) {
                return reply.status(404).send({ error: "room_not_found" })
            }
            for (const playerId of playerIds) deleteActiveQuest(playerId)
            return sendData(reply, viewer.viewerId, [])
        } catch (error) {
            return reply.status(400).send({ error: "invalid_room", message: (error as Error).message })
        }
    })
}
// //// /注册多人房间 HTTP 状态机 ////

export default registerMultiBattleQuestRoutes
