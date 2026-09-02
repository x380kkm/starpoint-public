// audience: external
// # cn-battle-protocol
// 此模块解析 CN 2.1.125 cooperation_battle 的 NUL JSON 业务帧.
// wire index 以客户端 TypePackerResource2 的枚举表为准.

export type CnBattleAction =
    | { kind: "sceneReady" }
    | { kind: "finalize" }
    | { kind: "heartbeat" }
    | { kind: "lineSpeedWarning", latency: number }
    | { kind: "measurement", frameCount: number, clientTime: number }
    | { kind: "send", targetConnectionIds: string[], message: unknown[] }
    | { kind: "broadcast", messages: unknown[][] }
    | { kind: "unmodeled" }

function isNonNegativeInteger(value: unknown): value is number {
    return typeof value === "number" && Number.isInteger(value) && value >= 0
}

function isConnectionId(value: unknown): value is string {
    return typeof value === "string" && value.length > 0 && value.length <= 128
}

// //// 解析客户端 battle notify, send 和 broadcast 帧 [@x380kkm 2026-07-24] ////
function readNotifyAction(command: unknown[]): CnBattleAction | null {
    if (command.length === 0 || !Number.isInteger(command[0])) return null
    if (command[0] === 0) return command.length === 1 ? { kind: "sceneReady" } : null
    if (command[0] === 1) return command.length === 1 ? { kind: "finalize" } : null
    if (command[0] === 4) return command.length === 1 ? { kind: "heartbeat" } : null
    if (command[0] === 2) {
        if (command.length !== 3 || !isNonNegativeInteger(command[1])) return null
        if (typeof command[2] !== "number" || !Number.isFinite(command[2])) return null
        return { kind: "measurement", frameCount: command[1], clientTime: command[2] }
    }
    if (command[0] === 3) {
        return command.length === 2 && typeof command[1] === "number" && Number.isFinite(command[1])
            ? { kind: "lineSpeedWarning", latency: command[1] }
            : null
    }
    return { kind: "unmodeled" }
}

function isBroadcastCommand(value: unknown): value is unknown[] {
    if (!Array.isArray(value) || value.length !== 6 || value[0] !== 0) return false
    if (!isNonNegativeInteger(value[1]) || !isNonNegativeInteger(value[2])) return false
    if (!isNonNegativeInteger(value[3]) || !isNonNegativeInteger(value[4])) return false
    return typeof value[5] === "string" || value[5] === null
}

function readSendAction(data: unknown[]): CnBattleAction | null {
    if (!Array.isArray(data[1]) || data[1].length === 0 || data[1].length > 32) return null
    const targetConnectionIds = data[1].filter(isConnectionId)
    if (targetConnectionIds.length !== data[1].length) return null
    const message = data[2]
    if (!Array.isArray(message) || message.length !== 2 || message[0] !== 0 || !Array.isArray(message[1])) return null
    if (message[1].length === 0 || !Number.isInteger(message[1][0])) return null
    return { kind: "send", targetConnectionIds: [...new Set(targetConnectionIds)], message }
}

export function readCnBattleAction(data: unknown): CnBattleAction | null {
    if (!Array.isArray(data) || !Number.isInteger(data[0])) return null
    if (data[0] === 0) return data.length === 2 && Array.isArray(data[1]) ? readNotifyAction(data[1]) : null
    if (data[0] === 1) {
        if (data.length !== 2) return null
        if (!Array.isArray(data[1]) || !data[1].every(isBroadcastCommand)) return null
        return { kind: "broadcast", messages: data[1] }
    }
    if (data[0] === 2) return data.length === 3 ? readSendAction(data) : null
    return { kind: "unmodeled" }
}
// //// /解析客户端 battle notify, send 和 broadcast 帧 ////

// //// 生成 CN battle 服务端帧 [@x380kkm 2026-07-24] ////
export function createCnBattleStartedFrame(): unknown[] {
    return [1, [1]]
}

export function createCnBattleFinalizedFrame(): unknown[] {
    return [1, [2]]
}

export function createCnBattleLeaveFrame(connectionId: string): unknown[] {
    return [1, [0, connectionId]]
}

export function createCnBattleMeasurementFrame(frameCount: number, clientTime: number, serverTime: number): unknown[] {
    return [1, [3, frameCount, clientTime, serverTime]]
}

export function createCnBattleBroadcastFrame(connectionId: string, messages: unknown[][]): unknown[] {
    return [2, connectionId, messages]
}

export function createCnBattleSendFrame(connectionId: string, message: unknown[]): unknown[] {
    return [3, connectionId, message]
}
// //// /生成 CN battle 服务端帧 ////
