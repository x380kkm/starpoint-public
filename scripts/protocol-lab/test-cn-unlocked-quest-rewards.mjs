// audience: internal
// # cn-unlocked-quest-reward-test
// 独立数据库中的未通关记录保留首通奖励, S+ 奖励和最佳成绩分别更新.

import assert from "node:assert/strict"
import { mkdtempSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { pack, unpack } from "msgpackr"

const require = createRequire(import.meta.url)
const root = mkdtempSync(join(tmpdir(), "starpoint-unlocked-quest-"))
const previousDatabase = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(root, "wdfp_data.db")
let database
let server

// //// 通过真实结算路由保留未通关记录的首通资格 [@x380kkm 2026-09-28] ////
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const { getDefaultPlayerData } = require("../../src/data/utils.ts")
    const {
        insertAccount, insertPlayerSync, insertSessionWithToken, insertPlayerQuestProgressSync,
        getPlayerSingleQuestProgressSync, getPlayerSync,
    } = require("../../src/data/wdfpData.ts")
    const { SessionType } = require("../../src/data/types.ts")
    const { insertActiveQuest } = require("../../src/control/battleSettlementState.ts")
    const { registerBattleSettlementRoutes } = require("../../src/routes/api/singleBattleQuest.ts")
    const account = await insertAccount({
        appId: "unlock-reward", idpAlias: "unlock-reward", idpCode: "device", idpId: "unlock-reward", status: "active",
    })
    const playerId = insertPlayerSync(account.id, getDefaultPlayerData())
    const viewerId = 900000 + account.id
    await insertSessionWithToken({ token: String(viewerId), accountId: account.id, type: SessionType.VIEWER, expires: new Date() })
    const questId = 400006102
    insertPlayerQuestProgressSync(playerId, 18, { questId, finished: false })
    server = require("fastify")()
    server.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    await server.register(registerBattleSettlementRoutes)

    // //// 结算真实关卡并核对独立发奖和成绩字段 [@x380kkm 2026-09-28] ////
    for (const [playId, accomplished, elapsed, expected] of [
        ["unfinished", false, 1000, 0],
        ["first-clear", true, 9999999, 120],
        ["repeated-clear", true, 9999999, 0],
        ["first-s-plus", true, 1000, 120],
        ["repeated-s-plus", true, 2000, 0],
    ]) {
        insertActiveQuest(playerId, {
            questId, playId, category: 18, useBossBoostPoint: false, useBoostPoint: false, isAutoStartMode: false,
        })
        const before = getPlayerSync(playerId).freeVmoney
        const response = await server.inject({
            method: "POST", url: "/finish", payload: {
                viewer_id: viewerId, category: 18, quest_id: questId, play_id: playId,
                is_restored: false, continue_count: 0, elapsed_time_ms: elapsed, score: 10000,
                add_mana: 0, is_accomplished: accomplished, api_count: 1,
                statistics: { clear_phase: 1, party: {
                    characters: [{ id: 1 }, null, null], unison_characters: [null, null, null],
                    equipments: [null, null, null], ability_soul_ids: [null, null, null],
                } },
            },
        })
        assert.equal(response.statusCode, 200, response.body)
        const result = unpack(response.rawPayload)
        assert.equal(result.data_headers.result_code, 1, playId)
        assert.equal(getPlayerSync(playerId).freeVmoney - before, expected, playId)
        assert.equal(getPlayerSingleQuestProgressSync(playerId, 18, questId).finished, accomplished, playId)
        assert.equal(database.prepare(
            "SELECT COUNT(*) AS count FROM players_quest_progress WHERE player_id=? AND section=18 AND quest_id=?",
        ).get(playerId, questId).count, 1)
    }
    const progress = getPlayerSingleQuestProgressSync(playerId, 18, questId)
    assert.equal(progress.clearRank, 5)
    assert.equal(progress.bestElapsedTimeMs, 1000)
    console.log("未通关记录的首通, S+ 和最佳成绩结算通过.")
} finally {
    await server?.close()
    database?.close()
    if (previousDatabase === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabase
    rmSync(root, { recursive: true, force: true })
}
// //// /通过真实结算路由保留未通关记录的首通资格 ////

