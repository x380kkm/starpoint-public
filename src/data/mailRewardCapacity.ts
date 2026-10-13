// audience: internal
// # mail-reward-capacity
// 邮件按每项资产的剩余空间拆分到账与余量, 免费和付费货币共用持有上限.
// 角色与装备数量包含已持有的本体, 可溢出道具沿用客户端允许的整数容量.

import capacities from "../../core/personal-service/assets/cn-shop-capacities.json"
import { getCharacterDataSync } from "../lib/assets"
import type { PlayerMailReward } from "./playerMails"
import { getPlayerCharacterSync, getPlayerEquipmentSync, getPlayerItemSync, getPlayerSync } from "./wdfpData"

const MAX_CHARACTER_STACK = 99

// //// 创建零数量的邮件奖励集合 [@x380kkm 2026-09-28] ////
export function emptyMailRewards(): PlayerMailReward {
    return { itemList: {}, equipmentList: {}, characterList: [], freeMana: 0, paidMana: 0, freeVmoney: 0, vmoney: 0, expPool: 0 }
}

// //// 检查邮件中是否仍有奖励数量 [@x380kkm 2026-09-28] ////
export function hasMailRewards(rewards: PlayerMailReward): boolean {
    return Object.keys(rewards.itemList).length > 0 || Object.keys(rewards.equipmentList).length > 0
        || rewards.characterList.length > 0 || rewards.freeMana > 0 || rewards.paidMana > 0
        || rewards.freeVmoney > 0 || rewards.vmoney > 0 || rewards.expPool > 0
}

// //// 读取邮件道具的有效持有容量 [@x380kkm 2026-09-28] ////
export function mailItemCapacity(id: number | string): number {
    const items: Record<string, number> = capacities.items
    const capacity = capacities.overflow_receivable_items.includes(Number(id)) ? 2147483647 : items[id]
    if (capacity === undefined) throw new Error(`Missing item capacity: ${id}`)
    return capacity
}

// //// 拆分一种奖励的到账数量与保留数量 [@x380kkm 2026-09-28] ////
function splitAmount(amount: number, held: number, limit: number): [number, number] {
    const credited = Math.min(amount, Math.max(0, limit - held))
    return [credited, amount - credited]
}

// //// 按各类资产空间分配邮件中的原始奖励 [@x380kkm 2026-09-28] ////
export function splitMailRewardsSync(playerId: number, rewards: PlayerMailReward) {
    const player = getPlayerSync(playerId)
    if (player === null) throw new Error("player not found.")
    const credited = emptyMailRewards()
    const remaining = emptyMailRewards()
    const mana = player.freeMana + player.paidMana
    const money = player.freeVmoney + player.vmoney
    ;[credited.freeMana, remaining.freeMana] = splitAmount(rewards.freeMana, mana, capacities.mana)
    ;[credited.paidMana, remaining.paidMana] = splitAmount(rewards.paidMana, mana + credited.freeMana, capacities.mana)
    ;[credited.freeVmoney, remaining.freeVmoney] = splitAmount(rewards.freeVmoney, money, capacities.virtual_money)
    ;[credited.vmoney, remaining.vmoney] = splitAmount(rewards.vmoney, money + credited.freeVmoney, capacities.virtual_money)
    ;[credited.expPool, remaining.expPool] = splitAmount(rewards.expPool, player.expPool, capacities.exp_pool)
    for (const [id, count] of Object.entries(rewards.itemList)) {
        const [received, rest] = splitAmount(count, getPlayerItemSync(playerId, id) ?? 0, mailItemCapacity(id))
        if (received > 0) credited.itemList[id] = received
        if (rest > 0) remaining.itemList[id] = rest
    }
    for (const [id, count] of Object.entries(rewards.equipmentList)) {
        const owned = getPlayerEquipmentSync(playerId, Number(id))
        const [received, rest] = splitAmount(count, owned === null ? 0 : owned.stack + 1, capacities.equipment_stack + 1)
        if (received > 0) credited.equipmentList[id] = received
        if (rest > 0) remaining.equipmentList[id] = rest
    }
    const counts = new Map<number, number>()
    for (const id of rewards.characterList) {
        if (getCharacterDataSync(id) === null) throw new Error(`character ${id} does not exist.`)
        if (!counts.has(id)) {
            const owned = getPlayerCharacterSync(playerId, id)
            counts.set(id, owned === null ? 0 : owned.stack + 1)
        }
        const held = counts.get(id)!
        if (held <= MAX_CHARACTER_STACK) {
            credited.characterList.push(id)
            counts.set(id, held + 1)
        } else {
            remaining.characterList.push(id)
        }
    }
    return { credited, remaining }
}
