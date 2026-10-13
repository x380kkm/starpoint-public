// audience: internal
// # test-cn-single-battle-fixture
//
// 该脚本校验打包用 CN 战斗资产与参考任务全集和关键活动字段一致.

import assert from "node:assert/strict"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

const EXPECTED_CATEGORY_COUNTS = {
    1: 419,
    2: 232,
    3: 1318,
    4: 221,
    6: 114,
    7: 459,
    10: 348,
    11: 7,
    13: 46,
    14: 6,
    15: 91,
    18: 913,
    19: 96,
    20: 480,
    21: 28,
    22: 171,
    23: 50,
    24: 110,
    25: 6,
    26: 12,
    27: 123,
}

// //// 从参考任务全集核对打包用战斗资产 [@x380kkm 2026-08-22] ////
function generate(generatorPath, assetRoot, outputPath) {
    const result = spawnSync(process.execPath, [generatorPath, "--asset-root", assetRoot, "--output", outputPath], {
        encoding: "utf8",
    })
    assert.equal(result.status, 0, result.stderr || result.stdout)
}

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, "utf8"))
}

function main() {
    const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
    const repositoryRoot = path.resolve(scriptDirectory, "..", "..")
    const assetRoot = process.argv[2] ? path.resolve(process.argv[2]) : path.resolve(repositoryRoot, "..", "startpoint-cn", "assets")
    const generatorPath = path.join(scriptDirectory, "generate-cn-single-battle-fixture.mjs")
    const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-battle-"))

    try {
        const firstOutput = path.join(tempRoot, "first.json")
        generate(generatorPath, assetRoot, firstOutput)
        const fixture = readJson(firstOutput)
        assert.deepEqual(readJson(path.join(repositoryRoot, "core/personal-service/assets/cn-single-battle.json")), fixture,
            "packaged battle fixture differs from the source catalog")
        assert.equal(fixture.source.source_total, 5250)
        assert.equal(fixture.source.quest_total, 5250)
        assert.deepEqual(fixture.source.category_counts, EXPECTED_CATEGORY_COUNTS)
        assert.equal(Object.keys(fixture.quests).length, 5250)
        for (const id of [80, 88, 89, 90, 98, 99, 100]) {
            assert.equal(fixture.quests[`15:${id}`], undefined)
        }
        assert.equal(Object.keys(fixture.characters).length, 505)
        for (const key of ["10:200031018", "7:100006001", "7:300000001", "7:300001001"]) {
            assert.equal(fixture.quests[key].clear_reward.count, 1)
        }

        const mainQuests = readJson(path.join(assetRoot, "main_quest.json"))
        assert.equal(Object.keys(mainQuests).length, 419)
        for (const questId of Object.keys(mainQuests)) {
            assert.ok(fixture.quests[`1:${questId}`], `main quest ${questId} is missing`)
        }

        assert.equal(fixture.quests["26:1001"].linked_quest_id, 200014004)
        assert.deepEqual(fixture.quests["26:1001"].clear_reward, { count: 30, type: 3 })
        assert.deepEqual(fixture.quests["26:1001"].s_plus_reward, { count: 30, type: 3 })
        assert.equal(fixture.quests["26:1001"].stamina_cost, 35)
        assert.deepEqual(fixture.quests["4:10001001"].s_plus_reward, { count: 25, type: 3 })
        for (const key of ["15:1", "23:1001", "25:1001"]) {
            assert.equal(fixture.quests[key].s_plus_reward, undefined)
        }
        assert.equal(fixture.quests["15:1"].clear_reward, undefined)
        assert.equal(fixture.quests["15:1"].a_rank_time, 173500)
        assert.equal(fixture.quests["7:1002"].a_rank_time, 604500)
        for (const [key, expected] of [["7:1002", [27, 112, 80, 112]],
            ["23:1001", [28, 94, 100, 94]], ["26:1001", [1590, 2900, 2790, 2900]]]) {
            const quest = fixture.quests[key]
            assert.deepEqual([quest.rank_point_reward, quest.character_exp_reward,
                quest.mana_reward, quest.pool_exp_reward], expected)
        }
        assert.equal(fixture.quests["27:1001"].stamina_cost, 10)
        for (const [quest, item] of [[200021009, 40323], [200013009, 40314],
            [200076009, 10000072], [200071009, 10000049], [200050009, 40335]]) {
            assert.equal(fixture.quests[`7:${quest}`].entry_item_id, item)
            assert.equal(fixture.quests[`7:${quest}`].entry_item_count, 1)
        }
        assert.equal(fixture.quests["13:2001"].entry_item_id, 500000)
        assert.equal(fixture.quests["13:2001"].entry_item_count, 1)
        assert.equal(fixture.quests["27:1101"].score_attack_reward_group_id, 101)
        assert.equal(fixture.quests["27:1101"].score_reward_group_id, undefined)
        const scoreAttack = fixture.quests["27:1001"]
        assert.equal(scoreAttack.clear_reward, undefined)
        assert.equal(scoreAttack.s_plus_reward, undefined)
        assert.deepEqual(scoreAttack.score_rank_thresholds, [150000000, 340000000, 530000000, 1171000000])
        assert.deepEqual([scoreAttack.rank_point_reward, scoreAttack.character_exp_reward,
            scoreAttack.mana_reward, scoreAttack.pool_exp_reward], [10, 15, 15, 15])
        const scoreAttackRewardIds = new Set()
        const itemCapacities = readJson(path.join(repositoryRoot, "core/personal-service/assets/cn-shop-capacities.json")).items
        for (const quest of Object.values(fixture.quests).filter((quest) => quest.category === 27)) {
            assert.equal(quest.score_attack_reward_group_id, quest.quest_id % 1000)
            assert.ok(quest.score_attack_border_rewards.length > 0, `score attack reward missing: ${quest.quest_id}`)
            assert.equal(quest.score_rank_thresholds.length, 4)
            for (const border of quest.score_attack_border_rewards) {
                assert.ok(!scoreAttackRewardIds.has(border.id), `duplicate score attack reward: ${border.id}`)
                scoreAttackRewardIds.add(border.id)
                assert.ok(border.rewards.length > 0)
                for (const reward of border.rewards) {
                    assert.equal(reward.kind, 1)
                    assert.ok(itemCapacities[reward.item_id] > 0, `missing score attack item: ${reward.item_id}`)
                    assert.ok(reward.amount > 0)
                }
            }
        }
        assert.equal(scoreAttackRewardIds.size, 11100)
        assert.deepEqual(fixture.quests["27:1101"].score_attack_border_rewards.find(reward => reward.id === 101075).rewards, [
            { kind: 1, item_id: 49100, amount: 1 },
            { kind: 1, item_id: 40502, amount: 2 },
        ])
        assert.deepEqual(
            {
                event: fixture.quests["22:1001"].carnival_event_id,
                folder: fixture.quests["22:1001"].carnival_folder_id,
                difficulty: fixture.quests["22:1001"].carnival_difficulty_score,
                timeLimit: fixture.quests["22:1001"].carnival_time_limit_ms,
            },
            { event: 1, folder: 1, difficulty: 200000, timeLimit: 1800000 },
        )
        assert.equal(fixture.quests["22:1002"].carnival_folder_id, 1)
        assert.equal(fixture.quests["22:1002"].carnival_difficulty_score, 2000000)

        const elementReward = Object.values(fixture.quests)
            .flatMap((quest) => quest.score_rewards)
            .find((reward) => reward.reward_type === 6 || reward.reward_type === 7)
        assert.ok(elementReward)
        assert.ok(Number.isSafeInteger(elementReward.id))
        assert.ok(Number.isSafeInteger(elementReward.element_rarity))
    } finally {
        fs.rmSync(tempRoot, { recursive: true, force: true })
    }
}
// //// /从参考任务全集核对打包用战斗资产 ////

main()
