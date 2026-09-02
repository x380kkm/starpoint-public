// audience: internal
// # test-cn-server
// 此测试启动短时 CN 服务, 并用客户端线格式验证账号隔离, viewer 轮换, load 归属, 单机战斗, 多人 TCP 会话和 CDN 入口.

const assert = require("node:assert/strict")
const { spawn } = require("node:child_process")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const Database = require("better-sqlite3")
const { pack, unpack } = require("msgpackr")

const repositoryRoot = path.resolve(__dirname, "..", "..")
const startupScript = path.join(repositoryRoot, "out", "start.js")
const cnBaselineGacha = require(path.join(repositoryRoot, "assets", "cn_gacha.json"))["1"]
const cnLegacyGachaMovieSeedsPath = path.join(repositoryRoot, "assets", "gacha_movie_seeds.json")
const cnBaselineCharacterIds = new Set(Object.values(cnBaselineGacha.pool).flat().map((item) => item.id))
const { getCnGachaSync } = require(path.join(repositoryRoot, "out", "lib", "cnAssets.js"))
const { getGachaSync } = require(path.join(repositoryRoot, "out", "lib", "assets.js"))
const { getCnTutorialGachaSync } = require(path.join(repositoryRoot, "out", "lib", "tutorial.js"))
const { selectWeightedPoolIndex } = require(path.join(repositoryRoot, "out", "lib", "gacha.js"))
const { readCnBattleAction } = require(path.join(repositoryRoot, "out", "multiplayer", "cnBattleProtocol.js"))
const {
    CN_MEETING_CLIENT_COMMAND,
    CN_MEETING_FRAME_INDEX,
    CN_MEETING_SERVER_MESSAGE,
    createCnMeetingServerFrame,
    readCnMeetingCommand,
} = require(path.join(repositoryRoot, "out", "multiplayer", "cnMeetingProtocol.js"))

// //// 按服务响应读取演出种子池 [@x380kkm 2026-08-25] ////
function readGachaMovieSeedPool(movieId, rarityKey, movieType) {
    const movieSeedsPath = path.join(repositoryRoot, "assets", `gacha_movie_seeds_${movieId}.json`)
    const selectedSeedsPath = fs.existsSync(movieSeedsPath) ? movieSeedsPath : cnLegacyGachaMovieSeedsPath
    const movieSeeds = JSON.parse(fs.readFileSync(selectedSeedsPath, "utf8"))
    const typedSeeds = movieSeeds[rarityKey]?.[movieType] ?? []
    return typedSeeds.length > 0 ? typedSeeds : (movieSeeds[rarityKey]?.["0"] ?? [])
}
// //// /按服务响应读取演出种子池 ////

const resolvedCnGacha = getCnGachaSync(1)
assert.deepEqual(resolvedCnGacha.rankRates.normal, [50, 250, 700])
assert.equal(resolvedCnGacha.startDate, "2025-07-10 00:00:00")
assert.ok(new Date(resolvedCnGacha.endDate.replace(" ", "T") + "Z") > new Date("2025-07-10T00:00:00.000Z"))
assert.notStrictEqual(getGachaSync(1), resolvedCnGacha)
assert.notEqual(getGachaSync(1704), null)
assert.notEqual(getCnTutorialGachaSync(1704), null)
assert.equal(selectWeightedPoolIndex(1, [50, 250, 700]), 0)
assert.equal(selectWeightedPoolIndex(50, [50, 250, 700]), 0)
assert.equal(selectWeightedPoolIndex(51, [50, 250, 700]), 1)
assert.equal(selectWeightedPoolIndex(300, [50, 250, 700]), 1)
assert.equal(selectWeightedPoolIndex(301, [50, 250, 700]), 2)
assert.equal(selectWeightedPoolIndex(1000, [50, 250, 700]), 2)
assert.equal(selectWeightedPoolIndex(1001, [50, 250, 700]), null)
assert.deepEqual(readCnBattleAction([0, [0]]), { kind: "sceneReady" })
assert.equal(readCnBattleAction([0, [0], "trailing"]), null)
assert.deepEqual(readCnBattleAction([0, [4]]), { kind: "heartbeat" })
assert.deepEqual(readCnBattleAction([0, [5]]), { kind: "unmodeled" })
assert.deepEqual(readCnBattleAction([0, [3, 120.5]]), { kind: "lineSpeedWarning", latency: 120.5 })
assert.equal(readCnBattleAction([0, [3, "120.5"]]), null)
assert.equal(readCnBattleAction([0, [3, 120.5, 1]]), null)
assert.deepEqual(readCnBattleAction([2, ["target"], [0, [22]]]), {
    kind: "send",
    targetConnectionIds: ["target"],
    message: [0, [22]],
})
assert.deepEqual(CN_MEETING_FRAME_INDEX, { clientToServer: 0, serverToClient: 1 })
assert.equal(CN_MEETING_CLIENT_COMMAND.changeParty, 2)
assert.equal(CN_MEETING_CLIENT_COMMAND.ready, 3)
assert.equal(CN_MEETING_CLIENT_COMMAND.heartbeat, 4)
assert.equal(CN_MEETING_CLIENT_COMMAND.suspend, 5)
assert.equal(CN_MEETING_CLIENT_COMMAND.startBattle, 6)
assert.equal(CN_MEETING_CLIENT_COMMAND.changeAutoplayMode, 7)
assert.equal(CN_MEETING_CLIENT_COMMAND.changeAutoStart, 8)
assert.equal(CN_MEETING_CLIENT_COMMAND.log, 9)
assert.equal(CN_MEETING_CLIENT_COMMAND.enterComs, 10)
assert.equal(CN_MEETING_SERVER_MESSAGE.autoplayModeChanged, 3)
assert.equal(CN_MEETING_SERVER_MESSAGE.autoStartChanged, 4)
assert.equal(CN_MEETING_SERVER_MESSAGE.startRemainingTime, 9)
assert.equal(CN_MEETING_SERVER_MESSAGE.ackHeartbeat, 10)
assert.deepEqual(readCnMeetingCommand([0, [4]]), [4])
assert.equal(readCnMeetingCommand([1, [4]]), null)
assert.deepEqual(createCnMeetingServerFrame([10, "connection"]), [1, [10, "connection"]])

// //// 验证 CN 2.1.125 TypePackerResource2 定义的大厅帧 [@x380kkm 2026-07-24] ////
const cnLobbyEnterIndex = 0
const cnLobbyWelcomeIndex = 0
const cnLobbyMatesIndex = 1
const cnLobbyStateChangedIndex = 2
const cnLobbyAutoplayModeChangedIndex = 3
const cnLobbyAutoStartChangedIndex = 4
const cnLobbyStartIndex = 5
const cnLobbyStartRemainingIndex = 9
const cnLobbyChangePartyIndex = 2
const cnLobbyReadyIndex = 3
const cnLobbySuspendIndex = 5
const cnLobbyStartBattleIndex = 6
const cnLobbyChangeAutoplayModeIndex = 7
const cnLobbyChangeAutoStartIndex = 8
const cnLobbySummonIndex = 10
const cnLobbyFirstHumanPlayerId = 2
const cnLobbyHeartbeatFrame = [0, [4]]
const cnLobbyHeartbeatResponseIndex = 10
const cnLobbyByeFrame = [0, [1]]
// //// /验证 CN 2.1.125 TypePackerResource2 定义的大厅帧 ////

function reservePort() {
    return new Promise((resolve, reject) => {
        const server = require("node:net").createServer()
        server.once("error", reject)
        server.listen(0, "127.0.0.1", () => {
            const address = server.address()
            const port = typeof address === "object" && address !== null ? address.port : null
            server.close((error) => error ? reject(error) : resolve(port))
        })
    })
}

// //// 读取一个 NUL JSON 响应帧 [@x380kkm 2026-07-23] ////
async function readNulJsonFrame(socket) {
    return (await readNulJsonFrames(socket, 1))[0]
}
// //// /读取一个 NUL JSON 响应帧 ////

// //// 读取同一 TCP 会话连续返回的多个 NUL JSON 帧 [@x380kkm 2026-07-23] ////
function readNulJsonFrames(socket, expectedCount) {
    return new Promise((resolve, reject) => {
        let buffer = ""
        const frames = []
        let timeout = null
        const cleanup = () => {
            if (timeout !== null) clearTimeout(timeout)
            socket.off("data", receive)
            socket.off("error", fail)
            socket.off("close", close)
        }
        const fail = (error) => {
            cleanup()
            reject(error)
        }
        const close = () => fail(new Error("Multiplayer session closed before sending all expected frames."))
        const receive = (chunk) => {
            buffer += chunk
            try {
                while (true) {
                    const separatorIndex = buffer.indexOf("\0")
                    if (separatorIndex < 0) break
                    frames.push(JSON.parse(buffer.slice(0, separatorIndex)))
                    buffer = buffer.slice(separatorIndex + 1)
                }
            } catch (error) {
                fail(error)
                return
            }
            if (frames.length > expectedCount) {
                fail(new Error("Multiplayer session returned more frames than expected."))
                return
            }
            if (frames.length === expectedCount) {
                cleanup()
                resolve(frames)
            }
        }
        timeout = setTimeout(() => fail(new Error("Timed out waiting for multiplayer session frames.")), 5000)
        socket.on("data", receive)
        socket.once("error", fail)
        socket.once("close", close)
    })
}
// //// /读取同一 TCP 会话连续返回的多个 NUL JSON 帧 ////

// //// 写入一个 NUL JSON 请求帧 [@x380kkm 2026-07-23] ////
function writeNulJsonFrame(socket, frame) {
    socket.write(`${JSON.stringify(frame)}\0`)
}
// //// /写入一个 NUL JSON 请求帧 ////

// //// 打开 TCP 连接并读取一个 NUL JSON 响应帧 [@x380kkm 2026-07-23] ////
function openNulJsonSession(port, requestFrame) {
    return new Promise((resolve, reject) => {
        const socket = require("node:net").createConnection({ host: "127.0.0.1", port })
        let timeout = null
        let isResolved = false
        const clearConnectTimeout = () => {
            if (timeout !== null) clearTimeout(timeout)
        }
        const rejectSessionOpen = (error) => {
            if (isResolved) return
            clearConnectTimeout()
            reject(error)
        }
        socket.once("error", rejectSessionOpen)
        socket.setEncoding("utf8")
        socket.once("connect", async () => {
            clearConnectTimeout()
            const response = readNulJsonFrame(socket)
            const encoded = `${JSON.stringify(requestFrame)}\0`
            const splitAt = Math.floor(encoded.length / 2)
            socket.write(encoded.slice(0, splitAt))
            socket.write(encoded.slice(splitAt))
            try {
                const frame = await response
                isResolved = true
                resolve({ socket, frame })
            } catch (error) {
                socket.destroy()
                reject(error)
            }
        })
        timeout = setTimeout(() => {
            clearConnectTimeout()
            socket.destroy()
            reject(new Error("Timed out connecting to the multiplayer session server."))
        }, 5000)
    })
}
// //// /打开 TCP 连接并读取一个 NUL JSON 响应帧 ////

// //// 等待 TCP 会话关闭 [@x380kkm 2026-07-23] ////
function waitForSocketClose(socket) {
    if (socket.destroyed) return Promise.resolve()
    return new Promise((resolve, reject) => {
        let timeout = null
        const cleanup = () => {
            if (timeout !== null) clearTimeout(timeout)
            socket.off("close", complete)
            socket.off("error", fail)
        }
        const complete = () => {
            cleanup()
            resolve()
        }
        const fail = (error) => {
            cleanup()
            reject(error)
        }
        timeout = setTimeout(() => fail(new Error("Timed out waiting for the multiplayer session to close.")), 5000)
        socket.once("close", complete)
        socket.once("error", fail)
    })
}
// //// /等待 TCP 会话关闭 ////

// //// 断言 TCP 会话在短窗口内不返回未经证实的帧 [@x380kkm 2026-07-23] ////
function assertSocketRemainsUnanswered(socket, milliseconds = 100) {
    return new Promise((resolve, reject) => {
        const cleanup = () => {
            clearTimeout(timeout)
            socket.off("data", receive)
            socket.off("error", fail)
            socket.off("close", close)
        }
        const receive = () => {
            cleanup()
            reject(new Error("Multiplayer session returned an unverified frame."))
        }
        const fail = (error) => {
            cleanup()
            reject(error)
        }
        const close = () => {
            cleanup()
            reject(new Error("Multiplayer session closed while an unmodeled command was pending."))
        }
        const timeout = setTimeout(() => {
            cleanup()
            resolve()
        }, milliseconds)
        socket.once("data", receive)
        socket.once("error", fail)
        socket.once("close", close)
    })
}
// //// /断言 TCP 会话在短窗口内不返回未经证实的帧 ////

