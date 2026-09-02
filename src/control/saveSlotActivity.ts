// audience: internal
// # save-slot-activity
//
// 此模块统一判断账号是否仍被房间或待结算战斗占用.
// 管理接口只在返回 null 时允许改变账号的激活存档槽.

import { matchmakingStore } from "../multiplayer/matchmakingStore"
import { hasActiveQuest } from "./battleSettlementState"

export type SaveSlotActivationBlock = "room" | "battle_or_settlement"

// //// 查询阻止账号切换存档槽的运行中状态 [@x380kkm 2026-07-27] ////
export function getSaveSlotActivationBlock(
    accountId: number,
    activePlayerId: number | null,
): SaveSlotActivationBlock | null {
    if (matchmakingStore.hasAccountParticipantInOpenRoom(accountId)) return "room"
    if (activePlayerId !== null && hasActiveQuest(activePlayerId)) return "battle_or_settlement"
    return null
}
// //// /查询阻止账号切换存档槽的运行中状态 ////
