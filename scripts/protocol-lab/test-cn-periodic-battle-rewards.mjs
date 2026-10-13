// audience: internal
// # cn-periodic-battle-rewards-test
// 隔离数据库覆盖共同决战的次数恢复, 实际物品入库, 时钟回退和跨端存档迁移.

import assert from "node:assert/strict"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"

const require = createRequire(import.meta.url)
const root = mkdtempSync(join(tmpdir(), "starpoint-periodic-rewards-"))
const previousDatabase = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(root, "players.sqlite3")
let database
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const {
        getDefaultPlayerData, getPortableSerializedData, deserializePlayerData,
        capturePortablePlayerSnapshotSync,
    } = require("../../src/data/utils.ts")
    const {
        insertAccount, insertPlayerSync, getPlayerPeriodicRewardPointsSync,
        getPlayerItemSync, importPlayerDataAsNewSlotSync,
    } = require("../../src/data/wdfpData.ts")
    const { getPortablePlayerSnapshotSync } = require("../../src/data/portablePlayerSnapshots.ts")
    const { refreshPlayerPeriodicRewardsSync, settlePlayerPeriodicRewardsSync } = require("../../src/data/periodicBattleRewards.ts")
    const { setServerTime } = require("../../src/utils.ts")
    const { normalizeAndSerializeCnPlayerDataSync } = require("../../src/data/cnPlayerData.ts")
    const catalog = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-periodic-battle-rewards.json", import.meta.url), "utf8"))
    const account = await insertAccount({ appId: "periodic", idpAlias: "periodic", idpCode: "device", idpId: "periodic", status: "active" })
    const makePlayer = () => insertPlayerSync(account.id, getDefaultPlayerData())
    const balance = (player, id) => getPlayerPeriodicRewardPointsSync(player).find(point => point.id === id)?.point
    const nextBoundary = at => (Math.floor((at + 3 * 3600) / 86400) + 1) * 86400 - 3 * 3600
    let paidQuests = 0
    let permanentQuests = 0

    // //// 使用每条共同决战配置检查奖励与每日次数 [@x380kkm 2026-09-28] ////
    for (const [key, quest] of Object.entries(catalog.quests)) {
        const [category, questId] = key.split(":").map(Number)
        const player = makePlayer()
        if (quest.point_id === null) {
            assert.deepEqual(settlePlayerPeriodicRewardsSync(player, category, questId, 1751000000), { drops: [] })
            assert.deepEqual(getPlayerPeriodicRewardPointsSync(player), [])
            permanentQuests++
            continue
        }
        const point = catalog.points[quest.point_id]
        const at = point.start_time + 1
        assert.deepEqual(refreshPlayerPeriodicRewardsSync(player, point.start_time - 1).filter(entry => entry.id === quest.point_id), [])
        refreshPlayerPeriodicRewardsSync(player, at)
        assert.equal(balance(player, quest.point_id), 2)
        const row = catalog.reward_groups[quest.group_id][0]
        const initialItem = getPlayerItemSync(player, row.reward.id) ?? 0
        const first = settlePlayerPeriodicRewardsSync(player, category, questId, at)
        assert.deepEqual(first.pointList, [{ id: quest.point_id, point: 1 }])
        assert.deepEqual(first.drops, [{ group_id: quest.group_id, index: row.position, number: row.reward.count }])
        assert.equal(first.rewardResult.items[row.reward.id], initialItem + row.reward.count)
        settlePlayerPeriodicRewardsSync(player, category, questId, at)
        assert.equal(getPlayerItemSync(player, row.reward.id), initialItem + row.reward.count * 2)
        const empty = settlePlayerPeriodicRewardsSync(player, category, questId, at)
        assert.deepEqual(empty.drops, [])
        assert.equal(empty.rewardResult, undefined)
        const boundary = nextBoundary(at)
        refreshPlayerPeriodicRewardsSync(player, boundary - 1)
        assert.equal(balance(player, quest.point_id), 0)
        refreshPlayerPeriodicRewardsSync(player, boundary)
        assert.equal(balance(player, quest.point_id), 2)
        refreshPlayerPeriodicRewardsSync(player, at)
        refreshPlayerPeriodicRewardsSync(player, boundary)
        assert.equal(balance(player, quest.point_id), 2)
        database.prepare("UPDATE players_periodic_reward_points SET point=98 WHERE player_id=? AND id=?").run(player, quest.point_id)
        refreshPlayerPeriodicRewardsSync(player, boundary + 86400)
        assert.equal(balance(player, quest.point_id), 99)
        assert.deepEqual(settlePlayerPeriodicRewardsSync(player, category, questId, point.end_time + 1), { drops: [] })
        assert.equal(balance(player, quest.point_id), 99)
        paidQuests++
    }

    // //// 保留旧档余额并跨可移植导入恢复同一个检查点 [@x380kkm 2026-09-28] ////
    const player = makePlayer()
    const point = catalog.points[1]
    const at = nextBoundary(point.start_time + 1)
    database.prepare("INSERT INTO players_periodic_reward_points (player_id,id,point) VALUES (?,1,22)").run(player)
    refreshPlayerPeriodicRewardsSync(player, at)
    assert.equal(balance(player, 1), 22)
    setServerTime(new Date(at * 1000))
    const loaded = normalizeAndSerializeCnPlayerDataSync(player, { viewerId: 42 })
    assert.equal(loaded.user_periodic_reward_point_list.find(entry => entry.id === 1).point, 22)
    const exported = getPortableSerializedData(player, { viewerId: 42, serializeRushEventData: true })
    assert.deepEqual(exported.periodic_reward_recovery_state[1], { last_recovery_time: at })
    exported.periodic_reward_recovery_state[999] = { last_recovery_time: at + 100 }
    const imported = importPlayerDataAsNewSlotSync(account.id, deserializePlayerData(0, exported))
    capturePortablePlayerSnapshotSync(imported.id, exported)
    refreshPlayerPeriodicRewardsSync(imported.id, at - 86400)
    refreshPlayerPeriodicRewardsSync(imported.id, at)
    assert.equal(balance(imported.id, 1), 22)
    refreshPlayerPeriodicRewardsSync(imported.id, at + 86400)
    assert.equal(balance(imported.id, 1), 24)
    const reexported = getPortableSerializedData(imported.id, { viewerId: 84, serializeRushEventData: true })
    assert.equal(reexported.periodic_reward_recovery_state[1].last_recovery_time, at + 86400)
    assert.deepEqual(reexported.periodic_reward_recovery_state[999], exported.periodic_reward_recovery_state[999])
    assert.equal(reexported.user_periodic_reward_point_list.find(entry => entry.id === 1).point, 24)

    // //// 发奖失败时恢复次数和检查点随库存事务回滚 [@x380kkm 2026-09-28] ////
    const failedPlayer = makePlayer()
    database.exec(`CREATE TRIGGER fail_periodic_item BEFORE INSERT ON players_items WHEN NEW.player_id=${failedPlayer}
        BEGIN SELECT RAISE(ABORT, 'periodic item insert failed'); END`)
    assert.throws(() => settlePlayerPeriodicRewardsSync(failedPlayer, 26, 1001, at), /periodic item insert failed/)
    assert.equal(balance(failedPlayer, 1), undefined)
    assert.equal(getPortablePlayerSnapshotSync(failedPlayer), null)
    database.exec("DROP TRIGGER fail_periodic_item")
    assert.equal(settlePlayerPeriodicRewardsSync(failedPlayer, 26, 1001, at).pointList[0].point, 1)

    // //// 开战后结束的活动沿用入场时间结算 [@x380kkm 2026-09-28] ////
    const crossedPlayer = makePlayer()
    setServerTime(new Date((point.end_time + 86400) * 1000))
    const crossed = settlePlayerPeriodicRewardsSync(crossedPlayer, 26, 1001, point.end_time)
    assert.equal(crossed.pointList[0].point, 1)
    assert.equal(crossed.rewardResult.items[40402], 9)
    setServerTime(null)
    console.log(`共同决战 ${paidQuests} 个限时关卡与 ${permanentQuests} 个常驻关卡完成奖励检查; 日界, 回退, 次数上限, 事务回滚和可移植存档覆盖完成.`)
} finally {
    database?.close()
    if (previousDatabase === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabase
    rmSync(root, { recursive: true, force: true })
}