async function unpackResponse(response) {
    const contentType = response.headers.get("content-type") ?? ""
    if (contentType.startsWith("application/x-msgpack")) {
        return unpack(Buffer.from(await response.text(), "base64"))
    }
    return response.json()
}

// //// 按 CN 客户端线格式编码请求体 [@x380kkm 2026-07-22] ////
function encodeCnRequestBody(data) {
    return Buffer.from(pack(data)).toString("base64")
}
// //// /按 CN 客户端线格式编码请求体 ////

// //// 验证雷霆防沉迷登录响应 [@x380kkm 2026-08-18] ////
async function verifyLeitingAntiAddictionLogin(baseUrl) {
    const response = await fetch(`${baseUrl}/api/index.php/channels/channel_leiting/leiting_antiaddiction_login`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({}),
    })

    assert.equal(response.status, 200)
    assert.match(response.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const payload = await unpackResponse(response)
    assert.equal(payload.data_headers.result_code, 1)
    assert.deepEqual(payload.data, {
        status: 0,
        message: "success",
        data: { onlineTime: 0, limitTime: 999999, usableTime: 999999 },
    })
}
// //// /验证雷霆防沉迷登录响应 ////

// //// 验证 CN BattleQuestFinishRealRemote 强制读取的基础字段 [@x380kkm 2026-07-24] ////
function assertBattleFinishProtocolData(data, battleGroup) {
    assert.equal(data.is_multi, battleGroup)
    for (const field of ["before_rank_point", "clear_rank", "old_high_score"]) {
        assert.equal(Number.isFinite(data[field]), true, field)
    }
    for (const field of [
        "drop_additional_reward_ids",
        "drop_periodic_reward_ids",
        "drop_rare_reward_ids",
        "drop_score_reward_ids",
        "joined_character_id_list",
        "presigned_quest_category",
    ]) assert.equal(Array.isArray(data[field]), true, field)
    assert.equal(typeof data.bond_token_status_list, "object")
    assert.notEqual(data.bond_token_status_list, null)
    assert.equal(typeof data.rewards, "object")
    for (const field of [
        "converted_pool_exp",
        "field_mana",
        "overflow_pool_exp",
        "reward_mana",
        "reward_pool_exp",
    ]) assert.equal(Number.isInteger(data.rewards[field]), true, `rewards.${field}`)
}
// //// /验证 CN BattleQuestFinishRealRemote 强制读取的基础字段 ////

// //// 构造 CN v1.8.1 房主 Enter 载荷 [@x380kkm 2026-07-23] ////
function createCnLobbyEnterPlayer(viewerId, connectionId, partyId) {
    return {
        allowHealFromOtherPlayers: true,
        autoskillMode: 1,
        entryTime: 1234.5,
        party: {
            unison_characters: [[1], [1], [1]],
            abilitySoulIds: [[1], [1], [1]],
            equipments: [[1], [1], [1]],
            characters: [[0, {
                illustration_settings: [1],
                id: 131012,
                mana_node_ids: {},
                evolution_level: 5,
                over_limit_step: 4,
                ex_boost: [1],
                exp: 379988,
            }], [1], [1]],
            options: null,
        },
        currentPartyId: partyId,
        autoStart: false,
        name: "CN Protocol Host",
        comId: null,
        autoSpeedLevel: 1,
        degreeId: 1,
        rank: 1,
        autoplayMode: true,
        isNewbie: true,
        playerRoleKind: 1,
        skillAbilityBehaviorMode: 1,
        state: [0],
        dashBehaviorMode: 1,
        viewerId,
        connectionId,
    }
}
// //// /构造 CN v1.8.1 房主 Enter 载荷 ////

// //// 从 HTTP summon 数据生成客户端命令和预期 COM 玩家 [@x380kkm 2026-07-23] ////
function createExpectedCnLobbyCharacter(character) {
    return {
        id: character.id,
        evolution_level: character.evolution_level,
        exp: character.exp,
        over_limit_step: character.over_limit_step,
        mana_node_ids: Object.fromEntries(character.mana_node_ids.map((nodeId) => [String(nodeId), 0])),
        illustration_settings: [1],
        ex_boost: character.ex_boost === undefined ? [1] : [0, character.ex_boost],
    }
}

function createExpectedCnLobbyParty(mate) {
    return {
        characters: mate.party.characters.map((character) => (
            character === null ? [1] : [0, createExpectedCnLobbyCharacter(character)]
        )),
        unison_characters: mate.party.unison_characters.map((character) => (
            character === null ? [1] : [0, createExpectedCnLobbyCharacter(character)]
        )),
        equipments: mate.party.equipments.map((equipment) => equipment === null ? [1] : [0, {
            equipmentId: equipment.equipment_id,
            level: equipment.level,
            enhancementLevel: equipment.enhancement_level,
        }]),
        abilitySoulIds: mate.party.ability_soul_ids.map((abilitySoulId) => (
            abilitySoulId === null ? [1] : [0, abilitySoulId]
        )),
        options: null,
    }
}

function createCnLobbySummonRequest(mate, name) {
    return {
        degreeId: mate.degree_id,
        rank: mate.rank,
        name,
        comId: mate.com_id,
        party: createExpectedCnLobbyParty(mate),
    }
}

function createExpectedCnLobbyNpc(mate, name, roomNumber, position, entryTime) {
    return {
        viewerId: 900000000 + position,
        comId: mate.com_id,
        name,
        rank: mate.rank,
        degreeId: mate.degree_id,
        playerRoleKind: 99,
        party: createExpectedCnLobbyParty(mate),
        connectionId: `${roomNumber}-npc-${position}`,
        autoplayMode: false,
        autoskillMode: 1,
        autoSpeedLevel: 1,
        autoStart: false,
        skillAbilityBehaviorMode: 1,
        dashBehaviorMode: 1,
        allowHealFromOtherPlayers: true,
        state: [0],
        entryTime,
        isNewbie: false,
        isHost: false,
    }
}
// //// /从 HTTP summon 数据生成客户端命令和预期 COM 玩家 ////

// //// 创建 CN 多人 HTTP 请求边界 [@x380kkm 2026-07-23] ////
function createCnMultiplayerRequest(baseUrl) {
    return (route, viewerId, data) => fetch(`${baseUrl}/api/index.php/multi_battle_quest/${route}`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({ viewer_id: viewerId, ...data }),
    })
}
// //// /创建 CN 多人 HTTP 请求边界 ////

// //// 验证失败的房主 Enter 不授权战斗连接 [@x380kkm 2026-07-23] ////
async function verifyRejectedHostEnterCannotAuthorizeBattle(baseUrl, viewerId, sessionPort) {
    const multiplayerRequest = createCnMultiplayerRequest(baseUrl)
    const invalidEnterCases = [
        {
            name: "connection-id",
            createFrame: () => [0, [cnLobbyEnterIndex, createCnLobbyEnterPlayer(viewerId, "not-the-issued-connection", 11), 11]],
        },
        {
            name: "viewer-id",
            createFrame: (connectionId) => [0, [cnLobbyEnterIndex, createCnLobbyEnterPlayer(viewerId + 1, connectionId, 11), 11]],
        },
        {
            name: "current-party-id",
            createFrame: (connectionId) => [0, [cnLobbyEnterIndex, createCnLobbyEnterPlayer(viewerId, connectionId, 12), 11]],
        },
        {
            name: "trailing-party-id",
            createFrame: (connectionId) => [0, [cnLobbyEnterIndex, createCnLobbyEnterPlayer(viewerId, connectionId, 11), 12]],
        },
    ]

    for (const invalidEnterCase of invalidEnterCases) {
        const createRoomResponse = await multiplayerRequest("create_room", viewerId, {
            quest_id: 1001001,
            party_id: 11,
            category: 2,
        })
        assert.equal(createRoomResponse.status, 200)
        const room = (await unpackResponse(createRoomResponse)).data
        assert.equal((await multiplayerRequest("prepare", viewerId, {
            room_number: room.room_number,
            quest_id: 1001001,
            category: 2,
        })).status, 200)

        const lobby = await openNulJsonSession(sessionPort, {
            reconnected: 0,
            socklet: "cooperation_room",
            viewerId,
            roomNumber: room.room_number,
            questCategory: 2,
            questId: 1001001,
        })
        const lobbyClosed = waitForSocketClose(lobby.socket)
        writeNulJsonFrame(lobby.socket, invalidEnterCase.createFrame(lobby.frame[1]))
        await lobbyClosed

        const playId = `cn-rejected-${invalidEnterCase.name}`
        const startResponse = await multiplayerRequest("start", viewerId, {
            room_number: room.room_number,
            quest_id: 1001001,
            party_id: 11,
            category: 2,
            play_id: playId,
            use_boost_point: false,
            use_boss_boost_point: false,
            is_auto_start_mode: false,
        })
        assert.equal(startResponse.status, 200)
        assert.deepEqual((await unpackResponse(startResponse)).data, { is_multi: "multi", play_id: playId })

        const battle = await openNulJsonSession(sessionPort, {
            reconnected: 0,
            socklet: "cooperation_battle",
            connectionId: lobby.frame[1],
            roomNumber: room.room_number,
        })
        assert.deepEqual(battle.frame, [3, "HANDSHAKE_DENIED"])
        battle.socket.destroy()
        assert.equal((await multiplayerRequest("disband_room", viewerId, { room_number: room.room_number })).status, 200)
    }
}
// //// /验证失败的房主 Enter 不授权战斗连接 ////

