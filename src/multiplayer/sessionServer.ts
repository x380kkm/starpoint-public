// audience: external
// # multiplayer-session-server
// 此模块接受 CN 客户端的 cooperation_room 和 cooperation_battle TCP 会话.
// 每帧是 UTF-8 JSON, 并以一个 NUL 字节结束; HTTP MessagePack 不进入此边界.
// 大厅会话处理人类成员入场, 队伍与准备状态, 自动设置, COM 入场, 开战, 心跳和退出.
// 战斗会话处理全员 SceneReady 后的开战广播, Finalize, Send, broadcast, Measurement 和异常离开通知; 未建模的合法命令不返回成功.
// 非法会话帧直接关闭连接.
// 服务时间, 身份查询与房间状态由启动入口注入, 会话传输不读取游戏数据库.
// 会话许可绑定房间序列, 新握手清理已解散或房号复用的许可.

import { randomBytes } from "crypto"
import * as net from "net"
import {
    createCnBattleBroadcastFrame,
    createCnBattleFinalizedFrame,
    createCnBattleLeaveFrame,
    createCnBattleMeasurementFrame,
    createCnBattleSendFrame,
    createCnBattleStartedFrame,
    readCnBattleAction,
} from "./cnBattleProtocol"
import {
    CN_MEETING_CLIENT_COMMAND,
    CN_MEETING_SERVER_MESSAGE,
    createCnMeetingServerFrame,
    readCnMeetingCommand,
} from "./cnMeetingProtocol"
import {
    createCnLobbyNpcPlayer,
    type CnLobbyNpcPlayer,
    validateCnLobbyNpcRequestsAndReadNames,
} from "./cnLobbyNpc"
import type { SelectedNpcFillMate } from "./npcMate"
import { getMultiplayerSessionListenHost, getMultiplayerSessionPort } from "./sessionConfig"
import type { MultiplayerSessionClock, MultiplayerSessionDependencies } from "./sessionDependencies"

const MAX_FRAME_BYTES = 4 * 1024 * 1024
const MAX_CN_LOBBY_PLAYERS = 3

const FIRST_HUMAN_PLAYER_ID = 2

interface LobbyPlayer {
    viewerId: number
    playerId: number
    name: string
    rank: number
    degreeId: number
    mainCharacterId: number
    party: Record<string, unknown>
    connectionId: string
    playerRoleKind: number
    isNewbie: boolean
    isHost: boolean
    entryTime: number
    currentPartyId: number
    autoplayMode: boolean
    autoskillMode: number
    autoSpeedLevel: number
    autoStart: boolean
    skillAbilityBehaviorMode: number
    dashBehaviorMode: number
    allowHealFromOtherPlayers: boolean
    state: unknown[]
}

type CnLobbyPlayer = LobbyPlayer | CnLobbyNpcPlayer

interface SessionAdmission {
    roomNumber: string
    roomSequence: number
    questCategory: number
    questId: number
    viewerId: number
    partyId: number
    isHost: boolean
    connectionId: string
    hasCompletedHostEntry: boolean
    hasStartedLobbyBattle: boolean
    sceneReady: boolean
    pendingNpcSelections: SelectedNpcFillMate[] | null
    lobbyPlayer: LobbyPlayer | null
    lobbyRoster: CnLobbyPlayer[] | null
    lobbySocket: net.Socket | null
    battleSocket: net.Socket | null
}

interface ConnectionState {
    buffer: string
    isHandshakePending: boolean
    battleFinalized: boolean
    admission: SessionAdmission | null
    processing: Promise<void>
}

const connectionStates = new Map<net.Socket, ConnectionState>()
const admissions = new Map<string, SessionAdmission>()
let sessionServer: net.Server | null = null
let activeSessionDependencies: MultiplayerSessionDependencies | null = null

function encodeFrame(data: unknown): string {
    return `${JSON.stringify(data)}\0`
}

function sendFrame(socket: net.Socket, data: unknown): void {
    socket.write(encodeFrame(data))
}

