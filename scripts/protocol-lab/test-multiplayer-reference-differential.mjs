// audience: internal
// # multiplayer-reference-differential-test
//
// 该测试用最小参考与本地源码验证协议事实抽取和自动差分.

import assert from "node:assert/strict"
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { buildMultiplayerDifferential } from "./audit-multiplayer-reference-differential.mjs"

function writeSource(root, relativePath, source) {
    const filePath = path.join(root, ...relativePath.split("/"))
    mkdirSync(path.dirname(filePath), { recursive: true })
    writeFileSync(filePath, source, "utf8")
}

// //// 构造可区分的协议源码 [@x380kkm 2026-08-24] ////
function writeReferenceFixture(root) {
    writeSource(root, "out/multi/tcp/server.js", `
function startSessionServer(socket) {
    socket.setEncoding("utf8")
    let buffer = ""
    if (buffer.includes("\\0")) JSON.parse(buffer)
}
`)
    writeSource(root, "out/multi/state/SessionManager.js", `
class SessionManager {
    sendJson(socket, data) { socket.write(JSON.stringify(data) + "\\0") }
    broadcastToRoom(roomNumber, data, excludeAddr) {
        for (const addr of clients) {
            if (excludeAddr !== undefined && addr === excludeAddr) continue
            this.sendJson(socket, data)
        }
    }
}
`)
    writeSource(root, "out/multi/tcp/handshake.js", `
function handleHandshake(socket, data) {
    const socklet = data.socklet
    if (socklet === "cooperation_battle") return [0, data.room_number, ""]
    if (socklet === "cooperation_room") return [0, data.connection_id, data.room_number]
}
`)
    writeSource(root, "out/multi/tcp/lobby.js", `
const NPC_JOIN_DELAY_MS = parseInt(process.env.NPC_JOIN_DELAY_MS || "2000")
const NPC_READY_DELAY_MS = parseInt(process.env.NPC_READY_DELAY_MS || "500")
function handleMessage(socket, data) { switch (data[0]) { case 0: break; case 1: handleBroadcast(socket, data); break; case 2: handleSend(socket, data); break } }
function handleNotify(socket, data) { switch (data[0]) { case 0: break; case 1: break; case 2: break; case 3: break; case 4: break; case 6: break; case 10: break } }
function handleBroadcast(_socket, client, data) { sessionManager.broadcastToRoom(client.roomNumber, data) }
function handleSend(_socket, client, data) { const targetViewerId = data[1]; for (const c of clients) if (c.viewerId === targetViewerId) sessionManager.sendJson(c.socket, data) }
function handleReady() { checkHostAutoReady() }
function handleChangeParty() { updatePlayerSync({ partySlot: pd.currentPartyId }) }
function handleEnterComs(client) {
    const needNPCs = 3 - realMates.length
    setTimeout(() => sessionManager.sendJson(client.socket, [1, [1, client.mates]]), NPC_JOIN_DELAY_MS)
    setTimeout(() => { sessionManager.broadcastToRoom(client.roomNumber, [1, [2, npc.connectionId, [1]]]); checkHostAutoReady() }, NPC_JOIN_DELAY_MS + NPC_READY_DELAY_MS)
}
`)
    writeSource(root, "out/multi/tcp/battle.js", `
function handleBattleNotify(socket, data) {
    switch (data[0]) {
        case BattleNotifyKind.SceneReady: sessionManager.markSceneReady(); sessionManager.sendJson(socket, [1, [1]]); break
        case BattleNotifyKind.LevelNext: break
        case BattleNotifyKind.Finalize: sessionManager.sendJson(socket, [1, [2]]); break
        case BattleNotifyKind.Measurement: { const frame = data[1]; const clientTime = data[2]; sessionManager.sendJson(socket, [1, [3, frame, clientTime, Date.now()]]); break }
        case BattleNotifyKind.LineSpeedWarning:
        case BattleNotifyKind.Heartbeat: break
    }
}
function handleBattleMessage(socket, data) {
    switch (data[0]) {
        case ClientMessageKind.Notify: handleBattleNotify(socket, data[1]); break
        case ClientMessageKind.Broadcast: { const message = [2, client.connectionId, bcData]; relayToBattleRoom(room, client.connectionId, message); sessionManager.sendJson(socket, message); break }
        case ClientMessageKind.Send: relayToBattleRoom(room, client.connectionId, [3, client.connectionId, sendMsg]); break
    }
}
`)
    writeSource(root, "out/multi/tcp/relay.js", `
function relayToBattleRoom(roomNumber, sourceCid, data) { for (const cid of clients) { if (cid === sourceCid) continue; send(data) } }
`)
    writeSource(root, "out/multi/npc/types.js", `
const NPC_TEMPLATES = [
    {
        com_id: 1,
        characters: [131012, 141007, 151001],
        unison_characters: [null, null, null],
        equipments: [300101, 300201, 300301],
        ability_soul_ids: [null, null, null],
        rank: 80,
        degree_id: 1,
    },
    {
        com_id: 2,
        characters: [141004, 121002, 161001],
        unison_characters: [null, null, null],
        equipments: [300101, 300201, 300301],
        ability_soul_ids: [null, null, null],
        rank: 80,
        degree_id: 2000,
    },
]
`)
    writeSource(root, "out/multi/npc/builder.js", `
function buildNpcParty(template) {
    const characters = template.characters.map((id) => ({ id, evolution_level: 5 }))
    const equipments = template.equipments.map((id) => ({ id, level: 1 }))
    const unisonCharacters = [...template.unison_characters]
    while (unisonCharacters.length < 3) unisonCharacters.push(null)
    const abilitySoulIds = [...template.ability_soul_ids]
    while (abilitySoulIds.length < 3) abilitySoulIds.push(null)
    return { characters, equipments, unisonCharacters, abilitySoulIds }
}
`)
}

