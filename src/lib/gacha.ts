// audience: internal
// # gacha
// 此模块按扭蛋配置抽取并发放角色, 装备和箱式奖励.

import { randomInt } from "crypto";
import { existsSync, readFileSync } from "fs";
import { join } from "path";
import { PlayerBoxGachaDrawnReward } from "../data/types";
import { getCharacterDataSync } from "./assets";
import { givePlayerCharacterSync } from "./character";
import { givePlayerEquipmentSync } from "./equipment";
import { givePlayerRewardsSync } from "./quest";
import { BoxGachaBox, BoxGachaDrawResult, BoxGachaIdReward, BoxGachaRewardTier, BoxGachaRewardType, CharacterGacha, CharacterReward, CurrencyReward, EquipmentItemReward, Gacha, GachaCharacterDraw, GachaDrawResult, GachaDraws, GachaMovieSeeds, GachaMovieType, GachaType, PlayerRewardResult, Reward, RewardPlayerGachaDrawResult, RewardType } from "./types";

// //// 按演出标识读取种子池 [@x380kkm 2026-08-22] ////
const assetsDirectory = join(__dirname, "..", "..", "assets")
const movieSeedsByPath = new Map<string, GachaMovieSeeds>()

function loadMovieSeeds(movieId: string): GachaMovieSeeds {
    const moviePath = join(assetsDirectory, `gacha_movie_seeds_${movieId}.json`)
    const fallbackPath = join(assetsDirectory, "gacha_movie_seeds.json")
    const seedPath = existsSync(moviePath) ? moviePath : fallbackPath
    if (!existsSync(seedPath)) return {}

    const cachedSeeds = movieSeedsByPath.get(seedPath)
    if (cachedSeeds !== undefined) return cachedSeeds

    const seeds = JSON.parse(readFileSync(seedPath, "utf8")) as GachaMovieSeeds
    movieSeedsByPath.set(seedPath, seeds)
    return seeds
}
// //// /按演出标识读取种子池 ////

const characterGachaRankRates = {
    normal: [
        75, // 5*
        250,  // 4*
        675 // 3*
    ],
    multiGuarantee: [
        75, // 5*
        925 // 4*
    ]
}
const rateUpCharacterGachaRates = {
    normal: [
        50, // 5*
        250, // 4*,
        700, // 3*
    ],
    multiGuarantee: [
        50, // 5*
        950 // 4*
    ],
}

const equipmentGachaRankRates = {
    normal: [
        50,  // 5*
        250, // 4*
        700  // 3*
    ],
    multiGuarantee: [
        50, // 5*
        950 // 4*
    ]
}

const rankMovieRates = [
    [ // 5*
        80,
        20
    ],
    [ // 4*
        80,
        20
    ],
    [
        100
    ]
]

export interface GachaResult {
    characterId: number,
    movieId: string,
    seed: number,
    entryCount: number
}

export interface SummonResult {
    freeVmoney: number,
    vmoney: number,
    pulls: GachaResult[],
}

// //// 按闭区间随机点选择权重项 [@x380kkm 2026-07-24] ////
export function selectWeightedPoolIndex(roll: number, pool: number[]): number | null {
    if (!Number.isInteger(roll) || roll < 1) return null
    let offset = 0;
    let index = 0
    for (const rate of pool) {
        if ((rate + offset) >= roll) return index;
        offset += rate;
        index += 1;
    }
    return null;
}

export function randomPoolItem(pool: number[]): number | null {
    const decimalPlaces = pool.reduce((maximum, rate) => {
        const text = String(rate)
        const point = text.indexOf(".")
        return Math.max(maximum, point === -1 ? 0 : Math.min(text.length - point - 1, 6))
    }, 0)
    const scale = 10 ** decimalPlaces
    const weights = pool.map((rate) => Math.max(0, Math.round(rate * scale)))
    const total = weights.reduce((sum, rate) => sum + rate, 0)
    if (total < 1) return null
    return selectWeightedPoolIndex(randomInt(1, total + 1), weights)
}
// //// /按闭区间随机点选择权重项 ////

// //// 验证兑换物属于当前扭蛋池 [@x380kkm 2026-07-24] ////
export function isGachaPoolItem(gacha: Gacha, itemId: number): boolean {
    if (!Number.isSafeInteger(itemId) || itemId <= 0) return false
    return Object.values(gacha.pool).some((pool) => pool.some((item) => item.id === itemId))
}
// //// /验证兑换物属于当前扭蛋池 ////