// //// 验证 CN 单机战斗开始, 继续, 结算和终止 [@x380kkm 2026-07-22] ////
async function verifyCnSingleBattle(baseUrl, viewerId) {
    const request = (route, data) => fetch(`${baseUrl}/api/index.php/single_battle_quest/${route}`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({ viewer_id: viewerId, api_count: 1, ...data }),
    })
    const questId = 1001002
    const category = 1
    const startBattle = () => request("start", {
        quest_id: questId,
        use_boss_boost_point: false,
        use_boost_point: false,
        category,
        play_id: "cn-single-battle-test",
        is_auto_start_mode: false,
        party_id: 1,
    })
    const finishBattle = (playId = "cn-single-battle-test") => request("finish", {
        is_restored: false,
        continue_count: 0,
        elapsed_time_ms: 100000,
        quest_id: questId,
        play_id: playId,
        category,
        score: 1000,
        add_mana: 7,
        is_accomplished: true,
        statistics: {
            clear_phase: 1,
            party: {
                characters: [{ id: 1 }, null, null],
                unison_characters: [null, null, null],
                equipments: [null, null, null],
                ability_soul_ids: [null, null, null],
            },
        },
    })

    const startResponse = await startBattle()
    assert.equal(startResponse.status, 200)
    assert.match(startResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const startData = (await unpackResponse(startResponse)).data
    assert.equal(startData.user_info.last_main_quest_id, questId)
    assert.equal(startData.category_id, category)
    assert.equal(startData.is_multi, "single")

    const continueResponse = await request("play_continue", {
        payment_type: 0,
        quest_id: questId,
        paly_id: "cn-single-battle-test",
        category,
    })
    assert.equal(continueResponse.status, 200)
    assert.match(continueResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const continueData = (await unpackResponse(continueResponse)).data
    assert.equal(continueData.user_info.free_vmoney, 100)
    assert.equal(continueData.user_info.vmoney, 0)

    assert.equal((await finishBattle("wrong-play-id")).status, 409)
    const finishResponse = await finishBattle()
    assert.equal(finishResponse.status, 200)
    assert.match(finishResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const finishData = (await unpackResponse(finishResponse)).data
    assertBattleFinishProtocolData(finishData, "single")
    assert.equal(finishData.clear_rank, 5)
    assert.equal(finishData.before_rank_point, 10)
    assert.equal(finishData.user_info.rank_point, 13)
    assert.ok(finishData.user_info.free_mana >= 1027)
    assert.ok(finishData.user_info.free_vmoney >= 130)
    assert.equal(finishData.rewards.reward_pool_exp, 13)
    assert.equal(finishData.rewards.reward_mana, 20)
    assert.equal(finishData.rewards.field_mana, 7)
    assert.equal(finishData.item_list[13], 10)
    assert.deepEqual(finishData.drop_score_reward_ids[0], {
        group_id: 40000,
        index: 1,
        number: 10,
    })
    const updatedCharacter = finishData.character_list.find((character) => character.character_id === 1)
    assert.equal(updatedCharacter?.exp, 23)
    assert.equal((await finishBattle()).status, 400)

    assert.equal((await startBattle()).status, 200)
    const abortResponse = await request("abort", {
        finish_kind: 1,
        quest_id: questId,
        play_id: "cn-single-battle-test",
        category,
        statistics: {
            clear_phase: 0,
            party: {
                characters: [{ id: 1 }, null, null],
                unison_characters: [null, null, null],
                equipments: [null, null, null],
                ability_soul_ids: [null, null, null],
            },
        },
    })
    assert.equal(abortResponse.status, 200)
    assert.match(abortResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const abortData = (await unpackResponse(abortResponse)).data
    assert.equal(abortData.aborted_play_id, "cn-single-battle-test")
    assert.equal(abortData.category_id, category)
    assert.equal((await finishBattle()).status, 400)
}
// //// /验证 CN 单机战斗开始, 继续, 结算和终止 ////

async function waitUntilReady(child, baseUrl, readErrorOutput) {
    const deadline = Date.now() + 10000
    while (Date.now() < deadline) {
        if (child.exitCode !== null) throw new Error(`CN server exited during startup.\n${readErrorOutput()}`)
        try {
            const response = await fetch(`${baseUrl}/shijtswy/version/client_release_android.dis`)
            if (response.ok) {
                const health = await fetch(`${baseUrl}/healthz`)
                assert.equal(health.status, 200)
                const healthData = await health.json()
                assert.equal(healthData.status, "ok")
                assert.equal(healthData.service, "starpoint")
                assert.equal(Number.isInteger(healthData.httpPort), true)
                assert.equal(Number.isInteger(healthData.sessionPort), true)
                return
            }
        } catch { }
        await new Promise((resolve) => setTimeout(resolve, 100))
    }
    throw new Error(`CN server did not become ready.\n${readErrorOutput()}`)
}

// //// 验证 CN Android 和 iOS 版本入口 [@x380kkm 2026-08-07] ////
async function verifyCnVersionDiscovery(baseUrl) {
    const expectedBody = [
        "// StarPoint CN compatibility endpoint",
        JSON.stringify({ default: { apiScheme: "http", apiPath: new URL(baseUrl).host } }),
    ].join("\r\n")

    for (const platform of ["android", "ios"]) {
        const response = await fetch(`${baseUrl}/shijtswy/version/client_release_${platform}.dis`)
        assert.equal(response.status, 200)
        assert.equal(response.headers.get("content-type"), "text/plain; charset=utf-8")
        assert.equal(await response.text(), expectedBody)
    }
}
// //// /验证 CN Android 和 iOS 版本入口 ////

// //// 验证 CN EntityLists 静态文件入口 [@x380kkm 2026-08-07] ////
async function verifyCnEntityListFile(baseUrl) {
    const response = await fetch(`${baseUrl}/patch/cn/entities/10939-android_medium.csv`)
    assert.equal(response.status, 200)
    assert.equal(await response.text(), "entity")
}
// //// /验证 CN EntityLists 静态文件入口 ////

async function stopChild(child) {
    if (child.exitCode !== null) return
    const closed = new Promise((resolve) => child.once("close", resolve))
    child.kill()
    await Promise.race([closed, new Promise((resolve) => setTimeout(resolve, 5000))])
    if (child.exitCode === null) {
        child.kill("SIGKILL")
        await closed
    }
}

// //// 验证 CN 双人房间不依赖 COM roster 开始战斗 [@x380kkm 2026-07-27] ////
async function verifyCnHumanOnlyLobbyStart(baseUrl, hostViewerId, guestViewerId, sessionPort) {
    const multiplayerRequest = createCnMultiplayerRequest(baseUrl)
    const createRoomResponse = await multiplayerRequest("create_room", hostViewerId, {
        quest_id: 1001001,
        party_id: 11,
        category: 2,
    })
    assert.equal(createRoomResponse.status, 200)
    const room = (await unpackResponse(createRoomResponse)).data

    const prepareResponse = await multiplayerRequest("prepare", hostViewerId, {
        room_number: room.room_number,
        quest_id: 1001001,
        category: 2,
    })
    assert.equal(prepareResponse.status, 200)
    const selectRoomResponse = await multiplayerRequest("select_room", guestViewerId, {
        room_number: room.room_number,
        quest_id: 1001001,
        party_id: 11,
        accepted_type: 1,
        category: 2,
    })
    assert.equal(selectRoomResponse.status, 200)

    const hostLobby = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_room",
        viewerId: hostViewerId,
        roomNumber: room.room_number,
        questCategory: 2,
        questId: 1001001,
    })
    const hostEnterPlayer = createCnLobbyEnterPlayer(hostViewerId, hostLobby.frame[1], 11)
    const hostWelcomeResponse = readNulJsonFrame(hostLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbyEnterIndex, hostEnterPlayer, 11]])
    const hostWelcomeFrame = await hostWelcomeResponse
    assert.equal(hostWelcomeFrame[1]?.[0], cnLobbyWelcomeIndex)

    const guestLobby = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_room",
        viewerId: guestViewerId,
        roomNumber: room.room_number,
        questCategory: 2,
        questId: 1001001,
    })
    const guestEnterPlayer = createCnLobbyEnterPlayer(guestViewerId, guestLobby.frame[1], 11)
    const hostMatesResponse = readNulJsonFrame(hostLobby.socket)
    const guestEnterResponses = readNulJsonFrames(guestLobby.socket, 2)
    writeNulJsonFrame(guestLobby.socket, [0, [cnLobbyEnterIndex, guestEnterPlayer, 11]])
    const [guestWelcomeFrame, guestMatesFrame] = await guestEnterResponses
    const hostMatesFrame = await hostMatesResponse
    const roster = guestWelcomeFrame[1]?.[2]
    assert.equal(Array.isArray(roster), true)
    assert.equal(roster.length, 2)
    assert.deepEqual(hostMatesFrame, [1, [cnLobbyMatesIndex, roster]])
    assert.deepEqual(guestMatesFrame, hostMatesFrame)

    const hostReadyResponse = readNulJsonFrame(hostLobby.socket)
    const guestSeesHostReadyResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbyReadyIndex, [1]]])
    const expectedHostReadyFrame = [1, [cnLobbyStateChangedIndex, hostLobby.frame[1], [1]]]
    assert.deepEqual(await hostReadyResponse, expectedHostReadyFrame)
    assert.deepEqual(await guestSeesHostReadyResponse, expectedHostReadyFrame)

    const hostSeesGuestReadyResponse = readNulJsonFrame(hostLobby.socket)
    const guestReadyResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(guestLobby.socket, [0, [cnLobbyReadyIndex, [1]]])
    const expectedGuestReadyFrame = [1, [cnLobbyStateChangedIndex, guestLobby.frame[1], [1]]]
    assert.deepEqual(await hostSeesGuestReadyResponse, expectedGuestReadyFrame)
    assert.deepEqual(await guestReadyResponse, expectedGuestReadyFrame)

    const readyRoster = roster.map((player) => ({ ...player, state: [1] }))
    const hostStartResponse = readNulJsonFrame(hostLobby.socket)
    const guestStartResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbyStartBattleIndex]])
    const expectedStartFrame = [1, [cnLobbyStartIndex, readyRoster]]
    assert.deepEqual(await hostStartResponse, expectedStartFrame)
    assert.deepEqual(await guestStartResponse, expectedStartFrame)

    const disbandResponse = await multiplayerRequest("disband_room", hostViewerId, { room_number: room.room_number })
    assert.equal(disbandResponse.status, 200)
    hostLobby.socket.destroy()
    guestLobby.socket.destroy()
}
// //// /验证 CN 双人房间不依赖 COM roster 开始战斗 ////