function writeLocalFixture(root) {
    writeSource(root, "core/personal-service/src/cn_multiplayer/transport.rs", `
fn read_client_frames() { client.buffer.iter().position(|byte| *byte == 0); serde_json::from_slice(&raw); }
fn queue_frame() { serde_json::to_writer(&mut encoded, frame); encoded.push(0); }
fn meeting_command(frame: &Value) { frame[0].as_i64() == Some(0); }
`)
    writeSource(root, "core/personal-service/src/cn_multiplayer/meeting.rs", `
fn handle_handshake() { match socklet { Some("cooperation_room") => {}, Some("cooperation_battle") => {}, _ => {} } }
fn handle_lobby() {
    if let Some(data) = frame.as_array().filter(|data| data.first().and_then(Value::as_i64) == Some(1)) {
        broadcast_lobby(json!([2, connection_id, messages]));
    }
    if let Some(data) = frame.as_array().filter(|data| data.first().and_then(Value::as_i64) == Some(2)) {
        let target_viewer_id = data[1];
        self.send_to_lobby_viewer(room, sequence, target_viewer_id, &frame)?;
    }
    let Some(command) = meeting_command(&frame) else { return; };
    match command.first().and_then(Value::as_i64) {
        Some(0) => {}
        Some(1) => {}
        Some(2) => { enter_multiplayer_lobby(); }
        Some(3) => { set_multiplayer_member_ready(); }
        Some(4) => {}
        Some(6) => {}
        Some(10) => {}
        _ => {}
    }
}
fn broadcast_lobby() { for client in &mut self.clients { queue_frame(client, frame); } }
fn send_to_lobby_viewer(viewer_id: i64) { if client_viewer == viewer_id { queue_frame(client, frame); } }
`)
    writeSource(root, "core/personal-service/src/cn_multiplayer/lobby_player.rs", `
fn normalize_lobby_player() { if member.as_ref().map(|member| member.party_id) != Some(party_id) { return; } }
`)
    writeSource(root, "core/personal-service/src/cn_multiplayer/battle.rs", `
fn handle_battle() {
    match data.first().and_then(Value::as_i64) {
        Some(0) => {
            match notify.first().and_then(Value::as_i64) {
                Some(0) if notify.len() == 1 => { scene_ready = true; self.start_battle_if_ready(); }
                Some(1) if notify.len() == 1 => { finalized = true; queue_frame(json!([1, [2]])); }
                Some(2) if legacy_protocol && notify.len() == 1 => { finalized = true; queue_frame(json!([1, [2]])); }
                Some(2) => { let values = battle_measurement(notify); send_battle_measurement_ack(values); }
                Some(3) if legacy_protocol && notify.len() == 3 => { let values = flat_battle_measurement(notify); send_battle_measurement_ack(values); }
                Some(3 | 4) if notify.len() == 2 => {}
                Some(4 | 5) if notify.len() == 1 => {}
                _ => {}
            }
        }
        Some(1) => { self.broadcast_battle(room, json!([2, connection_id, messages])); }
        Some(2) => { let target_connection_ids = ids; self.send_battle_to_connections(room, &target_connection_ids, json!([3, connection_id, message])); }
        _ => {}
    }
}
fn battle_measurement(notify: &[Value]) { notify.get(1).and_then(Value::as_array); }
fn flat_battle_measurement(notify: &[Value]) { notify[1]; notify[2]; }
fn send_battle_measurement_ack() { queue_frame(
    &mut self.clients[client_index],
    &json!([ 1, [ 3, 0, 0, now ] ]),
); }
fn start_battle_if_ready() { queue_frame(json!([1, [1]])); }
fn broadcast_battle() { for client in &mut self.clients { queue_frame(client, frame); } }
fn send_battle_to_connections() { for client in &mut self.clients { if target_connection_ids.contains(client_connection) { queue_frame(client, frame); } } }
`)
    writeSource(root, "core/personal-service/src/cn_multiplayer/lifecycle.rs", `
const NPC_JOIN_DELAY: Duration = Duration::from_millis(2_000);
const NPC_READY_DELAY: Duration = Duration::from_millis(500);
fn poll_pending_lobby_sequences() {
    match phase {
        PendingLobbyPhase::Join => { if roster.len() != 3 { return; } broadcast_lobby(); }
        PendingLobbyPhase::Ready => { broadcast_lobby(); set_multiplayer_member_ready(); broadcast_start_remaining_time(); }
    }
}
`)
    writeSource(root, "core/personal-service/src/cn_multi/room.rs", `
struct DefaultAiTemplate {
    com_id: i64,
    character_ids: [i64; 3],
    equipment_ids: [i64; 3],
    rank: i64,
    degree_id: i64,
}

const DEFAULT_AI_TEMPLATES: [DefaultAiTemplate; 2] = [
    DefaultAiTemplate {
        com_id: 1,
        character_ids: [131012, 141007, 151001],
        equipment_ids: [300101, 300201, 300301],
        rank: 80,
        degree_id: 1,
    },
    DefaultAiTemplate {
        com_id: 2,
        character_ids: [141004, 121002, 161001],
        equipment_ids: [300101, 300201, 300301],
        rank: 80,
        degree_id: 2_000,
    },
];

fn create_default_client_party(template: &DefaultAiTemplate) -> Value {
    json!({
        "characters": template.character_ids.map(|id| json!({
            "id": id,
            "evolution_level": 5,
        })),
        "unison_characters": [null, null, null],
        "equipments": template.equipment_ids.map(|equipment_id| json!({
            "equipment_id": equipment_id,
            "level": 1,
        })),
        "ability_soul_ids": [null, null, null],
    })
}
`)
}
// //// /构造可区分的协议源码 ////