// //// 按扭蛋配置抽取角色或装备 [@x380kkm 2026-07-23] ////
export function drawGachaSync(
    gacha: Gacha,
    drawAmount: number
): GachaDrawResult {
    const isCharacterGacha = gacha.type === GachaType.CHARACTER
    const isRateUp = isCharacterGacha ? (gacha as CharacterGacha).movieName !== "normal" : false
    const rankRates = gacha.rankRates ?? (isCharacterGacha ? (isRateUp ? rateUpCharacterGachaRates : characterGachaRankRates) : equipmentGachaRankRates)

    const pulls: Map<number, number> = new Map()

    for (let drawNumber = 0; drawNumber < drawAmount; drawNumber++) {
        const drawRankRates = (drawNumber !== 0) && ((drawNumber % 9) === 0) ? rankRates.multiGuarantee : rankRates.normal
        
        const ratePool = gacha.pool[(randomPoolItem(drawRankRates) ?? 0) + 1]

        // pick item from pool
        const selectedItem = ratePool[randomPoolItem(ratePool.map(item => item.rarity)) ?? 0]
        const selectedItemId = selectedItem.id

        pulls.set(selectedItemId, (pulls.get(selectedItemId) ?? 0) + 1)
    }

    return pulls
}
// //// /按扭蛋配置抽取角色或装备 ////

export function rewardPlayerGachaDrawResultSync(
    playerId: number,
    gacha: Gacha,
    gachaDrawResult: GachaDrawResult
): RewardPlayerGachaDrawResult {

    const draws: GachaDraws = []
    const characters: Map<number, Object> = new Map()
    const equipment: Map<number, Object> = new Map()
    const items: Map<number, number> = new Map()

    if (gacha.type == GachaType.CHARACTER) {
        const characterGacha = gacha as CharacterGacha
        // reward characters
        for (const [characterId, amount] of gachaDrawResult) {
            for (let n = 0; n < amount; n++) {
                const giveResult = givePlayerCharacterSync(playerId, characterId)
                
                if (giveResult !== null) {
                    const rarity = getCharacterDataSync(characterId)?.rarity ?? 3
                    const rarityIndex = 5 - rarity
                    const movieRates = rankMovieRates[rarityIndex] ?? rankMovieRates[rankMovieRates.length - 1]
                    const movieType = randomPoolItem(movieRates) ?? GachaMovieType.NORMAL
                    const movieId = movieType === GachaMovieType.GUARANTEE
                        ? (characterGacha.guaranteeMovieName || characterGacha.movieName || "normal")
                        : (characterGacha.movieName || "normal")

                    const seedKey = String(6 - rarity)
                    const movieSeeds = loadMovieSeeds(movieId)
                    const typedSeeds = movieSeeds[seedKey]?.[String(movieType)] ?? []
                    const normalSeeds = movieSeeds[seedKey]?.[String(GachaMovieType.NORMAL)] ?? []
                    const seeds = typedSeeds.length > 0 ? typedSeeds : normalSeeds
                    const seed = seeds.length > 0
                        ? seeds[randomInt(0, seeds.length)]
                        : characterId * 1000

                    // build draw
                    const draw: GachaCharacterDraw = {
                        "character_id": characterId,
                        "movie_id": movieId,
                        "seed": seed,
                        "entry_count": 1
                    }
                    
                    // set values in items map, characters map, and draws array.
                    const giveItem = giveResult.item
                    if (giveItem !== undefined) {
                        draw['ex_boost_item'] = giveItem // add ex_boost_item to draw
                        items.set(giveItem.id, (items.get(giveItem.id) ?? 0) + giveItem.count)
                    }

                    const existingCharacter = characters.get(characterId)
                    if (existingCharacter) {
                        characters.set(characterId, {...existingCharacter, ...giveResult.character})
                    } else {    
                        characters.set(characterId, giveResult.character)
                    }
                    draws.push(draw)
                }

            }
        }
    } else {
        for (const [equipmentId, amount] of gachaDrawResult) {
            const giveResult = givePlayerEquipmentSync(playerId, equipmentId, amount);

            equipment.set(equipmentId, giveResult)
            for (let i = 0; i < amount; i++) {
                draws.push({
                    "equipment_id": equipmentId,
                    "treasure_up_type": 0    
                })
            }
        }
    }
    
    const returnCharacters: Object[] = [];
    for (const value of characters.values()) {
        returnCharacters.push(value)
    }

    const returnEquipment: Object[] = []
    for (const value of equipment.values()) {
        returnEquipment.push(value)
    }
    
    const returnItems: Record<number, number> = {}
    for (const [itemId, amount] of items) {
        returnItems[itemId] = amount
    }

    return {
        draw: draws,
        characters: returnCharacters,
        equipment: returnEquipment,
        items: returnItems
    }
}