// //// 验证 CN 多人房间, TCP 大厅与战斗协议和 COM 回填接口 [@x380kkm 2026-07-22] ////
async function verifyCnMultiplayer(baseUrl, hostViewerId, guestViewerId, sourcePlayers, sessionPort) {
    const npcConfigs = sourcePlayers.map((sourcePlayer, index) => ({
        id: `cn-test-com-${index + 1}`,
        displayName: `CN Test COM ${index + 1}`,
        enabled: true,
        pairingKey: "2:1001001",
        sourcePlayerId: sourcePlayer.id,
        partySlot: sourcePlayer.party_slot,
        rank: 10 + index,
        degreeId: null,
    }))
    const configureNpcFill = (delaySeconds) => fetch(`${baseUrl}/manage/api/npc`, {
        method: "PUT",
        headers: {
            "content-type": "application/json",
            authorization: "Bearer cn-management-test-token",
        },
        body: JSON.stringify({
            enabled: true,
            delaySeconds,
            mates: npcConfigs,
        }),
    })
    assert.equal((await configureNpcFill(30)).status, 200)

    const virtualTimeResponse = await fetch(`${baseUrl}/manage/api/time`, {
        method: "PUT",
        headers: {
            "content-type": "application/json",
            authorization: "Bearer cn-management-test-token",
        },
        body: JSON.stringify({ enabled: true, iso: "2030-01-01T00:00:00.000Z", rate: 1 }),
    })
    assert.equal(virtualTimeResponse.status, 200)
    const readServerTimeMilliseconds = async () => {
        const response = await fetch(`${baseUrl}/manage/api/status`, {
            headers: { authorization: "Bearer cn-management-test-token" },
        })
        assert.equal(response.status, 200)
        const value = Date.parse((await response.json()).serverDate)
        assert.equal(Number.isFinite(value), true)
        return value
    }

    const multiplayerRequest = createCnMultiplayerRequest(baseUrl)
    const createRoomResponse = await multiplayerRequest("create_room", hostViewerId, {
        quest_id: 1001001,
        party_id: 11,
        category: 2,
    })
    if (createRoomResponse.status !== 200) {
        throw new Error(`Create room failed: ${createRoomResponse.status} ${await createRoomResponse.text()}`)
    }
    const createdRoom = (await unpackResponse(createRoomResponse)).data
    assert.match(createdRoom.room_number, /^\d{6}$/)

    const roomsResponse = await multiplayerRequest("get_rooms", guestViewerId, { category_id: 2 })
    assert.equal(roomsResponse.status, 200)
    const rooms = (await unpackResponse(roomsResponse)).data.rooms
    assert.equal(rooms.some((room) => room.room_number === createdRoom.room_number), true)

    const prepareResponse = await multiplayerRequest("prepare", hostViewerId, {
        room_number: createdRoom.room_number,
        quest_id: 1001001,
        category: 2,
    })
    assert.equal(prepareResponse.status, 200)
    const preparedRoom = (await unpackResponse(prepareResponse)).data
    assert.equal(preparedRoom.ip_address, "127.0.0.1")
    assert.equal(preparedRoom.port, sessionPort)

    const selectRoomResponse = await multiplayerRequest("select_room", guestViewerId, {
        room_number: createdRoom.room_number,
        quest_id: 1001001,
        party_id: 11,
        accepted_type: 1,
        category: 2,
    })
    assert.equal(selectRoomResponse.status, 200)
    const selectedRoom = (await unpackResponse(selectRoomResponse)).data
    assert.equal(selectedRoom.room_sequence, preparedRoom.room_sequence)
    assert.equal(selectedRoom.port, sessionPort)

    const hostLobby = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_room",
        viewerId: hostViewerId,
        roomNumber: createdRoom.room_number,
        questCategory: 2,
        questId: 1001001,
    })
    assert.equal(hostLobby.frame[0], 0)
    assert.match(hostLobby.frame[1], /^[a-f0-9]{32}$/)
    assert.equal(hostLobby.frame[2], createdRoom.room_number)

    const hostEnterPlayer = createCnLobbyEnterPlayer(hostViewerId, hostLobby.frame[1], 11)
    const welcomeStartedAt = await readServerTimeMilliseconds()
    const welcomeResponse = readNulJsonFrame(hostLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbyEnterIndex, hostEnterPlayer, 11]])
    const welcomeFrame = await welcomeResponse
    const welcomeCompletedAt = await readServerTimeMilliseconds()
    const normalizedHost = welcomeFrame[1]?.[2]?.[0]
    assert.equal(Number.isSafeInteger(normalizedHost?.entryTime), true)
    assert.ok(normalizedHost.entryTime >= welcomeStartedAt && normalizedHost.entryTime <= welcomeCompletedAt)
    assert.notEqual(normalizedHost.entryTime, hostEnterPlayer.entryTime)
    const expectedHost = {
        viewerId: hostViewerId,
        playerId: cnLobbyFirstHumanPlayerId,
        name: hostEnterPlayer.name,
        rank: hostEnterPlayer.rank,
        degreeId: hostEnterPlayer.degreeId,
        mainCharacterId: hostEnterPlayer.party.characters[0][1].id,
        party: hostEnterPlayer.party,
        connectionId: hostLobby.frame[1],
        playerRoleKind: hostEnterPlayer.playerRoleKind,
        isNewbie: hostEnterPlayer.isNewbie,
        isHost: true,
        entryTime: normalizedHost.entryTime,
        currentPartyId: hostEnterPlayer.currentPartyId,
        autoplayMode: hostEnterPlayer.autoplayMode,
        autoskillMode: hostEnterPlayer.autoskillMode,
        autoSpeedLevel: hostEnterPlayer.autoSpeedLevel,
        autoStart: hostEnterPlayer.autoStart,
        skillAbilityBehaviorMode: hostEnterPlayer.skillAbilityBehaviorMode,
        dashBehaviorMode: hostEnterPlayer.dashBehaviorMode,
        allowHealFromOtherPlayers: hostEnterPlayer.allowHealFromOtherPlayers,
        state: hostEnterPlayer.state,
    }
    const expectedRoom = {
        roomNumber: createdRoom.room_number,
        establisherConnectionId: hostLobby.frame[1],
        establisherName: hostEnterPlayer.name,
        establisherCharacter: hostEnterPlayer.party.characters[0][1].id,
        questCategory: 2,
        questId: 1001001,
        status: 2,
    }
    assert.deepEqual(welcomeFrame, [1, [cnLobbyWelcomeIndex, expectedRoom, [expectedHost]]])

    const heartbeatResponse = readNulJsonFrame(hostLobby.socket)
    writeNulJsonFrame(hostLobby.socket, cnLobbyHeartbeatFrame)
    assert.deepEqual(await heartbeatResponse, [1, [cnLobbyHeartbeatResponseIndex, hostLobby.frame[1]]])

    const earlyBattle = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_battle",
        connectionId: hostLobby.frame[1],
        roomNumber: createdRoom.room_number,
    })
    assert.deepEqual(earlyBattle.frame, [3, "HANDSHAKE_DENIED"])
    earlyBattle.socket.destroy()

    const guestLobby = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_room",
        viewerId: guestViewerId,
        roomNumber: createdRoom.room_number,
        questCategory: 2,
        questId: 1001001,
    })
    assert.equal(guestLobby.frame[0], 0)
    const guestEnterPlayer = createCnLobbyEnterPlayer(guestViewerId, guestLobby.frame[1], 11)
    const hostMatesResponse = readNulJsonFrame(hostLobby.socket)
    const guestLobbyFramesResponse = readNulJsonFrames(guestLobby.socket, 2)
    writeNulJsonFrame(guestLobby.socket, [0, [
        cnLobbyEnterIndex,
        guestEnterPlayer,
        11,
    ]])
    const [guestWelcomeFrame, guestMatesFrame] = await guestLobbyFramesResponse
    const hostMatesFrame = await hostMatesResponse
    let expectedGuest = {
        viewerId: guestViewerId,
        playerId: 3,
        name: guestEnterPlayer.name,
        rank: guestEnterPlayer.rank,
        degreeId: guestEnterPlayer.degreeId,
        mainCharacterId: guestEnterPlayer.party.characters[0][1].id,
        party: guestEnterPlayer.party,
        connectionId: guestLobby.frame[1],
        playerRoleKind: guestEnterPlayer.playerRoleKind,
        isNewbie: guestEnterPlayer.isNewbie,
        isHost: false,
        entryTime: guestWelcomeFrame[1][2][1].entryTime,
        currentPartyId: guestEnterPlayer.currentPartyId,
        autoplayMode: guestEnterPlayer.autoplayMode,
        autoskillMode: guestEnterPlayer.autoskillMode,
        autoSpeedLevel: guestEnterPlayer.autoSpeedLevel,
        autoStart: guestEnterPlayer.autoStart,
        skillAbilityBehaviorMode: guestEnterPlayer.skillAbilityBehaviorMode,
        dashBehaviorMode: guestEnterPlayer.dashBehaviorMode,
        allowHealFromOtherPlayers: guestEnterPlayer.allowHealFromOtherPlayers,
        state: guestEnterPlayer.state,
    }
    assert.equal(Number.isSafeInteger(expectedGuest.entryTime), true)
    assert.deepEqual(guestWelcomeFrame, [1, [cnLobbyWelcomeIndex, expectedRoom, [expectedHost, expectedGuest]]])
    assert.deepEqual(hostMatesFrame, [1, [cnLobbyMatesIndex, [expectedHost, expectedGuest]]])
    assert.deepEqual(guestMatesFrame, [1, [cnLobbyMatesIndex, [expectedHost, expectedGuest]]])

    const hostReadyResponse = readNulJsonFrame(hostLobby.socket)
    const guestReadyFromHostResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbyReadyIndex, [1]]])
    assert.deepEqual(await hostReadyResponse, [1, [cnLobbyStateChangedIndex, hostLobby.frame[1], [1]]])
    assert.deepEqual(await guestReadyFromHostResponse, [1, [cnLobbyStateChangedIndex, hostLobby.frame[1], [1]]])
    const hostReadyFromGuestResponse = readNulJsonFrame(hostLobby.socket)
    const guestReadyResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(guestLobby.socket, [0, [cnLobbyReadyIndex, [1]]])
    assert.deepEqual(await hostReadyFromGuestResponse, [1, [cnLobbyStateChangedIndex, guestLobby.frame[1], [1]]])
    assert.deepEqual(await guestReadyResponse, [1, [cnLobbyStateChangedIndex, guestLobby.frame[1], [1]]])

    const changePartyStartedAt = await readServerTimeMilliseconds()
    const hostPartyChangedResponse = readNulJsonFrame(hostLobby.socket)
    const guestPartyChangedResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(guestLobby.socket, [0, [
        cnLobbyChangePartyIndex,
        guestEnterPlayer,
        false,
        11,
    ]])
    const hostPartyChangedFrame = await hostPartyChangedResponse
    const guestPartyChangedFrame = await guestPartyChangedResponse
    const changePartyCompletedAt = await readServerTimeMilliseconds()
    const changedGuestEntryTime = hostPartyChangedFrame[1]?.[1]?.[1]?.entryTime
    assert.equal(Number.isSafeInteger(changedGuestEntryTime), true)
    assert.ok(changedGuestEntryTime >= changePartyStartedAt && changedGuestEntryTime <= changePartyCompletedAt)
    expectedGuest = { ...expectedGuest, entryTime: changedGuestEntryTime, state: [0] }
    const changedPartyRoster = [{ ...expectedHost, state: [1] }, expectedGuest]
    assert.deepEqual(hostPartyChangedFrame, [1, [cnLobbyMatesIndex, changedPartyRoster]])
    assert.deepEqual(guestPartyChangedFrame, hostPartyChangedFrame)

    const hostReadyAfterPartyChangeResponse = readNulJsonFrame(hostLobby.socket)
    const guestReadyAfterPartyChangeResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(guestLobby.socket, [0, [cnLobbyReadyIndex, [1]]])
    assert.deepEqual(await hostReadyAfterPartyChangeResponse, [1, [cnLobbyStateChangedIndex, guestLobby.frame[1], [1]]])
    assert.deepEqual(await guestReadyAfterPartyChangeResponse, [1, [cnLobbyStateChangedIndex, guestLobby.frame[1], [1]]])

    const hostSuspendedResponse = readNulJsonFrame(hostLobby.socket)
    const guestSeesHostSuspendedResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbySuspendIndex]])
    assert.deepEqual(await hostSuspendedResponse, [1, [cnLobbyStateChangedIndex, hostLobby.frame[1], [0]]])
    assert.deepEqual(await guestSeesHostSuspendedResponse, [1, [cnLobbyStateChangedIndex, hostLobby.frame[1], [0]]])

    const hostReadyAfterSuspendResponse = readNulJsonFrame(hostLobby.socket)
    const guestSeesHostReadyAfterSuspendResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbyReadyIndex, [1]]])
    assert.deepEqual(await hostReadyAfterSuspendResponse, [1, [cnLobbyStateChangedIndex, hostLobby.frame[1], [1]]])
    assert.deepEqual(await guestSeesHostReadyAfterSuspendResponse, [1, [cnLobbyStateChangedIndex, hostLobby.frame[1], [1]]])

    for (const [autoplayMode, resetAutoSpeed] of [[false, true], [true, false]]) {
        const hostAutoplayChangedResponse = readNulJsonFrame(hostLobby.socket)
        const guestAutoplayChangedResponse = readNulJsonFrame(guestLobby.socket)
        writeNulJsonFrame(guestLobby.socket, [0, [cnLobbyChangeAutoplayModeIndex, autoplayMode, resetAutoSpeed]])
        const expectedFrame = [1, [
            cnLobbyAutoplayModeChangedIndex,
            guestLobby.frame[1],
            autoplayMode,
            resetAutoSpeed,
        ]]
        assert.deepEqual(await hostAutoplayChangedResponse, expectedFrame)
        assert.deepEqual(await guestAutoplayChangedResponse, expectedFrame)
    }

    for (const autoStart of [true, false]) {
        const hostAutoStartChangedResponse = readNulJsonFrame(hostLobby.socket)
        const guestAutoStartChangedResponse = readNulJsonFrame(guestLobby.socket)
        writeNulJsonFrame(guestLobby.socket, [0, [cnLobbyChangeAutoStartIndex, autoStart]])
        const expectedFrame = [1, [cnLobbyAutoStartChangedIndex, guestLobby.frame[1], autoStart]]
        assert.deepEqual(await hostAutoStartChangedResponse, expectedFrame)
        assert.deepEqual(await guestAutoStartChangedResponse, expectedFrame)
    }

    const deniedLobby = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_room",
        viewerId: 999999999,
        roomNumber: createdRoom.room_number,
        questCategory: 2,
        questId: 1001001,
    })
    assert.deepEqual(deniedLobby.frame, [3, "HANDSHAKE_DENIED"])
    deniedLobby.socket.destroy()

    const summonBody = {
        room_number: createdRoom.room_number,
        quest_id: 1001001,
        category_id: 2,
    }
    const earlyMatesRemainUnanswered = assertSocketRemainsUnanswered(hostLobby.socket)
    const earlySummonResponse = await multiplayerRequest("summon", hostViewerId, summonBody)
    assert.equal(earlySummonResponse.status, 200)
    assert.deepEqual((await unpackResponse(earlySummonResponse)).data, {})
    await earlyMatesRemainUnanswered

    assert.equal((await configureNpcFill(0)).status, 200)
    const stagedMatesRemainUnanswered = assertSocketRemainsUnanswered(hostLobby.socket)
    const summonResponse = await multiplayerRequest("summon", hostViewerId, summonBody)
    assert.equal(summonResponse.status, 200)
    assert.match(summonResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const summonData = (await unpackResponse(summonResponse)).data
    const summonedMates = [summonData.mate1, summonData.mate2].filter(Boolean)
    assert.equal(summonedMates.length, 1)
    const clientMateNames = ["CN Random Mate A", "CN Random Mate B"].slice(0, summonedMates.length)
    for (const [index, summonedMate] of summonedMates.entries()) {
        assert.equal(summonedMate.com_id, sourcePlayers[index].id)
        assert.equal(summonedMate.rank, npcConfigs[index].rank)
        assert.equal(summonedMate.party.characters.length, 3)
    }
    await stagedMatesRemainUnanswered
    const clientMateRequests = summonedMates.map((mate, index) => (
        createCnLobbySummonRequest(mate, clientMateNames[index])
    ))
    const matesStartedAt = await readServerTimeMilliseconds()
    const expectedReadyFrameCount = summonedMates.length + 4
    const readyFramesResponse = readNulJsonFrames(hostLobby.socket, expectedReadyFrameCount)
    const guestReadyFramesResponse = readNulJsonFrames(guestLobby.socket, expectedReadyFrameCount)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbySummonIndex, clientMateRequests]])
    const readyFrames = await readyFramesResponse
    const guestReadyFrames = await guestReadyFramesResponse
    const matesCompletedAt = await readServerTimeMilliseconds()
    const matesFrame = readyFrames[0]
    const matesRoster = matesFrame[1]?.[1]
    assert.equal(Array.isArray(matesRoster), true)
    assert.equal(matesRoster.length, 2 + summonedMates.length)
    const npcEntryTime = matesRoster[2].entryTime
    assert.equal(Number.isSafeInteger(npcEntryTime), true)
    assert.ok(npcEntryTime >= matesStartedAt && npcEntryTime <= matesCompletedAt)
    const expectedNpcs = summonedMates.map((mate, index) => createExpectedCnLobbyNpc(
        mate,
        clientMateNames[index],
        createdRoom.room_number,
        index + 1,
        npcEntryTime,
    ))
    const readyHost = { ...expectedHost, state: [1] }
    const readyGuest = { ...expectedGuest, state: [1] }
    const joinedRoster = [readyHost, readyGuest, ...expectedNpcs]
    const readyRoster = joinedRoster.map((player) => ({ ...player, state: [1] }))
    const expectedStateChanges = expectedNpcs.map((npc) => ([
        1,
        [cnLobbyStateChangedIndex, npc.connectionId, [1]],
    ]))
    assert.deepEqual(readyFrames, [
        [1, [cnLobbyMatesIndex, joinedRoster]],
        ...expectedStateChanges,
        [1, [cnLobbyStateChangedIndex, expectedHost.connectionId, [1]]],
        [1, [cnLobbyStateChangedIndex, expectedGuest.connectionId, [1]]],
        [1, [cnLobbyStartRemainingIndex, 2]],
    ])
    assert.deepEqual(guestReadyFrames, readyFrames)

    const lobbyStartResponse = readNulJsonFrame(hostLobby.socket)
    const guestLobbyStartResponse = readNulJsonFrame(guestLobby.socket)
    writeNulJsonFrame(hostLobby.socket, [0, [cnLobbyStartBattleIndex]])
    assert.deepEqual(await lobbyStartResponse, [1, [cnLobbyStartIndex, readyRoster]])
    assert.deepEqual(await guestLobbyStartResponse, [1, [cnLobbyStartIndex, readyRoster]])

    const playId = "cn-session-integration-test"
    assert.equal((await multiplayerRequest("start", guestViewerId, {
        room_number: createdRoom.room_number,
        quest_id: 1001001,
        party_id: 11,
        category: 2,
        play_id: playId,
    })).status, 403)
    const startResponse = await multiplayerRequest("start", hostViewerId, {
        room_number: createdRoom.room_number,
        quest_id: 1001001,
        party_id: 11,
        category: 2,
        play_id: playId,
        use_boost_point: false,
        use_boss_boost_point: false,
        is_auto_start_mode: false,
    })
    assert.equal(startResponse.status, 200)
    assert.deepEqual((await unpackResponse(startResponse)).data, { is_multi: "multi", play_id: playId })
    const repeatedStart = await multiplayerRequest("start", hostViewerId, {
        room_number: createdRoom.room_number,
        quest_id: 1001001,
        party_id: 11,
        category: 2,
        play_id: playId,
    })
    assert.equal(repeatedStart.status, 200)
    assert.deepEqual((await unpackResponse(repeatedStart)).data, { is_multi: "multi", play_id: playId })
    assert.equal((await multiplayerRequest("start", hostViewerId, {
        room_number: createdRoom.room_number,
        quest_id: 1001001,
        party_id: 11,
        category: 2,
        play_id: "different-play-id",
    })).status, 403)

    const byeResponse = readNulJsonFrame(hostLobby.socket)
    const hostLobbyClosed = waitForSocketClose(hostLobby.socket)
    writeNulJsonFrame(hostLobby.socket, cnLobbyByeFrame)
    assert.deepEqual(await byeResponse, [1, [cnLobbyMatesIndex, readyRoster]])
    await hostLobbyClosed

    const hostBattle = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_battle",
        connectionId: hostLobby.frame[1],
        roomNumber: createdRoom.room_number,
    })
    assert.deepEqual(hostBattle.frame, [0, createdRoom.room_number, ""])
    const guestBattle = await openNulJsonSession(sessionPort, {
        reconnected: 0,
        socklet: "cooperation_battle",
        connectionId: guestLobby.frame[1],
        roomNumber: createdRoom.room_number,
    })
    assert.deepEqual(guestBattle.frame, [0, createdRoom.room_number, ""])

    const hostReadyRemainsUnanswered = assertSocketRemainsUnanswered(hostBattle.socket)
    writeNulJsonFrame(hostBattle.socket, [0, [0]])
    await hostReadyRemainsUnanswered
    const battleStartedResponse = readNulJsonFrame(hostBattle.socket)
    const guestBattleStartedResponse = readNulJsonFrame(guestBattle.socket)
    writeNulJsonFrame(guestBattle.socket, [0, [0]])
    assert.deepEqual(await battleStartedResponse, [1, [1]])
    assert.deepEqual(await guestBattleStartedResponse, [1, [1]])

    const battleHeartbeatRemainsUnanswered = assertSocketRemainsUnanswered(hostBattle.socket)
    writeNulJsonFrame(hostBattle.socket, [0, [4]])
    await battleHeartbeatRemainsUnanswered

    const battleMessages = [[0, 0, 0, 0, 0, "AAAAABMAEQAAAA"]]
    const battleBroadcastResponse = readNulJsonFrame(hostBattle.socket)
    const guestBattleBroadcastResponse = readNulJsonFrame(guestBattle.socket)
    writeNulJsonFrame(hostBattle.socket, [1, battleMessages])
    assert.deepEqual(await battleBroadcastResponse, [2, hostLobby.frame[1], battleMessages])
    assert.deepEqual(await guestBattleBroadcastResponse, [2, hostLobby.frame[1], battleMessages])

    const guestBattleMessages = [[0, 1, 0, 0, 101, null]]
    const hostReceivesGuestBroadcast = readNulJsonFrame(hostBattle.socket)
    const guestReceivesOwnBroadcast = readNulJsonFrame(guestBattle.socket)
    writeNulJsonFrame(guestBattle.socket, [1, guestBattleMessages])
    assert.deepEqual(await hostReceivesGuestBroadcast, [2, guestLobby.frame[1], guestBattleMessages])
    assert.deepEqual(await guestReceivesOwnBroadcast, [2, guestLobby.frame[1], guestBattleMessages])

    const userCommandMessage = [0, [22]]
    const hostDoesNotReceiveSend = assertSocketRemainsUnanswered(hostBattle.socket)
    const guestReceivesSend = readNulJsonFrame(guestBattle.socket)
    writeNulJsonFrame(hostBattle.socket, [2, [guestLobby.frame[1]], userCommandMessage])
    assert.deepEqual(await guestReceivesSend, [3, hostLobby.frame[1], userCommandMessage])
    await hostDoesNotReceiveSend

    const clientMeasurementTime = 1723710907565.6667
    const measurementStartedAt = await readServerTimeMilliseconds()
    const measurementResponse = readNulJsonFrame(hostBattle.socket)
    writeNulJsonFrame(hostBattle.socket, [0, [2, 600, clientMeasurementTime]])
    const measurementFrame = await measurementResponse
    const measurementCompletedAt = await readServerTimeMilliseconds()
    const measurementServerTime = measurementFrame[1]?.[3]
    assert.equal(Number.isSafeInteger(measurementServerTime), true)
    assert.ok(measurementServerTime >= measurementStartedAt && measurementServerTime <= measurementCompletedAt)
    assert.deepEqual(measurementFrame, [1, [3, 600, clientMeasurementTime, measurementServerTime]])

    const guestLeaveResponse = readNulJsonFrame(hostBattle.socket)
    const guestBattleClosed = waitForSocketClose(guestBattle.socket)
    guestBattle.socket.destroy()
    assert.deepEqual(await guestLeaveResponse, [1, [0, guestLobby.frame[1]]])
    await guestBattleClosed

    const hostFinalizedResponse = readNulJsonFrame(hostBattle.socket)
    const hostBattleClosed = waitForSocketClose(hostBattle.socket)
    writeNulJsonFrame(hostBattle.socket, [0, [1]])
    assert.deepEqual(await hostFinalizedResponse, [1, [2]])
    await hostBattleClosed

    const finishData = {
        is_restored: false,
        continue_count: 0,
        elapsed_time_ms: 100000,
        quest_id: 1001001,
        category: 2,
        play_id: playId,
        score: 1000,
        contribution_score: 500,
        add_mana: 7,
        is_accomplished: true,
        isolated: false,
        priority_factors: [],
        mate_player_result: [
            { viewer_id: hostViewerId, com_id: null, score: 1000, contribution_score: 500 },
            { viewer_id: guestViewerId, com_id: null, score: 1000, contribution_score: 500 },
        ],
        statistics: {
            clear_phase: 1,
            party: {
                characters: [{ id: 1 }, null, null],
                unison_characters: [null, null, null],
                equipments: [null, null, null],
                ability_soul_ids: [null, null, null],
            },
        },
    }
    assert.equal((await multiplayerRequest("finish", hostViewerId, {
        ...finishData,
        play_id: "wrong-play-id",
    })).status, 409)
    assert.equal((await multiplayerRequest("finish", hostViewerId, {
        ...finishData,
        quest_id: 1001002,
    })).status, 409)
    const hostFinishResponse = await multiplayerRequest("finish", hostViewerId, finishData)
    assert.equal(hostFinishResponse.status, 200)
    const hostFinish = (await unpackResponse(hostFinishResponse)).data
    assertBattleFinishProtocolData(hostFinish, "multi")
    assert.equal((await multiplayerRequest("finish", hostViewerId, finishData)).status, 400)

    assert.equal((await multiplayerRequest("abort", guestViewerId, {
        ...finishData,
        play_id: "wrong-play-id",
        finish_kind: 1,
    })).status, 409)
    const guestAbortResponse = await multiplayerRequest("abort", guestViewerId, {
        ...finishData,
        finish_kind: 1,
    })
    assert.equal(guestAbortResponse.status, 200)
    assert.match(guestAbortResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const guestAbort = (await unpackResponse(guestAbortResponse)).data
    assert.equal(guestAbort.aborted_play_id, playId)
    assert.equal(guestAbort.is_multi, "multi")
    assert.equal(guestAbort.category_id, 2)
    assert.equal((await multiplayerRequest("finish", guestViewerId, finishData)).status, 400)

    const disbandResponse = await multiplayerRequest("disband_room", hostViewerId, { room_number: createdRoom.room_number })
    assert.equal(disbandResponse.status, 200)
    assert.equal((await multiplayerRequest("summon", hostViewerId, summonBody)).status, 404)
    await verifyRejectedHostEnterCannotAuthorizeBattle(baseUrl, guestViewerId, sessionPort)
    hostLobby.socket.destroy()
    guestLobby.socket.destroy()
}
// //// /验证 CN 多人房间, TCP 大厅与战斗协议和 COM 回填接口 ////

