// audience: internal
// # gacha-campaign
// 活动选择与载入共享同一份活动次数投影.
// 玩家次数以扭蛋和活动组成的键保存.
// 活动时间使用 Unix 秒, 起止时刻均属于活动窗口.

import { UserGachaCampaign } from "../data/types"
import { GachaCampaign, getGachaCampaignListSync } from "./assets"

// //// 读取当前时间内的扭蛋活动 [@x380kkm 2026-09-28] ////
function getActiveGachaCampaigns(serverTime: number): GachaCampaign[] {
    return getGachaCampaignListSync().filter(campaign =>
        campaign.startTime <= serverTime && serverTime <= campaign.endTime,
    )
}

// //// 按载入顺序选择仍有次数的扭蛋活动 [@x380kkm 2026-09-28] ////
export function findAvailableGachaCampaign(
    gachaId: number,
    kind: GachaCampaign["kind"],
    activeCampaigns: readonly UserGachaCampaign[],
): GachaCampaign | null {
    const campaigns = new Map(getGachaCampaignListSync().map(campaign => [campaign.campaignId, campaign]))
    const availableIds = new Set(activeCampaigns.filter(campaign => campaign.count > 0)
        .map(campaign => campaign.campaign_id))
    for (const campaignId of availableIds) {
        const campaign = campaigns.get(campaignId)
        if (campaign?.kind === kind && campaign.gachaIds.includes(gachaId)) return campaign
    }
    return null
}

// //// 投影有效活动并保留每个扭蛋的活动次数 [@x380kkm 2026-09-28] ////
export function projectActiveGachaCampaigns(
    storedCampaigns: readonly UserGachaCampaign[],
    serverTime: number,
    isGachaAvailable: (gachaId: number) => boolean,
): UserGachaCampaign[] {
    const counts = new Map(storedCampaigns.map(campaign =>
        [`${campaign.gacha_id}:${campaign.campaign_id}`, campaign.count],
    ))
    return getActiveGachaCampaigns(serverTime).flatMap(campaign =>
        campaign.gachaIds.filter(isGachaAvailable).map(gachaId => ({
            campaign_id: campaign.campaignId,
            gacha_id: gachaId,
            count: counts.get(`${gachaId}:${campaign.campaignId}`) ?? 1,
        })),
    )
}