// //// 验证事实差分 [@x380kkm 2026-08-24] ////
const fixtureRoot = mkdtempSync(path.join(tmpdir(), "starpoint-multiplayer-differential-"))
try {
    const referenceRoot = path.join(fixtureRoot, "reference")
    const localRoot = path.join(fixtureRoot, "local")
    writeReferenceFixture(referenceRoot)
    writeLocalFixture(localRoot)
    const report = buildMultiplayerDifferential(referenceRoot, localRoot)
    const differences = new Map(report.comparison.differences.map((row) => [row.path, row]))
    const extensions = new Map(report.comparison.extensions.map((row) => [row.path, row]))

    assert.deepEqual(report.reference.protocol.handshake.socklets,
        ["cooperation_battle", "cooperation_room"])
    assert.deepEqual(report.local.protocol.lobby.outerCommands, [0, 1, 2])
    assert.equal(differences.has("lobby.outerCommands"), false)
    assert.equal(differences.has("lobby.forwarding.send.recipients"), false)
    assert.equal(differences.has("lobby.forwarding.send.payload"), false)
    assert.equal(differences.get("lobby.forwarding.broadcast.payload")?.local,
        "messages-with-sender")
    assert.deepEqual(report.reference.protocol.battle.finalize.requestTags, [2])
    assert.deepEqual(report.local.protocol.battle.finalize.requestTags, [2])
    assert.equal(differences.has("battle.finalize.requestTags"), false)
    assert.equal(differences.has("battle.finalize.reply"), false)
    assert.equal(differences.has("battle.sceneReady.reply"), false)
    assert.equal(differences.get("battle.levelNext.action")?.reference, "ignore")
    assert.equal(differences.get("battle.levelNext.action")?.local, "finalize")
    assert.equal(report.reference.protocol.battle.forwarding.broadcast.recipients,
        "room-including-sender")
    assert.equal(report.local.protocol.battle.forwarding.broadcast.recipients,
        "room-including-sender")
    assert.equal(differences.has("battle.forwarding.broadcast.recipients"), false)
    assert.equal(differences.has("battle.forwarding.broadcast.senderAck"), false)
    assert.equal(differences.get("battle.forwarding.send.recipients")?.reference,
        "room-excluding-sender")
    assert.equal(differences.get("battle.forwarding.send.recipients")?.local,
        "listed-connection-ids")
    assert.equal(differences.has("battle.forwarding.send.senderAck"), false)
    assert.equal(report.local.protocol.battle.forwarding.broadcast.senderAck, false)
    assert.deepEqual(report.reference.protocol.battle.notifyTags, [0, 1, 2, 3, 4, 5])
    assert.deepEqual(report.local.protocol.battle.notifyTags, [0, 1, 2, 3, 4, 5])
    assert.equal(extensions.has("battle.notifyTags"), false)
    for (const factPath of [
        "battle.measurement.requestTags",
        "battle.measurement.requestShape",
        "battle.measurement.reply",
        "battle.heartbeat.requestTags",
        "battle.heartbeat.reply",
    ]) assert.equal(differences.has(factPath), false)
    assert.equal(differences.get("ai.join.deliveryScope")?.reference, "requesting-client")
    assert.equal(differences.get("ai.join.deliveryScope")?.local, "room")
    assert.equal(report.local.protocol.ai.ready.totalDelayMs, 2500)
    assert.deepEqual(report.reference.protocol.ai.defaultTemplates, [
        {
            comId: 1,
            characterIds: [131012, 141007, 151001],
            equipmentIds: [300101, 300201, 300301],
            rank: 80,
            degreeId: 1,
        },
        {
            comId: 2,
            characterIds: [141004, 121002, 161001],
            equipmentIds: [300101, 300201, 300301],
            rank: 80,
            degreeId: 2000,
        },
    ])
    for (const factPath of [
        "ai.defaultTemplates",
        "ai.defaultParty.characterEvolutionLevel",
        "ai.defaultParty.equipmentLevel",
        "ai.defaultParty.unisonSlots",
        "ai.defaultParty.abilitySoulSlots",
    ]) assert.equal(differences.has(factPath), false)
} finally {
    rmSync(fixtureRoot, { recursive: true, force: true })
}
// //// /验证事实差分 ////
