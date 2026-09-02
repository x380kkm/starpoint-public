// audience: internal
// # matchmaking-store
// 此模块保存当前进程内的多人房间, 成员和战斗状态, 并为 HTTP 与 TCP 边界提供身份检查.
// 房间在 30 分钟后删除, 服务重启时不恢复运行中的战斗房间.
// 房主使用相同 play_id 重试开始请求时复用当前战斗, 不同 play_id 不能覆盖运行中的战斗.

import { randomBytes, randomInt } from "crypto"
import type { MultiplayerRoomRepository } from "./sessionDependencies"
import { getServerDate } from "../utils"

export interface CreateMatchmakingRoom {
    hostAccountId: number
    hostViewerId: number
    categoryId: number
    questId: number
    partyId: number
}

export interface MatchmakingParticipant {
    accountId: number
    viewerId: number
    partyId: number
    connectionId: string | null
}

export interface MatchmakingRoom extends CreateMatchmakingRoom {
    roomNumber: string
    roomSequence: number
    invitationToken: string
    createdAt: number
    raisingState: number
    battleStarted: boolean
    playId: string | null
    participants: Map<number, MatchmakingParticipant>
}

export interface MatchmakingBattleStart {
    room: MatchmakingRoom
    startedNow: boolean
}

const ROOM_LIFETIME_MILLISECONDS = 30 * 60 * 1000

export interface MatchmakingClock {
    getCurrentTimeMilliseconds(): number
}

const serverClock: MatchmakingClock = {
    getCurrentTimeMilliseconds: () => getServerDate().getTime(),
}

export class MatchmakingStore implements MultiplayerRoomRepository {
    private readonly rooms = new Map<string, MatchmakingRoom>()

    constructor(private readonly clock: MatchmakingClock = serverClock) {}

    private currentTime(): number {
        return this.clock.getCurrentTimeMilliseconds()
    }

    // //// 删除超过存活时间的房间 [@x380kkm 2026-07-22] ////
    private deleteExpiredRooms(currentTime: number): void {
        for (const [roomNumber, room] of this.rooms) {
            if (currentTime - room.createdAt >= ROOM_LIFETIME_MILLISECONDS) this.rooms.delete(roomNumber)
        }
    }
    // //// /删除超过存活时间的房间 ////

    // //// 为账号创建唯一 6 位房间号 [@x380kkm 2026-07-22] ////
    createRoom(request: CreateMatchmakingRoom, currentTime: number = this.currentTime()): MatchmakingRoom {
        this.deleteExpiredRooms(currentTime)
        for (const [roomNumber, room] of this.rooms) {
            if (room.hostAccountId === request.hostAccountId) this.rooms.delete(roomNumber)
        }

        let roomNumber: string
        do {
            roomNumber = randomInt(100000, 1000000).toString()
        } while (this.rooms.has(roomNumber))
        const room: MatchmakingRoom = {
            ...request,
            roomNumber,
            roomSequence: randomInt(10000000, 100000000),
            invitationToken: randomBytes(24).toString("base64url"),
            createdAt: currentTime,
            raisingState: 1,
            battleStarted: false,
            playId: null,
            participants: new Map([
                [request.hostViewerId, {
                    accountId: request.hostAccountId,
                    viewerId: request.hostViewerId,
                    partyId: request.partyId,
                    connectionId: null,
                }],
            ]),
        }
        this.rooms.set(roomNumber, room)
        return room
    }
    // //// /为账号创建唯一 6 位房间号 ////

    // //// 按分类列出仍可加入的房间 [@x380kkm 2026-07-22] ////
    listRooms(categoryId: number, currentTime: number = this.currentTime()): MatchmakingRoom[] {
        this.deleteExpiredRooms(currentTime)
        return Array.from(this.rooms.values()).filter((room) => room.categoryId === categoryId)
    }
    // //// /按分类列出仍可加入的房间 ////

