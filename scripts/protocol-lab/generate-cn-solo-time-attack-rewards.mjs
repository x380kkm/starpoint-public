// audience: internal
// # generate-cn-solo-time-attack-rewards
//
// 该脚本读取 solo_time_attack_clear_time_reward.orderedmap, 生成极时试炼的时间阈值和奖励.
// 奖励种类使用个人服务 MissionReward 的石和称号编码.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { parseArgs } from "node:util"
import { decodeOrderedMapFile } from "./decode-cn-orderedmap.mjs"
import { readEventRewards } from "./cn-event-reward-master.mjs"

// //// 按关卡汇总时间奖励 [@x380kkm 2026-09-21] ////
export function buildSoloTimeAttackRewards(rows) {
    const quests = {}
    for (const [key, row] of Object.entries(rows)) {
        const id = Number(key)
        const eventId = Number(row[1])
        const questId = Number(row[2])
        const elapsedTimeMs = Number(row[4]) * 1000
        if (![id, eventId, questId, elapsedTimeMs].every((value) => Number.isSafeInteger(value) && value > 0)) {
            throw new Error(`Invalid solo time attack reward row: ${key}`)
        }
        const compositeId = eventId * 1000 + questId
        const questRewards = quests[compositeId] ??= []
        questRewards.push({ id, elapsed_time_ms: elapsedTimeMs, rewards: readEventRewards(row, 6) })
    }
    for (const rewards of Object.values(quests)) rewards.sort((left, right) => left.id - right.id)
    return { source: "master/quest/event/solo_time_attack_clear_time_reward.orderedmap", quests }
}
// //// /按关卡汇总时间奖励 ////

// //// 写入时间奖励 fixture [@x380kkm 2026-09-21] ////
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const { values } = parseArgs({ options: { master: { type: "string" }, output: { type: "string" } } })
    if (!values.master || !values.output) throw new Error("--master and --output are required")
    const fixture = buildSoloTimeAttackRewards(decodeOrderedMapFile(values.master))
    fs.writeFileSync(values.output, `${JSON.stringify(fixture)}\n`, "utf8")
    console.log(JSON.stringify({ output: values.output, quests: Object.keys(fixture.quests).length }))
}
// //// /写入时间奖励 fixture ////
