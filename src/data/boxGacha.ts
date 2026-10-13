// audience: internal
// # box-gacha-settlement
// 代币, 箱内库存, 奖励邮件和请求回执在同一 SQLite 事务中提交.

import boxResetKinds from "../../core/personal-service/assets/cn-box-reset-kinds.json"
import getDatabase, { Database } from "."
import { getPlayerBoxGachaDrawnRewardsSync, getPlayerBoxGachaSync, getPlayerItemSync, getPlayerSync, insertPlayerBoxGachaDrawnRewardSync, insertPlayerBoxGachaSync, runPlayerDataTransactionSync, updatePlayerBoxGachaDrawnRewardSync, updatePlayerBoxGachaSync, updatePlayerItemSync } from "./wdfpData"
import { generateDataHeaders, getServerTime } from "../utils"
import { getBoxGachaSync } from "../lib/assets"
import { drawBoxGachaSync } from "../lib/gacha"
import { rewardPlayerBoxGachaResultSync } from "./boxGachaRewards"
import { BoxGachaBoxes } from "../lib/types"

const db = getDatabase(Database.WDFP_DATA)

export interface BoxGachaRequest {
    stop_on_featured_rewards: boolean
    box_gacha_id: number
    box_id: number
    api_count?: number
    viewer_id: number
    number: number
}

export class BoxGachaError extends Error {}

// //// 序列化一个箱的当前抽取记录 [@x380kkm 2026-09-28] ////
function boxInfo(playerId: number, poolId: number, boxId: number): object {
    const state = getPlayerBoxGachaSync(playerId, poolId, boxId)
    return {
        box_id: boxId, reset_times: state?.resetTimes ?? 0,
        all_drawn_reward_list: getPlayerBoxGachaDrawnRewardsSync(playerId, poolId, boxId)
            .map(reward => ({ reward_id: reward.id, number: reward.number })),
        coming_next_reward_list: [], is_closed: state?.isClosed ?? false,
    }
}

// //// 按目录顺序返回箱列表并支持单独排列当前箱 [@x380kkm 2026-09-28] ////
export function getAllBoxListSync(playerId: number, poolId: number, boxes: BoxGachaBoxes, skipBoxId?: number): object[] {
    return Object.keys(boxes).map(Number).filter(boxId => boxId !== skipBoxId)
        .map(boxId => boxInfo(playerId, poolId, boxId))
}

// //// 抽取箱内剩余奖励并原子保存资产与重试回执 [@x380kkm 2026-09-28] ////
export function executeBoxGachaSync(playerId: number, request: BoxGachaRequest): object {
    const { box_gacha_id: poolId, box_id: boxId, viewer_id: viewerId, api_count: apiCount } = request
    const identity = JSON.stringify({ poolId, boxId, number: request.number, stop: request.stop_on_featured_rewards })
    return runPlayerDataTransactionSync(() => {
        if (apiCount !== undefined) {
            const receipt = db.prepare("SELECT request_json, response_json FROM player_box_gacha_receipts WHERE player_id=? AND viewer_id=? AND api_count=?")
                .get(playerId, viewerId, apiCount) as { request_json: string, response_json: string } | undefined
            if (receipt) {
                if (receipt.request_json !== identity) throw new BoxGachaError("Box request conflicts with the saved request.")
                return JSON.parse(receipt.response_json)
            }
        }
        const definition = getBoxGachaSync(poolId)
        if (definition === null) throw new BoxGachaError("Invalid box gacha id.")
        const rewards = definition.boxes[boxId]
        if (rewards === undefined) throw new BoxGachaError("Invalid box ID.")
        const state = getPlayerBoxGachaSync(playerId, poolId, boxId)
        if (state?.isClosed) throw new BoxGachaError("Box is closed.")
        const previous = getPlayerBoxGachaDrawnRewardsSync(playerId, poolId, boxId)
        const resetKind = (boxResetKinds as Record<string, Record<string, number>>)[poolId]?.[boxId] ?? 0
        const draw = drawBoxGachaSync(rewards, previous, request.number, request.stop_on_featured_rewards && resetKind === 0)
        const drawCount = draw.rewards.reduce((total, entry) => total + entry.number, 0)
        const currencyId = definition.redeemItemId
        const currency = (getPlayerItemSync(playerId, currencyId) ?? 0) - drawCount * definition.redeemItemCount
        if (currency < 0) throw new BoxGachaError("Not enough pull currency.")
        updatePlayerItemSync(playerId, currencyId, currency)
        const reward = rewardPlayerBoxGachaResultSync(playerId, draw)
        if (reward === null) throw new Error("Box reward player is missing")
        const drawn = new Map(previous.map(entry => [entry.id, entry.number]))
        const totalDrawn = previous.reduce((total, entry) => total + entry.number, drawCount)
        const remainingNumber = Math.max(0, definition.availableCounts[boxId] - totalDrawn)
        const update = { boxId, remainingNumber, isClosed: remainingNumber === 0 }
        if (state === null) insertPlayerBoxGachaSync(playerId, poolId, { ...update, resetTimes: 0 })
        else updatePlayerBoxGachaSync(playerId, poolId, update)
        for (const entry of draw.rewards) {
            const old = drawn.get(entry.id)
            if (old === undefined) insertPlayerBoxGachaDrawnRewardSync(playerId, poolId, boxId, entry)
            else updatePlayerBoxGachaDrawnRewardSync(playerId, poolId, boxId, entry.id, old + entry.number)
        }
        const player = getPlayerSync(playerId)
        if (player === null) throw new Error("Box reward player is missing")
        const response = {
            data_headers: generateDataHeaders({ viewer_id: viewerId }),
            data: {
                user_info: { free_mana: player.freeMana, exp_pool: player.expPool, exp_pooled_time: getServerTime(player.expPooledTime) },
                drawn_reward_list: draw.rewards.map(entry => ({ reward_id: entry.id, number: entry.number })),
                all_box_info: [...getAllBoxListSync(playerId, poolId, definition.boxes, boxId), boxInfo(playerId, poolId, boxId)],
                joined_character_id_list: reward.joined_character_id_list, character_list: reward.character_list,
                equipment_list: reward.equipment_list,
                item_list: { ...reward.items, [currencyId]: getPlayerItemSync(playerId, currencyId) ?? 0 },
                over_max: reward.over_max, mail_arrived: reward.mail_arrived,
            },
        }
        if (apiCount !== undefined) db.prepare("INSERT INTO player_box_gacha_receipts (player_id,viewer_id,api_count,request_json,response_json) VALUES (?,?,?,?,?)")
            .run(playerId, viewerId, apiCount, identity, JSON.stringify(response))
        return response
    })
}
