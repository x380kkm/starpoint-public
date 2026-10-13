// audience: internal
// # periodic-battle-rewards
// 周期奖励按开战时的活动区间抽取, 每次抽中奖励消耗一个次数.
// 次数与 UTC+8 每日恢复检查点在同一事务保存, 检查点随可移植存档迁移.

import rewardSource from "../../core/personal-service/assets/cn-periodic-battle-rewards.json"
import { chooseWeighted, randomFraction } from "../lib/questDrops"
import { givePlayerRewardsSync } from "../lib/quest"
import { DropScoreRewardId, PlayerRewardResult, Reward } from "../lib/types"
import { getServerTime } from "../utils"
import getDatabase, { Database } from "."
import { getPortablePlayerSnapshotSync, setPortablePlayerSnapshotSync } from "./portablePlayerSnapshots"
import { PlayerPeriodicRewardPoint } from "./types"
import { getPlayerPeriodicRewardPointsSync, runPlayerDataTransactionSync } from "./wdfpData"

interface PeriodicPoint {
    max_point: number
    recovery_point: number
    start_time: number
    end_time: number
}

interface PeriodicRewardCatalog {
    daily_recovery_hour: number
    points: Record<string, PeriodicPoint>
    quests: Record<string, { point_id: number | null, group_id: number, slots: number }>
    reward_groups: Record<string, { position: number, probability: number, reward: Reward & { count: number } }[]>
}

const catalog = rewardSource as PeriodicRewardCatalog
const database = getDatabase(Database.WDFP_DATA)
const recoveryStateField = "periodic_reward_recovery_state"

// //// 保存单个活动的周期次数 [@x380kkm 2026-09-28] ////
function setPeriodicRewardPoint(playerId: number, id: number, point: number): void {
    database.prepare(`
        INSERT INTO players_periodic_reward_points (player_id, id, point) VALUES (?, ?, ?)
        ON CONFLICT(player_id, id) DO UPDATE SET point = excluded.point
    `).run(playerId, id, point)
}

// //// 按游戏日界恢复活动次数并保留较晚的恢复检查点 [@x380kkm 2026-09-28] ////
export function refreshPlayerPeriodicRewardsSync(playerId: number, at = getServerTime()): PlayerPeriodicRewardPoint[] {
    return runPlayerDataTransactionSync(() => {
        const points = new Map(getPlayerPeriodicRewardPointsSync(playerId).map(point => [point.id, point.point]))
        const snapshot = getPortablePlayerSnapshotSync(playerId) ?? { source: {}, baseline: {} }
        const states = (snapshot.source[recoveryStateField] ?? {}) as Record<string, { last_recovery_time: number }>
        let changed = false
        for (const [key, definition] of Object.entries(catalog.points)) {
            const id = Number(key)
            const stored = points.get(id)
            const previous = states[key]?.last_recovery_time
            if (at >= definition.start_time && at <= definition.end_time) {
                const from = Math.max(previous ?? at, definition.start_time)
                const offset = (8 - catalog.daily_recovery_hour) * 3600
                const cycles = Math.max(0, Math.floor((at + offset) / 86400) - Math.floor((from + offset) / 86400))
                const point = stored === undefined
                    ? Math.min(definition.recovery_point, definition.max_point)
                    : cycles > 0 ? Math.min(stored + cycles * definition.recovery_point, definition.max_point) : stored
                if (point !== stored) {
                    setPeriodicRewardPoint(playerId, id, point)
                    points.set(id, point)
                }
            } else if (stored === undefined && previous === undefined) {
                continue
            }
            const checkpoint = Math.max(previous ?? at, at)
            if (previous !== checkpoint) {
                states[key] = { last_recovery_time: checkpoint }
                changed = true
            }
        }
        if (changed) {
            snapshot.source[recoveryStateField] = states
            setPortablePlayerSnapshotSync(playerId, snapshot)
        }
        return [...points].map(([id, point]) => ({ id, point }))
    })
}

export interface PeriodicBattleRewards {
    rewardResult?: PlayerRewardResult
    drops: DropScoreRewardId[]
    pointList?: PlayerPeriodicRewardPoint[]
}

// //// 按入场时间抽取周期奖励并扣除实际命中的次数 [@x380kkm 2026-09-28] ////
export function settlePlayerPeriodicRewardsSync(
    playerId: number,
    category: number,
    questId: number,
    startedAt: number,
    roll = randomFraction,
): PeriodicBattleRewards {
    return runPlayerDataTransactionSync(() => {
        const result: PeriodicBattleRewards = { drops: [] }
        const quest = catalog.quests[`${category}:${questId}`]
        if (quest === undefined || quest.point_id === null) return result
        const point = catalog.points[quest.point_id]
        if (point === undefined || startedAt < point.start_time || startedAt > point.end_time) return result
        const available = refreshPlayerPeriodicRewardsSync(playerId, startedAt)
            .find(entry => entry.id === quest.point_id)?.point ?? 0
        const group = catalog.reward_groups[quest.group_id] ?? []
        const rewards: Reward[] = []
        for (let slot = 0; slot < Math.min(quest.slots, available); slot++) {
            const candidates = group.filter(entry => entry.probability === 1 || roll() < entry.probability)
            const selected = candidates.length === 1 ? candidates[0] : chooseWeighted(candidates, entry => entry.probability, roll())
            if (selected === undefined) continue
            rewards.push({ ...selected.reward })
            result.drops.push({ group_id: quest.group_id, index: selected.position, number: selected.reward.count })
        }
        const remaining = available - rewards.length
        setPeriodicRewardPoint(playerId, quest.point_id, remaining)
        if (rewards.length > 0) {
            const granted = givePlayerRewardsSync(playerId, rewards)
            if (granted === null) throw new Error("Periodic reward player is missing")
            result.rewardResult = granted
        }
        result.pointList = [{ id: quest.point_id, point: remaining }]
        return result
    })
}