/**
 * Performs box gacha draws.
 * 
 * @param rewards A record, where the key is the reward id and the value is a BoxGachaReward
 * @param drawnRewards The current draws the player has made on the box gacha.
 * @param drawAmount The number of draws to perform.
 */
export function drawBoxGachaSync(
    rewards: BoxGachaBox,
    drawnRewards: PlayerBoxGachaDrawnReward[],
    drawAmount: number, // the number of times to draw
    stopOnFeaturedReward: boolean = false
): BoxGachaDrawResult {
    // build drawn reward map
    const drawnRewardsMap = new Map(drawnRewards.map(reward => [reward.id, reward.number]))

    const rewardsPool: string[] = []
    for (const [rewardId, reward] of Object.entries(rewards)) {
        for (let i = 0; i < (reward.available - (drawnRewardsMap.get(Number(rewardId)) ?? 0)); i++) {
            rewardsPool.push(rewardId)
        }
    }

    let drawnMana = 0
    let drawnExp = 0
    const drawnCharacters: Map<number, number> = new Map()
    const drawnEquipment: Map<number, number> = new Map()
    const drawnItems: Map<number, number> = new Map()
    const sessionDrawnRewards: Map<string, number> = new Map()

    let totalDraws = 0

    for (let n = 0; n < drawAmount && rewardsPool.length > 0; n++) {
        const rollIndex = randomInt(rewardsPool.length)
        const rewardId = rewardsPool[rollIndex]
        const reward = rewards[rewardId]

        switch (reward.type) {
            case BoxGachaRewardType.ITEM: {
                const itemId = (reward as BoxGachaIdReward).id
                drawnItems.set(itemId, (drawnItems.get(itemId) ?? 0) + reward.count)
                break;
            }
            case BoxGachaRewardType.EQUIPMENT: {
                const equipmentId = (reward as BoxGachaIdReward).id
                drawnEquipment.set(equipmentId, (drawnEquipment.get(equipmentId) ?? 0) + reward.count)
                break;
            }
            case BoxGachaRewardType.MANA: {
                drawnMana += reward.count
                break;
            }
            case BoxGachaRewardType.EXP: {
                drawnExp += reward.count
                break;
            }
            case BoxGachaRewardType.CHARACTER: {
                const characterId = (reward as BoxGachaIdReward).id
                drawnCharacters.set(characterId, (drawnCharacters.get(characterId) ?? 0) + reward.count)
                break;
            }
        }
        
        sessionDrawnRewards.set(rewardId, (sessionDrawnRewards.get(rewardId) ?? 0) + 1)
        rewardsPool.splice(rollIndex, 1)
        totalDraws += 1

        // break if the reward was featured & stop of featured is enabled
        if (reward.tier == BoxGachaRewardTier.FEATURED && stopOnFeaturedReward) break;
    }

    // return the draw result
    const returnSessionDrawnRewards: PlayerBoxGachaDrawnReward[] = []

    sessionDrawnRewards.forEach((value, rewardId) => {
        returnSessionDrawnRewards.push({
            id: Number(rewardId),
            number: value
        })
    })

    return {
        mana: drawnMana,
        exp: drawnExp,
        characters: drawnCharacters,
        equipment: drawnEquipment,
        items: drawnItems,
        rewards: returnSessionDrawnRewards
    }
}

/**
 * Rewards a player with the results of a box gacha draw.
 * 
 * @param playerId The ID of the player.
 * @param drawResult The box gacha draw result.
 * @returns A PlayerRewardResult.
 */
export function rewardPlayerBoxGachaResultSync(
    playerId: number,
    drawResult: BoxGachaDrawResult
): PlayerRewardResult | null {
    const rewards: Reward[] = []

    // convert draw results into rewards

    // items
    for (const [itemId, number] of drawResult.items) {
        rewards.push({
            name: '',
            type: RewardType.ITEM,
            id: itemId,
            count: number
        } as EquipmentItemReward)
    }

    // equipment
    for (const [equipmentId, number] of drawResult.equipment) {
        rewards.push({
            name: '',
            type: RewardType.EQUIPMENT,
            id: equipmentId,
            count: number
        } as EquipmentItemReward)
    }

    // characters
    for (const [characterId, number] of drawResult.characters) {
        for (let i = 0; i < number; i++) {
            rewards.push({
                name: '',
                type: RewardType.CHARACTER,
                id: characterId,
            } as CharacterReward)
        }
    }

    // mana & exp
    rewards.push({
        name: '',
        type: RewardType.EXP,
        count: drawResult.exp,
    } as CurrencyReward)
    rewards.push({
        name: '',
        type: RewardType.MANA,
        count: drawResult.mana,
    } as CurrencyReward)

    return givePlayerRewardsSync(playerId, rewards)
}