    // //// 查询账号是否仍属于未开始战斗的房间 [@x380kkm 2026-07-27] ////
    hasAccountParticipantInOpenRoom(accountId: number, currentTime: number = this.currentTime()): boolean {
        this.deleteExpiredRooms(currentTime)
        return Array.from(this.rooms.values()).some((room) => !room.battleStarted &&
            Array.from(room.participants.values()).some((participant) => participant.accountId === accountId),
        )
    }
    // //// /查询账号是否仍属于未开始战斗的房间 ////

    // //// 读取指定房间并检查账号所有权 [@x380kkm 2026-07-22] ////
    getRoom(roomNumber: string, currentTime: number = this.currentTime()): MatchmakingRoom | null {
        this.deleteExpiredRooms(currentTime)
        return this.rooms.get(roomNumber) ?? null
    }

    getOwnedRoom(roomNumber: string, accountId: number, currentTime: number = this.currentTime()): MatchmakingRoom | null {
        const room = this.getRoom(roomNumber, currentTime)
        return room !== null && room.hostAccountId === accountId ? room : null
    }
    // //// /读取指定房间并检查账号所有权 ////

    // //// 把已认证玩家加入房间并保存当前队伍 [@x380kkm 2026-07-22] ////
    joinRoom(roomNumber: string, participant: Omit<MatchmakingParticipant, "connectionId">, currentTime: number = this.currentTime()): MatchmakingRoom | null {
        const room = this.getRoom(roomNumber, currentTime)
        if (room === null) return null
        if (!room.participants.has(participant.viewerId) && room.participants.size >= 3) return null
        const previous = room.participants.get(participant.viewerId)
        room.participants.set(participant.viewerId, {
            ...participant,
            connectionId: previous?.connectionId ?? null,
        })
        return room
    }

    getParticipant(roomNumber: string, viewerId: number, currentTime: number = this.currentTime()): MatchmakingParticipant | null {
        return this.getRoom(roomNumber, currentTime)?.participants.get(viewerId) ?? null
    }

    setParticipantConnection(roomNumber: string, viewerId: number, connectionId: string, currentTime: number = this.currentTime()): MatchmakingParticipant | null {
        const participant = this.getParticipant(roomNumber, viewerId, currentTime)
        if (participant === null) return null
        participant.connectionId = connectionId
        return participant
    }
    // //// /把已认证玩家加入房间并保存当前队伍 ////

    // //// 按 play_id 幂等地把房主房间转换为战斗状态 [@x380kkm 2026-07-24] ////
    startBattle(
        roomNumber: string,
        participantIdentity: Pick<MatchmakingParticipant, "accountId" | "viewerId">,
        playId: string,
        currentTime: number = this.currentTime(),
    ): MatchmakingBattleStart | null {
        const room = this.getRoom(roomNumber, currentTime)
        if (room === null) return null
        if (room.hostAccountId !== participantIdentity.accountId || room.hostViewerId !== participantIdentity.viewerId) return null
        const participant = room.participants.get(participantIdentity.viewerId)
        if (participant === undefined || participant.accountId !== participantIdentity.accountId || participant.connectionId === null) return null
        if (room.battleStarted) return room.playId === playId ? { room, startedNow: false } : null
        room.battleStarted = true
        room.playId = playId
        room.raisingState = 4
        return { room, startedNow: true }
    }
    // //// /按 play_id 幂等地把房主房间转换为战斗状态 ////

    // //// 解散账号拥有的指定房间 [@x380kkm 2026-07-22] ////
    disbandRoom(roomNumber: string, accountId: number, currentTime: number = this.currentTime()): boolean {
        const room = this.getRoom(roomNumber, currentTime)
        if (room === null || room.hostAccountId !== accountId) return false
        return this.rooms.delete(roomNumber)
    }
    // //// /解散账号拥有的指定房间 ////
}

export const matchmakingStore = new MatchmakingStore()
