// audience: internal
// # additional-quest-rewards
// 追加奖励按开战时间, 关卡范围和敌人等级独立选择, 客户端索引与发放物品共同来自原始主表.

import rewardSource from "../../core/personal-service/assets/cn-additional-battle-rewards.json"
import { getElementRewardItemId } from "./assets"
import { chooseWeighted, randomFraction } from "./questDrops"
import { DropScoreRewardId, EquipmentItemReward, QuestCategory, RewardType } from "./types"

interface QuestRewardRelation {
    quest_kind: number | null
    quest_parts: (number[] | null)[]
    reward_relation_id: number
}

interface AdditionalRewardRow {
    position: number
    weight: number
    reward: { type: number, id?: number | null, count: number }
    element_rarity?: number | null
}

interface AdditionalRewardCatalog {
    events: { id: number, start_time: number, end_time: number, required_quest?: { category: QuestCategory, quest_id: number }, relations: QuestRewardRelation[] }[]
    reward_relations: Record<string, { minimum_enemy_level: number, group_id: number }[]>
    reward_groups: Record<string, AdditionalRewardRow[]>
    quest_enemy_levels: Record<string, number>
}

const catalog = rewardSource as unknown as AdditionalRewardCatalog
const questCategories = [[1], [4], [2], [6], [14], [7], [10], [13], [11], [18], [19], [15], [6, 14, 13, 20], [20], [21], [22], [23], [24], [25], [26], [27]]

// //// 保存入场时满足日历和前置关卡的追加活动 [@x380kkm 2026-09-28] ////
export function getAdditionalQuestEvents(startedAt: number, hasCleared: (category: QuestCategory, questId: number) => boolean): number[] {
    return catalog.events.filter(event => startedAt >= event.start_time && startedAt <= event.end_time
        && (!event.required_quest || hasCleared(event.required_quest.category, event.required_quest.quest_id))).map(event => event.id)
}

export interface AdditionalQuestRewardContext {
    category: QuestCategory
    questId: number
    element: number
    eventIds: readonly number[]
}

// //// 按主表关卡编号和可用维度匹配追加范围 [@x380kkm 2026-09-28] ////
function matchesQuest(relation: QuestRewardRelation, category: number, questId: number): boolean {
    const kind = relation.quest_kind
    if (kind === null) return true
    if (!questCategories[kind]?.includes(category)) return false
    const parts = kind <= 2
        ? [Math.floor(questId / 1_000_000), Math.floor(questId / 1_000) % 1_000, questId % 1_000]
        : kind === 11 || kind === 12 ? [0, 0, questId] : [Math.floor(questId / 1_000), 0, questId % 1_000]
    const dimensions = kind <= 2 ? [0, 1, 2] : kind === 11 || kind === 12 ? [2] : [0, 2]
    return dimensions.every(index => {
        const allowed = relation.quest_parts[index]
        return allowed === null || allowed.includes(parts[index])
    })
}

// //// 选择有效活动的追加奖励并保留实际库存物品编号 [@x380kkm 2026-09-28] ////
export function drawAdditionalQuestRewards({ category, questId, element, eventIds }: AdditionalQuestRewardContext, roll = randomFraction) {
    const rewards: EquipmentItemReward[] = []
    const drops: DropScoreRewardId[] = []
    const enemyLevel = catalog.quest_enemy_levels[`${category}:${questId}`]
    if ([QuestCategory.CHARACTER, QuestCategory.TUTORIAL, QuestCategory.SKILL_PREVIEW, QuestCategory.TOWER_DUNGEON_EVENT].includes(category) || enemyLevel === undefined) return { rewards, drops }
    for (const event of catalog.events) {
        if (!eventIds.includes(event.id)) continue
        for (const relation of event.relations) {
            if (!matchesQuest(relation, category, questId)) continue
            for (const threshold of catalog.reward_relations[relation.reward_relation_id]) {
                if (enemyLevel < threshold.minimum_enemy_level) continue
                const selected = chooseWeighted(catalog.reward_groups[threshold.group_id], row => row.weight, roll())
                if (selected === undefined) continue
                const reward = selected.reward
                const isElement = reward.type === RewardType.ELEMENT || reward.type === RewardType.AETHER
                rewards.push({
                    type: isElement ? RewardType.ITEM : reward.type,
                    id: selected.element_rarity === undefined || selected.element_rarity === null
                        ? reward.id! : getElementRewardItemId(reward.type, selected.element_rarity, element),
                    count: reward.count,
                })
                drops.push({ group_id: threshold.group_id, index: selected.position, number: reward.count })
            }
        }
    }
    return { rewards, drops }
}
