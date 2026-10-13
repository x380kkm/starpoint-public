// audience: internal
// # quest-drops
// 普通掉落按评级次数和主表权重抽取, 稀有掉落使用独立概率和组内权重, 结果索引保留主表位置.

import { randomInt } from "crypto"
import { getElementRewardItemId, getMultiplayerCommonRewardMultiplier, getRareScoreRewardGroup } from "./assets"
import { BattleQuest, CurrencyScoreReward, DropScoreRewardId, EquipmentItemReward, ItemScoreReward, RareScoreRewardGroup, Reward, RewardType, ScoreRewardType } from "./types"

// //// 从加权记录中按半开区间选择一项 [@x380kkm 2026-09-28] ////
export function chooseWeighted<T>(entries: T[], weight: (entry: T) => number, roll: number): T | undefined {
    let remaining = roll * entries.reduce((sum, entry) => sum + weight(entry), 0)
    return entries.find(entry => {
        remaining -= weight(entry)
        return remaining < 0
    })
}

export function randomFraction(): number {
    return randomInt(0x100000000) / 0x100000000
}

// //// 应用素材属性和加成点并保留角色与星导石数量 [@x380kkm 2026-09-28] ////
function resolveReward(reward: Reward & { id?: number, count?: number }, element: number, boosted: boolean): EquipmentItemReward {
    const kind = reward.type
    const count = kind === RewardType.CHARACTER ? 1 : reward.count ?? 0
    const amount = boosted && kind !== RewardType.CHARACTER && kind !== RewardType.BEADS ? count * 2 : count
    const isElement = kind === RewardType.ELEMENT || kind === RewardType.AETHER
    return {
        type: isElement ? RewardType.ITEM : kind,
        id: isElement ? getElementRewardItemId(kind, reward.id!, element) : reward.id!,
        count: amount,
    }
}

// //// 生成关卡掉落及客户端展示索引 [@x380kkm 2026-09-28] ////
export function drawQuestDrops(quest: BattleQuest, clearRank: number, boosted: boolean, roll = randomFraction, isMulti = false) {
    const rewards: Reward[] = []
    const commonDrops: DropScoreRewardId[] = []
    const rareDrops: DropScoreRewardId[] = []
    const groupId = quest.scoreRewardGroupId
    if (groupId === undefined) return { rewards, commonDrops, rareDrops }
    const scoreRewards = quest.scoreRewardGroup ?? []
    const common = scoreRewards.filter(entry => entry.type === ScoreRewardType.ITEM) as (CurrencyScoreReward | ItemScoreReward)[]
    const drawCount = Math.ceil(quest.scoreRewardCounts[clearRank - 1] * (isMulti ? getMultiplayerCommonRewardMultiplier() : 1))
    for (let count = 0; count < drawCount && common.length > 0; count++) {
        const selected = chooseWeighted(common, entry => entry.field5, roll())
        if (selected === undefined) continue
        const reward = resolveReward({ ...selected, type: selected.reward_type }, quest.element ?? 0, boosted)
        rewards.push(reward)
        commonDrops.push({ group_id: groupId, index: selected.position, number: reward.count })
    }
    for (const selected of scoreRewards.filter(entry => entry.type === ScoreRewardType.RARE_POOL) as RareScoreRewardGroup[]) {
        if (roll() >= selected.rarity) continue
        const group = getRareScoreRewardGroup(selected.id) ?? []
        const entry = chooseWeighted(group, reward => reward.rarity, roll())
        if (entry === undefined) continue
        const reward = resolveReward(entry, quest.element ?? 0, boosted)
        rewards.push(reward)
        rareDrops.push({ group_id: selected.id, index: entry.position, number: reward.count })
    }
    return { rewards, commonDrops, rareDrops }
}
