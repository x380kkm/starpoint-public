// audience: internal
// # cn-event-currency-display
// 活动显示币按同图同名的本期兑换币关联, 对应货币具有活动奖励或兑换来源.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { decodeOrderedMapFile } from "./decode-cn-orderedmap.mjs"
import { auditActivityEconomy } from "./audit-cn-activity-economy.mjs"

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const OUTPUT = "core/personal-service/assets/cn-event-currency-display.json"
const DISPLAY_MASTER_SCHEMAS = [
    ["advent_event", "advent", [17, 18, 19, 20]],
    ["story_event", "story", [10, 11, 12, 13]],
    ["challenge_dungeon_event", "challenge-dungeon", [8, 9, 10, 11]],
    ["world_story_event", "world-story", [16, 17, 18, 19]],
    ["carnival_event", "carnival", [15, 16, 17, 18]],
    ["rush_event", "rush", [10, 11, 12, 13]],
    ["hard_multi_event", "hard-multi", [16, 17, 18, 19]],
    ["collect_item_event", "collect-item", [6, 8, 10]],
]

// //// 关联显示道具与同一期可获得的兑换货币 [@x380kkm 2026-10-02] ////
function replacementCurrency(itemId, shop, items) {
    if (shop.currencies.some((currency) => currency.item_id === itemId)) return null
    const item = items.get(itemId)
    if (!item) throw new Error(`Missing display item ${shop.activity_id}: ${itemId}`)
    const candidates = shop.currencies.filter((currency) => {
        const target = items.get(currency.item_id)
        return target?.name === item.name && target?.thumbnail_id === item.thumbnail_id
            && target?.kind === item.kind && target?.effect_kind === item.effect_kind
    })
    if (candidates.length === 0) return null
    if (candidates.length !== 1 || !["reward", "shop_exchange"].includes(candidates[0].status)) {
        throw new Error(`Ambiguous or unavailable display currency ${shop.activity_id}: ${itemId}`)
    }
    return candidates[0].item_id
}

// //// 审查全部活动显示字段并生成可证实的币引用映射 [@x380kkm 2026-10-02] ////
export function buildEventCurrencyDisplay(root = ROOT) {
    const economy = auditActivityEconomy(root)
    const shops = new Map(economy.shops.map((shop) => [shop.activity_id, shop]))
    const catalog = JSON.parse(fs.readFileSync(path.join(root, "assets/cn_item_catalog.json"), "utf8"))
    const items = new Map(catalog.items.map((item) => [Number(item.id), item]))
    const masters = {}
    const changes = []
    const nonShopItems = []
    const coverage = []
    for (const [name, prefix, fields] of DISPLAY_MASTER_SCHEMAS) {
        const rows = decodeOrderedMapFile(path.join(root, `core/personal-service/assets/cn-activity-masters/${name}.orderedmap`))
        let shopCount = 0
        for (const [eventId, row] of Object.entries(rows)) {
            const activityId = `${prefix}:${eventId}`
            const shop = shops.get(activityId)
            if (!shop) continue
            shopCount += 1
            for (const index of fields) {
                if (row[index] === "(None)") continue
                const itemId = Number(row[index])
                if (!Number.isSafeInteger(itemId) || itemId <= 0) {
                    throw new Error(`Invalid display item ${name}/${eventId}/${index}: ${row[index]}`)
                }
                const target = replacementCurrency(itemId, shop, items)
                if (target !== null) {
                    const events = masters[name] ??= {}
                    const replacements = events[eventId] ??= {}
                    replacements[index] = target
                    changes.push({ activity_id: activityId, field: index, source: itemId, target })
                } else if (!shop.currencies.some((currency) => currency.item_id === itemId)) {
                    nonShopItems.push({ activity_id: activityId, item_id: itemId, name: items.get(itemId).name })
                }
            }
        }
        coverage.push({ master: name, event_count: Object.keys(rows).length, shop_count: shopCount })
    }
    return { masters, changes, non_shop_items: nonShopItems, coverage }
}
// //// /审查全部活动显示字段并生成可证实的币引用映射 ////

// //// 写入活动显示币映射或核对现有资产 [@x380kkm 2026-10-02] ////
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const report = buildEventCurrencyDisplay()
    const output = path.join(ROOT, OUTPUT)
    const content = `${JSON.stringify(report.masters)}\n`
    if (process.argv.includes("--check")) {
        if (fs.readFileSync(output, "utf8") !== content) throw new Error(`Currency display asset differs: ${OUTPUT}`)
    } else {
        fs.writeFileSync(output, content, "utf8")
    }
    console.log(JSON.stringify({ output: OUTPUT, changes: report.changes, non_shop_items: report.non_shop_items, coverage: report.coverage }))
}
// //// /写入活动显示币映射或核对现有资产 ////