// //// 验证 CN 普通扭蛋扣费, 资源发放, 角色兑换和持久化 [@x380kkm 2026-07-24] ////
async function verifyCnGacha(baseUrl, viewerId, playerId, databasePath, load) {
    const requestGacha = (endpoint, body) => fetch(`${baseUrl}/api/index.php/gacha/${endpoint}`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody(body),
    })
    const execute = (targetViewerId, gachaId, overrides = {}) => requestGacha("exec", {
        api_count: 1,
        payment_type: 1,
        number_of_exec: 1,
        viewer_id: targetViewerId,
        gacha_id: gachaId,
        type: 1,
        ...overrides,
    })

    assert.equal((await execute(999999999, 1)).status, 400)
    assert.equal((await execute(viewerId, 9999)).status, 400)

    const response = await execute(viewerId, 1)
    assert.equal(response.status, 200)
    assert.match(response.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const data = (await unpackResponse(response)).data
    assert.equal(data.user_info.free_vmoney, 1350)
    assert.equal(data.draw.length, 1)
    assert.equal(data.character_list.length, 1)
    assert.deepEqual(data.gacha_info_list[0], {
        gacha_id: 1,
        is_account_first: false,
        is_daily_first: false,
        gacha_exchange_point: 1,
    })

    const characterId = data.draw[0].character_id
    assert.ok(cnBaselineCharacterIds.has(characterId))
    const rarityKey = String(Math.floor(characterId / 100000))
    const movieType = data.draw[0].movie_id.endsWith("guarantee") ? "1" : "0"
    const movieSeedPool = readGachaMovieSeedPool(data.draw[0].movie_id, rarityKey, movieType)
    assert.ok(movieSeedPool.includes(data.draw[0].seed))
    const database = new Database(databasePath)
    const gachaInfo = database.prepare(
        `SELECT gacha_id, is_account_first, is_daily_first, gacha_exchange_point, player_id
        FROM players_gacha_info
        WHERE player_id = ? AND gacha_id = 1`,
    ).get(playerId)
    const player = database.prepare("SELECT free_vmoney FROM players WHERE id = ?").get(gachaInfo.player_id)
    const character = database.prepare(
        "SELECT entry_count FROM players_characters WHERE player_id = ? AND id = ?",
    ).get(gachaInfo.player_id, characterId)
    database.close()
    assert.deepEqual(
        [gachaInfo.gacha_id, gachaInfo.is_account_first, gachaInfo.is_daily_first, gachaInfo.gacha_exchange_point],
        [1, 0, 0, 1],
    )
    assert.equal(player.free_vmoney, 1350)
    assert.ok(character.entry_count >= 1)

    const restored = (await unpackResponse(await load(viewerId))).data
    assert.equal(restored.user_info.free_vmoney, 1350)
    assert.deepEqual(restored.gacha_info_list.find((gachaInfo) => gachaInfo.gacha_id === 1), data.gacha_info_list[0])
    assert.ok(restored.gacha_info_list.some((gachaInfo) => gachaInfo.gacha_id === 1704))

    assert.equal((await execute(viewerId, 1, { type: 99 })).status, 400)
    assert.equal((await execute(viewerId, 1, { number_of_exec: 0 })).status, 400)

    const grantResponse = await fetch(`${baseUrl}/manage/api/mails`, {
        method: "POST",
        headers: { authorization: "Bearer cn-management-test-token", "content-type": "application/json" },
        body: JSON.stringify({
            playerId,
            title: "CN 扭蛋测试补给",
            body: "验证邮件资源进入普通扭蛋和兑换流程.",
            sender: "Starpoint",
            rewards: { freeVmoney: 37500 },
        }),
    })
    assert.equal(grantResponse.status, 200)
    const grantMail = await grantResponse.json()
    const claimResponse = await fetch(`${baseUrl}/api/index.php/mail/receive`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({ viewer_id: viewerId, mail_id: grantMail.id }),
    })
    assert.equal(claimResponse.status, 200)
    assert.equal((await unpackResponse(claimResponse)).data.user_info.free_vmoney, 38850)

    for (let drawNumber = 1; drawNumber <= 25; drawNumber++) {
        const multiResponse = await execute(viewerId, 1, {
            api_count: drawNumber + 1,
            type: 2,
        })
        assert.equal(multiResponse.status, 200)
        const multiData = (await unpackResponse(multiResponse)).data
        assert.equal(multiData.draw.length, 10)
        assert.equal(multiData.character_list.length > 0, true)
        assert.equal(multiData.gacha_info_list[0].gacha_exchange_point, 1 + drawNumber * 10)
    }

    const exchange = (characterId) => requestGacha("exchange_character", {
        character_id: characterId,
        api_count: 27,
        gacha_id: 1,
        viewer_id: viewerId,
    })
    assert.equal((await exchange(999999)).status, 400)
    const exchangeResponse = await exchange(111001)
    assert.equal(exchangeResponse.status, 200)
    const exchangeData = (await unpackResponse(exchangeResponse)).data
    assert.equal(exchangeData.character_list[0].character_id, 111001)
    assert.equal(exchangeData.gacha_info_list[0].gacha_exchange_point, 1)

    const exchangedSave = (await unpackResponse(await load(viewerId))).data
    assert.equal(exchangedSave.user_info.free_vmoney, 1350)
    assert.equal(exchangedSave.gacha_info_list.find((gachaInfo) => gachaInfo.gacha_id === 1).gacha_exchange_point, 1)
    assert.ok(exchangedSave.user_character_list["111001"])
}
// //// /验证 CN 普通扭蛋扣费, 资源发放, 角色兑换和持久化 ////

