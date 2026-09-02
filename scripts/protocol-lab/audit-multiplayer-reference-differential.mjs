// audience: internal
// # multiplayer-reference-differential
//
// 该脚本从 startpoint-cn 编译产物, TypeScript 会话状态机和个人服务源码抽取 TCP/AI 协议事实.
// 检测到 CN 1.8.x 客户端证据时, 数字索引和客户端自断开语义以客户端为准.

import { existsSync, readFileSync, readdirSync } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import {
    compact,
    compareFacts,
    createCollector,
    extractBlockAfter,
    extractFunction,
    extractJavaScriptMethod,
    jsSwitchArms,
    materializeFacts,
    numericPatternValues,
    numericRustTags,
    rustArmCallsSenderAck,
    rustArmRecipientScope,
    rustMatchArms,
} from "./multiplayer-protocol-differential-lib.mjs"
import { collectSessionScenarioDifferential } from "./multiplayer-session-scenario-differential.mjs"

const SCRIPT_ROOT = path.dirname(fileURLToPath(import.meta.url))
const DEFAULT_REPOSITORY_ROOT = path.resolve(SCRIPT_ROOT, "../..")

// //// 声明 CN 1.8.4 战斗枚举 [@x380kkm 2026-08-25] ////
const CN_CLIENT_BATTLE_MESSAGE_KIND = {
    Notify: 0,
    Broadcast: 1,
    Send: 2,
}
const CN_CLIENT_BATTLE_NOTIFY_KIND = {
    SceneReady: 0,
    LevelNext: 1,
    Finalize: 2,
    Measurement: 3,
    LineSpeedWarning: 4,
    Heartbeat: 5,
}
const CN_SERVER_BATTLE_NOTIFY_KIND = {
    BattleStart: 1,
    Finalized: 2,
    Measurement: 3,
}
// //// /声明 CN 1.8.4 战斗枚举 ////

// //// 读取审计输入 [@x380kkm 2026-08-24] ////
function readRequiredOption(args, name) {
    const optionIndex = args.indexOf(name)
    const value = args[optionIndex + 1]
    if (optionIndex < 0 || !value || value.startsWith("--")) {
        throw new Error(`missing required option: ${name}`)
    }
    return value
}