// //// 按大厅剩余席位暂存 HTTP summon 选择 [@x380kkm 2026-07-23] ////
export function stageCnLobbyMates(
    roomNumber: string,
    roomSequence: number,
    selections: SelectedNpcFillMate[],
): SelectedNpcFillMate[] {
    if (selections.length === 0) return []
    const dependencies = activeSessionDependencies
    if (dependencies === null) return []
    const room = dependencies.roomRepository.getRoom(roomNumber)
    if (room === null || room.roomSequence !== roomSequence) return []

    const humanPlayerCount = getActiveLobbyAdmissions(roomNumber, roomSequence).length
    const availableNpcSlots = Math.max(0, MAX_CN_LOBBY_PLAYERS - humanPlayerCount)
    const stagedSelections = selections.slice(0, Math.min(MAX_CN_LOBBY_PLAYERS - 1, availableNpcSlots))
    if (stagedSelections.length === 0) return []

    let hasStagedSelections = false
    for (const admission of admissions.values()) {
        if (admission.roomNumber !== roomNumber || admission.roomSequence !== roomSequence) continue
        if (!admission.isHost || admission.lobbyPlayer === null) continue
        if (admission.lobbySocket === null || admission.lobbySocket.destroyed) continue
        admission.pendingNpcSelections = stagedSelections
        hasStagedSelections = true
    }
    return hasStagedSelections ? stagedSelections : []
}
// //// /按大厅剩余席位暂存 HTTP summon 选择 ////

// //// 发布抓包确认的 Mates, Ready 和开战倒计时 [@x380kkm 2026-07-23] ////
function getActiveLobbyAdmissions(roomNumber: string, roomSequence: number): SessionAdmission[] {
    return [...admissions.values()]
        .filter((admission) => (
            admission.roomNumber === roomNumber
            && admission.roomSequence === roomSequence
            && admission.lobbyPlayer !== null
            && admission.lobbySocket !== null
            && !admission.lobbySocket.destroyed
        ))
        .sort((left, right) => Number(right.isHost) - Number(left.isHost))
}

function publishCnLobbyMates(admission: SessionAdmission, requests: unknown, clock: MultiplayerSessionClock): boolean {
    const selections = admission.pendingNpcSelections
    if (selections === null || selections.length === 0) return false

    const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
    const humanPlayers = activeAdmissions.map((item) => item.lobbyPlayer as LobbyPlayer)
    if (humanPlayers.length === 0) return false
    const availableNpcSlots = Math.max(0, MAX_CN_LOBBY_PLAYERS - humanPlayers.length)
    const selectedNpcSelections = selections.slice(0, availableNpcSlots)
    if (selectedNpcSelections.length === 0) return false
    if (!Array.isArray(requests)) return false
    const names = validateCnLobbyNpcRequestsAndReadNames(
        requests.slice(0, selectedNpcSelections.length),
        selectedNpcSelections,
    )
    if (names === null) return false
    const entryTime = clock.getCurrentTimeMilliseconds()
    const npcPlayers = selectedNpcSelections.map((selection, index) => (
        createCnLobbyNpcPlayer(selection, names[index], admission.roomNumber, index + 1, entryTime)
    ))
    const joinedRoster: CnLobbyPlayer[] = [...humanPlayers, ...npcPlayers]
    const readyRoster = joinedRoster.map((player) => ({ ...player, state: [1] }))
    const readyConnectionIds = [...npcPlayers, ...humanPlayers].map((player) => player.connectionId)

    for (const activeAdmission of activeAdmissions) {
        const socket = activeAdmission.lobbySocket as net.Socket
        activeAdmission.pendingNpcSelections = null
        activeAdmission.lobbyRoster = readyRoster
        sendFrame(socket, createCnMeetingServerFrame([CN_MEETING_SERVER_MESSAGE.mates, joinedRoster]))
        for (const connectionId of readyConnectionIds) {
            sendFrame(socket, createCnMeetingServerFrame([CN_MEETING_SERVER_MESSAGE.stateChanged, connectionId, [1]]))
        }
        sendFrame(socket, createCnMeetingServerFrame([CN_MEETING_SERVER_MESSAGE.startRemainingTime, 2]))
    }
    return true
}