// //// 验证体力恢复扣费, 溢出限制和持久化 [@x380kkm 2026-08-04] ////
async function exerciseCnStaminaRecovery(baseUrl, viewerId, playerId, databasePath, load) {
    const recover = (targetViewerId) => fetch(`${baseUrl}/api/index.php/shop/recover_stamina`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({ api_count: 1, viewer_id: targetViewerId }),
    })

    assert.equal((await recover(999999999)).status, 400)

    const withDatabase = (operation) => {
        const database = new Database(databasePath)
        try {
            return operation(database)
        } finally {
            database.close()
        }
    }
    const setPlayerResources = (stamina, freeVmoney, vmoney) => withDatabase((database) => database.prepare(
        "UPDATE players SET stamina = ?, free_vmoney = ?, vmoney = ? WHERE id = ?",
    ).run(stamina, freeVmoney, vmoney, playerId))

    setPlayerResources(0, 30, 40)

    const response = await recover(viewerId)
    assert.equal(response.status, 200)
    assert.match(response.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const payload = await unpackResponse(response)
    assert.equal(payload.data_headers.result_code, 1)
    assert.equal(payload.data.user_info.stamina, 100)
    assert.equal(payload.data.user_info.free_vmoney, 0)
    assert.equal(payload.data.user_info.vmoney, 20)
    assert.equal(Number.isInteger(payload.data.user_info.stamina_heal_time), true)

    const recoveredPlayer = withDatabase((database) => database.prepare(
        "SELECT stamina, free_vmoney, vmoney, stamina_heal_time FROM players WHERE id = ?",
    ).get(playerId))
    assert.equal(recoveredPlayer.stamina, 100)
    assert.equal(recoveredPlayer.free_vmoney, 0)
    assert.equal(recoveredPlayer.vmoney, 20)
    assert.equal(
        Math.floor(new Date(recoveredPlayer.stamina_heal_time).getTime() / 1000),
        payload.data.user_info.stamina_heal_time,
    )

    const restored = (await unpackResponse(await load(viewerId))).data.user_info
    assert.equal(restored.stamina, 100)
    assert.equal(restored.free_vmoney, 0)
    assert.equal(restored.vmoney, 20)

    setPlayerResources(999, 50, 0)
    const overflowResponse = await recover(viewerId)
    assert.equal(overflowResponse.status, 200)
    assert.match(overflowResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const overflowPayload = await unpackResponse(overflowResponse)
    assert.equal(overflowPayload.data_headers.result_code, 2102)
    assert.deepEqual(overflowPayload.data, {})
    assert.deepEqual(
        withDatabase((database) => database.prepare(
            "SELECT stamina, free_vmoney, vmoney FROM players WHERE id = ?",
        ).get(playerId)),
        { stamina: 999, free_vmoney: 50, vmoney: 0 },
    )

    setPlayerResources(0, 0, 0)
    assert.equal((await recover(viewerId)).status, 400)
    assert.deepEqual(
        withDatabase((database) => database.prepare(
            "SELECT stamina, free_vmoney, vmoney FROM players WHERE id = ?",
        ).get(playerId)),
        { stamina: 0, free_vmoney: 0, vmoney: 0 },
    )

}

async function verifyCnStaminaRecovery(baseUrl, viewerId, playerId, databasePath, load) {
    const database = new Database(databasePath)
    let originalPlayer
    try {
        originalPlayer = database.prepare(
            "SELECT stamina, free_vmoney, vmoney, stamina_heal_time FROM players WHERE id = ?",
        ).get(playerId)
    } finally {
        database.close()
    }
    assert.ok(originalPlayer)

    try {
        await exerciseCnStaminaRecovery(baseUrl, viewerId, playerId, databasePath, load)
    } finally {
        const restorationDatabase = new Database(databasePath)
        try {
            restorationDatabase.prepare(
                `UPDATE players
                SET stamina = ?, free_vmoney = ?, vmoney = ?, stamina_heal_time = ?
                WHERE id = ?`,
            ).run(
                originalPlayer.stamina,
                originalPlayer.free_vmoney,
                originalPlayer.vmoney,
                originalPlayer.stamina_heal_time,
                playerId,
            )
        } finally {
            restorationDatabase.close()
        }
    }
}
// //// /验证体力恢复扣费, 溢出限制和持久化 ////

// //// 验证 CN 剧情结算入口, 奖励幂等性和持久化 [@x380kkm 2026-08-04] ////
async function verifyCnStoryQuestSettlement(
    baseUrl,
    viewerId,
    playerId,
    databasePath,
    load,
    restartServer,
    endpoint,
) {
    const finish = (endpoint, body) => fetch(`${baseUrl}/api/index.php/story_quest/${endpoint}`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody(body),
    })
    const request = {
        party_id: 1,
        quest_id: 1001001,
        viewer_id: viewerId,
        category: 1,
    }
    const withDatabase = (operation) => {
        const database = new Database(databasePath)
        try {
            return operation(database)
        } finally {
            database.close()
        }
    }

    assert.equal((await finish(endpoint, { ...request, viewer_id: 999999999 })).status, 400)
    const { quest_id: _questId, ...missingQuestId } = request
    assert.equal((await finish(endpoint, missingQuestId)).status, 400)
    assert.equal((await finish(endpoint, { ...request, api_count: -1 })).status, 400)
    assert.equal((await finish(endpoint, { ...request, retry_count: -1 })).status, 400)

    const initialPlayer = withDatabase((database) => database.prepare(
        "SELECT free_vmoney, free_mana FROM players WHERE id = ?",
    ).get(playerId))
    const readProgress = () => withDatabase((database) => database.prepare(
        "SELECT finished FROM players_quest_progress WHERE player_id = ? AND section = ? AND quest_id = ?",
    ).get(playerId, request.category, request.quest_id))
    assert.ok(initialPlayer)
    assert.equal(readProgress(), undefined)

    withDatabase((database) => database.exec(`
        CREATE TRIGGER reject_story_progress_insert
        BEFORE INSERT ON players_quest_progress
        WHEN NEW.player_id = ${Number(playerId)}
          AND NEW.section = ${request.category}
          AND NEW.quest_id = ${request.quest_id}
        BEGIN
            SELECT RAISE(ABORT, 'forced story progress failure');
        END
    `))
    try {
        assert.equal((await finish(endpoint, request)).status, 500)
    } finally {
        withDatabase((database) => database.exec("DROP TRIGGER IF EXISTS reject_story_progress_insert"))
    }
    assert.deepEqual(
        withDatabase((database) => database.prepare(
            "SELECT free_vmoney, free_mana FROM players WHERE id = ?",
        ).get(playerId)),
        initialPlayer,
    )
    assert.equal(readProgress(), undefined)

    const response = await finish(endpoint, request)
    assert.equal(response.status, 200)
    assert.match(response.headers.get("content-type") ?? "", /^application\/x-msgpack/)
    const payload = await unpackResponse(response)
    assert.equal(payload.data_headers.result_code, 1)
    assert.equal(payload.data.user_info.free_vmoney, initialPlayer.free_vmoney + 15)
    assert.equal(payload.data.user_info.free_mana, initialPlayer.free_mana)

    const persistedPlayer = withDatabase((database) => database.prepare(
        "SELECT free_vmoney, free_mana FROM players WHERE id = ?",
    ).get(playerId))
    assert.deepEqual(persistedPlayer, {
        free_vmoney: initialPlayer.free_vmoney + 15,
        free_mana: initialPlayer.free_mana,
    })
    assert.deepEqual(readProgress(), { finished: 1 })

    const repeatedEndpoint = endpoint === "finish" ? "finish_with_skip" : "finish"
    const repeated = await finish(repeatedEndpoint, { ...request, retry_count: 1 })
    assert.equal(repeated.status, 200)
    assert.deepEqual((await unpackResponse(repeated)).data, [])
    const repeatedPlayer = withDatabase((database) => database.prepare(
        "SELECT free_vmoney, free_mana FROM players WHERE id = ?",
    ).get(playerId))
    assert.deepEqual(repeatedPlayer, persistedPlayer)

    const apiCountResponse = await finish(endpoint, { ...request, api_count: 1 })
    assert.equal(apiCountResponse.status, 200)
    assert.deepEqual((await unpackResponse(apiCountResponse)).data, [])

    await restartServer()
    const loaded = (await unpackResponse(await load(viewerId))).data
    const loadedProgress = loaded.quest_progress[request.category]
        .find((progress) => progress.quest_id === request.quest_id)
    assert.equal(loadedProgress.finished, true)
    assert.equal(loaded.user_info.free_vmoney, initialPlayer.free_vmoney + 15)
    assert.equal(loaded.user_info.free_mana, initialPlayer.free_mana)

    assert.equal((await finish(endpoint, { ...request, quest_id: 1001002 })).status, 400)
}
// //// /验证 CN 剧情结算入口, 奖励幂等性和持久化 ////

