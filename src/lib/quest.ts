// audience: internal
// # quest-rewards
// 战斗掉落沿用共用主表中的评级次数, 概率和位置, 库存响应记录各物品发放后的总量.

import { getPlayerCharacterSync, getPlayerItemSync, getPlayerSingleQuestProgressSync, getPlayerSync, givePlayerItemSync, updatePlayerSync } from "../data/wdfpData";
import { ActiveQuest, insertActiveQuest } from "../control/battleSettlementState";
import { getServerTime } from "../utils";
import { getAdditionalQuestEvents } from "./additionalQuestRewards";
import { givePlayerCharacterSync } from "./character";
import { givePlayerEquipmentSync } from "./equipment";
import { drawQuestDrops, randomFraction } from "./questDrops";
import { BattleQuest, CharacterReward, CurrencyReward, EquipmentItemReward, GivePlayerScoreRewardsResult, PlayerRewardResult, Reward, RewardType } from "./types";

// //// 开战时保存限时追加奖励资格 [@x380kkm 2026-09-28] ////
export function startPlayerQuest(playerId: number, quest: ActiveQuest): void {
    const startedAt = quest.startedAt ?? getServerTime()
    const additionalEventIds = getAdditionalQuestEvents(startedAt,
        (category, questId) => getPlayerSingleQuestProgressSync(playerId, category, questId)?.finished === true)
    insertActiveQuest(playerId, { ...quest, startedAt, additionalEventIds })
}

// //// 发放按关卡评级抽取的普通和稀有掉落 [@x380kkm 2026-09-28] ////
export function givePlayerScoreRewardsSync(
    playerId: number,
    quest: BattleQuest,
    clearRank: number,
    boostPointUsed: boolean,
    isMulti = false,
): GivePlayerScoreRewardsResult {
    const drops = drawQuestDrops(quest, clearRank, boostPointUsed, randomFraction, isMulti)
    const rewards = givePlayerRewardsSync(playerId, drops.rewards)
    if (rewards === null) throw new Error("Player for battle rewards is missing")
    return {
        ...rewards,
        drop_score_reward_ids: drops.commonDrops,
        drop_rare_reward_ids: drops.rareDrops,
    }
}

/**
 * Batch gives a specific player data an array of rewards.
 * 
 * @param playerId The ID of the player to reward.
 * @param rewards The array of rewards to give.
 * @returns A PlayerRewardResult.
 */
export function givePlayerRewardsSync(
    playerId: number,
    rewards: Reward[]
): PlayerRewardResult | null {
    let mana = 0
    let vmoney = 0
    let expPool = 0
    let joinedCharacterIdList: number[] = []
    let characters: Map<number, Object> = new Map()
    let equipment: Map<number, Object> = new Map()
    let items: Map<number, number> = new Map()

    for (const reward of rewards) {
        switch (reward.type) {
            case RewardType.ITEM: {
                const convertedReward = (reward as EquipmentItemReward)
                const itemId = convertedReward.id
                const result = givePlayerItemSync(playerId, itemId, convertedReward.count);
                items.set(itemId, result)
                break;
            }
            case RewardType.EQUIPMENT: {
                const convertedReward = (reward as EquipmentItemReward)
                const equipmentId = convertedReward.id
                const result = givePlayerEquipmentSync(playerId, equipmentId, convertedReward.count)
                equipment.set(equipmentId, result)
                break;
            }
            case RewardType.CHARACTER: {
                const characterId = (reward as CharacterReward).id
                const wasOwned = getPlayerCharacterSync(playerId, characterId) !== null
                const giveResult = givePlayerCharacterSync(playerId, characterId)

                const giveItem = giveResult?.item
                if (giveItem !== undefined) {
                    const itemId = giveItem.id
                    items.set(itemId, getPlayerItemSync(playerId, itemId) ?? 0)
                }
    
                const giveCharacter = giveResult?.character
                if (giveCharacter !== undefined) {
                    characters.set(characterId, giveCharacter)
                    if (!wasOwned) joinedCharacterIdList.push(characterId)
                }
                break;
            }
            case RewardType.BEADS: {
                vmoney += (reward as CurrencyReward).count
                break;
            }
            case RewardType.MANA: {
                mana += (reward as CurrencyReward).count
                break;
            }
            case RewardType.EXP: {
                expPool += (reward as CurrencyReward).count
                break;
            }
        }
    }

    if (mana > 0 || vmoney > 0 || expPool > 0) {
        // get player
        const player = getPlayerSync(playerId)
        if (player === null) return null;

        updatePlayerSync({
            id: playerId,
            freeVmoney: player.freeVmoney + vmoney,
            freeMana: player.freeMana + mana,
            expPool: player.expPool + expPool
        })
    }
    
    // build return values
    const characterList: Object[] = []
    const equipmentList: Object[] = []
    const itemsRecord: Record<string, number> = {}
    
    characters.forEach(character => {
        characterList.push(character)
    })

    equipment.forEach(equipment => {
        equipmentList.push(equipment)
    })

    items.forEach((number, id) => {
        itemsRecord[id] = number
    })

    return {   
        user_info: {
            free_mana: mana,
            free_vmoney: vmoney,
            exp_pool: expPool
        },
        character_list: characterList,
        joined_character_id_list: joinedCharacterIdList,
        equipment_list: equipmentList,
        items: itemsRecord
    }
}

/**
 * Gives a player a specific reward.
 * 
 * @param playerId The ID of the player.
 * @param reward The reward to give.
 * @returns A PlayerRewardResult.
 */
export function givePlayerRewardSync(
    playerId: number,
    reward: Reward
): PlayerRewardResult | null {
    return givePlayerRewardsSync(playerId, [reward])
}
