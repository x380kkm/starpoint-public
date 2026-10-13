// audience: internal
// # cn-event-reward-master
//
// 该模块把 CN 活动 master 的 GeneralRewardKind 奖励列转换为个人服务的奖励字段.

const REWARD_KINDS = new Map([
    ["0", { kind: 1, idField: "item_id" }],
    ["1", { kind: 2, idField: "equipment_id" }],
    ["2", { kind: 0 }],
    ["3", { kind: 3 }],
    ["4", { kind: 5 }],
    ["6", { kind: 4, idField: "character_id" }],
    ["7", { kind: 6, idField: "degree_id" }],
])

// //// 读取六组活动奖励列 [@x380kkm 2026-09-21] ////
export function readEventRewards(row, startIndex) {
    const rewards = []
    for (let index = startIndex; index < startIndex + 18; index += 3) {
        const kind = row[index]
        if (kind === "(None)") continue
        const descriptor = REWARD_KINDS.get(kind)
        if (!descriptor) throw new Error(`Unsupported event reward kind: ${kind}`)
        const amount = Number(row[index + 2])
        if (!Number.isSafeInteger(amount) || amount <= 0) throw new Error("Invalid event reward amount")
        const reward = { kind: descriptor.kind, amount }
        if (descriptor.idField) {
            const id = Number(row[index + 1])
            if (!Number.isSafeInteger(id) || id <= 0) throw new Error("Invalid event reward id")
            reward[descriptor.idField] = id
        }
        rewards.push(reward)
    }
    if (rewards.length === 0) throw new Error("Event reward row is empty")
    return rewards
}
// //// /读取六组活动奖励列 ////
