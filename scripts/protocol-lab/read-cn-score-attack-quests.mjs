// audience: internal
// # read-cn-score-attack-quests
// 无限演武的关卡编号与奖励表键来自主表索引, 评分阈值使用成绩单位.

import { decodeOrderedMapFile } from "./decode-cn-orderedmap.mjs"
import { readEventRewards } from "./cn-event-reward-master.mjs"

// //// 按活动和关卡读取完整分数档位奖励 [@x380kkm 2026-09-26] ////
export function readScoreAttackBorderRewards(masterPath = new URL("../../core/personal-service/assets/cn-activity-masters/score_attack_border_reward.orderedmap", import.meta.url)) {
    const master = decodeOrderedMapFile(masterPath)
    const groups = {}
    for (const [id, row] of Object.entries(master)) {
        const key = `${integer(row[1])}_${integer(row[2])}`
        const rewards = groups[key] ??= []
        rewards.push({ id: integer(id), score: integer(row[4]), rewards: readEventRewards(row, 6) })
    }
    for (const rewards of Object.values(groups)) rewards.sort((left, right) => left.score - right.score)
    return groups
}
// //// /按活动和关卡读取完整分数档位奖励 ////

// //// 读取无限演武主表的奖励和评分字段 [@x380kkm 2026-09-26] ////
export function readScoreAttackQuests() {
    const master = decodeOrderedMapFile(new URL("../../core/personal-service/assets/cn-activity-masters/score_attack_event_quest.orderedmap", import.meta.url))
    const quests = {}
    for (const [eventId, entries] of Object.entries(master)) {
        for (const [localId, row] of Object.entries(entries)) {
            const questId = integer(row[0])
            const scoreRankThresholds = row.slice(52, 56).map(integer)
            if (scoreRankThresholds.length !== 4 || scoreRankThresholds.some((score, index) =>
                score < 0 || (index > 0 && score < scoreRankThresholds[index - 1]))) {
                throw new Error(`Invalid score attack rank thresholds: ${questId}`)
            }
            if (quests[questId]) throw new Error(`Duplicate score attack quest: ${questId}`)
            quests[questId] = {
                name: row[4],
                eventId: integer(eventId),
                folderId: optionalInteger(row[1]),
                clearRewardId: optionalInteger(row[6]),
                scoreRewardGroupId: optionalInteger(row[72]),
                scoreAttackRewardGroupId: integer(localId),
                scoreRankThresholds,
                rankPointReward: integer(row[86]),
                characterExpReward: integer(row[87]),
                manaReward: integer(row[88]),
                poolExpReward: integer(row[89]),
            }
        }
    }
    return quests
}
// //// /读取无限演武主表的奖励和评分字段 ////

// //// 读取主表整数字段 [@x380kkm 2026-09-26] ////
function integer(value) {
    const parsed = Number(value)
    if (value === undefined || value === "" || !Number.isSafeInteger(parsed)) {
        throw new Error(`Invalid score attack integer: ${value}`)
    }
    return parsed
}

// //// 读取主表可选整数字段 [@x380kkm 2026-09-26] ////
function optionalInteger(value) {
    return value === "(None)" ? undefined : integer(value)
}