function readOptionalOption(args, name) {
    const optionIndex = args.indexOf(name)
    if (optionIndex < 0) return null
    const value = args[optionIndex + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing option value: ${name}`)
    return value
}

function readSource(filePath) {
    if (!existsSync(filePath)) throw new Error(`missing multiplayer source: ${filePath}`)
    return readFileSync(filePath, "utf8")
}
// //// /读取审计输入 ////

// //// 抽取 CN 1.8.x 客户端协议 [@x380kkm 2026-08-25] ////
const TARGET_CLIENT_FILES = {
    battleNotify: "ffdec-battle-flow/as/scripts/pinball/online/battle/message/BattleNotifyMessage.as",
    battleNotifyPcode: "ffdec-battle-flow/pcode/scripts/pinball/online/battle/message/BattleNotifyMessage.pcode",
    battleServerPcode: "ffdec-battle-flow/pcode/scripts/pinball/online/battle/message/BattleServerMessage.pcode",
    dummyTransmitter: "ffdec-battle-flow/as/scripts/pinball/context/socket/battle/DummyBattleTransmitter.as",
    realTransmitter: "ffdec-battle-flow/as/scripts/pinball/context/socket/battle/RealBattleTransmitter.as",
    battleConnection: "ffdec-battle-flow/as/scripts/pinball/context/socket/battle/BattleConnection.as",
    battleSocketContact: "ffdec-battle-flow/as/scripts/pinball/context/socket/battle/BattleSocketContact.as",
    battlePlayingState: "ffdec-battle-flow/as/scripts/pinball/scene/battle/state/BattleScenePlayingStateImpl.as",
    typePacker: "swf/V1.8.1_assets_worldflipper_android_release.swf/source/scripts/TypePackerResource2.as",
}

function targetClientFile(root, relativePath) {
    return path.join(root, ...relativePath.split("/"))
}

function findTargetClientRoot(repositoryRoot) {
    const analysisRoot = path.resolve(
        repositoryRoot,
        "..",
        "artifacts",
        "protocol-lab",
        "client-analysis",
    )
    if (!existsSync(analysisRoot)) return null
    const candidates = readdirSync(analysisRoot, { withFileTypes: true })
        .filter((entry) => entry.isDirectory() && /^cn-v1\.8\./.test(entry.name))
        .map((entry) => path.join(analysisRoot, entry.name))
        .sort()
        .reverse()
    return candidates.find((candidate) => Object.values(TARGET_CLIENT_FILES)
        .every((relativePath) => existsSync(targetClientFile(candidate, relativePath)))) ?? null
}

function pcodeEnumIndex(source, name) {
    const match = new RegExp(`pushstring "${name}"[\\s\\S]{0,160}?pushshort (\\d+)`).exec(source)
    if (!match) throw new Error(`missing client enum index: ${name}`)
    return Number(match[1])
}

function stringMapIndex(source, name) {
    const match = new RegExp(
        `(?:setReserved\\("${name}",(\\d+)\\)|h\\["${name}"\\]\\s*=\\s*(\\d+))`,
    ).exec(source)
    if (!match) throw new Error(`missing client string-map index: ${name}`)
    return Number(match[1] ?? match[2])
}

function switchArm(arms, tag, label) {
    const arm = arms.find((candidate) => candidate.tag === tag)
    if (!arm) throw new Error(`missing client switch arm: ${label}`)
    return arm
}

function assertClientEvidence(condition, label) {
    if (!condition) throw new Error(`invalid CN 1.8.x client evidence: ${label}`)
}

function parseTargetClientContract(repositoryRoot) {
    const clientRoot = findTargetClientRoot(repositoryRoot)
    if (clientRoot === null) return null
    const files = Object.fromEntries(Object.entries(TARGET_CLIENT_FILES)
        .map(([name, relativePath]) => [name, targetClientFile(clientRoot, relativePath)]))
    files.typeScriptSession = path.join(repositoryRoot, "src", "multiplayer", "sessionServer.ts")
    const source = Object.fromEntries(Object.entries(files).map(([name, file]) => [name, readSource(file)]))

    const clientNotify = {
        SceneReady: pcodeEnumIndex(source.battleNotifyPcode, "SceneReady"),
        LevelNext: pcodeEnumIndex(source.battleNotifyPcode, "LevelNext"),
        Finalize: pcodeEnumIndex(source.battleNotifyPcode, "Finalize"),
        Measurement: pcodeEnumIndex(source.battleNotifyPcode, "Measurement"),
        LineSpeedWarning: pcodeEnumIndex(source.battleNotifyPcode, "LineSpeedWarning"),
        Heartbeat: pcodeEnumIndex(source.battleNotifyPcode, "Heartbeat"),
    }
    const serverNotify = {
        BattleStart: pcodeEnumIndex(source.battleServerPcode, "BattleStart"),
        Finalized: pcodeEnumIndex(source.battleServerPcode, "Finalized"),
        Measurement: pcodeEnumIndex(source.battleServerPcode, "Measurement"),
    }
    const expectedClientNotify = Object.values(CN_CLIENT_BATTLE_NOTIFY_KIND)
    assertClientEvidence(
        JSON.stringify(Object.values(clientNotify)) === JSON.stringify(expectedClientNotify),
        "battle notify indexes",
    )
    assertClientEvidence(
        JSON.stringify(serverNotify) === JSON.stringify(CN_SERVER_BATTLE_NOTIFY_KIND),
        "battle server indexes",
    )

    const dummyNotify = extractFunction(source.dummyTransmitter, "notify", "javascript")
    const dummyNotifyArms = jsSwitchArms(dummyNotify)
    const sceneReadyArm = switchArm(dummyNotifyArms, clientNotify.SceneReady, "SceneReady")
    const levelNextArm = switchArm(dummyNotifyArms, clientNotify.LevelNext, "LevelNext")
    const finalizeArm = switchArm(dummyNotifyArms, clientNotify.Finalize, "Finalize")
    const measurementArm = switchArm(dummyNotifyArms, clientNotify.Measurement, "Measurement")
    const heartbeatArm = switchArm(dummyNotifyArms, clientNotify.Heartbeat, "Heartbeat")
    assertClientEvidence(/BattleServerMessage\.BattleStart/.test(sceneReadyArm.source), "SceneReady reply")
    assertClientEvidence(!/sendToSelf/.test(levelNextArm.source), "LevelNext is ignored")
    assertClientEvidence(/BattleServerMessage\.Finalized/.test(finalizeArm.source), "Finalize reply")
    assertClientEvidence(/BattleServerMessage\.Measurement/.test(measurementArm.source), "Measurement reply")
    assertClientEvidence(!/sendToSelf/.test(heartbeatArm.source), "Heartbeat is unanswered")

    const measurementFactory = extractFunction(source.battleNotify, "Measurement", "javascript")
    assertClientEvidence(
        new RegExp(`BattleNotifyMessage\\("Measurement",${clientNotify.Measurement},\\[param1,param2\\]\\)`).test(
            compact(measurementFactory.source),
        ),
        "flat Measurement payload",
    )
    const battleSocketInput = extractFunction(source.battleSocketContact, "socketInput", "javascript")
    assertClientEvidence(
        new RegExp(`params\\[0\\]\\.index == ${serverNotify.Finalized}[\\s\\S]*?discardSocket\\(\\)`).test(
            battleSocketInput.source,
        ),
        "Finalized disconnect",
    )

    const realSend = extractFunction(source.realTransmitter, "send", "javascript")
    assertClientEvidence(/Client2Server\.Send\(\[param1\],param3\)/.test(compact(realSend.source)), "direct target list")
    const clientBroadcastInput = extractFunction(source.battlePlayingState, "socketInput_broadcastMessage", "javascript")
    assertClientEvidence(
        /otherBattleDerailleurs/.test(clientBroadcastInput.source) && /== param1/.test(clientBroadcastInput.source),
        "sender echo is ignored",
    )
    const typeScriptBroadcast = extractFunction(source.typeScriptSession, "broadcastCnBattleMessages", "javascript")
    const typeScriptSend = extractFunction(source.typeScriptSession, "sendCnBattleMessage", "javascript")
    const typeScriptMates = extractFunction(source.typeScriptSession, "publishCnLobbyMates", "javascript")
    assertClientEvidence(
        /for \(const target of admissions\.values\(\)\)/.test(typeScriptBroadcast.source) &&
            !/target === admission/.test(typeScriptBroadcast.source),
        "room broadcast scope",
    )
    assertClientEvidence(
        /for \(const targetConnectionId of targetConnectionIds\)/.test(typeScriptSend.source),
        "direct send scope",
    )
    assertClientEvidence(
        /for \(const activeAdmission of activeAdmissions\)/.test(typeScriptMates.source),
        "AI roster delivery scope",
    )

    const meetingMap = extractFunction(source.typePacker, "resolveMap949", "javascript")
    const heartbeatReplyTag = stringMapIndex(meetingMap.source, "AckHeartbeat")
    const startRemainingTag = stringMapIndex(meetingMap.source, "StartRemainingTime")

    const required = createCollector(repositoryRoot)
    required.add("battle.notifyTags", Object.values(clientNotify).sort((left, right) => left - right),
        files.battleNotifyPcode, source.battleNotifyPcode, source.battleNotifyPcode.indexOf("__constructs__"))
    required.add("battle.sceneReady.requestTags", [clientNotify.SceneReady],
        files.dummyTransmitter, source.dummyTransmitter, sceneReadyArm.start)
    required.add("battle.sceneReady.reply", `notify-${serverNotify.BattleStart}`,
        files.dummyTransmitter, source.dummyTransmitter, sceneReadyArm.start)
    required.add("battle.levelNext.action", "ignore",
        files.dummyTransmitter, source.dummyTransmitter, levelNextArm.start)
    required.add("battle.finalize.requestTags", [clientNotify.Finalize],
        files.battleNotifyPcode, source.battleNotifyPcode, source.battleNotifyPcode.indexOf('pushstring "Finalize"'))
    required.add("battle.finalize.reply", `notify-${serverNotify.Finalized}`,
        files.dummyTransmitter, source.dummyTransmitter, finalizeArm.start)
    required.add("battle.measurement.requestTags", [clientNotify.Measurement],
        files.battleNotify, source.battleNotify, measurementFactory.start)
    required.add("battle.measurement.requestShape", "tag-frame-time",
        files.battleNotify, source.battleNotify, measurementFactory.start)
    required.add("battle.measurement.reply", `notify-${serverNotify.Measurement}`,
        files.dummyTransmitter, source.dummyTransmitter, measurementArm.start)
    required.add("battle.heartbeat.requestTags", [clientNotify.Heartbeat],
        files.battleNotifyPcode, source.battleNotifyPcode, source.battleNotifyPcode.indexOf('pushstring "Heartbeat"'))
    required.add("battle.heartbeat.reply", "none",
        files.dummyTransmitter, source.dummyTransmitter, heartbeatArm.start)
    required.add("battle.forwarding.broadcast.recipients", "room-including-sender",
        files.typeScriptSession, source.typeScriptSession, typeScriptBroadcast.start)
    required.add("battle.forwarding.broadcast.senderAck", false,
        files.typeScriptSession, source.typeScriptSession, typeScriptBroadcast.start)
    required.add("battle.forwarding.send.recipients", "listed-connection-ids",
        files.typeScriptSession, source.typeScriptSession, typeScriptSend.start)
    required.add("battle.forwarding.send.senderAck", false,
        files.typeScriptSession, source.typeScriptSession, typeScriptSend.start)
    required.add("ai.join.deliveryScope", "room",
        files.typeScriptSession, source.typeScriptSession, typeScriptMates.start)
    required.add("session.battle.finalizeAcknowledgesAndCloses", true,
        files.battleSocketContact, source.battleSocketContact, battleSocketInput.start)

    const policy = createCollector(repositoryRoot)
    policy.add("session.policy.lobbyHeartbeatReplyTag", heartbeatReplyTag,
        files.typePacker, source.typePacker, meetingMap.start + meetingMap.source.indexOf("AckHeartbeat"))
    policy.add("session.policy.lobbyStartRemainingTag", startRemainingTag,
        files.typePacker, source.typePacker, meetingMap.start + meetingMap.source.indexOf("StartRemainingTime"))
    return {
        root: clientRoot,
        required: required.finish(),
        policy: policy.finish(),
    }
}
// //// /抽取 CN 1.8.x 客户端协议 ////

// //// 抽取数字或具名 JavaScript 分支 [@x380kkm 2026-08-25] ////
function mappedJavaScriptSwitchArms(functionSection, enumName, enumValues) {
    const switchBlock = extractBlockAfter(functionSection, /\bswitch\s*\([^)]*\)/, "JavaScript switch")
    const casePattern = new RegExp(
        `\\bcase\\s+(?:(\\d+)|${enumName}\\.([A-Za-z_$][\\w$]*))\\s*:`,
        "g",
    )
    const starts = [...switchBlock.source.matchAll(casePattern)]
    return starts.flatMap((match, index) => {
        const tag = match[1] === undefined ? enumValues[match[2]] : Number(match[1])
        if (!Number.isInteger(tag)) return []
        return [{
            tag,
            source: switchBlock.source.slice(match.index, starts[index + 1]?.index ?? switchBlock.source.length),
            start: switchBlock.start + match.index,
        }]
    })
}
// //// /抽取数字或具名 JavaScript 分支 ////

// //// 抽取默认 AI 模板 [@x380kkm 2026-08-24] ////
function parseNumberList(source) {
    return [...source.matchAll(/[\d_]+/g)]
        .map((match) => Number(match[0].replaceAll("_", "")))
}

function parseReferenceAiTemplates(source) {
    const templates = []
    const pattern = /\{\s*com_id:\s*(\d+),\s*characters:\s*\[([^\]]*)\],\s*unison_characters:\s*\[[^\]]*\],\s*equipments:\s*\[([^\]]*)\],\s*ability_soul_ids:\s*\[[^\]]*\],\s*rank:\s*(\d+),\s*degree_id:\s*(\d+),?\s*\}/g
    for (const match of source.matchAll(pattern)) {
        templates.push({
            comId: Number(match[1]),
            characterIds: parseNumberList(match[2]),
            equipmentIds: parseNumberList(match[3]),
            rank: Number(match[4]),
            degreeId: Number(match[5]),
        })
    }
    return templates.length > 0 ? templates : null
}

function parseLocalAiTemplates(source) {
    const templates = []
    const pattern = /DefaultAiTemplate\s*\{\s*com_id:\s*([\d_]+),\s*character_ids:\s*\[([^\]]*)\],\s*equipment_ids:\s*\[([^\]]*)\],\s*rank:\s*([\d_]+),\s*degree_id:\s*([\d_]+),?\s*\}/g
    for (const match of source.matchAll(pattern)) {
        templates.push({
            comId: Number(match[1].replaceAll("_", "")),
            characterIds: parseNumberList(match[2]),
            equipmentIds: parseNumberList(match[3]),
            rank: Number(match[4].replaceAll("_", "")),
            degreeId: Number(match[5].replaceAll("_", "")),
        })
    }
    return templates.length > 0 ? templates : null
}
// //// /抽取默认 AI 模板 ////

// //// 抽取参考 TCP 与 AI 协议 [@x380kkm 2026-08-24] ////
function parseReferenceProtocol(referenceRoot) {
    const tcpRoot = path.join(referenceRoot, "out", "multi", "tcp")
    const files = {
        server: path.join(tcpRoot, "server.js"),
        handshake: path.join(tcpRoot, "handshake.js"),
        lobby: path.join(tcpRoot, "lobby.js"),
        battle: path.join(tcpRoot, "battle.js"),
        relay: path.join(tcpRoot, "relay.js"),
        sessions: path.join(referenceRoot, "out", "multi", "state", "SessionManager.js"),
        aiTemplates: path.join(referenceRoot, "out", "multi", "npc", "types.js"),
        aiBuilder: path.join(referenceRoot, "out", "multi", "npc", "builder.js"),
    }
    const source = Object.fromEntries(Object.entries(files).map(([name, file]) => [name, readSource(file)]))
    const collector = createCollector(referenceRoot)

    collector.add("framing.delimiterByte", source.server.includes('"\\0"') ? 0 : null,
        files.server, source.server, source.server.indexOf('"\\0"'))
    collector.add("framing.encoding", /setEncoding\(["']utf8["']\)/.test(source.server) ? "utf8" : null,
        files.server, source.server, source.server.search(/setEncoding\(["']utf8["']\)/))
    collector.add("framing.serialization", /JSON\.parse\(/.test(source.server) && /JSON\.stringify\(/.test(source.sessions)
        ? "json" : null, files.server, source.server, source.server.search(/JSON\.parse\(/))

    const handshake = extractFunction(source.handshake, "handleHandshake", "javascript")
    const socklets = [...handshake.source.matchAll(/socklet\s*===\s*["']([^"']+)["']/g)]
        .map((match) => match[1]).sort()
    collector.add("handshake.socklets", [...new Set(socklets)], files.handshake, source.handshake, handshake.start)

    const lobbyMessage = extractFunction(source.lobby, "handleMessage", "javascript")
    const lobbyNotify = extractFunction(source.lobby, "handleNotify", "javascript")
    const lobbyOuterArms = jsSwitchArms(lobbyMessage)
    const lobbyNotifyArms = jsSwitchArms(lobbyNotify)
    collector.add("lobby.outerCommands", lobbyOuterArms.map((arm) => arm.tag),
        files.lobby, source.lobby, lobbyMessage.start)
    collector.add("lobby.notifyTags", lobbyNotifyArms.map((arm) => arm.tag),
        files.lobby, source.lobby, lobbyNotify.start)

    const handleBroadcast = extractFunction(source.lobby, "handleBroadcast", "javascript")
    const broadcastSource = compact(handleBroadcast.source)
    const broadcastToRoom = extractJavaScriptMethod(source.sessions, "broadcastToRoom")
    const broadcastCallOmitsExclusion = /broadcastToRoom\(client\.roomNumber, data\)/.test(broadcastSource)
    const exclusionIsConditional = /excludeAddr\s*!==\s*undefined[\s\S]*?addr\s*===\s*excludeAddr[\s\S]*?continue/.test(
        broadcastToRoom.source,
    )
    collector.add("lobby.forwarding.broadcast.supported", lobbyOuterArms.some((arm) => arm.tag === 1),
        files.lobby, source.lobby, handleBroadcast.start)
    collector.add("lobby.forwarding.broadcast.recipients",
        broadcastCallOmitsExclusion && exclusionIsConditional ? "room-including-sender" : null,
    files.sessions, source.sessions, broadcastToRoom.start)
    collector.add("lobby.forwarding.broadcast.payload", /broadcastToRoom\(client\.roomNumber, data\)/.test(broadcastSource)
        ? "original-client-frame" : null, files.lobby, source.lobby, handleBroadcast.start)
    collector.add("lobby.forwarding.broadcast.senderAck", /sendJson\([^,]*socket/.test(broadcastSource),
        files.lobby, source.lobby, handleBroadcast.start)

    const handleSend = extractFunction(source.lobby, "handleSend", "javascript")
    const sendSource = compact(handleSend.source)
    collector.add("lobby.forwarding.send.supported", lobbyOuterArms.some((arm) => arm.tag === 2),
        files.lobby, source.lobby, handleSend.start)
    collector.add("lobby.forwarding.send.recipients", /viewerId === targetViewerId/.test(sendSource)
        ? "single-viewer-id" : null, files.lobby, source.lobby, handleSend.start)
    collector.add("lobby.forwarding.send.payload", /sendJson\(c\.socket, data\)/.test(sendSource)
        ? "original-client-frame" : null, files.lobby, source.lobby, handleSend.start)

    const handleReady = extractFunction(source.lobby, "handleReady", "javascript")
    collector.add("lobby.ready.humanHandlerEvaluatesAutoStart", /checkHostAutoReady\(/.test(handleReady.source),
        files.lobby, source.lobby, handleReady.start)
    const handleChangeParty = extractFunction(source.lobby, "handleChangeParty", "javascript")
    collector.add("lobby.changeParty.acceptsDifferentPartyId",
        /partySlot:\s*pd\.currentPartyId/.test(handleChangeParty.source), files.lobby, source.lobby,
        handleChangeParty.start)
    collector.add("lobby.changeParty.persistsPartyId", /updatePlayerSync|partySlot/.test(handleChangeParty.source),
        files.lobby, source.lobby, handleChangeParty.start)

    const battleMessage = extractFunction(source.battle, "handleBattleMessage", "javascript")
    const battleNotify = extractFunction(source.battle, "handleBattleNotify", "javascript")
    const battleOuterArms = mappedJavaScriptSwitchArms(
        battleMessage,
        "ClientMessageKind",
        CN_CLIENT_BATTLE_MESSAGE_KIND,
    )
    const battleNotifyArms = mappedJavaScriptSwitchArms(
        battleNotify,
        "BattleNotifyKind",
        CN_CLIENT_BATTLE_NOTIFY_KIND,
    )
    collector.add("battle.outerCommands", battleOuterArms.map((arm) => arm.tag),
        files.battle, source.battle, battleMessage.start)
    collector.add("battle.notifyTags", battleNotifyArms.map((arm) => arm.tag),
        files.battle, source.battle, battleNotify.start)

    const sceneReadyArm = battleNotifyArms.find((arm) => arm.tag === CN_CLIENT_BATTLE_NOTIFY_KIND.SceneReady)
    const levelNextArm = battleNotifyArms.find((arm) => arm.tag === CN_CLIENT_BATTLE_NOTIFY_KIND.LevelNext)
    const finalizeArm = battleNotifyArms.find((arm) => arm.tag === CN_CLIENT_BATTLE_NOTIFY_KIND.Finalize)
    const measurementArm = battleNotifyArms.find((arm) => arm.tag === CN_CLIENT_BATTLE_NOTIFY_KIND.Measurement)
    const heartbeatArm = battleNotifyArms.find((arm) => arm.tag === CN_CLIENT_BATTLE_NOTIFY_KIND.Heartbeat)
    collector.add("battle.sceneReady.requestTags", sceneReadyArm && /markSceneReady/.test(sceneReadyArm.source)
        ? [CN_CLIENT_BATTLE_NOTIFY_KIND.SceneReady] : null,
        files.battle, source.battle, sceneReadyArm?.start ?? battleNotify.start)
    collector.add("battle.sceneReady.reply", sceneReadyArm && /\[1,\s*\[1\]\]/.test(sceneReadyArm.source)
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.BattleStart}` : "none",
    files.battle, source.battle, sceneReadyArm?.start ?? battleNotify.start)
    collector.add("battle.levelNext.action", levelNextArm && /finaliz|\[1,\s*\[2\]\]/.test(levelNextArm.source)
        ? "finalize" : levelNextArm ? "ignore" : null,
    files.battle, source.battle, levelNextArm?.start ?? battleNotify.start)
    collector.add("battle.finalize.requestTags", finalizeArm ? [CN_CLIENT_BATTLE_NOTIFY_KIND.Finalize] : null,
        files.battle, source.battle, finalizeArm?.start ?? battleNotify.start)
    collector.add("battle.finalize.reply", finalizeArm && /\[1,\s*\[2\]\]/.test(finalizeArm.source)
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.Finalized}` : "none",
        files.battle, source.battle, finalizeArm?.start ?? battleNotify.start)
    collector.add("battle.measurement.requestTags", measurementArm ? [CN_CLIENT_BATTLE_NOTIFY_KIND.Measurement] : null,
        files.battle, source.battle, measurementArm?.start ?? battleNotify.start)
    collector.add("battle.measurement.requestShape", measurementArm && /data\[1\]/.test(measurementArm.source) &&
        /data\[2\]/.test(measurementArm.source) ? "tag-frame-time" : null,
    files.battle, source.battle, measurementArm?.start ?? battleNotify.start)
    collector.add("battle.measurement.reply", measurementArm && /\[1,\s*\[3,/.test(measurementArm.source)
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.Measurement}` : "none",
    files.battle, source.battle, measurementArm?.start ?? battleNotify.start)
    collector.add("battle.heartbeat.requestTags", heartbeatArm ? [CN_CLIENT_BATTLE_NOTIFY_KIND.Heartbeat] : null,
        files.battle, source.battle, heartbeatArm?.start ?? battleNotify.start)
    collector.add("battle.heartbeat.reply", heartbeatArm && /\[1,\s*\[3,/.test(heartbeatArm.source)
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.Measurement}` : "none",
        files.battle, source.battle, heartbeatArm?.start ?? battleNotify.start)

    const relay = extractFunction(source.relay, "relayToBattleRoom", "javascript")
    const relayExcludesSender = /cid === sourceCid[\s\S]*?continue/.test(relay.source)
    const battleBroadcastArm = battleOuterArms.find((arm) => arm.tag === 1)
    const battleSendArm = battleOuterArms.find((arm) => arm.tag === 2)
    const broadcastSenderEcho = Boolean(battleBroadcastArm &&
        /sendJson\(socket,\s*message\)/.test(battleBroadcastArm.source))
    collector.add("battle.forwarding.broadcast.recipients", battleBroadcastArm &&
        (!relayExcludesSender || broadcastSenderEcho)
        ? "room-including-sender" : battleBroadcastArm ? "room-excluding-sender" : null,
    files.battle, source.battle, battleBroadcastArm?.start ?? battleMessage.start)
    collector.add("battle.forwarding.broadcast.payload", battleBroadcastArm && /\[2,\s*client\.connectionId,\s*bcData\]/.test(battleBroadcastArm.source)
        ? "messages-with-sender" : null, files.battle, source.battle, battleBroadcastArm?.start ?? battleMessage.start)
    collector.add("battle.forwarding.broadcast.senderAck", Boolean(battleBroadcastArm &&
        /sendJson\(socket,\s*\[1,\s*\[3,/.test(battleBroadcastArm.source)),
        files.battle, source.battle, battleBroadcastArm?.start ?? battleMessage.start)
    collector.add("battle.forwarding.send.recipients", relayExcludesSender
        ? "room-excluding-sender" : "room-including-sender", files.relay, source.relay, relay.start)
    collector.add("battle.forwarding.send.payload", battleSendArm && /\[3,\s*client\.connectionId,\s*sendMsg\]/.test(battleSendArm.source)
        ? "message-with-sender" : null, files.battle, source.battle, battleSendArm?.start ?? battleMessage.start)
    collector.add("battle.forwarding.send.senderAck", Boolean(battleSendArm && /sendJson\(socket/.test(battleSendArm.source)),
        files.battle, source.battle, battleSendArm?.start ?? battleMessage.start)

    const joinDelayMatch = /NPC_JOIN_DELAY_MS\s*=\s*parseInt\([^|]+\|\|\s*["'](\d+)["']/.exec(source.lobby)
    const readyDelayMatch = /NPC_READY_DELAY_MS\s*=\s*parseInt\([^|]+\|\|\s*["'](\d+)["']/.exec(source.lobby)
    const joinDelay = joinDelayMatch ? Number(joinDelayMatch[1]) : null
    const readyDelay = readyDelayMatch ? Number(readyDelayMatch[1]) : null
    const enterComs = extractFunction(source.lobby, "handleEnterComs", "javascript")
    collector.add("ai.join.delayMs", joinDelay, files.lobby, source.lobby, joinDelayMatch?.index ?? enterComs.start)
    collector.add("ai.join.deliveryScope", /sendJson\(client\.socket,\s*\[1,\s*\[1,\s*client\.mates\]\]\)/.test(enterComs.source)
        ? "requesting-client" : /broadcastToRoom/.test(enterComs.source) ? "room" : null,
    files.lobby, source.lobby, enterComs.start)
    collector.add("ai.join.requiresExactRosterSize",
        /(?:client\.mates|roster)\.length\s*(?:!==|!=)\s*3/.test(enterComs.source),
    files.lobby, source.lobby, enterComs.start)
    collector.add("ai.ready.delayAfterJoinMs", readyDelay, files.lobby, source.lobby,
        readyDelayMatch?.index ?? enterComs.start)
    collector.add("ai.ready.totalDelayMs", joinDelay === null || readyDelay === null ? null : joinDelay + readyDelay,
        files.lobby, source.lobby, enterComs.start)
    collector.add("ai.ready.deliveryScope", /broadcastToRoom\([^;]+\[1,\s*\[2,\s*npc\.connectionId/.test(compact(enterComs.source))
        ? "room" : null, files.lobby, source.lobby, enterComs.start)
    collector.add("ai.ready.evaluatesHostAutoReady", /checkHostAutoReady\(/.test(enterComs.source),
        files.lobby, source.lobby, enterComs.start)
    collector.add("ai.defaultTemplates", parseReferenceAiTemplates(source.aiTemplates),
        files.aiTemplates, source.aiTemplates, source.aiTemplates.indexOf("NPC_TEMPLATES"))
    collector.add("ai.defaultParty.characterEvolutionLevel",
        /evolution_level:\s*5/.test(source.aiBuilder) ? 5 : null,
        files.aiBuilder, source.aiBuilder, source.aiBuilder.search(/evolution_level:\s*5/))
    collector.add("ai.defaultParty.equipmentLevel", /level:\s*1/.test(source.aiBuilder) ? 1 : null,
        files.aiBuilder, source.aiBuilder, source.aiBuilder.search(/level:\s*1/))
    collector.add("ai.defaultParty.unisonSlots",
        /unisonCharacters\.length\s*<\s*3/.test(source.aiBuilder) ? 3 : null,
        files.aiBuilder, source.aiBuilder, source.aiBuilder.search(/unisonCharacters\.length\s*<\s*3/))
    collector.add("ai.defaultParty.abilitySoulSlots",
        /abilitySoulIds\.length\s*<\s*3/.test(source.aiBuilder) ? 3 : null,
        files.aiBuilder, source.aiBuilder, source.aiBuilder.search(/abilitySoulIds\.length\s*<\s*3/))
    return collector.finish()
}
// //// /抽取参考 TCP 与 AI 协议 ////

// //// 抽取个人服务 TCP 与 AI 协议 [@x380kkm 2026-08-24] ////
function parseLocalProtocol(repositoryRoot) {
    const multiplayerRoot = path.join(repositoryRoot, "core", "personal-service", "src", "cn_multiplayer")
    const files = {
        transport: path.join(multiplayerRoot, "transport.rs"),
        meeting: path.join(multiplayerRoot, "meeting.rs"),
        battle: path.join(multiplayerRoot, "battle.rs"),
        lifecycle: path.join(multiplayerRoot, "lifecycle.rs"),
        lobbyPlayer: path.join(multiplayerRoot, "lobby_player.rs"),
        aiTemplates: path.join(repositoryRoot, "core", "personal-service", "src", "cn_multi", "room.rs"),
    }
    const source = Object.fromEntries(Object.entries(files).map(([name, file]) => [name, readSource(file)]))
    const collector = createCollector(repositoryRoot)

    collector.add("framing.delimiterByte", /position\(\|byte\| \*byte == 0\)/.test(source.transport) && /encoded\.push\(0\)/.test(source.transport)
        ? 0 : null, files.transport, source.transport, source.transport.search(/position\(\|byte\| \*byte == 0\)/))
    collector.add("framing.encoding", /serde_json::from_slice/.test(source.transport) ? "utf8" : null,
        files.transport, source.transport, source.transport.search(/serde_json::from_slice/))
    collector.add("framing.serialization", /serde_json::from_slice/.test(source.transport) && /serde_json::to_writer/.test(source.transport)
        ? "json" : null, files.transport, source.transport, source.transport.search(/serde_json::from_slice/))

    const handshake = extractFunction(source.meeting, "handle_handshake", "rust")
    const socklets = [...handshake.source.matchAll(/Some\(["']([^"']+)["']\)\s*=>/g)]
        .map((match) => match[1]).sort()
    collector.add("handshake.socklets", [...new Set(socklets)], files.meeting, source.meeting, handshake.start)

    const handleLobby = extractFunction(source.meeting, "handle_lobby", "rust")
    const lobbyMatch = extractBlockAfter(handleLobby,
        /match\s+command\.first\(\)\.and_then\(Value::as_i64\)/, "Rust lobby command match")
    const lobbyArms = rustMatchArms(lobbyMatch)
    const meetingCommand = extractFunction(source.transport, "meeting_command", "rust")
    const lobbyOuterCommands = []
    if (/frame\[0\]\.as_i64\(\)\s*==\s*Some\(0\)/.test(meetingCommand.source)) lobbyOuterCommands.push(0)
    for (const match of handleLobby.source.matchAll(
        /data\.first\(\)\.and_then\(Value::as_i64\)\s*==\s*Some\((\d+)\)/g,
    )) lobbyOuterCommands.push(Number(match[1]))
    const uniqueLobbyOuterCommands = [...new Set(lobbyOuterCommands)]
        .sort((left, right) => left - right)
    collector.add("lobby.outerCommands", uniqueLobbyOuterCommands, files.meeting, source.meeting, handleLobby.start)
    collector.add("lobby.notifyTags", numericRustTags(lobbyArms), files.meeting, source.meeting, lobbyMatch.start)

    const broadcastBranchStart = handleLobby.source.indexOf("broadcast_lobby")
    const broadcastBranchEnd = handleLobby.source.indexOf("return Ok", broadcastBranchStart)
    const broadcastBranch = broadcastBranchStart >= 0 && broadcastBranchEnd > broadcastBranchStart
        ? handleLobby.source.slice(broadcastBranchStart, broadcastBranchEnd)
        : ""
    const sendBranchStart = handleLobby.source.indexOf("send_to_lobby_viewer")
    const broadcastLobby = extractFunction(source.meeting, "broadcast_lobby", "rust")
    collector.add("lobby.forwarding.broadcast.supported", uniqueLobbyOuterCommands.includes(1),
        files.meeting, source.meeting, handleLobby.start)
    collector.add("lobby.forwarding.broadcast.recipients",
        /for client in &mut self\.clients/.test(broadcastLobby.source) &&
            !/client_connection|sender_connection|exclude/.test(broadcastLobby.source)
            ? "room-including-sender" : null,
    files.meeting, source.meeting, broadcastLobby.start)
    const compactHandleLobby = compact(handleLobby.source)
    let lobbyBroadcastPayload = null
    if (/broadcast_lobby\([^;]+&frame\s*\)/.test(compactHandleLobby)) {
        lobbyBroadcastPayload = "original-client-frame"
    } else if (/json!\(\[2,\s*connection_id,\s*messages\]\)/.test(compactHandleLobby)) {
        lobbyBroadcastPayload = "messages-with-sender"
    }
    collector.add("lobby.forwarding.broadcast.payload", lobbyBroadcastPayload,
        files.meeting, source.meeting, handleLobby.start)
    collector.add("lobby.forwarding.broadcast.senderAck",
        /queue_frame\(&mut self\.clients\[client_index\]/.test(broadcastBranch),
    files.meeting, source.meeting, handleLobby.start + Math.max(0, broadcastBranchStart))
    collector.add("lobby.forwarding.send.supported", uniqueLobbyOuterCommands.includes(2),
        files.meeting, source.meeting, handleLobby.start)
    if (uniqueLobbyOuterCommands.includes(2)) {
        const sendToLobbyViewer = extractFunction(source.meeting, "send_to_lobby_viewer", "rust")
        collector.add("lobby.forwarding.send.recipients",
            /client_viewer\s*==\s*viewer_id/.test(sendToLobbyViewer.source)
                ? "single-viewer-id" : null,
        files.meeting, source.meeting, sendToLobbyViewer.start)
        collector.add("lobby.forwarding.send.payload",
            /send_to_lobby_viewer\([^;]+&frame\s*\)/.test(compactHandleLobby) &&
                /queue_frame\(client,\s*frame\)/.test(compact(sendToLobbyViewer.source))
                ? "original-client-frame" : null,
        files.meeting, source.meeting, sendBranchStart + handleLobby.start)
    }

    const readyArm = lobbyArms.find((arm) => arm.pattern === "3")
    collector.add("lobby.ready.humanHandlerEvaluatesAutoStart", Boolean(readyArm &&
        /all_human_members_ready|broadcast_start_remaining_time|schedule_npc_lobby_sequence|evaluate_lobby_readiness/.test(readyArm.source)),
    files.meeting, source.meeting, readyArm?.start ?? lobbyMatch.start)
    const changePartyArm = lobbyArms.find((arm) => arm.pattern === "2")
    const normalizePlayer = extractFunction(source.lobbyPlayer, "normalize_lobby_player", "rust")
    collector.add("lobby.changeParty.acceptsDifferentPartyId",
        Boolean(changePartyArm && /normalize_changed_lobby_player/.test(changePartyArm.source)) ||
            !/member\.as_ref\(\)\.map\(\|member\| member\.party_id\)\s*!=\s*Some\(party_id\)/.test(normalizePlayer.source),
    files.lobbyPlayer, source.lobbyPlayer, normalizePlayer.start)
    collector.add("lobby.changeParty.persistsPartyId", Boolean(changePartyArm &&
        /change_multiplayer_member_party|set_multiplayer_member_party|update.*party_id/.test(changePartyArm.source)),
    files.meeting, source.meeting, changePartyArm?.start ?? lobbyMatch.start)

    const handleBattle = extractFunction(source.battle, "handle_battle", "rust")
    const battleOuterMatch = extractBlockAfter(handleBattle,
        /match\s+data\.first\(\)\.and_then\(Value::as_i64\)/, "Rust battle command match")
    const battleNotifyMatch = extractBlockAfter(handleBattle,
        /match\s+notify\.first\(\)\.and_then\(Value::as_i64\)/, "Rust battle notify match")
    const battleOuterArms = rustMatchArms(battleOuterMatch)
    const battleNotifyArms = rustMatchArms(battleNotifyMatch)
    const notifyTags = new Set(numericRustTags(battleNotifyArms))
    for (const match of battleNotifyMatch.source.matchAll(/(?:legacy_protocol|!legacy_protocol)\s*&&\s*kind\s*==\s*(\d+)/g)) {
        notifyTags.add(Number(match[1]))
    }
    collector.add("battle.outerCommands", numericRustTags(battleOuterArms),
        files.battle, source.battle, battleOuterMatch.start)
    collector.add("battle.notifyTags", [...notifyTags].sort((left, right) => left - right),
        files.battle, source.battle, battleNotifyMatch.start)

    const sceneReadyArm = battleNotifyArms.find((arm) =>
        numericPatternValues(arm.pattern).includes(CN_CLIENT_BATTLE_NOTIFY_KIND.SceneReady) &&
        /scene_ready/.test(arm.source))
    const levelNextArm = battleNotifyArms.find((arm) =>
        numericPatternValues(arm.pattern).includes(CN_CLIENT_BATTLE_NOTIFY_KIND.LevelNext) &&
        !/!\s*legacy_protocol/.test(arm.guard))
    const finalizeArms = battleNotifyArms.filter((arm) => /finalized|json!\(\[1,\s*\[2\]\]\)/.test(arm.source))
    const finalizeArm = finalizeArms.find((arm) =>
        numericPatternValues(arm.pattern).includes(CN_CLIENT_BATTLE_NOTIFY_KIND.Finalize) &&
        !/!\s*legacy_protocol/.test(arm.guard))
    const measurementArm = battleNotifyArms.find((arm) =>
        numericPatternValues(arm.pattern).includes(CN_CLIENT_BATTLE_NOTIFY_KIND.Measurement) &&
        /flat_battle_measurement|send_battle_measurement_ack/.test(arm.source) &&
        !/!\s*legacy_protocol/.test(arm.guard))
    const heartbeatArm = battleNotifyArms.find((arm) =>
        numericPatternValues(arm.pattern).includes(CN_CLIENT_BATTLE_NOTIFY_KIND.Heartbeat) &&
        /notify\.len\(\)\s*==\s*1/.test(arm.guard))
    collector.add("battle.sceneReady.requestTags", sceneReadyArm ? [CN_CLIENT_BATTLE_NOTIFY_KIND.SceneReady] : null,
        files.battle, source.battle, sceneReadyArm?.start ?? battleNotifyMatch.start)
    collector.add("battle.sceneReady.reply", sceneReadyArm && /start_battle_if_ready/.test(sceneReadyArm.source) &&
        /json!\(\[1,\s*\[1\]\]\)/.test(source.battle)
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.BattleStart}` : "none",
    files.battle, source.battle, sceneReadyArm?.start ?? battleNotifyMatch.start)
    collector.add("battle.levelNext.action", levelNextArm && /finalized|json!\(\[1,\s*\[2\]\]\)/.test(levelNextArm.source)
        ? "finalize" : levelNextArm ? "ignore" : null,
    files.battle, source.battle, levelNextArm?.start ?? battleNotifyMatch.start)
    collector.add("battle.finalize.requestTags", finalizeArm ? [CN_CLIENT_BATTLE_NOTIFY_KIND.Finalize] : null,
        files.battle, source.battle, finalizeArm?.start ?? battleNotifyMatch.start)
    collector.add("battle.finalize.reply", /json!\(\[1,\s*\[2\]\]\)/.test(compact(battleNotifyMatch.source))
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.Finalized}` : "none",
    files.battle, source.battle, battleNotifyMatch.start)
    collector.add("battle.measurement.requestTags", measurementArm ? [CN_CLIENT_BATTLE_NOTIFY_KIND.Measurement] : null,
        files.battle, source.battle, measurementArm?.start ?? battleNotifyMatch.start)
    const battleMeasurement = extractFunction(source.battle, "flat_battle_measurement", "rust")
    let measurementRequestShape = null
    if (/notify\s*(?:\.get\(1\)|\[1\])/.test(battleMeasurement.source) &&
        /notify\s*(?:\.get\(2\)|\[2\])/.test(battleMeasurement.source)) {
        measurementRequestShape = "tag-frame-time"
    }
    collector.add("battle.measurement.requestShape", measurementRequestShape,
        files.battle, source.battle, battleMeasurement.start)
    collector.add("battle.measurement.reply", measurementArm && /send_battle_measurement_ack/.test(measurementArm.source)
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.Measurement}` : "none",
    files.battle, source.battle, measurementArm?.start ?? battleNotifyMatch.start)
    collector.add("battle.heartbeat.requestTags", heartbeatArm ? [CN_CLIENT_BATTLE_NOTIFY_KIND.Heartbeat] : null,
        files.battle, source.battle, heartbeatArm?.start ?? battleNotifyMatch.start)
    collector.add("battle.heartbeat.reply", heartbeatArm && rustArmCallsSenderAck(heartbeatArm, source.battle)
        ? `notify-${CN_SERVER_BATTLE_NOTIFY_KIND.Measurement}` : "none",
    files.battle, source.battle, heartbeatArm?.start ?? battleNotifyMatch.start)

    const broadcastArm = battleOuterArms.find((arm) => arm.pattern === "1")
    const sendArm = battleOuterArms.find((arm) => arm.pattern === "2")
    collector.add("battle.forwarding.broadcast.recipients",
        broadcastArm ? rustArmRecipientScope(broadcastArm, source.battle) : null,
    files.battle, source.battle, broadcastArm?.start ?? battleOuterMatch.start)
    collector.add("battle.forwarding.broadcast.payload", broadcastArm && /json!\(\[2,\s*connection_id,\s*messages\]\)/.test(compact(broadcastArm.source))
        ? "messages-with-sender" : null, files.battle, source.battle, broadcastArm?.start ?? battleOuterMatch.start)
    collector.add("battle.forwarding.broadcast.senderAck", Boolean(broadcastArm &&
        rustArmCallsSenderAck(broadcastArm, source.battle)),
        files.battle, source.battle, broadcastArm?.start ?? battleOuterMatch.start)
    collector.add("battle.forwarding.send.recipients", sendArm &&
        /target_connection_ids|send_battle_to_connections/.test(sendArm.source)
        ? "listed-connection-ids" : sendArm ? rustArmRecipientScope(sendArm, source.battle) : null,
    files.battle, source.battle, sendArm?.start ?? battleOuterMatch.start)
    collector.add("battle.forwarding.send.payload", sendArm && /json!\(\[3,\s*connection_id,\s*message\]\)/.test(compact(sendArm.source))
        ? "message-with-sender" : null, files.battle, source.battle, sendArm?.start ?? battleOuterMatch.start)
    collector.add("battle.forwarding.send.senderAck", Boolean(sendArm &&
        rustArmCallsSenderAck(sendArm, source.battle)),
        files.battle, source.battle, sendArm?.start ?? battleOuterMatch.start)

    const joinDelayMatch = /NPC_JOIN_DELAY\s*:\s*Duration\s*=\s*Duration::from_millis\(([\d_]+)\)/.exec(source.lifecycle)
    const readyDelayMatch = /NPC_READY_DELAY\s*:\s*Duration\s*=\s*Duration::from_millis\(([\d_]+)\)/.exec(source.lifecycle)
    const joinDelay = joinDelayMatch ? Number(joinDelayMatch[1].replaceAll("_", "")) : null
    const readyDelay = readyDelayMatch ? Number(readyDelayMatch[1].replaceAll("_", "")) : null
    const pollSequences = extractFunction(source.lifecycle, "poll_pending_lobby_sequences", "rust")
    const joinPhase = extractBlockAfter(pollSequences, /PendingLobbyPhase::Join\s*=>/, "NPC join phase")
    const readyPhase = extractBlockAfter(pollSequences, /PendingLobbyPhase::Ready\s*=>/, "NPC ready phase")
    collector.add("ai.join.delayMs", joinDelay, files.lifecycle, source.lifecycle, joinDelayMatch?.index ?? pollSequences.start)
    let aiJoinDeliveryScope = null
    if (/send_to_lobby_viewer\(/.test(joinPhase.source)) aiJoinDeliveryScope = "requesting-client"
    else if (/broadcast_lobby\(/.test(joinPhase.source)) aiJoinDeliveryScope = "room"
    collector.add("ai.join.deliveryScope", aiJoinDeliveryScope,
        files.lifecycle, source.lifecycle, joinPhase.start)
    collector.add("ai.join.requiresExactRosterSize", /roster\.len\(\)\s*!=\s*3/.test(joinPhase.source),
        files.lifecycle, source.lifecycle, joinPhase.start)
    collector.add("ai.ready.delayAfterJoinMs", readyDelay, files.lifecycle, source.lifecycle,
        readyDelayMatch?.index ?? pollSequences.start)
    collector.add("ai.ready.totalDelayMs", joinDelay === null || readyDelay === null ? null : joinDelay + readyDelay,
        files.lifecycle, source.lifecycle, pollSequences.start)
    collector.add("ai.ready.deliveryScope", /broadcast_lobby\(/.test(readyPhase.source) ? "room" : null,
        files.lifecycle, source.lifecycle, readyPhase.start)
    let aiReadyEvaluatesHost = /set_multiplayer_member_ready/.test(readyPhase.source) &&
        /broadcast_start_remaining_time/.test(readyPhase.source)
    if (!aiReadyEvaluatesHost && /evaluate_lobby_readiness/.test(readyPhase.source)) {
        const evaluateReadiness = extractFunction(source.lifecycle, "evaluate_lobby_readiness", "rust")
        aiReadyEvaluatesHost = /set_multiplayer_member_ready/.test(evaluateReadiness.source) &&
            /broadcast_start_remaining_time/.test(evaluateReadiness.source)
    }
    collector.add("ai.ready.evaluatesHostAutoReady", aiReadyEvaluatesHost,
        files.lifecycle, source.lifecycle, readyPhase.start)
    collector.add("ai.defaultTemplates", parseLocalAiTemplates(source.aiTemplates),
        files.aiTemplates, source.aiTemplates, source.aiTemplates.indexOf("DEFAULT_AI_TEMPLATES"))
    collector.add("ai.defaultParty.characterEvolutionLevel",
        /"evolution_level":\s*5/.test(source.aiTemplates) ? 5 : null,
        files.aiTemplates, source.aiTemplates, source.aiTemplates.search(/"evolution_level":\s*5/))
    collector.add("ai.defaultParty.equipmentLevel", /"level":\s*1/.test(source.aiTemplates) ? 1 : null,
        files.aiTemplates, source.aiTemplates, source.aiTemplates.search(/"level":\s*1/))
    collector.add("ai.defaultParty.unisonSlots",
        /"unison_characters":\s*\[null,\s*null,\s*null\]/.test(source.aiTemplates) ? 3 : null,
        files.aiTemplates, source.aiTemplates, source.aiTemplates.search(/"unison_characters"/))
    collector.add("ai.defaultParty.abilitySoulSlots",
        /"ability_soul_ids":\s*\[null,\s*null,\s*null\]/.test(source.aiTemplates) ? 3 : null,
        files.aiTemplates, source.aiTemplates, source.aiTemplates.search(/"ability_soul_ids"/))
    return collector.finish()
}
// //// /抽取个人服务 TCP 与 AI 协议 ////

// //// 合并协议事实 [@x380kkm 2026-08-24] ////
function mergeProtocolFacts(primary, additional) {
    if (additional === null) return primary
    const facts = new Map(primary.facts)
    for (const [factPath, fact] of additional.facts) {
        if (facts.has(factPath)) throw new Error(`duplicate merged multiplayer fact: ${factPath}`)
        facts.set(factPath, fact)
    }
    return {
        facts,
        sources: [...new Set([...primary.sources, ...additional.sources])].sort(),
    }
}

function overlayProtocolFacts(primary, overlay) {
    if (overlay === null) return primary
    const facts = new Map(primary.facts)
    for (const [factPath, fact] of overlay.facts) facts.set(factPath, fact)
    return {
        facts,
        sources: [...new Set([...primary.sources, ...overlay.sources])].sort(),
    }
}
// //// /合并协议事实 ////

// //// 裁决目标客户端策略差异 [@x380kkm 2026-08-25] ////
function sameFactValue(left, right) {
    return JSON.stringify(left) === JSON.stringify(right)
}

function adjudicatePolicyComparison(policyComparison, targetClient) {
    if (targetClient === null) {
        return { differences: policyComparison.differences, adjudications: [] }
    }
    const differences = []
    const adjudications = []
    for (const row of policyComparison.differences) {
        const targetFact = targetClient.policy.facts.get(row.path)
        if (targetFact && sameFactValue(row.local, targetFact.value)) {
            adjudications.push({
                ...row,
                status: "target-client-match",
                target: targetFact.value,
                targetEvidence: targetFact.evidence,
            })
            continue
        }
        if (row.path === "session.policy.lobbyReadyEvaluatesAutoStart" && row.local === true) {
            adjudications.push({ ...row, status: "local-readiness-extension" })
            continue
        }
        if (row.path === "session.policy.battleDisconnectGraceMs" &&
            Number.isFinite(row.local) && row.local > 0) {
            adjudications.push({ ...row, status: "local-reconnect-grace" })
            continue
        }
        differences.push(row)
    }
    return { differences, adjudications }
}
// //// /裁决目标客户端策略差异 ////

// //// 比较同名协议事实 [@x380kkm 2026-08-24] ////
export function buildMultiplayerDifferential(referenceRoot, repositoryRoot = DEFAULT_REPOSITORY_ROOT) {
    const resolvedReferenceRoot = path.resolve(referenceRoot)
    const resolvedRepositoryRoot = path.resolve(repositoryRoot)
    const sessionScenarios = collectSessionScenarioDifferential(resolvedRepositoryRoot)
    const targetClient = parseTargetClientContract(resolvedRepositoryRoot)
    const launcherReference = mergeProtocolFacts(
        parseReferenceProtocol(resolvedReferenceRoot),
        sessionScenarios?.reference ?? null,
    )
    const reference = overlayProtocolFacts(launcherReference, targetClient?.required ?? null)
    const local = mergeProtocolFacts(
        parseLocalProtocol(resolvedRepositoryRoot),
        sessionScenarios?.local ?? null,
    )
    const referenceReport = materializeFacts(reference.facts)
    const localReport = materializeFacts(local.facts)
    const comparison = compareFacts(reference.facts, local.facts)
    const launcherComparison = targetClient === null
        ? null
        : compareFacts(launcherReference.facts, local.facts)
    const policyComparison = sessionScenarios === null
        ? { differences: [], adjudications: [] }
        : adjudicatePolicyComparison(sessionScenarios.policyComparison, targetClient)
    const targetClientReport = targetClient === null
        ? null
        : materializeFacts(targetClient.required.facts)
    return {
        targetClient: targetClient === null ? null : {
            root: targetClient.root,
            sources: [...new Set([...targetClient.required.sources, ...targetClient.policy.sources])].sort(),
            ...targetClientReport,
        },
        reference: {
            root: resolvedReferenceRoot,
            sessionRoot: sessionScenarios === null ? null : resolvedRepositoryRoot,
            sources: reference.sources,
            ...referenceReport,
        },
        local: {
            root: resolvedRepositoryRoot,
            sources: local.sources,
            ...localReport,
        },
        comparison: {
            ...comparison,
            launcherDifferences: launcherComparison?.differences ?? [],
            targetOverrides: targetClient === null ? [] : [...targetClient.required.facts.keys()].sort(),
            policyDifferences: policyComparison.differences,
            policyAdjudications: policyComparison.adjudications,
        },
    }
}
// //// /比较同名协议事实 ////

// //// 输出协议差异报告 [@x380kkm 2026-08-24] ////
function main() {
    const args = process.argv.slice(2)
    const referenceRoot = readRequiredOption(args, "--reference-root")
    const repositoryRoot = readOptionalOption(args, "--repository-root") ?? DEFAULT_REPOSITORY_ROOT
    const report = buildMultiplayerDifferential(referenceRoot, repositoryRoot)
    const output = args.includes("--summary")
        ? {
            targetClient: report.targetClient === null ? null : {
                root: report.targetClient.root,
                sources: report.targetClient.sources,
            },
            reference: { root: report.reference.root, sources: report.reference.sources },
            local: { root: report.local.root, sources: report.local.sources },
            comparison: {
                summary: report.comparison.summary,
                differences: report.comparison.differences,
                extensions: report.comparison.extensions,
                launcherDifferences: report.comparison.launcherDifferences,
                targetOverrides: report.comparison.targetOverrides,
                policyDifferences: report.comparison.policyDifferences,
                policyAdjudications: report.comparison.policyAdjudications,
            },
        }
        : report
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`)
    if (args.includes("--fail-on-differences") &&
        (report.comparison.differences.length > 0 || report.comparison.policyDifferences.length > 0)) {
        process.exitCode = 1
    }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
// //// /输出协议差异报告 ////
