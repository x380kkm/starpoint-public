// audience: internal
// # item-use
// 自选素材序号遵循客户端主表顺序, 道具库存, 素材, 邮件余量和请求回执共用 SQLite 事务.

import selectionSource from "../../core/personal-service/assets/cn-item-selection.json"
import effectSource from "../../core/personal-service/assets/cn-item-effects.json"
import rankProfiles from "../../core/personal-service/assets/cn-player-rank-stamina.json"
import capacities from "../../core/personal-service/assets/cn-shop-capacities.json"
import getDatabase, { Database } from "."
import { createPlayerMailSync } from "./playerMails"
import { getPlayerItemSync, getPlayerSync, givePlayerItemSync, runPlayerDataTransactionSync, updatePlayerSync } from "./wdfpData"
import { generateDataHeaders, getServerTime } from "../utils"

interface SelectionItem {
    start_time: number
    end_time: number
    choices: { item_id: number, amount: number }[]
}

interface ItemUseEntry {
    id: number
    number: number
    selectIndex: number
}

const catalog: Record<string, SelectionItem> = selectionSource
const effects: Record<string, { effectKind: number, effectValue: number, start_time?: number, end_time?: number }> = effectSource
const itemCapacities: Record<string, number> = capacities.items
const staminaProfiles = [...rankProfiles].reverse()
const db = getDatabase(Database.WDFP_DATA)

export class ItemUseError extends Error {}

// //// 校验并合并同一种道具的同一个素材选项 [@x380kkm 2026-09-28] ////
function normalizeUses(value: unknown): ItemUseEntry[] {
    if (!Array.isArray(value) || value.length === 0) throw new ItemUseError("invalid_request_body")
    const choices = new Map<string, ItemUseEntry>()
    for (const entry of value) {
        if (entry === null || typeof entry !== "object") throw new ItemUseError("invalid_item_selection")
        const { id, number, selectIndex = 0 } = entry as ItemUseEntry
        if (!Number.isSafeInteger(id) || id <= 0 || !Number.isSafeInteger(number) || number <= 0 ||
            !Number.isSafeInteger(selectIndex) || (catalog[id]
                ? selectIndex <= 0 || !catalog[id].choices[selectIndex - 1]
                : selectIndex !== 0 || ![2, 3].includes(effects[id]?.effectKind))) {
            throw new ItemUseError("invalid_item_selection")
        }
        const key = `${id}:${selectIndex}`
        const total = checkedCount((choices.get(key)?.number ?? 0) + number)
        choices.set(key, { id, number: total, selectIndex })
    }
    return [...choices.values()].sort((left, right) => left.id - right.id || left.selectIndex - right.selectIndex)
}
// //// /校验并合并同一种道具的同一个素材选项 ////

// //// 保持批量道具数量在整数精度范围内 [@x380kkm 2026-09-28] ////
function checkedCount(number: number): number {
    if (!Number.isSafeInteger(number) || number <= 0) throw new ItemUseError("invalid_item_number")
    return number
}

// //// 汇总库存用量, 体力恢复和素材奖励 [@x380kkm 2026-09-28] ////
function totals(selections: ItemUseEntry[]) {
    const consumed: Record<string, number> = {}
    const rewards: Record<string, number> = {}
    let recovery = 0
    for (const entry of selections) {
        consumed[entry.id] = checkedCount((consumed[entry.id] ?? 0) + entry.number)
        if (entry.selectIndex === 0) {
            const effect = effects[entry.id]
            const perItem = effect.effectKind === 2 ? effect.effectValue : Math.floor(999 * effect.effectValue / 100)
            recovery = checkedCount(recovery + checkedCount(perItem * entry.number))
            continue
        }
        const choice = catalog[entry.id].choices[entry.selectIndex - 1]
        const amount = checkedCount(choice.amount * entry.number)
        rewards[choice.item_id] = checkedCount((rewards[choice.item_id] ?? 0) + amount)
    }
    return { consumed, rewards, recovery }
}

