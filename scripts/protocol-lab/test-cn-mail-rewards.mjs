// audience: internal
// # cn-mail-rewards-test
// 邮件在隔离数据库中覆盖部分到账, 原邮件余量, 重复领取, 写入回滚和重启续领.

import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { pack, unpack } from "msgpackr"

const require = createRequire(import.meta.url)
const resumed = process.argv[2] === "resume"
const root = resumed ? null : mkdtempSync(join(tmpdir(), "starpoint-mail-rewards-"))
const previousDatabase = process.env.DATABASE_PATH
const databasePath = resumed ? process.argv[3] : join(root, "players.sqlite3")
process.env.DATABASE_PATH = databasePath
let database
let server
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const {
        insertAccount, insertPlayerSync, insertSessionWithToken,
        getPlayerSync, getPlayerItemSync, getPlayerCharacterSync, getPlayerEquipmentSync,
        givePlayerItemSync, updatePlayerItemSync, updatePlayerSync, updatePlayerCharacterSync, updatePlayerEquipmentSync,
    } = require("../../src/data/wdfpData.ts")
    const { claimPlayerMailSync, claimAllPlayerMailsSync, createPlayerMailSync } = require("../../src/data/playerMails.ts")
    const { getDefaultPlayerData } = require("../../src/data/utils.ts")
    const { givePlayerCharacterSync } = require("../../src/lib/character.ts")
    const { givePlayerEquipmentSync } = require("../../src/lib/equipment.ts")
    const capacities = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-shop-capacities.json", import.meta.url), "utf8"))
    const rawMail = id => database.prepare("SELECT * FROM player_mails WHERE id=?").get(id)
    const mailRewards = id => JSON.parse(rawMail(id).rewards_json)

    if (resumed) {
        const player = Number(process.argv[4])
        const mailId = Number(process.argv[5])
        assert.equal(rawMail(mailId).received_at, null)
        assert.equal(mailRewards(mailId).itemList[14014], 3)
        const claimed = claimPlayerMailSync(player, mailId)
        assert.deepEqual(claimed.mailIds, [mailId])
        assert.equal(getPlayerItemSync(player, 1), capacities.items[1])
        assert.equal(getPlayerItemSync(player, 14014), capacities.items[14014])
        assert.equal(getPlayerItemSync(player, 40000), 2147483647)
        assert.equal(getPlayerEquipmentSync(player, 101001).stack, capacities.equipment_stack)
        assert.equal(getPlayerCharacterSync(player, 263002).stack, 99)
        const restored = getPlayerSync(player)
        assert.equal(restored.freeMana + restored.paidMana, capacities.mana)
        assert.equal(restored.freeVmoney + restored.vmoney, capacities.virtual_money)
        assert.equal(restored.expPool, capacities.exp_pool)
        assert.notEqual(rawMail(mailId).received_at, null)
        assert.deepEqual(claimPlayerMailSync(player, mailId).mailIds, [])
        console.log("重启后领取同封邮件剩余数量, 再次领取保持资产不变.")
    } else {
        const account = await insertAccount({ appId: "mail", idpAlias: "mail", idpCode: "device", idpId: "mail", status: "active" })
        const player = insertPlayerSync(account.id, getDefaultPlayerData())
        const viewerId = 841286
        const { SessionType } = require("../../src/data/types.ts")
        await insertSessionWithToken({ token: String(viewerId), accountId: account.id, type: SessionType.VIEWER, expires: new Date(Date.now() + 86400000) })
        const createMail = rewards => createPlayerMailSync({ playerId: player, title: "奖励", body: "活动奖励.", sender: "系统", rewards })
        for (const [id, count] of [[1, capacities.items[1] - 2], [2, capacities.items[2]], [14014, capacities.items[14014]], [40000, 2147483646]]) {
            givePlayerItemSync(player, id, count)
        }
        updatePlayerSync({ id: player, freeMana: capacities.mana - 6, paidMana: 3, freeVmoney: capacities.virtual_money - 5, vmoney: 2, expPool: capacities.exp_pool - 2 })
        givePlayerCharacterSync(player, 263002)
        updatePlayerCharacterSync(player, 263002, { stack: 98 })
        givePlayerEquipmentSync(player, 101001, 999)
        const mail = createMail({
            itemList: { 1: 5, 2: 1, 14014: 2, 40000: 3 }, equipmentList: { 101001: 3 },
            characterList: [263002, 263002], freeMana: 5, paidMana: 4, freeVmoney: 2, vmoney: 4, expPool: 5,
        })
        server = require("fastify")()
        server.addHook("onSend", (_request, reply, payload, done) => done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload))
        await server.register(require("../../src/routes/api/mail.ts").default)
        const request = async (url, payload) => {
            const response = await server.inject({ method: "POST", url, payload: { viewer_id: viewerId, ...payload } })
            assert.equal(response.statusCode, 200, response.body)
            const decoded = unpack(response.rawPayload)
            assert.equal(decoded.data_headers.result_code, 1)
            return decoded.data
        }

        // //// 混合奖励按各自空间到账并保存同封邮件余量 [@x380kkm 2026-09-28] ////
        const partial = await request("/receive_all", { mail_ids: [mail.id, mail.id] })
        assert.deepEqual(partial.mail_ids, [mail.id])
        assert.equal(partial.max_overed_mail_count, 0)
        assert.equal(partial.total_count, 1)
        assert.equal(partial.item_list[1], capacities.items[1])
        assert.equal(partial.item_list[40000], 2147483647)
        assert.equal(getPlayerCharacterSync(player, 263002).stack, 99)
        assert.equal(getPlayerEquipmentSync(player, 101001).stack, capacities.equipment_stack)
        assert.equal(partial.user_info.free_mana, capacities.mana - 3)
        assert.equal(partial.user_info.paid_mana, 3)
        assert.equal(partial.user_info.free_vmoney, capacities.virtual_money - 3)
        assert.equal(partial.user_info.vmoney, 3)
        assert.equal(partial.user_info.exp_pool, capacities.exp_pool)
        assert.deepEqual(mailRewards(mail.id), {
            itemList: { 1: 3, 2: 1, 14014: 3, 40000: 2 }, equipmentList: { 101001: 2 },
            characterList: [263002], freeMana: 2, paidMana: 4, freeVmoney: 0, vmoney: 3, expPool: 3,
        })
        const unchanged = rawMail(mail.id)
        const full = await request("/receive", { mail_id: mail.id })
        assert.equal(full.total_count, 1)
        assert.deepEqual(full.item_list, {})
        assert.deepEqual(rawMail(mail.id), unchanged)
        const other = createMail({ itemList: { 3: 1 } })
        const batch = await request("/receive_all", { mail_ids: [mail.id, other.id, other.id] })
        assert.deepEqual(batch.mail_ids, [other.id])
        assert.equal(batch.max_overed_mail_count, 0)
        assert.equal(getPlayerItemSync(player, 3), 1)

        // //// 邮件余量写入失败时回滚已经到账的资产 [@x380kkm 2026-09-28] ////
        givePlayerItemSync(player, 4, capacities.items[4] - 1)
        const failing = createMail({ itemList: { 4: 3 } })
        database.exec(`CREATE TRIGGER reject_mail_update BEFORE UPDATE ON player_mails WHEN NEW.id=${failing.id}
            BEGIN SELECT RAISE(ABORT, 'mail remainder write failed'); END`)
        assert.throws(() => claimPlayerMailSync(player, failing.id), /mail remainder write failed/)
        assert.equal(getPlayerItemSync(player, 4), capacities.items[4] - 1)
        assert.equal(mailRewards(failing.id).itemList[4], 3)
        database.exec("DROP TRIGGER reject_mail_update")
        assert.deepEqual(claimPlayerMailSync(player, failing.id).mailIds, [failing.id])
        assert.equal(mailRewards(failing.id).itemList[4], 2)
        updatePlayerItemSync(player, 4, capacities.items[4] - 2)
        claimPlayerMailSync(player, failing.id)

        // //// 无效奖励保留原邮件并回滚同批此前到账的内容 [@x380kkm 2026-09-28] ////
        const valid = createMail({ itemList: { 5: 1 } })
        const invalid = createMail({ itemList: { 99999999: 1 } })
        assert.throws(() => claimAllPlayerMailsSync(player, [valid.id, invalid.id]), /Missing item capacity/)
        assert.equal(getPlayerItemSync(player, 5), null)
        assert.equal(rawMail(valid.id).received_at, null)
        assert.equal(rawMail(invalid.id).received_at, null)

        // //// 腾出空间后通过新进程读取并领取持久化余量 [@x380kkm 2026-09-28] ////
        for (const [id, count] of [[1, capacities.items[1] - 3], [2, capacities.items[2] - 1], [14014, capacities.items[14014] - 4], [40000, 2147483645]]) {
            updatePlayerItemSync(player, id, count)
        }
        updatePlayerSync({ id: player, freeMana: capacities.mana - 10, paidMana: 4, freeVmoney: capacities.virtual_money - 6, vmoney: 3, expPool: capacities.exp_pool - 3 })
        updatePlayerCharacterSync(player, 263002, { stack: 98 })
        updatePlayerEquipmentSync(player, 101001, { stack: 997 })
        await server.close()
        server = undefined
        database.close()
        database = undefined
        const restarted = spawnSync(process.execPath, [fileURLToPath(import.meta.url), "resume", databasePath, String(player), String(mail.id)], { encoding: "utf8", windowsHide: true })
        assert.equal(restarted.status, 0, restarted.stdout + restarted.stderr)
        process.stdout.write(restarted.stdout)
        console.log("邮件覆盖部分到账, 满仓成功, 免费/付费共用容量, 角色装备堆叠, 重复素材余量, 批量去重和事务回滚.")
    }
} finally {
    await server?.close()
    database?.close()
    if (previousDatabase === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabase
    if (root !== null) rmSync(root, { recursive: true, force: true })
}
