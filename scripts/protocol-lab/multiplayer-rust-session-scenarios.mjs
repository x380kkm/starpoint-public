// audience: internal
// # multiplayer-rust-session-scenarios
//
// 此模块从 Rust 个人服务抽取多人状态机契约事实.

import path from "node:path"
import {
    createCollector,
    extractBlockAfter,
    extractFunction,
} from "./multiplayer-protocol-differential-lib.mjs"
import { finishCollectors, hasAll, readSource, verified } from "./multiplayer-session-scenario-lib.mjs"

// //// 抽取 Rust 会话状态机场景 [@x380kkm 2026-08-24] ////
export function parseRustSession(repositoryRoot) {
    const serviceRoot = path.join(repositoryRoot, "core", "personal-service", "src")
    const multiplayerRoot = path.join(serviceRoot, "cn_multiplayer")
    const databaseRoot = path.join(serviceRoot, "database", "multiplayer")
    const files = {
        session: path.join(serviceRoot, "cn_multiplayer.rs"),
        transport: path.join(multiplayerRoot, "transport.rs"),
        meeting: path.join(multiplayerRoot, "meeting.rs"),
        battle: path.join(multiplayerRoot, "battle.rs"),
        lifecycle: path.join(multiplayerRoot, "lifecycle.rs"),
        lobbyPlayer: path.join(multiplayerRoot, "lobby_player.rs"),
        database: path.join(serviceRoot, "database", "multiplayer.rs"),
        room: path.join(databaseRoot, "room.rs"),
        member: path.join(databaseRoot, "member.rs"),
        databaseLifecycle: path.join(databaseRoot, "lifecycle.rs"),
        ai: path.join(databaseRoot, "ai.rs"),
    }
    const source = Object.fromEntries(Object.entries(files).map(([name, file]) => [name, readSource(file)]))
    const required = createCollector(repositoryRoot)
    const policy = createCollector(repositoryRoot)

    const poll = extractFunction(source.session, "poll", "rust")
    const removeClosed = extractFunction(source.session, "remove_closed_clients", "rust")
    const flushDisconnects = extractFunction(source.session, "flush_expired_battle_disconnects", "rust")
    const readFrames = extractFunction(source.transport, "read_client_frames", "rust")
    const handleHandshake = extractFunction(source.meeting, "handle_handshake", "rust")
    const handleLobby = extractFunction(source.meeting, "handle_lobby", "rust")
    const disconnectBattle = extractFunction(source.meeting, "disconnect_previous_battle_session", "rust")
    const handleBattle = extractFunction(source.battle, "handle_battle", "rust")
    const broadcastBattle = extractFunction(source.battle, "broadcast_battle", "rust")
    const sendBattle = extractFunction(source.battle, "send_battle_to_connections", "rust")
    const startBattle = extractFunction(source.battle, "start_battle_if_ready", "rust")
    const pollSequences = extractFunction(source.lifecycle, "poll_pending_lobby_sequences", "rust")
    const joinPhase = extractBlockAfter(pollSequences, /PendingLobbyPhase::Join\s*=>/, "NPC join phase")
    const pollRoomEvents = extractFunction(source.lifecycle, "poll_room_events", "rust")
    const createRoom = extractFunction(source.room, "create_multiplayer_room", "rust")
    const listRooms = extractFunction(source.room, "list_multiplayer_rooms", "rust")
    const joinRoom = extractFunction(source.room, "join_multiplayer_room", "rust")
    const disbandRoom = extractFunction(source.databaseLifecycle, "disband_multiplayer_room", "rust")
    const leaveLobby = extractFunction(source.member, "leave_multiplayer_lobby", "rust")
    const trimAi = extractFunction(source.ai, "trim_multiplayer_ai_mates_to_capacity", "rust")
    const nameAi = extractFunction(source.ai, "name_multiplayer_ai_mates", "rust")

    required.add("session.transport.maxFrameBytes", source.transport.includes("4 * 1024 * 1024") ? 4_194_304 : null,
        files.transport, source.transport, source.transport.indexOf("MAX_FRAME_BYTES"))
    required.add("session.transport.ignoresWhitespaceFrames", verified(/raw\.iter\(\)\.all\(u8::is_ascii_whitespace\)/.test(readFrames.source)),
        files.transport, source.transport, readFrames.start)
    required.add("session.transport.closesOversizeFrames", verified(hasAll(readFrames.source, [
        /client\.buffer\.len\(\) > MAX_FRAME_BYTES/,
        /client\.peer_closed = true/,
    ])), files.transport, source.transport, readFrames.start)
    required.add("session.transport.closesMalformedJson", verified(hasAll(readFrames.source, [
        /serde_json::from_slice/,
        /client\.close_after_write = true/,
    ])), files.transport, source.transport, readFrames.start)
    required.add("session.transport.ordersFramesPerConnection", verified(/for \(client_index, frame\) in frames/.test(poll.source)),
        files.session, source.session, poll.start)
    required.add("session.transport.dispatchStates", hasAll(source.session, [
        /SessionState::Handshake => self\.handle_handshake/,
        /SessionState::Lobby \{ \.\. \} => self\.handle_lobby/,
        /SessionState::Battle \{ \.\. \} => self\.handle_battle/,
    ]) ? ["battle", "handshake", "lobby"] : null, files.session, source.session,
    source.session.indexOf("fn handle_frame"))

    required.add("session.handshake.denialFrames", /reason == "HANDSHAKE_DENIED" \{ 3 \} else \{ 1 \}/.test(source.meeting)
        ? ["DENIED:1", "HANDSHAKE_DENIED:3"] : null,
    files.meeting, source.meeting, source.meeting.indexOf("fn deny"))
    required.add("session.handshake.lobbyAcceptedFields",
        ["questCategory", "questId", "quest_category", "quest_id", "roomNumber", "room_number", "viewerId", "viewer_id"].sort(),
    files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.battleAcceptedFields",
        ["connectionId", "connection_id", "roomNumber", "room_number"].sort(),
    files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.requiresReconnectInteger", verified(/get\("reconnected"\)\.and_then\(Value::as_i64\)\.is_none\(\)/.test(handleHandshake.source)),
        files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.roomNumberShape", /value\.len\(\) == 6/.test(source.lobbyPlayer)
        ? "six-digit" : null, files.lobbyPlayer, source.lobbyPlayer, source.lobbyPlayer.indexOf("is_room_number"))
    required.add("session.handshake.connectionIdShape", /value\.len\(\) == 32/.test(source.lobbyPlayer)
        ? "32-lowercase-hex" : null, files.lobbyPlayer, source.lobbyPlayer, source.lobbyPlayer.indexOf("is_connection_id"))
    required.add("session.handshake.lobbyChecksRoomMemberQuestAndIdentity", verified(hasAll(handleHandshake.source, [
        /database\.multiplayer_room\(&room_number\)/,
        /database\.multiplayer_member\(&room_number, viewer_id\)/,
        /room\.category_id != category_id/,
        /room\.quest_id != quest_id/,
        /lookup_viewer_session_player/,
    ])), files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.lobbyBindsRoomSequence", verified(/room_sequence: room\.room_sequence/.test(handleHandshake.source)),
        files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.lobbySupersedesPreviousViewer", verified(/disconnect_previous_viewer_sessions/.test(handleHandshake.source)),
        files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.issuesRandomConnectionId", /let mut bytes = \[0_u8; 16\]/.test(source.transport)
        ? "16-random-bytes" : null, files.transport, source.transport, source.transport.indexOf("random_connection_id"))
    required.add("session.handshake.lobbyReply", /json!\(\[0, connection_id, room_number\]\)/.test(handleHandshake.source)
        ? "accept-connection-room" : null, files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.battleChecksConnectionAndStartedRoom", verified(hasAll(handleHandshake.source, [
        /multiplayer_member_by_connection/,
        /!room\.battle_started/,
        /!room\.lobby_started/,
    ])), files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.battleSupersedesPreviousSocket", verified(/disconnect_previous_battle_session/.test(handleHandshake.source)),
        files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.battleResetsSceneReady", verified(hasAll(handleHandshake.source, [
        /set_multiplayer_member_scene_ready/,
        /scene_ready: false/,
    ])), files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.battleReply", /json!\(\[0, room_number, ""\]\)/.test(handleHandshake.source)
        ? "accept-room" : null, files.meeting, source.meeting, handleHandshake.start)
    required.add("session.handshake.unknownSockletCloses", verified(/_ => self\.deny\(client_index, "DENIED"\)/.test(handleHandshake.source)),
        files.meeting, source.meeting, handleHandshake.start)

    required.add("session.room.maxMembers", /MAX_ROOM_MEMBERS: i64 = 3/.test(source.database) ? 3 : null,
        files.database, source.database, source.database.indexOf("MAX_ROOM_MEMBERS"))
    required.add("session.room.numberShape", /format!\("\{:06\}"/.test(createRoom.source) ? "six-digit" : null,
        files.room, source.room, createRoom.start)
    required.add("session.room.hostInsertedOnCreate", verified(/INSERT INTO multiplayer_room_members/.test(createRoom.source)),
        files.room, source.room, createRoom.start)
    required.add("session.room.sequencePreventsNumberReuse", verified(/allocate_room_sequence/.test(createRoom.source) &&
        /room_sequence/.test(source.database)), files.room, source.room, createRoom.start)
    required.add("session.room.listFiltersCategory", verified(/rooms\.category_id = \?1/.test(listRooms.source)),
        files.room, source.room, listRooms.start)
    required.add("session.room.joinRejectsFull", verified(/available_member_index/.test(joinRoom.source) && /return Ok\(None\)/.test(joinRoom.source)),
        files.room, source.room, joinRoom.start)
    required.add("session.room.rejoinPreservesConnection", verified(/if !existing/.test(joinRoom.source)),
        files.room, source.room, joinRoom.start)
    required.add("session.room.disbandRequiresOwner", verified(/host_account_id = \?2/.test(disbandRoom.source)),
        files.databaseLifecycle, source.databaseLifecycle, disbandRoom.start)
    required.add("session.room.expiryMinutes", hasAll(source.databaseLifecycle, [
        /15 \* 60 \* 1_000/,
        /30 \* 60 \* 1_000/,
    ]) ? [15, 30] : null, files.databaseLifecycle, source.databaseLifecycle,
    source.databaseLifecycle.indexOf("INCOMPLETE_ROOM_LIFETIME_MS"))

    required.add("session.lobby.serverBroadcastIncludesAllActiveSockets", verified(/for client in &mut self\.clients/.test(source.meeting) &&
        !/exclude/.test(extractFunction(source.meeting, "broadcast_lobby", "rust").source)),
    files.meeting, source.meeting, source.meeting.indexOf("fn broadcast_lobby"))
    required.add("session.lobby.malformedFrameCloses", verified(hasAll(handleLobby.source, [
        /meeting_command\(&frame\)/,
        /self\.close_client\(client_index\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.enterWelcome", verified(/json!\(\[1, \[0, welcome_context, welcome_roster\]\]\)/.test(handleLobby.source)),
        files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.enterBroadcastsRosterAndInitialReady", verified(hasAll(handleLobby.source, [
        /json!\(\[1, \[1, roster\]\]\)/,
        /json!\(\[1, \[2, connection_id, \[1\]\]\]\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.heartbeatRequiresNoArguments", verified(/Some\(4\)[\s\S]*?command\.len\(\) != 1/.test(handleLobby.source)),
        files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.heartbeatRepliesConnection", verified(/json!\(\[1, \[if legacy_protocol \{ 11 \} else \{ 10 \}, connection_id\]\]\)/.test(handleLobby.source)),
        files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.changePartyValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /command\.len\(\) != 4/,
        /change_multiplayer_member_party/,
        /json!\(\[1, \[1, roster\]\]\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.readyValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /state\.len\(\) != 1/,
        /set_multiplayer_member_ready/,
        /json!\(\[1, \[2, connection_id, \[i64::from\(ready\)\]\]\]\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.autoplayValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /Some\(7\)/,
        /set_multiplayer_member_autoplay/,
        /json!\(\[1, \[3, connection_id, autoplay, speed_up\]\]\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.autoStartValidatesPersistsAndBroadcasts", verified(hasAll(handleLobby.source, [
        /Some\(8\)/,
        /set_multiplayer_member_auto_start/,
        /json!\(\[1, \[4, connection_id, auto_start\]\]\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.suspendClearsReadyAndBroadcasts", verified(hasAll(handleLobby.source, [
        /Some\(5\)/,
        /set_multiplayer_member_ready\(&room_number, viewer_id, false\)/,
        /json!\(\[1, \[2, connection_id, \[0\]\]\]\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.enterComsRequiresHostAndValidatedPayload", verified(hasAll(handleLobby.source, [
        /Some\(10\)/,
        /!is_host/,
        /validate_ai_names|validate_ai_requests/,
        /name_multiplayer_ai_mates/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.startRequiresHostReadyRoster", verified(hasAll(handleLobby.source, [
        /Some\(6\)/,
        /!is_host/,
        /all_human_members_ready/,
        /json!\(\[1, \[5, roster\]\]\)/,
    ])), files.meeting, source.meeting, handleLobby.start)
    required.add("session.lobby.byeRepliesRosterAndCloses", verified(hasAll(handleLobby.source, [
        /Some\(1\)/,
        /room\.lobby_started/,
        /close_after_write = true/,
    ])), files.meeting, source.meeting, handleLobby.start)

    required.add("session.ai.maxMates", /MAX_ROOM_MEMBERS - human_count - 1/.test(source.room) ? 2 : null,
        files.room, source.room, source.room.indexOf("ai_capacity"))
    required.add("session.ai.totalRosterCapacity", /MAX_ROOM_MEMBERS: i64 = 3/.test(source.database) ? 3 : null,
        files.database, source.database, source.database.indexOf("MAX_ROOM_MEMBERS"))
    required.add("session.ai.stageRequiresCurrentRoomAndActiveHost", verified(hasAll(pollSequences.source, [
        /room\.room_sequence == sequence\.room_sequence/,
        /has_active_host_lobby/,
    ])), files.lifecycle, source.lifecycle, pollSequences.start)
    required.add("session.ai.requestPayloadValidated", verified(/validate_ai_names|validate_ai_requests/.test(handleLobby.source) &&
        /name_multiplayer_ai_mates/.test(nameAi.source)), files.meeting, source.meeting, handleLobby.start)
    required.add("session.ai.joinReadyAndCountdownFrames", verified(hasAll(source.lifecycle, [
        /json!\(\[1, \[1, roster\]\]\)/,
        /json!\(\[\s*1,[\s\S]*?2,[\s\S]*?\[1\]/,
        /broadcast_start_remaining_time/,
    ])), files.lifecycle, source.lifecycle, pollSequences.start)
    required.add("session.ai.selectionConsumedAndRosterFrozen", verified(/DELETE FROM multiplayer_ai_mates/.test(trimAi.source) &&
        /UPDATE multiplayer_ai_mates/.test(nameAi.source)), files.ai, source.ai, trimAi.start)

    required.add("session.battle.malformedFrameCloses", verified(hasAll(handleBattle.source, [
        /frame\.as_array\(\)/,
        /self\.close_client\(client_index\)/,
    ])), files.battle, source.battle, handleBattle.start)
    required.add("session.battle.sceneReadyWaitsForAllParticipants", verified(hasAll(handleBattle.source, [
        /set_multiplayer_member_scene_ready/,
        /start_battle_if_ready/,
        /expected_viewers\.is_subset\(&ready_viewers\)/,
    ]) || hasAll(handleBattle.source, [/start_battle_if_ready/]) && /expected_viewers\.is_subset/.test(startBattle.source)),
    files.battle, source.battle, handleBattle.start)
    required.add("session.battle.finalizeAcknowledgesAndCloses", verified(hasAll(handleBattle.source, [
        /finalized = true/,
        /json!\(\[1, \[2\]\]\)/,
    ])), files.battle, source.battle, handleBattle.start)
    const measurementShapes = []
    if (/fn flat_battle_measurement/.test(source.battle)) measurementShapes.push("flat")
    if (/fn battle_measurement/.test(source.battle)) measurementShapes.push("pair")
    required.add("session.battle.measurementShapes", measurementShapes.length > 0 ? measurementShapes : null,
        files.battle, source.battle, source.battle.indexOf("battle_measurement"))
    required.add("session.battle.measurementAcknowledgesServerTime", verified(/current_server_time_millis/.test(source.battle)),
        files.battle, source.battle, source.battle.indexOf("send_battle_measurement_ack"))
    required.add("session.battle.broadcastValidatesAndScopesRoomSequence", verified(/valid_broadcast_messages/.test(handleBattle.source) &&
        /room_sequence/.test(broadcastBattle.source)), files.battle, source.battle, handleBattle.start)
    required.add("session.battle.sendValidatesAndScopesRoomSequence", verified(hasAll(handleBattle.source, [
        /valid_send_message/,
        /valid_targets/,
    ]) && /room_sequence/.test(sendBattle.source)), files.battle, source.battle, handleBattle.start)
    required.add("session.battle.heartbeatAndLineWarningKeepConnection", verified(hasAll(handleBattle.source, [
        /Some\(4\)\s*if notify\.len\(\) == 2/,
        /Some\(5\) if notify\.len\(\) == 1 => \{\}/,
    ])), files.battle, source.battle, handleBattle.start)
    required.add("session.battle.unmodeledNumericFramesKeepConnection", verified(/Some\(_\) => \{\}/.test(handleBattle.source)),
        files.battle, source.battle, handleBattle.start)
    required.add("session.battle.nonfinalizedDisconnectBroadcastsLeave", verified(hasAll(removeClosed.source, [
        /if !finalized/,
        /pending_battle_disconnects/,
    ]) && /json!\(\[1, \[0, disconnect\.connection_id\]\]\)/.test(flushDisconnects.source)),
    files.session, source.session, removeClosed.start)
    required.add("session.battle.leaveExcludesSenderAndScopesSequence", verified(/broadcast_battle/.test(flushDisconnects.source) &&
        /room_sequence/.test(flushDisconnects.source)), files.session, source.session, flushDisconnects.start)
    required.add("session.battle.reconnectSupersedesSocketWithoutLeave", verified(/disconnect_previous_battle_session/.test(handleHandshake.source) &&
        /active_connection == connection_id/.test(removeClosed.source)),
    files.meeting, source.meeting, disconnectBattle.start)

    required.add("session.cleanup.lobbyDisconnectClearsState", verified(/leave_multiplayer_lobby/.test(removeClosed.source) &&
        /entered = 0/.test(leaveLobby.source)), files.session, source.session, removeClosed.start)
    required.add("session.cleanup.roomSequencePreventsStaleReuse", verified(hasAll(pollRoomEvents.source, [
        /MultiplayerRoomEventKind::Dismissed/,
        /close_after_write = true/,
        /room_sequence == event\.room_sequence/,
    ])), files.lifecycle, source.lifecycle, pollRoomEvents.start)

    policy.add("session.policy.roomCreateReplacesExistingHostRoom",
        /DELETE FROM multiplayer_rooms WHERE host_account_id = \?1/.test(createRoom.source),
        files.room, source.room, createRoom.start)
    let aiJoinDeliveryScope = null
    if (/broadcast_lobby/.test(joinPhase.source)) aiJoinDeliveryScope = "room"
    else if (/send_to_lobby_viewer/.test(joinPhase.source)) aiJoinDeliveryScope = "requesting-viewer"
    policy.add("session.policy.aiJoinDeliveryScope", aiJoinDeliveryScope,
        files.lifecycle, source.lifecycle, joinPhase.start)
    policy.add("session.policy.lobbyHeartbeatReplyTag", /legacy_protocol \{ 11 \}/.test(handleLobby.source) ? 11 : null,
        files.meeting, source.meeting, handleLobby.start)
    policy.add("session.policy.lobbyStartRemainingTag", /legacy_protocol \{ 10 \}/.test(source.lifecycle) ? 10 : null,
        files.lifecycle, source.lifecycle, source.lifecycle.indexOf("broadcast_start_remaining_time"))
    policy.add("session.policy.lobbyReadyEvaluatesAutoStart", /evaluate_lobby_readiness/.test(handleLobby.source),
        files.meeting, source.meeting, handleLobby.start)
    policy.add("session.policy.lobbyUnknownCommand", /_ => \{\}/.test(handleLobby.source)
        ? "ignore" : null, files.meeting, source.meeting, handleLobby.start)
    policy.add("session.policy.battleBroadcastRecipients", !/connection_id\s*!=/.test(broadcastBattle.source)
        ? "room-including-sender" : null, files.battle, source.battle, broadcastBattle.start)
    policy.add("session.policy.battleSendRecipients", /target_connection_ids\.contains\(connection_id\)/.test(sendBattle.source)
        ? "listed-connection-ids" : null, files.battle, source.battle, sendBattle.start)
    policy.add("session.policy.battleDisconnectGraceMs", source.session.includes("Duration::from_secs(2)") ? 2_000 : null,
        files.session, source.session, source.session.indexOf("BATTLE_RECONNECT_GRACE"))

    return finishCollectors(required, policy)
}
// //// /抽取 Rust 会话状态机场景 ////