// //// 按段位体力恢复配置计算道具使用前的当前体力 [@x380kkm 2026-09-28] ////
function currentStamina(playerId: number, now: number): number {
    const player = getPlayerSync(playerId)
    if (player === null) throw new ItemUseError("missing_player")
    const profile = staminaProfiles.find(([threshold]) => player.rankPoint >= threshold)
    if (!profile) throw new Error("Missing stamina rank")
    const [, maximum, healRate] = profile
    const elapsed = Math.max(0, now - getServerTime(player.staminaHealTime))
    const recovery = Math.floor(elapsed / (300 * Math.max(Number.EPSILON, 1 - healRate)))
    return Math.min(999, Math.max(0, Math.min(player.stamina + recovery, Math.max(maximum, player.stamina))))
}

// //// 原子保存道具使用结果并重放同一客户端请求 [@x380kkm 2026-09-28] ////
export function useItemsSync(playerId: number, viewerId: number, apiCount: number | undefined, items: unknown, now: number): object {
    const selections = normalizeUses(items)
    const request = JSON.stringify(selections)
    const { consumed, rewards, recovery } = totals(selections)
    return runPlayerDataTransactionSync(() => {
        if (apiCount !== undefined) {
            const receipt = db.prepare("SELECT request_json, response_json FROM player_item_use_receipts WHERE player_id=? AND viewer_id=? AND api_count=?")
                .get(playerId, viewerId, apiCount) as { request_json: string, response_json: string } | undefined
            if (receipt) {
                if (receipt.request_json !== request) throw new ItemUseError("item_request_conflict")
                return JSON.parse(receipt.response_json)
            }
        }
        if (selections.some(({ id }) => {
            const item = catalog[id] ?? effects[id]
            return (item.start_time !== undefined && now < item.start_time) || (item.end_time !== undefined && now > item.end_time)
        })) {
            return { data_headers: generateDataHeaders({ viewer_id: viewerId, servertime: now, result_code: 2057 }), data: {} }
        }
        let userInfo: { stamina: number, stamina_heal_time: number } | undefined
        if (recovery > 0) {
            const stamina = currentStamina(playerId, now)
            if (stamina >= 999) return { data_headers: generateDataHeaders({ viewer_id: viewerId, servertime: now, result_code: 2102 }), data: {} }
            userInfo = { stamina: Math.min(999, stamina + recovery), stamina_heal_time: now }
        }
        for (const [id, number] of Object.entries(consumed)) {
            if ((getPlayerItemSync(playerId, id) ?? 0) < number) throw new ItemUseError("insufficient_items")
        }
        const updates: Record<string, number> = {}
        for (const [id, number] of Object.entries(consumed)) updates[id] = givePlayerItemSync(playerId, id, -number)
        const excess: Record<string, number> = {}
        const overMax: object[] = []
        for (const [id, number] of Object.entries(rewards)) {
            const capacity = itemCapacities[id]
            if (!Number.isSafeInteger(capacity) || capacity <= 0) throw new Error(`Missing item capacity: ${id}`)
            const held = getPlayerItemSync(playerId, id) ?? 0
            const credited = Math.min(number, Math.max(0, capacity - held))
            updates[id] = givePlayerItemSync(playerId, id, credited)
            if (credited < number) {
                excess[id] = number - credited
                overMax.push({ process_type: 1, item: { item_id: Number(id), number: number - credited } })
            }
        }
        const mailArrived = Object.keys(excess).length > 0
        if (mailArrived) createPlayerMailSync({
            playerId, title: "持有上限奖励", body: "超出持有上限的自选素材存入邮件.", sender: "系统", rewards: { itemList: excess },
        })
        if (userInfo) updatePlayerSync({ id: playerId, stamina: userInfo.stamina, staminaHealTime: new Date(now * 1000) })
        const response = {
            data_headers: generateDataHeaders({ viewer_id: viewerId, servertime: now }),
            data: { item_list: updates, over_max: overMax, mail_arrived: mailArrived, ...(userInfo ? { user_info: userInfo } : {}) },
        }
        if (apiCount !== undefined) db.prepare("INSERT INTO player_item_use_receipts (player_id,viewer_id,api_count,request_json,response_json) VALUES (?,?,?,?,?)")
            .run(playerId, viewerId, apiCount, request, JSON.stringify(response))
        return response
    })
}
// //// /原子保存道具使用结果并重放同一客户端请求 ////
