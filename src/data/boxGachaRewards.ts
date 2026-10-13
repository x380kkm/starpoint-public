// audience: internal
// # box-gacha-rewards
// 箱池普通奖励按当前容量入库, 同次抽取的余量共同保存为邮件.

import capacities from "../../core/personal-service/assets/cn-shop-capacities.json"
import { createPlayerMailSync } from "./playerMails"
import { getPlayerItemSync, getPlayerSync, givePlayerItemSync, runPlayerDataTransactionSync, updatePlayerSync } from "./wdfpData"
import { givePlayerRewardsSync } from "../lib/quest"
import { BoxGachaDrawResult, CharacterReward, EquipmentItemReward, PlayerRewardResult, Reward, RewardType } from "../lib/types"

export interface BoxGachaRewardResult extends PlayerRewardResult {
    over_max: object[]
    mail_arrived: boolean
}

// //// 发放箱池角色和装备并按容量分配普通奖励 [@x380kkm 2026-09-28] ////
export function rewardPlayerBoxGachaResultSync(playerId: number, draw: BoxGachaDrawResult): BoxGachaRewardResult | null {
    return runPlayerDataTransactionSync(() => {
        if (getPlayerSync(playerId) === null) return null
        const rewards: Reward[] = []
        for (const [id, count] of draw.equipment) rewards.push({ name: "", type: RewardType.EQUIPMENT, id, count } as EquipmentItemReward)
        for (const [id, count] of draw.characters) {
            for (let index = 0; index < count; index++) rewards.push({ name: "", type: RewardType.CHARACTER, id } as CharacterReward)
        }
        const result = givePlayerRewardsSync(playerId, rewards)
        const player = getPlayerSync(playerId)
        if (result === null || player === null) throw new Error("Box reward player is missing")
        const itemCapacities: Record<string, number> = capacities.items
        const mailedItems: Record<string, number> = {}
        const overMax: object[] = []
        for (const [id, amount] of draw.items) {
            const capacity = itemCapacities[id]
            if (capacity === undefined) throw new Error(`Missing item capacity: ${id}`)
            const held = getPlayerItemSync(playerId, id) ?? 0
            const credited = Math.min(amount, Math.max(0, capacity - held))
            result.items[id] = givePlayerItemSync(playerId, id, credited)
            if (credited < amount) {
                mailedItems[id] = amount - credited
                overMax.push({ process_type: 1, item: { item_id: id, number: amount - credited } })
            }
        }
        const creditedMana = Math.min(draw.mana, Math.max(0, capacities.mana - player.freeMana - player.paidMana))
        const creditedExp = Math.min(draw.exp, Math.max(0, capacities.exp_pool - player.expPool))
        updatePlayerSync({ id: playerId, freeMana: player.freeMana + creditedMana, expPool: player.expPool + creditedExp })
        result.user_info.free_mana = creditedMana
        result.user_info.exp_pool = creditedExp
        const mailedMana = draw.mana - creditedMana
        const mailedExp = draw.exp - creditedExp
        if (mailedMana > 0) overMax.push({ process_type: 1, free_mana: mailedMana })
        if (mailedExp > 0) overMax.push({ process_type: 1, pooled_exp: mailedExp })
        const mailArrived = overMax.length > 0
        if (mailArrived) createPlayerMailSync({
            playerId, title: "持有上限奖励", body: "超出持有上限的箱式扭蛋奖励存入邮件.", sender: "系统",
            rewards: { itemList: mailedItems, freeMana: mailedMana, expPool: mailedExp },
        })
        return { ...result, over_max: overMax, mail_arrived: mailArrived }
    })
}
