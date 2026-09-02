// audience: internal
// # tutorial-data
// 此模块提供教程流程共享的角色集合和客户端版本对应的扭蛋元数据.

import { randomInt } from "crypto"
import { getCnGachaSync } from "./cnAssets"
import { drawGachaSync } from "./gacha"
import { CharacterGacha, Gacha, GachaDrawResult, GachaType } from "./types"

// //// 提供教程扭蛋元数据, 抽取和角色识别 [@x380kkm 2026-07-23] ////
export const tutorialGachaCharacterIds = [251001, 251002, 251003, 251004, 251005, 251006, 251007, 251008]
export const CN_TUTORIAL_GACHA_ID = 1704

const tutorialGachaCharacterIdSet = new Set(tutorialGachaCharacterIds)

const cnTutorialGachas: ReadonlyMap<number, CharacterGacha> = new Map([
    [CN_TUTORIAL_GACHA_ID, {
        type: GachaType.CHARACTER,
        paymentType: 0,
        singleCost: 150,
        multiCost: 1500,
        discountCost: 50,
        movieName: "fes",
        guaranteeMovieName: "fes_guarantee",
        startDate: "2025-07-03 12:00:00",
        endDate: "2025-08-14 23:59:59",
        pool: {},
    }],
])
const cnTutorialGachaSet = new Set<Gacha>(cnTutorialGachas.values())

export function getCnTutorialGachaSync(gachaId: number): Gacha | null {
    return getCnGachaSync(gachaId) ?? cnTutorialGachas.get(gachaId) ?? null
}

export function drawCnGachaSync(gacha: Gacha, drawAmount: number): GachaDrawResult {
    if (!cnTutorialGachaSet.has(gacha)) return drawGachaSync(gacha, drawAmount)

    const drawResult: GachaDrawResult = new Map()
    for (let drawNumber = 0; drawNumber < drawAmount; drawNumber += 1) {
        const characterIndex = randomInt(0, tutorialGachaCharacterIds.length)
        const characterId = tutorialGachaCharacterIds[characterIndex]
        drawResult.set(characterId, (drawResult.get(characterId) ?? 0) + 1)
    }
    return drawResult
}

export function findTutorialGachaCharacterId(characterIds: Iterable<string | number>): number | null {
    for (const characterId of characterIds) {
        const numericCharacterId = Number(characterId)
        if (tutorialGachaCharacterIdSet.has(numericCharacterId)) return numericCharacterId
    }

    return null
}
// //// /提供教程扭蛋元数据, 抽取和角色识别 ////