function startCnLobbyBattle(admission: SessionAdmission): boolean {
    if (!admission.isHost) return false
    const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
    if (activeAdmissions.length === 0) return false
    const roster = admission.lobbyRoster
        ?? activeAdmissions.map((activeAdmission) => activeAdmission.lobbyPlayer as LobbyPlayer)
    if (roster.some((player) => player.state.length !== 1 || player.state[0] !== 1)) return false
    for (const roomAdmission of admissions.values()) {
        if (roomAdmission.roomNumber !== admission.roomNumber) continue
        if (roomAdmission.roomSequence !== admission.roomSequence) continue
        roomAdmission.hasStartedLobbyBattle = true
        roomAdmission.lobbyRoster = roster
    }
    for (const activeAdmission of activeAdmissions) {
        sendFrame(
            activeAdmission.lobbySocket as net.Socket,
            createCnMeetingServerFrame([CN_MEETING_SERVER_MESSAGE.start, roster]),
        )
    }
    return true
}
// //// /发布抓包确认的 Mates, Ready 和开战倒计时 ////

// //// 处理 CN battle 的开始, 结束, 单播, 广播和测量帧 [@x380kkm 2026-07-24] ////
function broadcastCnBattleMessages(admission: SessionAdmission, messages: unknown[][]): void {
    const frame = createCnBattleBroadcastFrame(admission.connectionId, messages)
    for (const target of admissions.values()) {
        if (target.roomNumber !== admission.roomNumber || target.roomSequence !== admission.roomSequence) continue
        if (target.battleSocket === null || target.battleSocket.destroyed) continue
        sendFrame(target.battleSocket, frame)
    }
}

function sendCnBattleMessage(admission: SessionAdmission, targetConnectionIds: string[], message: unknown[]): void {
    for (const targetConnectionId of targetConnectionIds) {
        const target = admissions.get(targetConnectionId)
        if (target === undefined) continue
        if (target.roomNumber !== admission.roomNumber || target.roomSequence !== admission.roomSequence) continue
        if (target.battleSocket === null || target.battleSocket.destroyed) continue
        sendFrame(target.battleSocket, createCnBattleSendFrame(admission.connectionId, message))
    }
}

// //// 获取同一房间仍连接的 CN battle 成员 [@x380kkm 2026-07-24] ////
function getActiveBattleAdmissions(roomNumber: string, roomSequence: number): SessionAdmission[] {
    return [...admissions.values()].filter((admission) => (
        admission.roomNumber === roomNumber
        && admission.roomSequence === roomSequence
        && admission.battleSocket !== null
        && !admission.battleSocket.destroyed
    ))
}
// //// /获取同一房间仍连接的 CN battle 成员 ////

// //// 通知同一房间的战斗成员异常离开 [@x380kkm 2026-07-24] ////
function broadcastCnBattleLeave(admission: SessionAdmission): void {
    const frame = createCnBattleLeaveFrame(admission.connectionId)
    for (const target of admissions.values()) {
        if (target === admission) continue
        if (target.roomNumber !== admission.roomNumber || target.roomSequence !== admission.roomSequence) continue
        if (target.battleSocket === null || target.battleSocket.destroyed) continue
        sendFrame(target.battleSocket, frame)
    }
}
// //// /通知同一房间的战斗成员异常离开 ////

function handleBattleFrame(
    socket: net.Socket,
    state: ConnectionState,
    data: unknown,
    dependencies: MultiplayerSessionDependencies,
): void {
    const admission = state.admission
    if (admission === null || admission.battleSocket !== socket) return
    const room = dependencies.roomRepository.getRoom(admission.roomNumber)
    if (room === null || room.roomSequence !== admission.roomSequence || !room.battleStarted) {
        socket.end()
        return
    }
    const action = readCnBattleAction(data)
    if (action === null) {
        socket.end()
        return
    }
    switch (action.kind) {
        case "sceneReady":
            admission.sceneReady = true
            const activeAdmissions = getActiveBattleAdmissions(admission.roomNumber, admission.roomSequence)
            if (activeAdmissions.length === 0 || activeAdmissions.some((item) => !item.sceneReady)) return
            const startedFrame = createCnBattleStartedFrame()
            for (const activeAdmission of activeAdmissions) {
                sendFrame(activeAdmission.battleSocket as net.Socket, startedFrame)
            }
            return
        case "finalize":
            state.battleFinalized = true
            sendFrame(socket, createCnBattleFinalizedFrame())
            socket.end()
            return
        case "measurement":
            sendFrame(socket, createCnBattleMeasurementFrame(
                action.frameCount,
                action.clientTime,
                dependencies.clock.getCurrentTimeMilliseconds(),
            ))
            return
        case "send":
            sendCnBattleMessage(admission, action.targetConnectionIds, action.message)
            return
        case "broadcast":
            broadcastCnBattleMessages(admission, action.messages)
            return
        case "heartbeat":
        case "lineSpeedWarning":
        case "unmodeled":
            return
    }
}
// //// /处理 CN battle 的开始, 结束, 单播, 广播和测量帧 ////

