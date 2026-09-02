// audience: internal
// # test-cn-single-battle-fixture
//
// 该脚本校验 CN 单机战斗生成结果覆盖参考任务全集, 关键活动字段和确定性输出.

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
    15: 98,
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

// //// 生成两份数据并核对完整任务闭包 [@x380kkm 2026-08-22] ////
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
    const assetRoot = path.resolve(repositoryRoot, "..", "startpoint-cn", "assets")
    const generatorPath = path.join(scriptDirectory, "generate-cn-single-battle-fixture.mjs")
    const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-battle-"))

    try {
        const firstOutput = path.join(tempRoot, "first.json")
        const secondOutput = path.join(tempRoot, "second.json")
        generate(generatorPath, assetRoot, firstOutput)
        generate(generatorPath, assetRoot, secondOutput)
        assert.deepEqual(fs.readFileSync(firstOutput), fs.readFileSync(secondOutput))

        const fixture = readJson(firstOutput)
        assert.equal(fixture.source.source_total, 5257)
        assert.equal(fixture.source.quest_total, 5257)
        assert.deepEqual(fixture.source.category_counts, EXPECTED_CATEGORY_COUNTS)
        assert.equal(Object.keys(fixture.quests).length, 5257)
        assert.equal(Object.keys(fixture.characters).length, 505)

        const mainQuests = readJson(path.join(assetRoot, "main_quest.json"))
        assert.equal(Object.keys(mainQuests).length, 419)
        for (const questId of Object.keys(mainQuests)) {
            assert.ok(fixture.quests[`1:${questId}`], `main quest ${questId} is missing`)
        }

        assert.equal(fixture.quests["26:1001"].linked_quest_id, 200014004)
        assert.equal(fixture.quests["26:1001"].clear_reward, undefined)
        assert.equal(fixture.quests["27:1101"].score_attack_reward_group_id, 1)
        assert.equal(fixture.quests["27:1101"].score_reward_group_id, undefined)
        assert.deepEqual(
            {
                event: fixture.quests["22:1001"].carnival_event_id,
                folder: fixture.quests["22:1001"].carnival_folder_id,
                difficulty: fixture.quests["22:1001"].carnival_difficulty_score,
                timeLimit: fixture.quests["22:1001"].carnival_time_limit_ms,
            },
            { event: 1, folder: 1, difficulty: 20, timeLimit: 108000 },
        )

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
// //// /生成两份数据并核对完整任务闭包 ////

main()
