// audience: internal
// # battle-settlement-state
//
// 此模块保存进程内待结算战斗, 并为存档槽切换提供一致的活动状态查询.
// 状态以玩家 ID 为边界, 服务重启后不恢复未完成战斗.

import { QuestCategory } from "../lib/types"

export interface ActiveQuest {
    questId: number
    playId: string
    category: QuestCategory
    useBossBoostPoint: boolean
    useBoostPoint: boolean
    isAutoStartMode: boolean
}

const activeQuests = new Map<number, ActiveQuest>()

// //// 管理玩家的一次性战斗结算状态 [@x380kkm 2026-07-27] ////
export function insertActiveQuest(playerId: number, quest: ActiveQuest): void {
    activeQuests.set(playerId, quest)
}

export function getActiveQuest(playerId: number): ActiveQuest | null {
    return activeQuests.get(playerId) ?? null
}

export function hasActiveQuest(playerId: number): boolean {
    return activeQuests.has(playerId)
}

export function deleteActiveQuest(playerId: number): void {
    activeQuests.delete(playerId)
}
// //// /管理玩家的一次性战斗结算状态 ////
