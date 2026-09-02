// audience: external
// # multiplayer-session-dependencies
// 此模块定义多人 TCP 会话访问时间, 身份和房间状态所需的最小接口.
// 会话传输不依赖游戏数据库结构, 平台入口负责注入具体实现.

export interface MultiplayerSessionRoom {
    hostViewerId: number
    roomSequence: number
    categoryId: number
    questId: number
    battleStarted: boolean
}

export interface MultiplayerSessionParticipant {
    accountId: number
    partyId: number
    connectionId: string | null
}

export interface MultiplayerRoomRepository {
    getRoom(roomNumber: string): MultiplayerSessionRoom | null
    getParticipant(roomNumber: string, viewerId: number): MultiplayerSessionParticipant | null
    setParticipantConnection(roomNumber: string, viewerId: number, connectionId: string): MultiplayerSessionParticipant | null
}

export interface ParticipantIdentityProvider {
    isPlayableParticipant(viewerId: number, accountId: number): Promise<boolean>
}

export interface MultiplayerSessionClock {
    getCurrentTimeMilliseconds(): number
}

export interface MultiplayerSessionDependencies {
    clock: MultiplayerSessionClock
    identityProvider: ParticipantIdentityProvider
    roomRepository: MultiplayerRoomRepository
}
