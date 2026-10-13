// audience: internal
// # generate-cn-carnival-rewards
//
// 该脚本从 carnival_event_total_score_reward.orderedmap 生成嘉年华总分奖励.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { parseArgs } from "node:util"
import { decodeOrderedMapFile } from "./decode-cn-orderedmap.mjs"
import { readEventRewards } from "./cn-event-reward-master.mjs"

// //// 按活动汇总总分奖励 [@x380kkm 2026-09-21] ////
export function buildCarnivalRewards(rows) {
    const events = {}
    for (const [key, row] of Object.entries(rows)) {
        const id = Number(key)
        const eventId = Number(row[0])
        const score = Number(row[2])
        if (![id, eventId, score].every((value) => Number.isSafeInteger(value) && value > 0)) {
            throw new Error(`Invalid carnival reward row: ${key}`)
        }
        const eventRewards = events[eventId] ??= []
        eventRewards.push({ id, score, rewards: readEventRewards(row, 4) })
    }
    for (const rewards of Object.values(events)) rewards.sort((left, right) => left.id - right.id)
    return { source: "master/quest/event/carnival_event_total_score_reward.orderedmap", events }
}
// //// /按活动汇总总分奖励 ////

// //// 写入嘉年华总分奖励 fixture [@x380kkm 2026-09-21] ////
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const { values } = parseArgs({ options: { master: { type: "string" }, output: { type: "string" } } })
    if (!values.master || !values.output) throw new Error("--master and --output are required")
    const fixture = buildCarnivalRewards(decodeOrderedMapFile(values.master))
    fs.writeFileSync(values.output, `${JSON.stringify(fixture)}\n`, "utf8")
    console.log(JSON.stringify({ output: values.output, events: Object.keys(fixture.events).length }))
}
// //// /写入嘉年华总分奖励 fixture ////
