// audience: internal
// # cn-battle-drops-tests
// 共用关卡收益主表驱动库存发放和客户端掉落索引, 常驻剧情挑战和限时同名关卡分别结算.

import assert from "node:assert/strict"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { pack, unpack } from "msgpackr"

const require = createRequire(import.meta.url)
const root = mkdtempSync(join(tmpdir(), "starpoint-battle-drops-"))
const previousDatabase = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(root, "wdfp_data.db")
let database
let server

try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const { getDefaultPlayerData } = require("../../src/data/utils.ts")
    const {
        insertAccount, insertPlayerSync, insertSessionWithToken, getPlayerSync,
        getPlayerItemSync, givePlayerItemSync, getPlayerSingleQuestProgressSync, insertPlayerQuestProgressSync, updatePlayerQuestProgressSync,
    } = require("../../src/data/wdfpData.ts")
    const { SessionType } = require("../../src/data/types.ts")
    const { startPlayerQuest: insertActiveQuest } = require("../../src/lib/quest.ts")
    const { registerBattleSettlementRoutes } = require("../../src/routes/api/singleBattleQuest.ts")
    const { getQuestFromCategorySync, getRareScoreRewardGroup } = require("../../src/lib/assets.ts")
    const { drawQuestDrops } = require("../../src/lib/questDrops.ts")
    const { drawAdditionalQuestRewards, getAdditionalQuestEvents } = require("../../src/lib/additionalQuestRewards.ts")
    const { setServerTime } = require("../../src/utils.ts")
    const { givePlayerRewardsSync } = require("../../src/lib/quest.ts")
    const fixture = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-single-battle.json", import.meta.url), "utf8"))
    const account = await insertAccount({
        appId: "battle-drops", idpAlias: "battle-drops", idpCode: "device", idpId: "battle-drops", status: "active",
    })
    const player = insertPlayerSync(account.id, getDefaultPlayerData())
    const viewer = 900000 + account.id
    await insertSessionWithToken({ token: String(viewer), accountId: account.id, type: SessionType.VIEWER, expires: new Date() })

    // //// 按全部掉落关卡核对评级次数和主表位置 [@x380kkm 2026-09-28] ////
    let checked = 0
    let elemental = 0
    for (const [key, expected] of Object.entries(fixture.quests)) {
        if (expected.score_reward_group_id === undefined) continue
        const quest = getQuestFromCategorySync(expected.category, expected.quest_id)
        assert.equal(quest.scoreRewardGroupId, expected.score_reward_group_id, key)
        assert.deepEqual(quest.scoreRewardCounts, expected.score_reward_counts, key)
        const common = expected.score_rewards.filter(entry => entry.type === 0)
        for (const rank of [1, 5]) {
            const drops = drawQuestDrops(quest, rank, false, () => 0)
            assert.equal(drops.commonDrops.length, common.length ? expected.score_reward_counts[rank - 1] : 0, key)
            for (const drop of drops.commonDrops) {
                const entry = expected.score_rewards.find(entry => entry.position === drop.index)
                assert.equal(drop.number, entry.count, key)
            }
            for (const drop of drops.rareDrops) {
                const entry = fixture.rare_reward_groups[drop.group_id].find(entry => entry.position === drop.index)
                assert.equal(drop.number, entry.count, key)
            }
        }
        const materials = common.filter(entry => entry.reward_type === 6 || entry.reward_type === 7)
        if (materials.length) {
            const drops = drawQuestDrops(quest, 5, false, () => 0)
            const item = materials[0].id
            const before = getPlayerItemSync(player, item) ?? 0
            const delivered = givePlayerRewardsSync(player, drops.rewards)
            const amount = drops.rewards.filter(reward => reward.type === 0 && reward.id === item)
                .reduce((sum, reward) => sum + reward.count, 0)
            assert.ok(amount > 0, key)
            assert.equal(getPlayerItemSync(player, item) - before, amount, key)
            assert.equal(delivered.items[item], before + amount, key)
            elemental++
        }
        checked++
    }
    assert.equal(elemental, 71)

    // //// 按组内概率边界选择稀有奖励并保存最终库存总量 [@x380kkm 2026-09-28] ////
    const talos = getQuestFromCategorySync(18, 500009101)
    const oddSlots = { ...talos, scoreRewardCounts: [1, 2, 3, 4, 5] }
    assert.equal(drawQuestDrops(oddSlots, 3, false, () => 0, true).commonDrops.length, 5)
    assert.equal(drawQuestDrops(oddSlots, 3, false, () => 0).commonDrops.length, 3)
    const rarityQuest = { ...talos, scoreRewardCounts: [0, 0, 0, 0, 0],
        scoreRewardGroup: [{ type: 1, id: 11000359, position: 14, rarity: 1 }] }
    const rolls = [0, 0.5]
    const rare = drawQuestDrops(rarityQuest, 5, true, () => rolls.shift())
    assert.deepEqual(rare.rareDrops, [{ group_id: 11000359, index: 2, number: 4 }])
    assert.deepEqual(getRareScoreRewardGroup(11000359).map(reward => reward.rarity), [0.5, 0.5])
    assert.equal(drawQuestDrops({ ...rarityQuest, scoreRewardGroup: [
        { type: 1, id: 11000359, position: 14, rarity: 0 },
    ] }, 5, false, () => 0).rareDrops.length, 0)
    const oldTotal = getPlayerItemSync(player, 62) ?? 0
    givePlayerItemSync(player, 62, 100)
    const repeated = givePlayerRewardsSync(player, [{ type: 0, id: 62, count: 2 }, { type: 0, id: 62, count: 3 }])
    assert.equal(repeated.items[62], oldTotal + 105)
    assert.equal(repeated.items[62], getPlayerItemSync(player, 62))

    server = require("fastify")()
    server.addHook("onSend", (_request, reply, payload, done) => {
        done(null, reply.getHeader("content-type") === "application/x-msgpack" ? pack(payload) : payload)
    })
    await server.register(registerBattleSettlementRoutes)
    async function finish(questId, attempt, elapsed, { startedAt, category = 18, accomplished = true, clearedPhases = 1 } = {}) {
        const play = `story-challenge-${questId}-${attempt}`
        insertActiveQuest(player, {
            questId, playId: play, category, useBossBoostPoint: false, useBoostPoint: false, isAutoStartMode: false, startedAt,
        })
        const response = await server.inject({ method: "POST", url: "/finish", payload: {
            viewer_id: viewer, category, quest_id: questId, play_id: play,
            elapsed_time_ms: elapsed, score: 1000, add_mana: 0, is_accomplished: accomplished,
            statistics: { clear_phase: clearedPhases, party: {
                characters: [{ id: 1 }, null, null], unison_characters: [null, null, null],
                equipments: [null, null, null], ability_soul_ids: [null, null, null],
            } },
        } })
        assert.equal(response.statusCode, 200, response.body)
        return unpack(response.rawPayload).data
    }

    // //// 验证常驻剧情挑战的首通和评级奖励各发放一次 [@x380kkm 2026-09-28] ////
    const challenges = Object.values(fixture.quests).filter(quest => quest.category === 18
        && quest.stamina_cost > 0 && quest.score_reward_group_id === undefined
        && quest.clear_reward_id === 55 && quest.s_plus_reward_id === 55)
    assert.equal(challenges.length, 23)
    for (const quest of challenges) {
        const id = quest.quest_id
        if (id === 400016102) insertPlayerQuestProgressSync(player, 18, { questId: id, finished: false })
        for (const [attempt, elapsed, amount] of [[0, 9999999, 75], [1, 1000, 75], [2, 1000, 0]]) {
            const before = getPlayerSync(player).freeVmoney
            const result = await finish(id, attempt, elapsed)
            assert.equal(getPlayerSync(player).freeVmoney - before, amount, `${id}:${attempt}`)
            assert.deepEqual(result.drop_score_reward_ids, [])
            assert.deepEqual(result.drop_rare_reward_ids, [])
        }
    }
    for (const id of [500009101, 500014101]) {
        const result = await finish(id, 0, 1000)
        assert.equal(result.drop_score_reward_ids.length, 12)
        assert.ok(result.item_list[62] >= 3)
    }
    // //// 在基础掉落为空时结算入场活动的独立追加币 [@x380kkm 2026-09-28] ////
    const collectEnd = 1749139199
    const unlockProgress = { questId: 1004002, finished: true }
    if (getPlayerSingleQuestProgressSync(player, 1, unlockProgress.questId)) updatePlayerQuestProgressSync(player, 1, unlockProgress)
    else insertPlayerQuestProgressSync(player, 1, unlockProgress)
    assert.equal(getAdditionalQuestEvents(collectEnd, () => false).includes(100008), false)
    setServerTime(new Date((collectEnd + 60) * 1000))
    const beforeCollect = [10000053, 10000054].map(id => getPlayerItemSync(player, id) ?? 0)
    const extra = await finish(400016102, "collect", 1000, { startedAt: collectEnd })
    assert.equal(extra.start_time, collectEnd)
    assert.equal(extra.drop_additional_reward_ids.length, 7)
    assert.deepEqual(extra.drop_score_reward_ids, [])
    for (const [index, id, amount] of [[0, 10000053, 1500], [1, 10000054, 100]]) {
        assert.equal(getPlayerItemSync(player, id) - beforeCollect[index], amount)
        assert.equal(extra.item_list[id], beforeCollect[index] + amount)
    }
    assert.deepEqual(drawAdditionalQuestRewards({ category: 18, questId: 400016102, element: 0,
        eventIds: getAdditionalQuestEvents(collectEnd + 1, () => true) }).drops, [])
    // //// 首通货币与基础结算相加并返回真实库存 [@x380kkm 2026-09-28] ////
    for (const [questId, field, responseField, amount] of [[200004001, "freeMana", "free_mana", 15000], [200004002, "expPool", "exp_pool", 30000]]) {
        const before = getPlayerSync(player)[field]
        const result = await finish(questId, "currency", 1000, { category: 10 })
        const after = getPlayerSync(player)[field]
        assert.ok(after - before >= amount, `${questId}: ${after - before}`)
        assert.equal(result.user_info[responseField], after)
    }
    // //// 未完成普通战斗保持库存, 多阶段战斗结算已完成阶段 [@x380kkm 2026-09-28] ////
    const failed = await finish(500009101, "failed", 1000, { accomplished: false, clearedPhases: 0 })
    assert.deepEqual(failed.drop_score_reward_ids, [])
    assert.deepEqual(failed.drop_rare_reward_ids, [])
    assert.deepEqual(failed.drop_additional_reward_ids, [])
    const partial = await finish(1001, "partial", 1000, { category: 11, accomplished: false })
    assert.equal(partial.drop_score_reward_ids.length, 4)
    assert.equal(partial.drop_score_reward_ids[0].group_id, 20082101)
    // //// 战斗接口投递共同决战的周期奖励并回传剩余次数 [@x380kkm 2026-09-28] ////
    const periodicSource = JSON.parse(readFileSync(new URL("../../core/personal-service/assets/cn-periodic-battle-rewards.json", import.meta.url), "utf8"))
    const periodic = await finish(1001, "periodic", 1000, { category: 26, startedAt: periodicSource.points[1].start_time + 60 })
    assert.deepEqual(periodic.drop_periodic_reward_ids, [{ group_id: 1, index: 1, number: 9 }])
    assert.deepEqual(periodic.user_periodic_reward_point_list, [{ id: 1, point: 1 }])
    assert.equal(periodic.item_list[40402], getPlayerItemSync(player, 40402))
    console.log(JSON.stringify({ rewardBearingQuests: checked, elementRewardQuests: elemental, storyChallenges: challenges.length }))
} finally {
    await server?.close()
    database?.close()
    if (previousDatabase === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previousDatabase
    rmSync(root, { recursive: true, force: true })
}
