// audience: internal
// # cn-item-use-test
// 独立 SQLite 数据库和真实路由覆盖自选素材, 道具消耗, 邮件余量及请求回执.

import assert from "node:assert/strict"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { pack, unpack } from "msgpackr"

const require = createRequire(import.meta.url)
const root = mkdtempSync(join(tmpdir(), "starpoint-item-use-"))
const previousDatabase = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(root, "wdfp_data.db")
let database
let server

// //// 调用实际道具接口并核对全部素材选项 [@x380kkm 2026-09-28] ////
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const { getDefaultPlayerData } = require("../../src/data/utils.ts")
    const { insertAccount, insertPlayerSync, insertSessionWithToken, givePlayerItemSync, getPlayerItemsSync, updatePlayerSync } = require("../../src/data/wdfpData.ts")
    const { SessionType } = require("../../src/data/types.ts")
    const { setServerTime } = require("../../src/utils.ts")
    const { default: routes } = require("../../src/routes/api/item.ts")
    const catalog = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-item-selection.json", import.meta.url), "utf8"))
    const capacities = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-shop-capacities.json", import.meta.url), "utf8"))
    setServerTime(new Date("2025-01-02T12:00:00Z"))
    const account = await insertAccount({ appId: "item-use-test", idpAlias: "item-use-test", idpCode: "device", idpId: "item-use-test", status: "active" })
    const playerId = insertPlayerSync(account.id, getDefaultPlayerData())
    const viewerId = 900000 + account.id
    await insertSessionWithToken({ token: String(viewerId), accountId: account.id, type: SessionType.VIEWER, expires: new Date() })
    database.prepare("DELETE FROM players_items WHERE player_id=?").run(playerId)
    server = require("fastify")()
    server.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    await server.register(routes)

    // //// 发送具有客户端请求计数的道具使用请求 [@x380kkm 2026-09-28] ////
    async function use(items, apiCount) {
        const response = await server.inject({ method: "POST", url: "/use_item", payload: { viewer_id: viewerId, api_count: apiCount, items } })
        return { status: response.statusCode, body: response.statusCode === 200 ? unpack(response.rawPayload) : response.json() }
    }

    const items = []
    const expected = {}
    for (const [id, definition] of Object.entries(catalog)) {
        givePlayerItemSync(playerId, id, definition.choices.length)
        expected[id] = 0
        definition.choices.forEach((choice, index) => {
            items.push({ id: Number(id), number: 1, selectIndex: index + 1 })
            expected[choice.item_id] = (expected[choice.item_id] ?? 0) + choice.amount
        })
    }
    const used = await use(items, 1)
    assert.equal(used.status, 200)
    assert.deepEqual(used.body.data.item_list, expected)
    assert.deepEqual(getPlayerItemsSync(playerId), expected)
    assert.deepEqual(used.body.data.over_max, [])
    assert.deepEqual((await use(items.toReversed(), 1)).body, used.body)
    assert.deepEqual(getPlayerItemsSync(playerId), expected)

    givePlayerItemSync(playerId, 999100, 3)
    const before = getPlayerItemsSync(playerId)
    const differentChoices = [{ id: 999100, number: 1, selectIndex: 1 }, { id: 999100, number: 2, selectIndex: 2 }]
    for (const invalid of [0, 7, 1.5]) {
        assert.equal((await use([{ id: 999100, number: 1, selectIndex: invalid }], 2)).status, 400)
        assert.deepEqual(getPlayerItemsSync(playerId), before)
    }
    const different = await use(differentChoices, 2)
    assert.equal(different.body.data.item_list[999100], 0)
    assert.equal(different.body.data.item_list[2], before[2] + 300)
    assert.equal(different.body.data.item_list[6], before[6] + 600)
    assert.equal((await use([{ id: 999100, number: 3, selectIndex: 1 }], 2)).body.error, "item_request_conflict")

    givePlayerItemSync(playerId, 999100, 2)
    const held = getPlayerItemsSync(playerId)
    givePlayerItemSync(playerId, 2, capacities.items[2] - 10 - held[2])
    const overflowRequest = [{ id: 999100, number: 2, selectIndex: 1 }]
    const storageBefore = getPlayerItemsSync(playerId)
    database.exec("CREATE TRIGGER reject_item_receipt BEFORE INSERT ON player_item_use_receipts BEGIN SELECT RAISE(ABORT, 'receipt unavailable'); END")
    assert.equal((await use(overflowRequest, 3)).status, 500)
    assert.deepEqual(getPlayerItemsSync(playerId), storageBefore)
    assert.equal(database.prepare("SELECT COUNT(*) AS count FROM player_mails").get().count, 0)
    database.exec("DROP TRIGGER reject_item_receipt")
    const overflow = await use(overflowRequest, 3)
    assert.equal(overflow.body.data.item_list[999100], 0)
    assert.equal(overflow.body.data.item_list[2], capacities.items[2])
    assert.deepEqual(overflow.body.data.over_max, [{ process_type: 1, item: { item_id: 2, number: 590 } }])
    assert.equal(overflow.body.data.mail_arrived, true)
    assert.deepEqual((await use([{ id: 999100, number: 1, selectIndex: 1 }, { id: 999100, number: 1, selectIndex: 1 }], 3)).body, overflow.body)
    assert.equal(database.prepare("SELECT COUNT(*) AS count FROM player_mails").get().count, 1)
    assert.equal(JSON.parse(database.prepare("SELECT rewards_json FROM player_mails").get().rewards_json).itemList[2], 590)
    const effects = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-item-effects.json", import.meta.url), "utf8"))
    let apiCount = 10
    for (const id of [10000013, 10000015, 10000022, 10000024, 10000028, 10000041, 10000050, 10000052, 10000080]) {
        const effect = effects[id]
        const now = new Date((effect.end_time - 86400) * 1000)
        setServerTime(now)
        updatePlayerSync({ id: playerId, stamina: 950, staminaHealTime: now })
        givePlayerItemSync(playerId, id, 2)
        const request = [{ id, number: 1, selectIndex: 0 }]
        const used = await use(request, apiCount)
        assert.equal(used.body.data.user_info.stamina, 975, String(id))
        assert.equal(used.body.data.item_list[id], 1)
        const stored = getPlayerItemsSync(playerId)
        setServerTime(new Date((effect.end_time + 1) * 1000))
        const expired = await use(request, apiCount + 100)
        assert.equal(expired.body.data_headers.result_code, 2057, String(id))
        assert.deepEqual(getPlayerItemsSync(playerId), stored)
        assert.deepEqual((await use(request, apiCount)).body, used.body)
        apiCount += 1
    }
    console.log(`已核对 ${Object.keys(catalog).length} 类道具的 ${items.length} 个选项, 最后一箱归零, 同箱多选, 整批回滚, 溢出邮件和请求重放.`)
    console.log("已核对 9 类期间限定体力道具的有效期, 到期拒绝与成功请求重放.")
} finally {
    await server?.close()
    database?.close()
    if (previousDatabase === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabase
    rmSync(root, { recursive: true, force: true })
}
// //// /调用实际道具接口并核对全部素材选项 ////
