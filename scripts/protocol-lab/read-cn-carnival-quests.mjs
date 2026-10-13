// audience: internal
// # read-cn-carnival-quests
// 嘉年华关卡的基础收益和计分参数使用同一条原始主表记录.

import { decodeOrderedMapFile } from "./decode-cn-orderedmap.mjs"

// //// 读取嘉年华关卡和分数参数 [@x380kkm 2026-09-26] ////
export function readCarnivalQuests() {
    const master = decodeOrderedMapFile(new URL("../../core/personal-service/assets/cn-activity-masters/carnival_event_quest.orderedmap", import.meta.url))
    const quests = {}
    for (const [eventId, entries] of Object.entries(master)) {
        for (const row of Object.values(entries)) {
            const questId = integer(row[0])
            if (quests[questId]) throw new Error(`Duplicate carnival quest: ${questId}`)
            quests[questId] = {
                name: row[4],
                eventId: integer(eventId),
                folderId: integer(row[1]),
                clearRewardId: optionalInteger(row[6]),
                scoreRewardGroupId: optionalInteger(row[68]),
                element: integer(row[69]),
                rankPointReward: integer(row[82]),
                characterExpReward: integer(row[83]),
                manaReward: integer(row[84]),
                poolExpReward: integer(row[85]),
                fixedParty: optionalInteger(row[101]),
                carnivalScore: {
                    event_id: integer(eventId),
                    folder_id: integer(row[1]),
                    difficulty_score: integer(row[104]),
                    time_limit_ms: Math.round(integer(row[100]) * 1000 / 60),
                },
            }
        }
    }
    return quests
}
// //// /读取嘉年华关卡和分数参数 ////

// //// 读取主表整数字段 [@x380kkm 2026-09-26] ////
function integer(value) {
    const parsed = Number(value)
    if (value === undefined || value === "" || !Number.isSafeInteger(parsed)) {
        throw new Error(`Invalid carnival integer: ${value}`)
    }
    return parsed
}

// //// 读取主表可选整数字段 [@x380kkm 2026-09-26] ////
function optionalInteger(value) {
    return value === "(None)" ? undefined : integer(value)
}