function sendHandshakeDenialAndClose(socket: net.Socket, reason: "HANDSHAKE_DENIED" | "DENIED"): void {
    const tag = reason === "HANDSHAKE_DENIED" ? 3 : 1
    socket.end(encodeFrame([tag, reason]))
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value)
}

function readPositiveInteger(value: unknown): number | null {
    return typeof value === "number" && Number.isSafeInteger(value) && value > 0 ? value : null
}

function readNonNegativeInteger(value: unknown): number | null {
    return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null
}

function readRoomNumber(value: unknown): string | null {
    return typeof value === "string" && /^\d{6}$/.test(value) ? value : null
}

function readConnectionId(value: unknown): string | null {
    return typeof value === "string" && /^[a-f0-9]{32}$/.test(value) ? value : null
}

function hasReconnectFlag(value: unknown): boolean {
    return typeof value === "number" && Number.isInteger(value)
}

function removeAdmissionAndCloseSockets(admission: SessionAdmission): void {
    admissions.delete(admission.connectionId)
    if (admission.lobbySocket !== null && !admission.lobbySocket.destroyed) admission.lobbySocket.destroy()
    if (admission.battleSocket !== null && !admission.battleSocket.destroyed) admission.battleSocket.destroy()
}

// //// 清理已解散或房号复用的会话许可 [@x380kkm 2026-07-23] ////
function pruneStaleAdmissions(dependencies: MultiplayerSessionDependencies): void {
    for (const admission of admissions.values()) {
        const room = dependencies.roomRepository.getRoom(admission.roomNumber)
        if (room === null || room.roomSequence !== admission.roomSequence) removeAdmissionAndCloseSockets(admission)
    }
}
// //// /清理已解散或房号复用的会话许可 ////

// //// 验证大厅握手并签发连接 ID [@x380kkm 2026-07-22] ////
async function acceptLobbyHandshake(socket: net.Socket, state: ConnectionState, data: Record<string, unknown>, dependencies: MultiplayerSessionDependencies): Promise<void> {
    const viewerId = readPositiveInteger(data.viewerId)
    const roomNumber = readRoomNumber(data.roomNumber)
    const categoryId = readPositiveInteger(data.questCategory)
    const questId = readPositiveInteger(data.questId)
    if (viewerId === null || roomNumber === null || categoryId === null || questId === null || !hasReconnectFlag(data.reconnected)) {
        sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        return
    }

    const requestedRoom = dependencies.roomRepository.getRoom(roomNumber)
    const requestedParticipant = dependencies.roomRepository.getParticipant(roomNumber, viewerId)
    if (requestedRoom === null || requestedParticipant === null || requestedRoom.categoryId !== categoryId || requestedRoom.questId !== questId) {
        sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        return
    }

    const isPlayableParticipant = await dependencies.identityProvider.isPlayableParticipant(viewerId, requestedParticipant.accountId)
    if (!isPlayableParticipant) {
        sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        return
    }
    if (socket.destroyed) return

    const room = dependencies.roomRepository.getRoom(roomNumber)
    const participant = dependencies.roomRepository.getParticipant(roomNumber, viewerId)
    if (room === null || participant === null || room.roomSequence !== requestedRoom.roomSequence || participant.accountId !== requestedParticipant.accountId || room.categoryId !== categoryId || room.questId !== questId) {
        sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        return
    }
    if (participant.connectionId !== null) {
        const previous = admissions.get(participant.connectionId)
        if (previous !== undefined) removeAdmissionAndCloseSockets(previous)
    }
    const connectionId = randomBytes(16).toString("hex")
    const connectedParticipant = dependencies.roomRepository.setParticipantConnection(roomNumber, viewerId, connectionId)
    if (connectedParticipant === null || connectedParticipant.accountId !== participant.accountId) {
        sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        return
    }
    const admission: SessionAdmission = {
        roomNumber,
        roomSequence: room.roomSequence,
        questCategory: room.categoryId,
        questId: room.questId,
        viewerId,
        partyId: connectedParticipant.partyId,
        isHost: room.hostViewerId === viewerId,
        connectionId,
        hasCompletedHostEntry: false,
        hasStartedLobbyBattle: false,
        sceneReady: false,
        pendingNpcSelections: null,
        lobbyPlayer: null,
        lobbyRoster: null,
        lobbySocket: socket,
        battleSocket: null,
    }
    admissions.set(connectionId, admission)
    state.isHandshakePending = false
    state.admission = admission
    sendFrame(socket, [0, connectionId, roomNumber])
}
// //// /验证大厅握手并签发连接 ID ////