// //// 验证 CN 邮件查询, 单封领取和全部领取 [@x380kkm 2026-07-24] ////
async function verifyCnMail(baseUrl, viewerId, playerId, databasePath) {
    const createMail = async (rewards) => {
        const response = await fetch(`${baseUrl}/manage/api/mails`, {
            method: "POST",
            headers: { authorization: "Bearer cn-management-test-token", "content-type": "application/json" },
            body: JSON.stringify({ playerId, title: "CN 测试补给", body: "协议测试邮件", sender: "Starpoint", rewards }),
        })
        assert.equal(response.status, 200)
        return response.json()
    }
    const requestMailIndex = async () => fetch(`${baseUrl}/api/index.php/mail/index`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({ viewer_id: viewerId, current_page: 1 }),
    })
    const requestReceive = async (mailId) => fetch(`${baseUrl}/api/index.php/mail/receive`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({ viewer_id: viewerId, mail_id: mailId }),
    })
    const requestReceiveAll = async (mailIds) => fetch(`${baseUrl}/api/index.php/mail/receive_all`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody({ viewer_id: viewerId, mail_ids: mailIds }),
    })

    const firstMail = await createMail({ itemList: { "100000": 4 }, freeVmoney: 100 })
    const indexResponse = await requestMailIndex()
    assert.equal(indexResponse.status, 200)
    const indexData = (await unpackResponse(indexResponse)).data
    assert.equal(indexData.total_count, 1)
    assert.equal(indexData.mail[0].id, firstMail.id)
    assert.equal(indexData.mail[0].subject, "CN 测试补给")
    assert.equal(indexData.mail[0].description, "协议测试邮件")
    assert.equal(indexData.mail[0].number, 4)
    assert.equal(indexData.mail[0].reason_id, 999998)
    assert.equal(indexData.mail[0].receive_time, "0000-00-00 00:00:00")
    assert.equal(indexData.mail[0].reward_limit_time, null)
    assert.equal(indexData.mail[0].reward_period_limited, false)
    assert.equal(indexData.mail[0].type, 1)
    assert.equal(indexData.mail[0].type_id, 100000)
    assert.match(indexData.mail[0].create_time, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)

    const receiveResponse = await requestReceive(firstMail.id)
    assert.equal(receiveResponse.status, 200)
    const receiveData = (await unpackResponse(receiveResponse)).data
    assert.equal(receiveData.item_list["100000"], 4)
    assert.equal(receiveData.user_info.free_vmoney, 1450)
    assert.equal(receiveData.auto_sale_expired_mail, false)
    assert.equal(receiveData.dispose_expired_mail, false)
    assert.equal(receiveData.total_count, 0)

    const secondMail = await createMail({ freeMana: 20 })
    const thirdMail = await createMail({ itemList: { "100001": 3 }, vmoney: 5 })
    const allIndexData = (await unpackResponse(await requestMailIndex())).data
    assert.deepEqual(allIndexData.mail.map((mail) => mail.id), [thirdMail.id, secondMail.id])
    assert.equal(allIndexData.mail[0].type, 1)
    assert.equal(allIndexData.mail[0].type_id, 100001)
    assert.equal(allIndexData.mail[0].number, 3)
    assert.equal(allIndexData.mail[1].type, 8)
    assert.equal(allIndexData.mail[1].type_id, null)
    assert.equal(allIndexData.mail[1].number, 20)
    const database = new Database(databasePath)
    database.prepare("UPDATE players SET exp_pool = ? WHERE id = ?").run(150000, playerId)
    database.close()
    const allResponse = await requestReceiveAll(allIndexData.mail.map((mail) => mail.id))
    assert.equal(allResponse.status, 200)
    const allData = (await unpackResponse(allResponse)).data
    assert.deepEqual(allData.mail_ids, [thirdMail.id, secondMail.id])
    assert.equal(allData.item_list["100001"], 3)
    assert.equal(allData.user_info.free_mana, 1020)
    assert.equal(allData.user_info.vmoney, 5)
    assert.equal(allData.user_info.exp_pool, 150000)
    for (const field of [
        "already_mail_count",
        "auto_sale_expired_mail_count",
        "deleted_mail_count",
        "dispose_expired_mail_count",
        "max_overed_mail_count",
        "outdated_mail_count",
        "total_count",
    ]) assert.equal(Number.isInteger(allData[field]), true, field)
    assert.equal(allData.auto_sale_expired_mail_count, 0)
    assert.equal(allData.dispose_expired_mail_count, 0)
    assert.equal(allData.total_count, 0)
}
// //// /验证 CN 邮件查询, 单封领取和全部领取 ////

// //// 验证 CN 任务计数持久化和完成回执 [@x380kkm 2026-07-24] ////
async function verifyCnMission(baseUrl, viewerId, databasePath) {
    const request = (endpoint, body) => fetch(`${baseUrl}/api/index.php/mission/${endpoint}`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody(body),
    })

    const initialResponse = await request("get_mission_progress", {
        api_count: 1,
        viewer_id: viewerId,
        category_list: [{ category: 5 }],
    })
    assert.equal(initialResponse.status, 200)
    const initialData = (await unpackResponse(initialResponse)).data
    assert.deepEqual(initialData.mission_progress_list, [{
        mission_category: 5,
        mission_id: 49000,
        progress_value: 0,
        stage: 1,
    }])

    const firstUpdateResponse = await request("update_mission_progress", {
        api_count: 2,
        viewer_id: viewerId,
        mission_param_list: [{
            progress_value: 1,
            mission_pattern: "home_tap_town_character_count",
        }],
    })
    assert.equal(firstUpdateResponse.status, 200)
    const firstUpdateData = (await unpackResponse(firstUpdateResponse)).data
    assert.deepEqual(firstUpdateData.mission_info, [{
        mission_category_id: 5,
        mission_id: 49000,
        mission_reward_id: 49000001,
    }])
    assert.deepEqual(firstUpdateData.degree_list, [{ viewer_id: viewerId, degree_id: 49000 }])

    const secondUpdateResponse = await request("update_mission_progress", {
        api_count: 3,
        viewer_id: viewerId,
        mission_param_list: [{
            progress_value: 1,
            mission_pattern: "home_tap_town_character_count",
        }],
    })
    assert.equal(secondUpdateResponse.status, 200)
    const secondUpdateData = (await unpackResponse(secondUpdateResponse)).data
    assert.deepEqual(secondUpdateData.mission_info, [])
    assert.deepEqual(secondUpdateData.degree_list, [])

    const finalResponse = await request("get_mission_progress", {
        api_count: 4,
        viewer_id: viewerId,
        category_list: [{ category: 5 }],
    })
    const finalData = (await unpackResponse(finalResponse)).data
    assert.equal(finalData.mission_progress_list[0].progress_value, 2)

    const database = new Database(databasePath)
    const counter = database.prepare(
        "SELECT pattern, value FROM players_mission_counters WHERE pattern = ?",
    ).get("home_tap_town_character_count")
    database.close()
    assert.deepEqual(counter, { pattern: "home_tap_town_character_count", value: 2 })
    assert.equal((await request("update_mission_progress", {
        api_count: 5,
        viewer_id: viewerId,
        mission_param_list: {},
    })).status, 400)
}
// //// /验证 CN 任务计数持久化和完成回执 ////

// //// 验证 CN 养成路由共享入口 [@x380kkm 2026-07-24] ////
async function verifyCnSharedRoutes(baseUrl, viewerId) {
    const request = (path, body) => fetch(`${baseUrl}/api/index.php/${path}`, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: encodeCnRequestBody(body),
    })

    const optionResponse = await request("option/update", {
        api_count: 1,
        viewer_id: viewerId,
        option_params: { stamina: true },
    })
    assert.equal(optionResponse.status, 200)
    assert.deepEqual((await unpackResponse(optionResponse)).data.user_option, { stamina: true })

    const paymentResponse = await request("payment/item_list", {
        api_count: 2,
        viewer_id: viewerId,
    })
    assert.equal(paymentResponse.status, 200)
    assert.deepEqual((await unpackResponse(paymentResponse)).data.payment_item_list, [])

    const characterResponse = await request("character/open_mana_board", {
        api_count: 3,
        viewer_id: viewerId,
    })
    assert.equal(characterResponse.status, 400)

    const invalidOverLimitResponse = await request("character/over_limit", {
        api_count: 4,
        viewer_id: viewerId,
        character_id: 1,
        over_limit_count: -1,
        use_stack: true,
    })
    assert.equal(invalidOverLimitResponse.status, 400)

    const expodResponse = await request("expod/inject_exp", {
        api_count: 5,
        viewer_id: viewerId,
        character_id: 999999,
        exp: 0,
    })
    assert.equal(expodResponse.status, 400)
}
// //// /验证 CN 养成路由共享入口 ////

// //// 验证 CN 会话切换激活槽后继续载入和游玩 [@x380kkm 2026-07-27] ////
async function verifyCnActiveSaveSlot(baseUrl, viewerId, sourcePlayer, databasePath, load) {
    const managementHeaders = {
        authorization: "Bearer cn-management-test-token",
        "content-type": "application/json",
    }
    const exported = await fetch(`${baseUrl}/manage/api/saves/${sourcePlayer.id}`, {
        headers: managementHeaders,
    })
    assert.equal(exported.status, 200)
    const portableSave = await exported.json()

    const imported = await fetch(`${baseUrl}/manage/api/saves/${sourcePlayer.id}/slots`, {
        method: "POST",
        headers: managementHeaders,
        body: JSON.stringify(portableSave),
    })
    assert.equal(imported.status, 201)
    const importedPlayerId = Number((await imported.json()).playerId)
    assert.ok(Number.isInteger(importedPlayerId))
    assert.notEqual(importedPlayerId, sourcePlayer.id)

    const activated = await fetch(`${baseUrl}/manage/api/saves/${importedPlayerId}/activate`, {
        method: "POST",
        headers: managementHeaders,
    })
    assert.equal(activated.status, 200)

    const database = new Database(databasePath)
    const oldSlot = database.prepare("SELECT name FROM players WHERE id = ?").get(sourcePlayer.id)
    const activeSlotName = `${oldSlot.name} Slot 2`
    database.prepare("UPDATE players SET name = ? WHERE id = ?").run(activeSlotName, importedPlayerId)
    database.close()

    const activeLoad = await load(viewerId)
    assert.equal(activeLoad.status, 200)
    assert.equal((await unpackResponse(activeLoad)).data.user_info.name, activeSlotName)
    return { id: importedPlayerId, party_slot: sourcePlayer.party_slot }
}
// //// /验证 CN 会话切换激活槽后继续载入和游玩 ////

