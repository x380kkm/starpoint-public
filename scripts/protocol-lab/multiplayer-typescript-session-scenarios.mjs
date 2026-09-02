// audience: internal
// # multiplayer-typescript-session-scenarios
//
// 此模块从 TypeScript 多人会话服务抽取状态机契约事实.

import path from "node:path"
import {
    createCollector,
    extractFunction,
    extractJavaScriptMethod,
} from "./multiplayer-protocol-differential-lib.mjs"
import { finishCollectors, hasAll, readSource, verified } from "./multiplayer-session-scenario-lib.mjs"

// //// 抽取 TypeScript 会话状态机场景 [@x380kkm 2026-08-24] ////
export function parseTypeScriptSession(repositoryRoot) {
    const multiplayerRoot = path.join(repositoryRoot, "src", "multiplayer")
    const files = {
        session: path.join(multiplayerRoot, "sessionServer.ts"),
        store: path.join(multiplayerRoot, "matchmakingStore.ts"),
        battleProtocol: path.join(multiplayerRoot, "cnBattleProtocol.ts"),
        meetingProtocol: path.join(multiplayerRoot, "cnMeetingProtocol.ts"),
        lobbyNpc: path.join(multiplayerRoot, "cnLobbyNpc.ts"),
    }
    const source = Object.fromEntries(Object.entries(files).map(([name, file]) => [name, readSource(file)]))
    const required = createCollector(repositoryRoot)
    const policy = createCollector(repositoryRoot)

    const handleFrame = extractFunction(source.session, "handleFrame", "javascript")
    const acceptSocket = extractFunction(source.session, "acceptSocket", "javascript")
    const acceptLobby = extractFunction(source.session, "acceptLobbyHandshake", "javascript")
    const acceptBattle = extractFunction(source.session, "acceptBattleHandshake", "javascript")
    const handleHandshake = extractFunction(source.session, "handleHandshakeFrame", "javascript")
    const handleLobby = extractFunction(source.session, "handleLobbyFrame", "javascript")
    const handleBattle = extractFunction(source.session, "handleBattleFrame", "javascript")
    const detachSocket = extractFunction(source.session, "detachSocket", "javascript")
    const stageMates = extractFunction(source.session, "stageCnLobbyMates", "javascript")
    const publishMates = extractFunction(source.session, "publishCnLobbyMates", "javascript")
    const startBattle = extractFunction(source.session, "startCnLobbyBattle", "javascript")
    const broadcastLobby = extractFunction(source.session, "broadcastCnLobbyMessage", "javascript")
    const broadcastBattle = extractFunction(source.session, "broadcastCnBattleMessages", "javascript")
    const sendBattle = extractFunction(source.session, "sendCnBattleMessage", "javascript")
    const broadcastLeave = extractFunction(source.session, "broadcastCnBattleLeave", "javascript")
    const pruneAdmissions = extractFunction(source.session, "pruneStaleAdmissions", "javascript")
    const createRoom = extractJavaScriptMethod(source.store, "createRoom")
    const listRooms = extractJavaScriptMethod(source.store, "listRooms")
    const joinRoom = extractJavaScriptMethod(source.store, "joinRoom")
    const disbandRoom = extractJavaScriptMethod(source.store, "disbandRoom")

    required.add("session.transport.maxFrameBytes", source.session.includes("4 * 1024 * 1024") ? 4_194_304 : null,
        files.session, source.session, source.session.indexOf("MAX_FRAME_BYTES"))
    required.add("session.transport.ignoresWhitespaceFrames", verified(/rawFrame\.trim\(\)\.length === 0/.test(acceptSocket.source)),
        files.session, source.session, acceptSocket.start)
    required.add("session.transport.closesOversizeFrames", verified(hasAll(acceptSocket.source, [
        /Buffer\.byteLength\(rawFrame, "utf8"\) > MAX_FRAME_BYTES/,
        /socket\.destroy\(\)/,
    ])), files.session, source.session, acceptSocket.start)
    required.add("session.transport.closesMalformedJson", verified(hasAll(handleFrame.source, [
        /JSON\.parse\(rawFrame\)/,
        /socket\.end\(\)/,
    ])), files.session, source.session, handleFrame.start)
    required.add("session.transport.ordersFramesPerConnection", verified(/state\.processing = state\.processing\.then/.test(acceptSocket.source)),
        files.session, source.session, acceptSocket.start)
    required.add("session.transport.dispatchStates", hasAll(handleFrame.source, [
        /state\.isHandshakePending/,
        /admission\?\.lobbySocket === socket/,
        /admission\?\.battleSocket === socket/,
    ]) ? ["battle", "handshake", "lobby"] : null, files.session, source.session, handleFrame.start)

    required.add("session.handshake.denialFrames", /reason === "HANDSHAKE_DENIED" \? 3 : 1/.test(source.session)
        ? ["DENIED:1", "HANDSHAKE_DENIED:3"] : null,
    files.session, source.session, source.session.indexOf("sendHandshakeDenialAndClose"))
    required.add("session.handshake.lobbyAcceptedFields",
        ["questCategory", "questId", "roomNumber", "viewerId"], files.session, source.session, acceptLobby.start)
    required.add("session.handshake.battleAcceptedFields",
        ["connectionId", "roomNumber"], files.session, source.session, acceptBattle.start)
    required.add("session.handshake.requiresReconnectInteger", verified(/Number\.isInteger\(value\)/.test(source.session)),
        files.session, source.session, source.session.indexOf("hasReconnectFlag"))
    required.add("session.handshake.roomNumberShape", /\^\\d\{6\}\$/.test(source.session)
        ? "six-digit" : null, files.session, source.session, source.session.indexOf("readRoomNumber"))
    required.add("session.handshake.connectionIdShape", /\^\[a-f0-9\]\{32\}\$/.test(source.session)
        ? "32-lowercase-hex" : null, files.session, source.session, source.session.indexOf("readConnectionId"))
    required.add("session.handshake.lobbyChecksRoomMemberQuestAndIdentity", verified(hasAll(acceptLobby.source, [
        /getRoom\(roomNumber\)/,
        /getParticipant\(roomNumber, viewerId\)/,
        /requestedRoom\.categoryId !== categoryId/,
        /requestedRoom\.questId !== questId/,
        /isPlayableParticipant/,
    ])), files.session, source.session, acceptLobby.start)
    required.add("session.handshake.lobbyBindsRoomSequence", verified(hasAll(acceptLobby.source, [
        /room\.roomSequence !== requestedRoom\.roomSequence/,
        /roomSequence: room\.roomSequence/,
    ])), files.session, source.session, acceptLobby.start)
    required.add("session.handshake.lobbySupersedesPreviousViewer", verified(hasAll(acceptLobby.source, [
        /participant\.connectionId !== null/,
        /removeAdmissionAndCloseSockets\(previous\)/,
    ])), files.session, source.session, acceptLobby.start)
    required.add("session.handshake.issuesRandomConnectionId", /randomBytes\(16\)\.toString\("hex"\)/.test(acceptLobby.source)
        ? "16-random-bytes" : null, files.session, source.session, acceptLobby.start)
    required.add("session.handshake.lobbyReply", /sendFrame\(socket, \[0, connectionId, roomNumber\]\)/.test(acceptLobby.source)
        ? "accept-connection-room" : null, files.session, source.session, acceptLobby.start)
    required.add("session.handshake.battleChecksConnectionAndStartedRoom", verified(hasAll(acceptBattle.source, [
        /admissions\.get\(connectionId\)/,
        /!room\.battleStarted/,
        /hasCompletedLobbyStart/,
    ])), files.session, source.session, acceptBattle.start)
    required.add("session.handshake.battleSupersedesPreviousSocket", verified(hasAll(acceptBattle.source, [
        /admission\.battleSocket\.destroy\(\)/,
        /admission\.battleSocket = socket/,
    ])), files.session, source.session, acceptBattle.start)
    required.add("session.handshake.battleResetsSceneReady", verified(/admission\.sceneReady = false/.test(acceptBattle.source)),
        files.session, source.session, acceptBattle.start)
    required.add("session.handshake.battleReply", /sendFrame\(socket, \[0, roomNumber, ""\]\)/.test(acceptBattle.source)
        ? "accept-room" : null, files.session, source.session, acceptBattle.start)
    required.add("session.handshake.unknownSockletCloses", verified(hasAll(handleHandshake.source, [
        /sendHandshakeDenialAndClose\(socket, "DENIED"\)/,
        /data\.socklet === "cooperation_room"/,
        /data\.socklet === "cooperation_battle"/,
    ])), files.session, source.session, handleHandshake.start)

    required.add("session.room.maxMembers", /participants\.size >= 3/.test(joinRoom.source) ? 3 : null,
        files.store, source.store, joinRoom.start)
    required.add("session.room.numberShape", /randomInt\(100000, 1000000\)/.test(createRoom.source) ? "six-digit" : null,
        files.store, source.store, createRoom.start)
    required.add("session.room.hostInsertedOnCreate", verified(/participants: new Map\(\[/.test(createRoom.source)),
        files.store, source.store, createRoom.start)
    required.add("session.room.sequencePreventsNumberReuse", verified(/roomSequence: randomInt\(10000000, 100000000\)/.test(createRoom.source)),
        files.store, source.store, createRoom.start)
    required.add("session.room.listFiltersCategory", verified(/room\.categoryId === categoryId/.test(listRooms.source)),
        files.store, source.store, listRooms.start)
    required.add("session.room.joinRejectsFull", verified(/!room\.participants\.has\(participant\.viewerId\) && room\.participants\.size >= 3/.test(joinRoom.source)),
        files.store, source.store, joinRoom.start)
    required.add("session.room.rejoinPreservesConnection", verified(/previous\?\.connectionId \?\? null/.test(joinRoom.source)),
        files.store, source.store, joinRoom.start)
    required.add("session.room.disbandRequiresOwner", verified(/room\.hostAccountId !== accountId/.test(disbandRoom.source)),
        files.store, source.store, disbandRoom.start)
    required.add("session.room.expiryMinutes", source.store.includes("30 * 60 * 1000") ? [30] : null,
        files.store, source.store, source.store.indexOf("ROOM_LIFETIME_MILLISECONDS"))

    required.add("session.lobby.serverBroadcastIncludesAllActiveSockets", verified(
        /for \(const activeAdmission of activeAdmissions\)/.test(broadcastLobby.source),
    ), files.session, source.session, broadcastLobby.start)
    required.add("session.lobby.malformedFrameCloses", verified(hasAll(handleLobby.source, [
        /readCnMeetingCommand\(data\)/,
        /socket\.end\(\)/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.enterWelcome", verified(/CN_MEETING_SERVER_MESSAGE\.welcome, room, roster/.test(handleLobby.source)),
        files.session, source.session, handleLobby.start)
    required.add("session.lobby.enterBroadcastsRosterAndInitialReady", verified(hasAll(handleLobby.source, [
        /CN_MEETING_SERVER_MESSAGE\.mates, roster/,
        /CN_MEETING_SERVER_MESSAGE\.stateChanged, player\.connectionId, \[1\]/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.heartbeatRequiresNoArguments", verified(/command\.length !== 1/.test(handleLobby.source)),
        files.session, source.session, handleLobby.start)
    required.add("session.lobby.heartbeatRepliesConnection", verified(/CN_MEETING_SERVER_MESSAGE\.ackHeartbeat, admission\.connectionId/.test(handleLobby.source)),
        files.session, source.session, handleLobby.start)
    required.add("session.lobby.changePartyValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /command\.length !== 4/,
        /updateLobbyPlayer\(admission, player\)/,
        /CN_MEETING_SERVER_MESSAGE\.mates, roster/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.readyValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /command\[1\]\.length !== 1/,
        /updateLobbyPlayer\(admission, \{ \.\.\.player, state: readyState \}\)/,
        /CN_MEETING_SERVER_MESSAGE\.stateChanged/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.autoplayValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /command\.length !== 3/,
        /autoplayMode: command\[1\]/,
        /CN_MEETING_SERVER_MESSAGE\.autoplayModeChanged/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.autoStartValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /command\.length !== 2/,
        /autoStart: command\[1\]/,
        /CN_MEETING_SERVER_MESSAGE\.autoStartChanged/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.suspendClearsReadyAndBroadcasts", verified(hasAll(handleLobby.source, [
        /CN_MEETING_CLIENT_COMMAND\.suspend/,
        /state: \[0\]/,
        /CN_MEETING_SERVER_MESSAGE\.stateChanged/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.enterComsRequiresHostAndValidatedPayload", verified(hasAll(handleLobby.source, [
        /!admission\.isHost/,
        /publishCnLobbyMates\(admission, command\[1\], clock\)/,
    ])), files.session, source.session, handleLobby.start)
    required.add("session.lobby.startRequiresHostReadyRoster", verified(hasAll(startBattle.source, [
        /!admission\.isHost/,
        /roster\.some/,
        /CN_MEETING_SERVER_MESSAGE\.start, roster/,
    ])), files.session, source.session, startBattle.start)
    required.add("session.lobby.byeRepliesRosterAndCloses", verified(hasAll(handleLobby.source, [
        /admission\.hasStartedLobbyBattle \? admission\.lobbyRoster \?\? \[\] : \[\]/,
        /socket\.end\(encodeFrame/,
    ])), files.session, source.session, handleLobby.start)

    required.add("session.ai.maxMates", /MAX_CN_LOBBY_PLAYERS - 1/.test(stageMates.source) ? 2 : null,
        files.session, source.session, stageMates.start)
    required.add("session.ai.totalRosterCapacity", source.session.includes("MAX_CN_LOBBY_PLAYERS = 3") ? 3 : null,
        files.session, source.session, source.session.indexOf("MAX_CN_LOBBY_PLAYERS"))
    required.add("session.ai.stageRequiresCurrentRoomAndActiveHost", verified(hasAll(stageMates.source, [
        /room\.roomSequence !== roomSequence/,
        /!admission\.isHost/,
        /admission\.lobbySocket === null/,
    ])), files.session, source.session, stageMates.start)
    required.add("session.ai.requestPayloadValidated", verified(/validateCnLobbyNpcRequestsAndReadNames/.test(publishMates.source) &&
        /isDeepStrictEqual/.test(source.lobbyNpc)), files.lobbyNpc, source.lobbyNpc,
    source.lobbyNpc.indexOf("validateCnLobbyNpcRequestsAndReadNames"))
    required.add("session.ai.joinReadyAndCountdownFrames", verified(hasAll(publishMates.source, [
        /CN_MEETING_SERVER_MESSAGE\.mates/,
        /CN_MEETING_SERVER_MESSAGE\.stateChanged/,
        /CN_MEETING_SERVER_MESSAGE\.startRemainingTime/,
    ])), files.session, source.session, publishMates.start)
    required.add("session.ai.selectionConsumedAndRosterFrozen", verified(hasAll(publishMates.source, [
        /pendingNpcSelections = null/,
        /lobbyRoster = readyRoster/,
    ])), files.session, source.session, publishMates.start)

    required.add("session.battle.malformedFrameCloses", verified(hasAll(handleBattle.source, [
        /readCnBattleAction\(data\)/,
        /socket\.end\(\)/,
    ])), files.session, source.session, handleBattle.start)
    required.add("session.battle.sceneReadyWaitsForAllParticipants", verified(hasAll(handleBattle.source, [
        /admission\.sceneReady = true/,
        /activeAdmissions\.some\(\(item\) => !item\.sceneReady\)/,
        /createCnBattleStartedFrame\(\)/,
    ])), files.session, source.session, handleBattle.start)
    required.add("session.battle.finalizeAcknowledgesAndCloses", verified(hasAll(handleBattle.source, [
        /state\.battleFinalized = true/,
        /createCnBattleFinalizedFrame\(\)/,
        /socket\.end\(\)/,
    ])), files.session, source.session, handleBattle.start)
    required.add("session.battle.measurementShapes", /command\.length !== 3/.test(source.battleProtocol) ? ["flat"] : null,
        files.battleProtocol, source.battleProtocol, source.battleProtocol.indexOf("readNotifyAction"))
    required.add("session.battle.measurementAcknowledgesServerTime", verified(/createCnBattleMeasurementFrame/.test(handleBattle.source)),
        files.session, source.session, handleBattle.start)
    required.add("session.battle.broadcastValidatesAndScopesRoomSequence", verified(hasAll(broadcastBattle.source, [
        /target\.roomSequence !== admission\.roomSequence/,
        /createCnBattleBroadcastFrame/,
    ]) && /every\(isBroadcastCommand\)/.test(source.battleProtocol)),
    files.session, source.session, broadcastBattle.start)
    required.add("session.battle.sendValidatesAndScopesRoomSequence", verified(hasAll(sendBattle.source, [
        /target\.roomSequence !== admission\.roomSequence/,
        /createCnBattleSendFrame/,
    ]) && /targetConnectionIds\.length/.test(source.battleProtocol)),
    files.session, source.session, sendBattle.start)
    required.add("session.battle.heartbeatAndLineWarningKeepConnection", verified(hasAll(handleBattle.source, [
        /case "heartbeat":/,
        /case "lineSpeedWarning":/,
    ])), files.session, source.session, handleBattle.start)
    required.add("session.battle.unmodeledNumericFramesKeepConnection", verified(/case "unmodeled":/.test(handleBattle.source)),
        files.session, source.session, handleBattle.start)
    required.add("session.battle.nonfinalizedDisconnectBroadcastsLeave", verified(hasAll(detachSocket.source, [
        /!state\.battleFinalized/,
        /broadcastCnBattleLeave\(state\.admission\)/,
    ])), files.session, source.session, detachSocket.start)
    required.add("session.battle.leaveExcludesSenderAndScopesSequence", verified(hasAll(broadcastLeave.source, [
        /target === admission/,
        /target\.roomSequence !== admission\.roomSequence/,
        /createCnBattleLeaveFrame/,
    ])), files.session, source.session, broadcastLeave.start)
    required.add("session.battle.reconnectSupersedesSocketWithoutLeave", verified(hasAll(acceptBattle.source, [
        /admission\.battleSocket\.destroy\(\)/,
        /admission\.battleSocket = socket/,
    ])), files.session, source.session, acceptBattle.start)

    required.add("session.cleanup.lobbyDisconnectClearsState", verified(hasAll(detachSocket.source, [
        /lobbyPlayer = null/,
        /pendingNpcSelections = null/,
        /lobbyRoster = null/,
    ])), files.session, source.session, detachSocket.start)
    required.add("session.cleanup.roomSequencePreventsStaleReuse", verified(hasAll(pruneAdmissions.source, [
        /room === null/,
        /room\.roomSequence !== admission\.roomSequence/,
        /removeAdmissionAndCloseSockets/,
    ])), files.session, source.session, pruneAdmissions.start)

    policy.add("session.policy.roomCreateReplacesExistingHostRoom", verified(/room\.hostAccountId === request\.hostAccountId/.test(createRoom.source)),
        files.store, source.store, createRoom.start)
    policy.add("session.policy.aiJoinDeliveryScope", /for \(const activeAdmission of activeAdmissions\)/.test(publishMates.source)
        ? "room" : null, files.session, source.session, publishMates.start)
    policy.add("session.policy.lobbyHeartbeatReplyTag", source.meetingProtocol.includes("ackHeartbeat: 10") ? 10 : null,
        files.meetingProtocol, source.meetingProtocol, source.meetingProtocol.indexOf("ackHeartbeat"))
    policy.add("session.policy.lobbyStartRemainingTag", source.meetingProtocol.includes("startRemainingTime: 9") ? 9 : null,
        files.meetingProtocol, source.meetingProtocol, source.meetingProtocol.indexOf("startRemainingTime"))
    policy.add("session.policy.lobbyReadyEvaluatesAutoStart", false,
        files.session, source.session, handleLobby.start)
    policy.add("session.policy.lobbyUnknownCommand", "ignore",
        files.session, source.session, handleLobby.start)
    policy.add("session.policy.battleBroadcastRecipients", /for \(const target of admissions\.values\(\)\)/.test(broadcastBattle.source)
        ? "room-including-sender" : null, files.session, source.session, broadcastBattle.start)
    policy.add("session.policy.battleSendRecipients", /admissions\.get\(targetConnectionId\)/.test(sendBattle.source)
        ? "listed-connection-ids" : null, files.session, source.session, sendBattle.start)
    policy.add("session.policy.battleDisconnectGraceMs", /broadcastCnBattleLeave\(state\.admission\)/.test(detachSocket.source) ? 0 : null,
        files.session, source.session, detachSocket.start)

    return finishCollectors(required, policy)
}
// //// /抽取 TypeScript 会话状态机场景 ////