// //// 验证战斗握手与大厅连接的关联 [@x380kkm 2026-07-22] ////
function acceptBattleHandshake(socket: net.Socket, state: ConnectionState, data: Record<string, unknown>, dependencies: MultiplayerSessionDependencies): void {
    const roomNumber = readRoomNumber(data.roomNumber)
    const connectionId = readConnectionId(data.connectionId)
    if (roomNumber === null || connectionId === null || !hasReconnectFlag(data.reconnected)) {
        sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        return
    }
    const admission = admissions.get(connectionId)
    const room = dependencies.roomRepository.getRoom(roomNumber)
    const hasCompletedLobbyStart = admission !== undefined
        && (!admission.isHost || admission.hasCompletedHostEntry)
        && admission.hasStartedLobbyBattle
    if (admission === undefined || admission.roomNumber !== roomNumber || room === null || room.roomSequence !== admission.roomSequence || !room.battleStarted || !hasCompletedLobbyStart) {
        sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        return
    }

    if (admission.battleSocket !== null && !admission.battleSocket.destroyed) admission.battleSocket.destroy()
    admission.battleSocket = socket
    admission.sceneReady = false
    state.isHandshakePending = false
    state.admission = admission
    sendFrame(socket, [0, roomNumber, ""])
}
// //// /验证战斗握手与大厅连接的关联 ////

// //// 按 socklet 类型处理第一帧 [@x380kkm 2026-07-22] ////
async function handleHandshakeFrame(socket: net.Socket, state: ConnectionState, data: unknown, dependencies: MultiplayerSessionDependencies): Promise<void> {
    pruneStaleAdmissions(dependencies)
    if (!isRecord(data) || typeof data.socklet !== "string") {
        sendHandshakeDenialAndClose(socket, "DENIED")
        return
    }
    if (data.socklet === "cooperation_room") {
        await acceptLobbyHandshake(socket, state, data, dependencies)
        return
    }
    if (data.socklet === "cooperation_battle") {
        acceptBattleHandshake(socket, state, data, dependencies)
        return
    }
    sendHandshakeDenialAndClose(socket, "DENIED")
}
// //// /按 socklet 类型处理第一帧 ////

// //// 归一化 CN v1.8.1 人类玩家载荷 [@x380kkm 2026-07-23] ////
function readMainCharacterId(party: Record<string, unknown>): number | null {
    if (!Array.isArray(party.characters) || party.characters.length === 0) return null
    const firstCharacter = party.characters[0]
    if (!Array.isArray(firstCharacter) || firstCharacter.length !== 2 || firstCharacter[0] !== 0 || !isRecord(firstCharacter[1])) return null
    return readPositiveInteger(firstCharacter[1].id)
}

function nextHumanPlayerId(admission: SessionAdmission): number {
    const usedPlayerIds = new Set(
        getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
            .map((item) => (item.lobbyPlayer as LobbyPlayer).playerId),
    )
    let playerId = FIRST_HUMAN_PLAYER_ID
    while (usedPlayerIds.has(playerId)) playerId += 1
    return playerId
}

function createCnLobbyRoom(
    admission: SessionAdmission,
    activeAdmissions: SessionAdmission[],
): Record<string, unknown> {
    const host = activeAdmissions.find((item) => item.isHost)?.lobbyPlayer as LobbyPlayer | undefined
    return {
        roomNumber: admission.roomNumber,
        establisherConnectionId: host?.connectionId ?? admission.connectionId,
        establisherName: host?.name ?? "",
        establisherCharacter: host?.mainCharacterId ?? 0,
        questCategory: admission.questCategory,
        questId: admission.questId,
        status: 2,
    }
}

function broadcastCnLobbyMessage(activeAdmissions: SessionAdmission[], command: unknown[]): void {
    const frame = createCnMeetingServerFrame(command)
    for (const activeAdmission of activeAdmissions) {
        const socket = activeAdmission.lobbySocket as net.Socket
        sendFrame(socket, frame)
    }
}

