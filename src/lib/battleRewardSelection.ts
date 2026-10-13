// audience: internal
// # battle-reward-selection
// 多阶段战斗取已完成的最高奖励档, 剧情败北战按主表获得通关资格, 计分活动独立结算成绩.

import source from "../../core/personal-service/assets/cn-battle-reward-rules.json"
import { BattleQuest, QuestCategory, ScoreReward } from "./types"

interface PhaseReward {
    phase: number
    rank_point_reward: number
    character_exp_reward: number
    mana_reward: number
    pool_exp_reward: number
    score_reward_group_id: number | null
    score_reward_count: number
    score_rewards: ScoreReward[]
}

export interface BattleRewardOutcome {
    accomplished: boolean
    lost: boolean
    clearedPhases: number
    fieldMana: number
}

const rules = source as unknown as { lose_quests: string[], phase_quests: Record<string, { phases: PhaseReward[] }> }

// //// 按战斗结果选择基础奖励和独立追加奖励资格 [@x380kkm 2026-09-28] ////
export function selectBattleRewards(quest: BattleQuest, category: QuestCategory, questId: number, outcome: BattleRewardOutcome) {
    const key = `${category}:${questId}`
    const scriptedLoss = outcome.lost && rules.lose_quests.includes(key)
    const phases = rules.phase_quests[key]?.phases
    const phase = phases?.filter(entry => outcome.accomplished || entry.phase <= outcome.clearedPhases)
        .reduce<PhaseReward | undefined>((highest, entry) => !highest || entry.phase > highest.phase ? entry : highest, undefined)
    const receivesRewards = phases ? phase !== undefined : outcome.accomplished || scriptedLoss || category === QuestCategory.SCORE_ATTACK_EVENT
    const fieldMana = phases || receivesRewards ? outcome.fieldMana : 0
    if (!phases && receivesRewards) return { quest, receivesRewards, scriptedLoss, fieldMana }
    return {
        quest: {
            ...quest,
            rankPointReward: phase?.rank_point_reward ?? 0,
            characterExpReward: phase?.character_exp_reward ?? 0,
            manaReward: phase?.mana_reward ?? 0,
            poolExpReward: phase?.pool_exp_reward ?? 0,
            scoreRewardGroupId: phase?.score_reward_group_id ?? undefined,
            scoreRewardGroup: phase?.score_rewards ?? [],
            scoreRewardCounts: Array(5).fill(phase?.score_reward_count ?? 0),
        },
        receivesRewards,
        scriptedLoss,
        fieldMana,
    }
}