// //// 验证 CN HTTP, 战斗和多人协议保持账号隔离 [@x380kkm 2026-07-21] ////
async function run() {
    assert.ok(fs.existsSync(startupScript), "Run npm run build before the CN server test.")
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-test-"))
    const databasePath = path.join(root, "wdfp_data.db")
    const cdnDirectory = path.join(root, ".cdn")
    const cnCdnDirectory = path.join(cdnDirectory, "cn")
    fs.mkdirSync(path.join(cnCdnDirectory, "entities"), { recursive: true })
    fs.mkdirSync(path.join(cnCdnDirectory, "archive-common-full"), { recursive: true })
    fs.mkdirSync(path.join(cnCdnDirectory, "archive-common-diff"), { recursive: true })
    fs.writeFileSync(path.join(cnCdnDirectory, "entities", "10939-android_medium.csv"), "entity")
    fs.writeFileSync(path.join(cnCdnDirectory, "archive-common-full", "pinball-1.4.0-1-test.zip"), "full")
    fs.writeFileSync(path.join(cnCdnDirectory, "archive-common-diff", "pinball-1.4.0-1.4.1-1-test.zip"), "diff")
    const port = await reservePort()
    let sessionPort = await reservePort()
    while (sessionPort === port) sessionPort = await reservePort()
    const baseUrl = `http://127.0.0.1:${port}`
    let output = ""
    let errors = ""
    const serverEnvironment = {
        ...process.env,
        LISTEN_HOST: "127.0.0.1",
        LISTEN_PORT: String(port),
        SESSION_HOST: "127.0.0.1",
        SESSION_PUBLIC_HOST: "127.0.0.1",
        SESSION_PORT: String(sessionPort),
        CN_API_HOST: `127.0.0.1:${port}`,
        CN_API_SCHEME: "http",
        CDN_DIR: cdnDirectory,
        MANAGEMENT_STATE_FILE: path.join(root, ".management", "state.json"),
        DATABASE_PATH: databasePath,
        MANAGEMENT_ADMIN_TOKEN: "cn-management-test-token",
    }
    const startServer = () => {
        output = ""
        errors = ""
        const server = spawn(process.execPath, [startupScript], {
            cwd: root,
            env: serverEnvironment,
            stdio: ["ignore", "pipe", "pipe"],
        })
        server.stdout.on("data", (chunk) => { output += chunk.toString() })
        server.stderr.on("data", (chunk) => { errors += chunk.toString() })
        return server
    }
    let child = startServer()
    const restartServer = async () => {
        await stopChild(child)
        child = startServer()
        await waitUntilReady(child, baseUrl, () => `${output}\n${errors}`)
    }

    try {
        await waitUntilReady(child, baseUrl, () => `${output}\n${errors}`)
        await verifyCnVersionDiscovery(baseUrl)
        await verifyCnEntityListFile(baseUrl)
        await verifyLeitingAntiAddictionLogin(baseUrl)
        assert.equal((await fetch(`${baseUrl}/api/player`)).status, 404)
        const signup = async (deviceId, udid) => {
            const response = await fetch(`${baseUrl}/api/index.php/tool/signup`, {
                method: "POST",
                headers: { "content-type": "application/x-www-form-urlencoded", udid },
                body: encodeCnRequestBody({
                    device_id: deviceId,
                    channelNo: "leiting",
                    media: "none",
                    androidId: "",
                    oaid: "",
                    mac: "",
                    terminInfo: "",
                    osVer: "",
                    storage_directory_path: "/data/user/0/com.leiting.wf",
                }),
            })
            if (response.status !== 200) throw new Error(`Signup failed: ${response.status} ${await response.text()}`)
            return unpackResponse(response)
        }

        const first = await signup(101, "device-a")
        const second = await signup(202, "device-b")
        const repeat = await signup(101, "device-a")
        assert.deepEqual(
            [first.data.newAccount, second.data.newAccount, repeat.data.newAccount],
            [1, 1, 0],
        )
        assert.equal(first.data.roleName, repeat.data.roleName)
        assert.notEqual(first.data.roleName, second.data.roleName)
        assert.equal(typeof first.data.serverId, "string")

        const firstViewer = Number(first.data_headers.viewer_id)
        const secondViewer = Number(second.data_headers.viewer_id)
        const repeatViewer = Number(repeat.data_headers.viewer_id)
        assert.equal(new Set([firstViewer, secondViewer, repeatViewer]).size, 3)

        const firstAccount = Number(first.data.roleName.replace("Player", ""))
        const secondAccount = Number(second.data.roleName.replace("Player", ""))
        const database = new Database(databasePath)
        database.prepare("UPDATE players SET name = ? WHERE account_id = ?").run("Alpha", firstAccount)
        database.prepare("UPDATE players SET name = ? WHERE account_id = ?").run("Beta", secondAccount)
        const selectSourcePlayer = database.prepare("SELECT id, party_slot FROM players WHERE account_id = ?")
        const sourcePlayers = [selectSourcePlayer.get(firstAccount), selectSourcePlayer.get(secondAccount)]
        database.close()
        assert.equal(sourcePlayers.every(Boolean), true)

        const load = (viewerId) => fetch(`${baseUrl}/api/index.php/load`, {
            method: "POST",
            headers: { "content-type": "application/x-www-form-urlencoded" },
            body: encodeCnRequestBody({
                device_id: 101,
                device_token: "",
                keychain: viewerId,
                graphics_device_name: "Android Emulator",
                platform_os_version: "Android 15",
                storage_directory_path: "/data/user/0/com.leiting.wf",
                viewer_id: viewerId,
            }),
        })
        const firstLoad = await load(repeatViewer)
        const secondLoad = await load(secondViewer)
        assert.equal(firstLoad.status, 200)
        assert.equal(secondLoad.status, 200)
        const firstLoadData = (await unpackResponse(firstLoad)).data
        const secondLoadData = (await unpackResponse(secondLoad)).data
        assert.equal(firstLoadData.user_info.name, "Alpha")
        assert.equal(secondLoadData.user_info.name, "Beta")
        for (const loadData of [firstLoadData, secondLoadData]) {
            const characters = Object.values(loadData.user_character_list ?? {})
            assert.ok(characters.length > 0)
            assert.equal(typeof loadData.cn_crash_url, "string")
            assert.equal(loadData.gacha_info_list[0].gacha_id, 1704)
            assert.ok(loadData.gacha_info_list.some((gachaInfo) => gachaInfo.gacha_id === 1))
            for (const character of characters) {
                assert.ok(Number.isFinite(character.join_time))
                assert.ok(Number.isFinite(character.update_time))
            }
        }
        assert.equal((await load(firstViewer)).status, 400)
        assert.equal((await load(999999999)).status, 400)

        for (const [deviceId, udid, endpoint] of [
            [303, "device-story-skip", "finish_with_skip"],
            [404, "device-story-finish", "finish"],
        ]) {
            const storySignup = await signup(deviceId, udid)
            const storyViewer = Number(storySignup.data_headers.viewer_id)
            const storyAccount = Number(storySignup.data.roleName.replace("Player", ""))
            const storyDatabase = new Database(databasePath)
            const storyPlayer = storyDatabase.prepare("SELECT id FROM players WHERE account_id = ?").get(storyAccount)
            storyDatabase.close()
            assert.ok(storyPlayer)
            await verifyCnStoryQuestSettlement(
                baseUrl,
                storyViewer,
                storyPlayer.id,
                databasePath,
                load,
                restartServer,
                endpoint,
            )
        }

        await verifyCnSingleBattle(baseUrl, repeatViewer)

        const updateTutorialStep = (
            viewerId,
            tutorial,
            endpoint = "/api/index.php/tutorial/update_step",
        ) => fetch(`${baseUrl}${endpoint}`, {
            method: "POST",
            headers: { "content-type": "application/x-www-form-urlencoded" },
            body: encodeCnRequestBody({
                retry_count: 1,
                api_count: 1,
                viewer_id: viewerId,
                ...tutorial,
            }),
        })
        const tutorialStepResponse = await updateTutorialStep(repeatViewer, { skip: true, step: 0 })
        assert.equal(tutorialStepResponse.status, 200)
        assert.match(tutorialStepResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
        const tutorialStepData = (await unpackResponse(tutorialStepResponse)).data
        assert.equal(tutorialStepData.step, 12)
        assert.equal(tutorialStepData.mail_arrived, true)
        assert.ok(Number.isFinite(tutorialStepData.start_time))
        assert.equal((await updateTutorialStep(999999999, { skip: true, step: 0 })).status, 400)

        const interruptedTutorialDatabase = new Database(databasePath)
        interruptedTutorialDatabase.prepare(
            "UPDATE players SET tutorial_step = 4, tutorial_skip_flag = 1, free_vmoney = 150 WHERE account_id = ?",
        ).run(firstAccount)
        interruptedTutorialDatabase.close()
        const interruptedTutorialData = (await unpackResponse(await load(repeatViewer))).data
        assert.equal(interruptedTutorialData.user_tutorial.tutorial_step, 3)
        assert.equal(interruptedTutorialData.tutorial_gacha, null)

        const shortenedTutorialGachaResponse = await updateTutorialStep(repeatViewer, {
            skip: true,
            step: 3,
            gacha_id: 1704,
        })
        assert.equal(shortenedTutorialGachaResponse.status, 200)
        const shortenedTutorialGachaData = (await unpackResponse(shortenedTutorialGachaResponse)).data
        assert.equal(shortenedTutorialGachaData.step, 15)
        const shortenedTutorialCharacterId = shortenedTutorialGachaData.gacha.draw[0].character_id
        const restoredShortenedTutorialData = (await unpackResponse(await load(repeatViewer))).data
        assert.equal(restoredShortenedTutorialData.user_tutorial.tutorial_step, 4)
        assert.equal(restoredShortenedTutorialData.user_tutorial.skip_flag, true)
        assert.equal(restoredShortenedTutorialData.tutorial_gacha.character_id, shortenedTutorialCharacterId)

        const missingTutorialGachaResponse = await updateTutorialStep(secondViewer, {
            skip: false,
            step: 14,
            gacha_id: 9999,
        })
        assert.equal(missingTutorialGachaResponse.status, 400)
        const unchangedTutorialData = (await unpackResponse(await load(secondViewer))).data
        assert.equal(unchangedTutorialData.user_tutorial.tutorial_step, 0)
        assert.equal(unchangedTutorialData.tutorial_gacha, null)

        const tutorialGachaResponse = await updateTutorialStep(secondViewer, {
            skip: false,
            step: 14,
            gacha_id: 1704,
        })
        assert.equal(tutorialGachaResponse.status, 200)
        assert.match(tutorialGachaResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
        const tutorialGachaData = (await unpackResponse(tutorialGachaResponse)).data
        assert.equal(tutorialGachaData.step, 15)
        assert.equal(tutorialGachaData.user_info.free_vmoney, 0)
        assert.equal(tutorialGachaData.gacha.gacha_info_list[0].gacha_id, 1704)
        assert.equal(tutorialGachaData.gacha.draw.length, 1)
        const tutorialGachaCharacterId = tutorialGachaData.gacha.draw[0].character_id
        const restoredTutorialData = (await unpackResponse(await load(secondViewer))).data
        assert.equal(restoredTutorialData.user_tutorial.tutorial_step, 15)
        assert.equal(restoredTutorialData.tutorial_gacha.character_id, tutorialGachaCharacterId)

        const tutorialRewardResponse = await updateTutorialStep(secondViewer, { skip: false, step: 15 })
        assert.equal(tutorialRewardResponse.status, 200)
        assert.match(tutorialRewardResponse.headers.get("content-type") ?? "", /^application\/x-msgpack/)
        const tutorialRewardData = (await unpackResponse(tutorialRewardResponse)).data
        assert.equal(tutorialRewardData.step, 16)
        const tutorialCharacter = tutorialRewardData.character_list[0]
        assert.match(tutorialCharacter.create_time, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)
        assert.match(tutorialCharacter.update_time, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)
        assert.match(tutorialCharacter.join_time, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)

        const tutorialDatabase = new Database(databasePath)
        const tutorialPlayers = tutorialDatabase.prepare(
            "SELECT account_id, tutorial_step, tutorial_skip_flag, free_vmoney FROM players WHERE account_id IN (?, ?)",
        ).all(firstAccount, secondAccount)
        tutorialDatabase.close()
        const firstTutorialPlayer = tutorialPlayers.find((player) => player.account_id === firstAccount)
        const secondTutorialPlayer = tutorialPlayers.find((player) => player.account_id === secondAccount)
        assert.deepEqual(
            [firstTutorialPlayer.tutorial_step, firstTutorialPlayer.tutorial_skip_flag],
            [4, 1],
        )
        assert.equal(firstTutorialPlayer.free_vmoney, 0)
        assert.deepEqual(
            [secondTutorialPlayer.tutorial_step, secondTutorialPlayer.tutorial_skip_flag, secondTutorialPlayer.free_vmoney],
            [16, 0, 1500],
        )

        sourcePlayers[1] = await verifyCnActiveSaveSlot(baseUrl, secondViewer, sourcePlayers[1], databasePath, load)
        await verifyCnGacha(baseUrl, secondViewer, sourcePlayers[1].id, databasePath, load)
        await verifyCnMail(baseUrl, secondViewer, sourcePlayers[1].id, databasePath)
        await verifyCnMission(baseUrl, secondViewer, databasePath)
        await verifyCnSharedRoutes(baseUrl, secondViewer)
        await verifyCnStaminaRecovery(baseUrl, secondViewer, sourcePlayers[1].id, databasePath, load)

        await verifyCnMultiplayer(baseUrl, repeatViewer, secondViewer, sourcePlayers, sessionPort)
        await verifyCnHumanOnlyLobbyStart(baseUrl, repeatViewer, secondViewer, sessionPort)

        const queryUnfinishedOrder = (viewerId) => fetch(`${baseUrl}/api/index.php/channels/channel_leiting_pay/query_unfinish_order`, {
            method: "POST",
            headers: { "content-type": "application/x-www-form-urlencoded" },
            body: encodeCnRequestBody({ viewer_id: viewerId }),
        })
        const unfinishedOrderResponse = await queryUnfinishedOrder(repeatViewer)
        assert.equal(unfinishedOrderResponse.status, 200)
        assert.equal((await unpackResponse(unfinishedOrderResponse)).data.order_id, "")
        assert.equal((await queryUnfinishedOrder(999999999)).status, 400)

        const assetResponse = await fetch(`${baseUrl}/api/index.php/asset/version_info`, {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: "{}",
        })
        assert.equal(assetResponse.status, 200)
        const assetInfo = (await assetResponse.json()).data
        assert.match(assetInfo.files_list, /\/entities\/10939-android_medium\.csv$/)
        assert.equal(assetInfo.total_size, 8)

        const pathResponse = await fetch(`${baseUrl}/api/index.php/asset/get_path`, {
            method: "POST",
            headers: { "content-type": "application/json", res_ver: "1.4.0" },
            body: "{}",
        })
        assert.equal(pathResponse.status, 200)
        const pathInfo = (await pathResponse.json()).data
        assert.equal(pathInfo.full.version, "1.4.0")
        assert.equal(pathInfo.info.target_asset_version, "1.4.1")
        assert.equal(pathInfo.diff.length, 1)
        console.log("CN server integration test passed.")
    } finally {
        await stopChild(child)
        fs.rmSync(root, { recursive: true, force: true })
    }
}
// //// /验证 CN HTTP, 战斗和多人协议保持账号隔离 ////

run().catch((error) => {
    console.error(error)
    process.exitCode = 1
})