function updateLobbyPlayer(admission: SessionAdmission, player: LobbyPlayer): void {
    admission.lobbyPlayer = player
    if (admission.lobbyRoster === null) return
    admission.lobbyRoster = admission.lobbyRoster.map((mate) => (
        mate.connectionId === player.connectionId ? player : mate
    ))
}

function normalizeLobbyPlayer(
    command: unknown[],
    admission: SessionAdmission,
    entryTime: number,
    playerId: number,
    isHost: boolean,
): LobbyPlayer | null {
    if (command.length !== 3 || !isRecord(command[1])) return null
    const player = command[1]
    const partyId = readPositiveInteger(command[2])
    const currentPartyId = readPositiveInteger(player.currentPartyId)
    if (partyId === null || partyId !== admission.partyId || currentPartyId !== partyId) return null
    if (readPositiveInteger(player.viewerId) !== admission.viewerId || player.connectionId !== admission.connectionId) return null
    if (("comId" in player && player.comId !== null) || typeof player.entryTime !== "number" || !Number.isFinite(player.entryTime)) return null

    const name = typeof player.name === "string" && player.name.length > 0 ? player.name : null
    const rank = readPositiveInteger(player.rank)
    const degreeId = readPositiveInteger(player.degreeId)
    const playerRoleKind = readPositiveInteger(player.playerRoleKind)
    const autoskillMode = readNonNegativeInteger(player.autoskillMode)
    const autoSpeedLevel = readNonNegativeInteger(player.autoSpeedLevel)
    const skillAbilityBehaviorMode = readNonNegativeInteger(player.skillAbilityBehaviorMode)
    const dashBehaviorMode = readNonNegativeInteger(player.dashBehaviorMode)
    if (name === null || rank === null || degreeId === null || playerRoleKind === null || autoskillMode === null || autoSpeedLevel === null || skillAbilityBehaviorMode === null || dashBehaviorMode === null) return null
    if (typeof player.isNewbie !== "boolean" || typeof player.autoplayMode !== "boolean" || typeof player.autoStart !== "boolean" || typeof player.allowHealFromOtherPlayers !== "boolean") return null
    if (!Array.isArray(player.state) || player.state.length !== 1 || ![0, 1].includes(player.state[0] as number) || !isRecord(player.party)) return null

    const mainCharacterId = readMainCharacterId(player.party)
    if (mainCharacterId === null) return null
    return {
        viewerId: admission.viewerId,
        playerId,
        name,
        rank,
        degreeId,
        mainCharacterId,
        party: player.party,
        connectionId: admission.connectionId,
        playerRoleKind,
        isNewbie: player.isNewbie,
        isHost,
        entryTime,
        currentPartyId,
        autoplayMode: player.autoplayMode,
        autoskillMode,
        autoSpeedLevel,
        autoStart: player.autoStart,
        skillAbilityBehaviorMode,
        dashBehaviorMode,
        allowHealFromOtherPlayers: player.allowHealFromOtherPlayers,
        state: player.state,
    }
}
// //// /归一化 CN v1.8.1 人类玩家载荷 ////

