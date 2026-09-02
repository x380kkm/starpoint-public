// audience: internal
// # cn-assets
// 此模块读取仅供 CN 使用的规范化主数据, 并把归档内容日期映射到 CN 新实例基线.

import rawCnGachas from "../../assets/cn_gacha.json"
import { Gacha, Gachas } from "./types"

export const CN_BASELINE_GACHA_ID = 1
export const CN_CONTENT_BASELINE_ISO = "2025-07-10T00:00:00.000Z"

const cnAssetStartTime = Date.parse("2017-02-05T15:00:00.000Z")
const cnContentBaselineTime = Date.parse(CN_CONTENT_BASELINE_ISO)

function rebaseCnDate(value: string): string {
    const sourceTime = Date.parse(value.replace(" ", "T") + "Z")
    if (!Number.isFinite(sourceTime)) return value
    return new Date(sourceTime - cnAssetStartTime + cnContentBaselineTime)
        .toISOString()
        .replace("T", " ")
        .replace(".000Z", "")
}

const cnGachas = Object.fromEntries(
    Object.entries(rawCnGachas as Gachas).map(([id, gacha]) => [id, {
        ...gacha,
        startDate: rebaseCnDate(gacha.startDate),
        endDate: rebaseCnDate(gacha.endDate),
    }]),
) as Gachas

// //// 读取 CN 扭蛋数据并映射内容日期 [@x380kkm 2026-07-24] ////
export function getCnGachaSync(id: string | number): Gacha | null {
    return cnGachas[String(id)] ?? null
}
// //// /读取 CN 扭蛋数据并映射内容日期 ////
