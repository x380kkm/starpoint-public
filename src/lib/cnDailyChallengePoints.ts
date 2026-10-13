// audience: internal
// # cn-daily-challenge-points
// CN 客户端在日期筛选前构造每个挑战活动的 master 引用.
// 活动所属关系来自两端共用的 CN 主表目录, 同一记录的剩余次数取最大值.

import campaignParentCatalog from "../../core/personal-service/assets/cn-daily-challenge-campaigns.json"

const campaignParents: Record<string, number> = campaignParentCatalog

interface ChallengeCampaign extends Record<string, unknown> {
    campaign_id: number
    additional_point: number
}

interface ChallengePoint extends Record<string, unknown> {
    id: number
    point: number
    campaign_list: ChallengeCampaign[]
}

// //// 合并挑战次数并整理所属匹配的活动引用 [@x380kkm 2026-09-28] ////
export function normalizeCnDailyChallengePoints(value: unknown): ChallengePoint[] {
    const points = new Map<number, ChallengePoint>()
    for (const entry of Array.isArray(value) ? value : []) {
        if (!isRecord(entry) || !Number.isSafeInteger(entry.id) || Number(entry.id) <= 0) continue
        const id = Number(entry.id)
        const point = remainingPoints(entry.point)
        const campaigns = normalizeCampaigns(id, entry.campaign_list)
        const current = points.get(id)
        if (current === undefined) {
            points.set(id, { ...entry, id, point, campaign_list: campaigns })
        } else {
            current.point = Math.max(current.point, point)
            current.campaign_list = normalizeCampaigns(id, [...current.campaign_list, ...campaigns])
        }
    }
    return [...points.values()]
}

// //// 合并 master 所属匹配的活动和剩余加成次数 [@x380kkm 2026-09-28] ////
function normalizeCampaigns(pointId: number, value: unknown): ChallengeCampaign[] {
    const campaigns = new Map<number, ChallengeCampaign>()
    for (const entry of Array.isArray(value) ? value : []) {
        if (!isRecord(entry) || !Number.isSafeInteger(entry.campaign_id)) continue
        const id = Number(entry.campaign_id)
        if (campaignParents[id] !== pointId) continue
        const additional = remainingPoints(entry.additional_point)
        const current = campaigns.get(id)
        if (current === undefined) {
            campaigns.set(id, { ...entry, campaign_id: id, additional_point: additional })
        } else {
            current.additional_point = Math.max(current.additional_point, additional)
        }
    }
    return [...campaigns.values()].sort((left, right) => left.campaign_id - right.campaign_id)
}

// //// 读取非负整数次数 [@x380kkm 2026-09-28] ////
function remainingPoints(value: unknown): number {
    return Number.isSafeInteger(value) ? Math.max(0, Number(value)) : 0
}

// //// 判断挑战数据对象 [@x380kkm 2026-09-28] ////
function isRecord(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === "object" && !Array.isArray(value)
}