// //// 处理抓包确认的 CN 大厅成员, 准备状态, 自动设置, COM, 开战, 心跳和退出命令 [@x380kkm 2026-07-23] ////
function handleLobbyFrame(socket: net.Socket, state: ConnectionState, data: unknown, clock: MultiplayerSessionClock): void {
    const admission = state.admission
    if (admission === null || admission.lobbySocket !== socket) return

    const command = readCnMeetingCommand(data)
    if (command === null) {
        socket.end()
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.enter) {
        if (admission.lobbyPlayer !== null) {
            socket.end()
            return
        }
        const hadExistingPlayers = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence).length > 0
        const player = normalizeLobbyPlayer(
            command,
            admission,
            clock.getCurrentTimeMilliseconds(),
            nextHumanPlayerId(admission),
            admission.isHost,
        )
        if (player === null) {
            socket.end()
            return
        }
        admission.hasCompletedHostEntry = admission.isHost
        admission.lobbyPlayer = player
        const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
        const room = createCnLobbyRoom(admission, activeAdmissions)
        const roster = activeAdmissions.map((item) => item.lobbyPlayer as LobbyPlayer)
        sendFrame(socket, createCnMeetingServerFrame([CN_MEETING_SERVER_MESSAGE.welcome, room, roster]))
        if (hadExistingPlayers) {
            broadcastCnLobbyMessage(activeAdmissions, [CN_MEETING_SERVER_MESSAGE.mates, roster])
        }
        if (player.state[0] === 1) {
            broadcastCnLobbyMessage(activeAdmissions, [CN_MEETING_SERVER_MESSAGE.stateChanged, player.connectionId, [1]])
        }
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.heartbeat) {
        if (command.length !== 1) {
            socket.end()
            return
        }
        sendFrame(socket, createCnMeetingServerFrame([CN_MEETING_SERVER_MESSAGE.ackHeartbeat, admission.connectionId]))
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.changeParty) {
        if (command.length !== 4 || typeof command[2] !== "boolean") {
            socket.end()
            return
        }
        const currentPlayer = admission.lobbyPlayer
        const player = currentPlayer === null
            ? null
            : normalizeLobbyPlayer(
                [CN_MEETING_CLIENT_COMMAND.enter, command[1], command[3]],
                admission,
                clock.getCurrentTimeMilliseconds(),
                currentPlayer.playerId,
                admission.isHost,
            )
        if (player === null) {
            socket.end()
            return
        }
        updateLobbyPlayer(admission, player)
        const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
        const roster = activeAdmissions.map((item) => item.lobbyPlayer as LobbyPlayer)
        broadcastCnLobbyMessage(activeAdmissions, [CN_MEETING_SERVER_MESSAGE.mates, roster])
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.ready) {
        if (command.length !== 2 || !Array.isArray(command[1]) || command[1].length !== 1 || ![0, 1].includes(command[1][0] as number)) {
            socket.end()
            return
        }
        const player = admission.lobbyPlayer
        if (player === null) {
            socket.end()
            return
        }
        const readyState = command[1] as [number]
        updateLobbyPlayer(admission, { ...player, state: readyState })
        const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
        broadcastCnLobbyMessage(activeAdmissions, [CN_MEETING_SERVER_MESSAGE.stateChanged, player.connectionId, readyState])
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.changeAutoplayMode) {
        if (command.length !== 3 || typeof command[1] !== "boolean" || typeof command[2] !== "boolean" || admission.lobbyPlayer === null) {
            socket.end()
            return
        }
        const player = {
            ...admission.lobbyPlayer,
            autoplayMode: command[1],
            autoSpeedLevel: command[2] ? 1 : admission.lobbyPlayer.autoSpeedLevel,
        }
        updateLobbyPlayer(admission, player)
        const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
        broadcastCnLobbyMessage(activeAdmissions, [CN_MEETING_SERVER_MESSAGE.autoplayModeChanged, player.connectionId, command[1], command[2]])
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.changeAutoStart) {
        if (command.length !== 2 || typeof command[1] !== "boolean" || admission.lobbyPlayer === null) {
            socket.end()
            return
        }
        const player = { ...admission.lobbyPlayer, autoStart: command[1] }
        updateLobbyPlayer(admission, player)
        const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
        broadcastCnLobbyMessage(activeAdmissions, [CN_MEETING_SERVER_MESSAGE.autoStartChanged, player.connectionId, command[1]])
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.suspend) {
        if (command.length !== 1 || admission.lobbyPlayer === null) {
            socket.end()
            return
        }
        const player = { ...admission.lobbyPlayer, state: [0] as [number] }
        updateLobbyPlayer(admission, player)
        const activeAdmissions = getActiveLobbyAdmissions(admission.roomNumber, admission.roomSequence)
        broadcastCnLobbyMessage(activeAdmissions, [CN_MEETING_SERVER_MESSAGE.stateChanged, player.connectionId, [0]])
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.enterComs) {
        if (command.length !== 2 || !admission.isHost || !publishCnLobbyMates(admission, command[1], clock)) {
            socket.end()
        }
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.startBattle) {
        if (command.length !== 1 || !startCnLobbyBattle(admission)) socket.end()
        return
    }
    if (command[0] === CN_MEETING_CLIENT_COMMAND.bye) {
        if (command.length !== 1) {
            socket.end()
            return
        }
        const roster = admission.hasStartedLobbyBattle ? admission.lobbyRoster ?? [] : []
        socket.end(encodeFrame(createCnMeetingServerFrame([CN_MEETING_SERVER_MESSAGE.mates, roster])))
    }
}
// //// /处理抓包确认的 CN 大厅成员, 准备状态, 自动设置, COM, 开战, 心跳和退出命令 ////

