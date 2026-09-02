// audience: external
// # cn-meeting-protocol
// 此模块集中定义 CN 2.1.125 cooperation_room 的 TypePacker 枚举索引和外层帧.
// 客户端使用 Client2Server index 0, 服务端使用 MeetingServer2Client index 1.

export const CN_MEETING_CLIENT_COMMAND = {
    enter: 0,
    bye: 1,
    changeParty: 2,
    ready: 3,
    heartbeat: 4,
    suspend: 5,
    startBattle: 6,
    changeAutoplayMode: 7,
    changeAutoStart: 8,
    log: 9,
    enterComs: 10,
} as const

export const CN_MEETING_SERVER_MESSAGE = {
    welcome: 0,
    mates: 1,
    stateChanged: 2,
    autoplayModeChanged: 3,
    autoStartChanged: 4,
    start: 5,
    startRemainingTime: 9,
    ackHeartbeat: 10,
} as const

export const CN_MEETING_FRAME_INDEX = {
    clientToServer: 0,
    serverToClient: 1,
} as const

// //// 读取客户端 MeetingServer 外层命令 [@x380kkm 2026-07-24] ////
export function readCnMeetingCommand(data: unknown): unknown[] | null {
    if (!Array.isArray(data) || data.length !== 2) return null
    if (data[0] !== CN_MEETING_FRAME_INDEX.clientToServer) return null
    if (!Array.isArray(data[1]) || data[1].length === 0 || !Number.isInteger(data[1][0])) return null
    return data[1]
}
// //// /读取客户端 MeetingServer 外层命令 ////

// //// 创建服务端 MeetingServer 外层消息 [@x380kkm 2026-07-24] ////
export function createCnMeetingServerFrame(command: unknown[]): unknown[] {
    return [CN_MEETING_FRAME_INDEX.serverToClient, command]
}
// //// /创建服务端 MeetingServer 外层消息 ////
