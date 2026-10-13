// audience: internal
// # generate-cn-active-mission-incentives
//
// 此生成器从 CN 归档主表生成主动任务归属和游戏内激励奖励池. 奖励类型和列位置对应客户端生成类.

import fs from "node:fs"
import path from "node:path"
import { createRequire } from "node:module"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"

const unzipper = createRequire(import.meta.url)("unzipper")
const SOURCES = {
    missions: ["archive-common-full/pinball-1.4.0-226-e4ae73c8.zip", "production/upload/b1/b53fdd934dc5ab6d8bfc1c9132cff75c473c48"],
    rewards: ["archive-common-full/pinball-1.4.0-234-bca369df.zip", "production/upload/b8/293e71f0270514675b9cf950014e2a3b65bc66"],
    bosses: ["archive-common-diff/pinball-1.4.48-1.4.49-1-4c2dd780.zip", "production/upload/eb/f8ef19148af9c1330b78c7fb3ce75e3f202e64"],
}

function argument(name) {
    const index = process.argv.indexOf(name)
    if (index < 0 || !process.argv[index + 1]) throw new Error(`missing ${name}`)
    return process.argv[index + 1]
}

async function readSource(cdnRoot, name) {
    const [archiveName, entryName] = SOURCES[name]
    const archive = await unzipper.Open.file(path.join(cdnRoot, archiveName))
    const entry = archive.files.find((entry) => entry.path === entryName)
    if (!entry) throw new Error(`missing master entry ${entryName}`)
    return decodeOrderedMap(await entry.buffer())
}

function seconds(value) {
    const timestamp = parseCnMasterTimestamp(value)
    return timestamp === null ? null : timestamp / 1000
}

function optionalNumber(value) {
    return value === undefined || value === "" || value === "(None)" ? null : Number(value)
}

function numbers(value) {
    return value === undefined || value === "" || value === "(None)" ? [] : value.split(",").map(Number)
}

function missionDefinitions(missions, clientSource) {
    const source = fs.readFileSync(clientSource, "utf8")
    const patterns = new Map([...source.matchAll(/_loc7_ == "(\d+)"\)\s*\{\s*_loc9_ = MissionPatternKind\.(\w+)/g)].map((match) => [match[1], match[2]]))
    return Object.fromEntries(Object.entries(missions).map(([id, row]) => {
        const pattern = patterns.get(row[29])
        if (!pattern) throw new Error(`active mission ${id} has unknown pattern ${row[29]}`)
        return [id, {
            event_id: Number(row[0]), pattern,
            phase: optionalNumber(row[1]), battle_kind: optionalNumber(row[32]), statistics_kind: optionalNumber(row[30]),
            quest_kind: optionalNumber(row[34]), quest_parts: [numbers(row[35]), numbers(row[36]), numbers(row[37])],
            quest_categories: numbers(row[38]), quest_ids: numbers(row[39]), quest_rank: optionalNumber(row[40]),
            item_id: optionalNumber(row[41]), equipment_ids: numbers(row[42]), character_id: optionalNumber(row[43]),
            campaign_kind: optionalNumber(row[44]), campaign_id: optionalNumber(row[45]),
            main_character_ids: row.slice(46,49).map(optionalNumber), unison_character_ids: row.slice(49,52).map(optionalNumber),
            party_equipment_ids: row.slice(52,55).map(optionalNumber), mission_ids: numbers(row[55]),
            need_mission_id: optionalNumber(row[56]), need_mission_stage: optionalNumber(row[57]),
            start_time: seconds(row[60]), end_time: seconds(row[61]),
            character_element: optionalNumber(row[69]), equipment_element: optionalNumber(row[70]),
            action_effects: row[71] === "(None)" ? [] : row[71].split(",").filter(Boolean),
            effects_ignore_characters: row[72] === "(None)" ? [] : row[72].split(",").filter(Boolean),
        }]
    }))
}

function bossQuestRanks(bosses) {
    const ranks = {}
    const read = (value) => {
        if (Array.isArray(value)) {
            const questId = Number(value[0])
            const rank = Number(value[107])
            if (!Number.isSafeInteger(questId) || questId <= 0 || !Number.isSafeInteger(rank) || rank <= 0) throw new Error("invalid boss quest rank")
            ranks[`2:${questId}`] = rank
        } else for (const entry of Object.values(value)) read(entry)
    }
    read(bosses)
    return ranks
}

// //// 生成任务归属和按权重抽取的游戏内奖励 [@x380kkm 2026-09-21] ////
const cdnRoot = argument("--cdn-root")
const output = argument("--output")
const missions = await readSource(cdnRoot, "missions")
const rewards = await readSource(cdnRoot, "rewards")
const bosses = await readSource(cdnRoot, "bosses")
const events = decodeOrderedMap(fs.readFileSync("core/personal-service/assets/cn-activity-masters/active_mission_event.orderedmap"))
const document = {
    missions: missionDefinitions(missions, argument("--client-source")),
    quest_ranks: bossQuestRanks(bosses),
    events: Object.fromEntries(Object.entries(events).map(([id, row]) => [id, {
        start_time: seconds(row[14]), end_time: seconds(row[15]), playable_end_time: seconds(row[16]), lottery_end_time: seconds(row[17]),
    }])),
    rewards: Object.entries(rewards).map(([id, row]) => {
        const reward = { id: Number(id), event_id: Number(row[0]), weight: Number(row[22]), items: {}, free_vmoney: 0, free_mana: 0, exp_pool: 0 }
        for (let slot = 0; slot < 6; slot++) {
            const column = 4 + slot * 3
            if (row[column] === "(None)") continue
            const kind = Number(row[column])
            const amount = Number(row[column + 2])
            if (!Number.isSafeInteger(amount) || amount <= 0) throw new Error(`invalid reward ${id}`)
            switch (kind) {
                case 0: reward.free_vmoney += amount; break
                case 1: reward.items[row[column + 1]] = (reward.items[row[column + 1]] ?? 0) + amount; break
                case 2: reward.exp_pool += amount; break
                case 3: reward.free_mana += amount; break
                default: throw new Error(`unsupported incentive reward ${kind}`)
            }
        }
        return reward
    }),
}
fs.writeFileSync(output, `${JSON.stringify(document, null, 2)}\n`, "utf8")
process.stdout.write(`${JSON.stringify({ missions: Object.keys(document.missions).length, rewards: document.rewards.length, output })}\n`)
// //// /生成任务归属和按权重抽取的游戏内奖励 ////
