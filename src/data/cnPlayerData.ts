// audience: internal
// # cn-player-data
// CN 载入在同一 SQLite 事务中整理挑战活动引用并恢复周期奖励次数.
// 可移植来源和基线分别保留各自的挑战次数与扩展字段.

import { isDeepStrictEqual } from "node:util"
import { normalizeCnDailyChallengePoints } from "../lib/cnDailyChallengePoints"
import { ClientPlayerData } from "./types"
import { refreshPlayerPeriodicRewardsSync } from "./periodicBattleRewards"
import { getClientSerializedData, SerializePlayerDataOptions } from "./utils"
import { replacePlayerDailyChallengePointListSync, runPlayerDataTransactionSync } from "./wdfpData"
import {
    getPortablePlayerSnapshotSync,
    PortablePlayerSnapshot,
    setPortablePlayerSnapshotSync,
} from "./portablePlayerSnapshots"

// //// 保存 CN 挑战引用并序列化客户端数据 [@x380kkm 2026-09-28] ////
export function normalizeAndSerializeCnPlayerDataSync(
    playerId: number,
    options: SerializePlayerDataOptions,
): ClientPlayerData | null {
    return runPlayerDataTransactionSync(() => {
        const data = getClientSerializedData(playerId, options)
        if (data === null) return null
        data.user_periodic_reward_point_list = refreshPlayerPeriodicRewardsSync(playerId)
        const points = normalizeCnDailyChallengePoints(data.user_daily_challenge_point_list)
        if (!isDeepStrictEqual(points, data.user_daily_challenge_point_list)) {
            replacePlayerDailyChallengePointListSync(playerId, points.map(entry => ({
                id: entry.id,
                point: entry.point,
                campaignList: entry.campaign_list.map(campaign => ({
                    campaignId: campaign.campaign_id,
                    additionalPoint: campaign.additional_point,
                })),
            })))
            data.user_daily_challenge_point_list = points
        }
        const snapshot = getPortablePlayerSnapshotSync(playerId)
        if (snapshot !== null && normalizeCnSnapshotChallenges(snapshot)) {
            setPortablePlayerSnapshotSync(playerId, snapshot)
        }
        return data
    })
}

// //// 整理可移植来源和基线中的 CN 挑战引用 [@x380kkm 2026-09-28] ////
function normalizeCnSnapshotChallenges(snapshot: PortablePlayerSnapshot): boolean {
    let changed = false
    for (const document of [snapshot.source, snapshot.baseline]) {
        if (!("user_daily_challenge_point_list" in document)) continue
        const current = document.user_daily_challenge_point_list
        const normalized = normalizeCnDailyChallengePoints(current)
        if (!isDeepStrictEqual(current, normalized)) {
            document.user_daily_challenge_point_list = normalized
            changed = true
        }
    }
    return changed
}