// //// 分派握手和握手后的会话帧 [@x380kkm 2026-07-23] ////
async function handleFrame(socket: net.Socket, state: ConnectionState, rawFrame: string, dependencies: MultiplayerSessionDependencies): Promise<void> {
    let data: unknown
    try {
        data = JSON.parse(rawFrame)
    } catch {
        if (state.isHandshakePending) sendHandshakeDenialAndClose(socket, "HANDSHAKE_DENIED")
        else socket.end()
        return
    }
    if (state.isHandshakePending) {
        await handleHandshakeFrame(socket, state, data, dependencies)
        return
    }
    const admission = state.admission
    if (admission?.lobbySocket === socket) {
        handleLobbyFrame(socket, state, data, dependencies.clock)
        return
    }
    if (admission?.battleSocket === socket) {
        handleBattleFrame(socket, state, data, dependencies)
        return
    }
    socket.end()
}
// //// /分派握手和握手后的会话帧 ////

function detachSocket(socket: net.Socket, state: ConnectionState): void {
    connectionStates.delete(socket)
    if (state.admission?.lobbySocket === socket) {
        state.admission.lobbySocket = null
        state.admission.lobbyPlayer = null
        state.admission.pendingNpcSelections = null
        state.admission.lobbyRoster = null
    }
    if (state.admission?.battleSocket === socket) {
        if (!state.battleFinalized) broadcastCnBattleLeave(state.admission)
        state.admission.battleSocket = null
        state.admission.sceneReady = false
    }
}

// //// 从 TCP 字节流提取并顺序处理 NUL JSON 帧 [@x380kkm 2026-07-22] ////
function acceptSocket(socket: net.Socket, dependencies: MultiplayerSessionDependencies): void {
    socket.setEncoding("utf8")
    const state: ConnectionState = {
        buffer: "",
        isHandshakePending: true,
        battleFinalized: false,
        admission: null,
        processing: Promise.resolve(),
    }
    connectionStates.set(socket, state)

    socket.on("data", (chunk: string) => {
        state.buffer += chunk
        while (true) {
            const separatorIndex = state.buffer.indexOf("\0")
            if (separatorIndex < 0) break
            const rawFrame = state.buffer.slice(0, separatorIndex)
            state.buffer = state.buffer.slice(separatorIndex + 1)
            if (Buffer.byteLength(rawFrame, "utf8") > MAX_FRAME_BYTES) {
                socket.destroy()
                return
            }
            if (rawFrame.trim().length === 0) continue
            state.processing = state.processing.then(() => handleFrame(socket, state, rawFrame, dependencies)).catch(() => {
                socket.destroy()
            })
        }
        if (Buffer.byteLength(state.buffer, "utf8") > MAX_FRAME_BYTES) socket.destroy()
    })
    socket.on("close", () => detachSocket(socket, state))
    socket.on("error", () => detachSocket(socket, state))
}
// //// /从 TCP 字节流提取并顺序处理 NUL JSON 帧 ////

// //// 启动和停止同进程多人 TCP 监听器 [@x380kkm 2026-07-22] ////
export async function startMultiplayerSessionServer(dependencies: MultiplayerSessionDependencies): Promise<void> {
    if (sessionServer !== null) return
    const server = net.createServer((socket) => acceptSocket(socket, dependencies))
    await new Promise<void>((resolve, reject) => {
        const rejectStart = (error: Error) => reject(error)
        server.once("error", rejectStart)
        server.listen(getMultiplayerSessionPort(), getMultiplayerSessionListenHost(), () => {
            server.off("error", rejectStart)
            resolve()
        })
    })
    server.on("error", (error) => console.error("Multiplayer session server error.", error))
    sessionServer = server
    activeSessionDependencies = dependencies
}

export async function stopMultiplayerSessionServer(): Promise<void> {
    const server = sessionServer
    sessionServer = null
    activeSessionDependencies = null
    for (const socket of connectionStates.keys()) socket.destroy()
    connectionStates.clear()
    admissions.clear()
    if (server === null) return
    await new Promise<void>((resolve, reject) => server.close((error) => error === undefined ? resolve() : reject(error)))
}
// //// /启动和停止同进程多人 TCP 监听器 ////
