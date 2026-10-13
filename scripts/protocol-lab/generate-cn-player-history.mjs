// audience: internal
// # generate-cn-player-history
// 此脚本从 CN 当前资源主表生成回忆录时窗, 主题, 背景和挑战领主目录.

import fs from "node:fs"
import path from "node:path"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"

// //// 读取四份回忆录主表并保存服务端目录 [@x380kkm 2026-09-26] ////
const cdnRoot = process.argv[2]
if (!cdnRoot) throw new Error("CN resource directory is required")
const names = ["player_history", "player_history_topic", "player_history_card_background", "player_history_challenge_single_boss"]
const schemas = names.map(name => ({ name, logicalPath: `master/player_history/${name}.orderedmap` }))
const buffers = await readMasterBuffers(cdnRoot, readEntityRecords(cdnRoot), schemas)
const tables = Object.fromEntries(schemas.map(({ name }) => {
    if (!buffers.has(name)) throw new Error(`CN player history master is missing: ${name}`)
    return [name, decodeOrderedMap(buffers.get(name))]
}))
const missionMaster = JSON.parse(fs.readFileSync("core/personal-service/assets/cn-mission-master.json", "utf8"))
const equipmentShop = JSON.parse(fs.readFileSync("assets/equipment_enhancement_shop.json", "utf8"))
const milestones = []
for (const [category, missions] of Object.entries(missionMaster.categories)) {
    for (const mission of missions) {
        for (const stage of mission.stages) {
            const chapter = Number(category) === 1 && mission.pattern === "chapter_clear" && mission.quest_kind === 0 && mission.quest_parts[0].length === 1
            const threshold = Number(category) === 1 && ["player_rank_achievement", "characters_count"].includes(mission.pattern) && stage.target === 100
            const manaBoard = mission.pattern === "mana_board_2nd_complete_count" && mission.character_id && stage.target === 1
            if (chapter || threshold || manaBoard) milestones.push({ category: Number(category), mission_id: mission.id, stage: stage.stage,
                pattern: mission.pattern, chapter_id: chapter ? mission.quest_parts[0][0] : null, character_id: mission.character_id })
        }
    }
}
const equipmentMaxEnhancement = {}
for (const item of Object.values(equipmentShop)) {
    equipmentMaxEnhancement[item.equipmentId] = Math.max(equipmentMaxEnhancement[item.equipmentId] ?? 0, item.enhancementMaxLevel)
}
const catalog = {
    milestones,
    equipment_max_enhancement: equipmentMaxEnhancement,
    histories: Object.entries(tables.player_history).map(([id, row]) => ({
        id: Number(id), string_id: row[0], aggregation_time: parseCnMasterTimestamp(row[1]) / 1000,
        start_time: parseCnMasterTimestamp(row[2]) / 1000, end_time: parseCnMasterTimestamp(row[3]) / 1000,
    })),
    topics: Object.fromEntries(Object.entries(tables.player_history_topic).map(([historyId, topics]) => [
        historyId, Object.entries(topics).map(([index, row]) => ({
            index: Number(index), multiplied_id: Number(row[0]), display_order: Number(row[1]),
            aggregation_target: Number(row[2]), tab_group: Number(row[3]), toggle_default: row[4] === "true",
        })),
    ])),
    backgrounds: Object.entries(tables.player_history_card_background).map(([id, row]) => ({
        id: Number(id), frame_image_path: row[0], thumbnail_image_path: row[1], display_order: Number(row[2]),
        tab_group: Number(row[3]), is_default: row[4] === "true",
    })),
    challenge_single_bosses: Object.entries(tables.player_history_challenge_single_boss).map(([id, row]) => ({
        id: Number(id), string_id: row[0], name: row[1], degree_mission_id: Number(row[2]), thumbnail_image_path: row[3],
    })),
}
const output = path.resolve("core/personal-service/assets/cn-player-history.json")
fs.writeFileSync(output, `${JSON.stringify(catalog, null, 2)}\n`, "utf8")
fs.writeFileSync(path.resolve("core/personal-service/assets/cn-activity-masters/player_history.orderedmap"), buffers.get("player_history"))
console.log(JSON.stringify({ output, histories: catalog.histories.length, topics: Object.values(catalog.topics).reduce((total, topics) => total + topics.length, 0), backgrounds: catalog.backgrounds.length, challenge_single_bosses: catalog.challenge_single_bosses.length }))
