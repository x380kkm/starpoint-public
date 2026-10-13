// audience: internal
// # cn-ranking-summary-test
// 独立存档通过两个实际注册前缀核对竞速目录, 成绩类别, 百分数和空成绩.

import assert from "node:assert/strict"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { pack, unpack } from "msgpackr"

const require = createRequire(import.meta.url)
const temporaryParent = resolve(tmpdir())
const temporaryRoot = mkdtempSync(join(temporaryParent, "starpoint-ranking-summary-"))
const previousDatabase = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(temporaryRoot, "wdfp_data.db")
let database
let server

// //// 调用真实路由核对全部竞速活动的摘要 [@x380kkm 2026-09-28] ////
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const data = require("../../src/data/wdfpData.ts")
    const { getDefaultPlayerData } = require("../../src/data/utils.ts")
    const { SessionType } = require("../../src/data/types.ts")
    const { getCharacterDataSync } = require("../../src/lib/assets.ts")
    const { default: routes } = require("../../src/routes/api/rankingEvent.ts")
    const catalog = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-ranking-rewards.json", import.meta.url), "utf8"))
    const account = await data.insertAccount({ appId: "ranking-test", idpAlias: "ranking-test", idpCode: "device", idpId: "ranking-test", status: "active" })
    const playerId = data.insertPlayerSync(account.id, getDefaultPlayerData())
    const viewerId = 920000 + account.id
    await data.insertSessionWithToken({ token: String(viewerId), accountId: account.id, type: SessionType.VIEWER, expires: new Date() })
    server = require("fastify")()
    server.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    const prefixes = ["/api/index.php/ranking_event", "/latest/api/index.php/ranking_event"]
    for (const prefix of prefixes) await server.register(routes, { prefix })
    async function summary(prefix, eventId) {
        const response = await server.inject({ method: "POST", url: `${prefix}/get_summary`, payload: {
            viewer_id: viewerId, ranking_event_id: Number(eventId), quest_kind: 1,
        } })
        assert.equal(response.statusCode, 200, response.body)
        return unpack(response.rawPayload).data
    }
    for (const [eventId, definition] of Object.entries(catalog)) {
        for (const prefix of prefixes) assert.equal((await summary(prefix, eventId)).best_record, null)
        for (const questId of definition.quest_ids) {
            data.insertPlayerQuestProgressSync(playerId, 14, { questId, finished: true, bestElapsedTimeMs: 1, highScore: 999 })
            data.insertPlayerQuestProgressSync(playerId, 11, { questId, finished: true, bestElapsedTimeMs: definition.reference_time_ms * 2, highScore: 37 })
        }
        for (const prefix of prefixes) {
            const result = await summary(prefix, eventId)
            assert.equal(result.best_record.elapsed_time_ms, definition.reference_time_ms * 2)
            assert.equal(result.best_record.score, 37)
            assert.equal(result.rank_percentage, 50)
            assert.notEqual(getCharacterDataSync(result.leader_character_id), null)
        }
    }
    database.prepare("UPDATE players_quest_progress SET best_elapsed_time_ms=1 WHERE player_id=? AND section=11").run(playerId)
    assert.equal((await summary(prefixes[0], 1)).rank_percentage, 0)
    database.prepare("DELETE FROM players_parties WHERE player_id=?").run(playerId)
    database.prepare("DELETE FROM players_characters WHERE player_id=?").run(playerId)
    const emptyRoster = await summary(prefixes[0], 1)
    assert.notEqual(getCharacterDataSync(emptyRoster.leader_character_id), null)
    assert.equal(emptyRoster.best_record.elapsed_time_ms, 1)
    console.log(`已核对 ${Object.keys(catalog).length} 个活动的双前缀路由, 空成绩, 类别隔离, 0-100 百分数和空角色存档.`)
} finally {
    await server?.close()
    database?.close()
    if (previousDatabase === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabase
    if (dirname(resolve(temporaryRoot)) !== temporaryParent) throw new Error("Temporary directory is outside its parent")
    rmSync(temporaryRoot, { recursive: true, force: true })
}
