// audience: internal
// # audit-cn-activity-economy
// 该脚本读取关卡, 活动收益和商店资产, 输出成本货币来源及兑换可达集合.
// 来源存在表示正概率掉落或条件奖励可用, 日期开放和玩家资格由对应运行时规则判定.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { decodeOrderedMapFile } from "./decode-cn-orderedmap.mjs"
import { recordAdditionalSources, recordBattleSources, recordPeriodicSources } from "./audit-cn-activity-economy-rewards.mjs"

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..")
const EVENT_PREFIXES = ["advent", "ranking", "story", "daily-week", "challenge-dungeon", "daily-exp-mana", "world-story", "tower-dungeon", "expert-single", "collect-item", "carnival", "rush", "score-attack"]

// //// 读取生产资产并保留消费路径 [@x380kkm 2026-09-29] ////
function readAssets(root) {
    const read = (name) => JSON.parse(fs.readFileSync(path.join(root, name), "utf8"))
    const service = (name) => read(`core/personal-service/assets/${name}.json`)
    return { battle: service("cn-single-battle"), rules: service("cn-battle-reward-rules"), windows: service("cn-quest-windows"), additional: service("cn-additional-battle-rewards"), periodic: service("cn-periodic-battle-rewards"), carnival: service("cn-carnival-rewards"), regional: service("cn-regional-activity-periods"), shops: read("assets/event_item_shop.json"), items: read("assets/cn_item_catalog.json"), folders: read("assets/rush_event_quest_folder.json"), boxes: read("assets/box_gacha.json"), catalog: read("assets/cn-activity-catalog-source.json"), rushMaster: decodeOrderedMapFile(path.join(root, "core/personal-service/assets/cn-activity-masters/rush_event.orderedmap")) }
}

// //// 为每个活动保留独立的商店商品集合 [@x380kkm 2026-09-29] ////
function shopCatalog(assets) {
    const shops = []
    for (const [type, events] of Object.entries(assets.shops)) {
        for (const [eventId, items] of Object.entries(events)) {
            shops.push({ activity_id: `${EVENT_PREFIXES[type]}:${eventId}`, event_type: Number(type), event_id: Number(eventId), anchor: `assets/event_item_shop.json#/${type}/${eventId}`, items })
        }
    }
    for (const activityId of Object.keys(assets.windows.events).filter((id) => id.startsWith("rush:"))) {
        const eventId = Number(activityId.split(":")[1])
        if (eventId < 700010 || eventId > 700019 || shops.some((shop) => shop.activity_id === activityId)) continue
        const source = shops.find((shop) => shop.activity_id === `rush:${eventId - 10}`)
        if (source) shops.push({ ...source, activity_id: activityId, event_id: eventId, alias_of: source.activity_id })
    }
    for (const [activityId, period] of Object.entries(assets.regional)) {
        if (!period.canonical_activity_id || shops.some((shop) => shop.activity_id === activityId)) continue
        const source = shops.find((shop) => shop.activity_id === period.canonical_activity_id)
        if (source) shops.push({ ...source, activity_id: activityId, event_id: Number(activityId.split(":")[1]), alias_of: source.activity_id })
    }
    return shops
}

// //// 识别常驻突进的无货币关卡配置 [@x380kkm 2026-09-29] ////
function isCurrencyFreeRush(activityId, assets) {
    const [kind, eventId] = activityId.split(":")
    const row = assets.rushMaster[eventId]
    return kind === "rush" && row?.[0].startsWith("combat_diver_constant_")
        && row.slice(10, 14).every((id) => id === "(None)")
        && Object.values(assets.folders[eventId] ?? {}).every((rewards) => rewards.length === 0)
}

// //// 保存特殊积分和突进编组的奖励来源 [@x380kkm 2026-09-29] ////
function recordEventSources(context) {
    for (const [eventId, tiers] of Object.entries(context.carnival.events)) {
        for (const [tierIndex, tier] of tiers.entries()) {
            for (const [index, reward] of tier.rewards.entries()) {
                if (reward.kind === 1 && reward.amount > 0) context.addSource(reward.item_id, { activity_id: `carnival:${eventId}`, kind: "carnival_total_score", score: tier.score, anchor: `core/personal-service/assets/cn-carnival-rewards.json#/events/${eventId}/${tierIndex}/rewards/${index}` })
            }
        }
    }
    for (const [eventId, folders] of Object.entries(context.folders)) {
        for (const [folderId, rewards] of Object.entries(folders)) {
            for (const [index, reward] of rewards.entries()) {
                if (reward.type === 0 && reward.count > 0) context.addSource(reward.id, { activity_id: `rush:${eventId}`, kind: "rush_folder_clear", folder_id: Number(folderId), anchor: `assets/rush_event_quest_folder.json#/${eventId}/${folderId}/${index}` })
            }
        }
    }
}

// //// 按奖励主表锚点合并完整关卡来源集合 [@x380kkm 2026-09-29] ////
function compactSources(sources) {
    for (const [itemId, entries] of sources) {
        const groups = new Map()
        for (const source of entries) {
            const { quest_key: questKey, quest_activity_id: questActivity, ...definition } = source
            const key = JSON.stringify(definition)
            if (!groups.has(key)) groups.set(key, { ...definition, quest_keys: new Set(), quest_activity_ids: new Set() })
            const group = groups.get(key)
            if (questKey) group.quest_keys.add(questKey)
            if (questActivity) group.quest_activity_ids.add(questActivity)
        }
        sources.set(itemId, [...groups.values()].map(({ quest_keys, quest_activity_ids, ...definition }) => ({ ...definition, quest_count: quest_keys.size, quest_keys: [...quest_keys].sort(), quest_activity_ids: [...quest_activity_ids].sort() })))
    }
}

// //// 由关卡来源扩展同活动的货币兑换链 [@x380kkm 2026-09-29] ////
function reachableCurrencies(shop, sources, itemCatalog, regional) {
    const reachable = new Map()
    const canonical = (id) => regional[id]?.canonical_activity_id ?? id
    for (const [id, entries] of sources) {
        const local = entries.filter((entry) => canonical(entry.activity_id) === canonical(shop.activity_id))
        if (local.length) reachable.set(id, { kind: "reward", sources: local })
        else if (itemCatalog.get(id)?.kind === "character-growth") reachable.set(id, { kind: "shared_material", sources: entries })
    }
    let changed = true
    while (changed) {
        changed = false
        for (const [itemId, product] of Object.entries(shop.items)) {
            if (!(product.stock > 0) || !product.costs.every((cost) => reachable.has(cost.id))) continue
            for (const reward of product.rewards) {
                if (reward.type !== 0 || !(reward.count > 0) || reachable.has(reward.id)) continue
                reachable.set(reward.id, { kind: "shop_exchange", cost_ids: product.costs.map((cost) => cost.id), anchor: `${shop.anchor}/${itemId}` })
                changed = true
            }
        }
    }
    return reachable
}

// //// 汇总各活动商店的成本来源和不可达货币 [@x380kkm 2026-09-29] ////
function auditShops(shops, sources, assets) {
    const itemCatalog = new Map(assets.items.items.map((item) => [Number(item.id), item]))
    return shops.map((shop) => {
        const costs = [...new Set(Object.values(shop.items).flatMap((item) => item.costs.map((cost) => cost.id)))].sort((a, b) => a - b)
        const reachable = reachableCurrencies(shop, sources, itemCatalog, assets.regional)
        const historicalOnly = isCurrencyFreeRush(shop.activity_id, assets)
        return { activity_id: shop.activity_id, alias_of: shop.alias_of ?? null, product_count: Object.keys(shop.items).length, anchor: shop.anchor, currencies: costs.map((id) => ({ item_id: id, name: itemCatalog.get(id)?.name ?? null, status: reachable.has(id) ? reachable.get(id).kind : historicalOnly ? "historical_currency" : "unresolved", evidence: reachable.get(id) ?? null, other_sources: reachable.has(id) ? [] : sources.get(id) ?? [], catalog_present: itemCatalog.has(id) })) }
    })
}

// //// 比较同图同名货币的活动归属 [@x380kkm 2026-09-29] ////
function auditCurrencyVariants(shops, sources, items) {
    const catalog = new Map(items.items.map((item) => [Number(item.id), item]))
    const findings = []
    for (const shop of shops) {
        const costs = [...new Set(Object.values(shop.items).flatMap((item) => item.costs.map((cost) => cost.id)))]
        for (const [id, entries] of sources) {
            const sourceItem = catalog.get(id)
            if (!sourceItem || costs.includes(id)) continue
            const targets = costs.filter((targetId) => {
                const target = catalog.get(targetId)
                return target?.name === sourceItem.name && target?.thumbnail_id === sourceItem.thumbnail_id
            })
            if (!targets.length) continue
            const evidence = entries.filter((entry) => entry.activity_id === shop.activity_id)
            if (evidence.length) findings.push({ activity_id: shop.activity_id, dropped_item_id: id, shop_item_ids: targets, name: sourceItem.name, source_string_id: sourceItem.string_id, target_string_ids: targets.map((targetId) => catalog.get(targetId).string_id), evidence })
        }
    }
    return findings
}

// //// 分类全量活动的关卡和商店覆盖 [@x380kkm 2026-09-29] ////
function activityCoverage(assets, shops, sources) {
    const activityIds = new Set([...Object.keys(assets.windows.events), ...Object.keys(assets.regional), ...Object.values(assets.windows.quests).map((quest) => quest.activity_id), ...Object.values(assets.battle.quests).map((quest) => quest.activity_id), ...assets.additional.events.map((event) => `collect-item:${event.id}`), ...shops.map((shop) => shop.activity_id)].filter(Boolean))
    const definitions = new Map(assets.catalog.activities.map((activity) => [activity.activity_id, activity]))
    const boxCurrencies = new Set(Object.values(assets.boxes).map((box) => box.itemId))
    return [...activityIds].sort().map((activityId) => {
        const questKeys = Object.entries(assets.battle.quests).filter(([key, quest]) => (quest.activity_id ?? assets.windows.quests[key]?.activity_id) === activityId).map(([key]) => key)
        const kind = activityId.split(":")[0]
        const shop = shops.find((entry) => entry.activity_id === activityId)
        const currencyIds = [...sources].filter(([, entries]) => entries.some((entry) => entry.activity_id === activityId)).map(([id]) => id)
        const hasDirectRewards = questKeys.some((key) => assets.battle.quests[key].clear_reward || assets.battle.quests[key].s_plus_reward || assets.battle.quests[key].score_rewards?.length)
        const currencyFree = isCurrencyFreeRush(activityId, assets)
        const classification = currencyFree ? "intentional_currency_free" : shop ? "event_shop" : currencyIds.some((id) => boxCurrencies.has(id)) ? "box_pool" : kind === "collect-item" ? "collection_rewards" : hasDirectRewards || ["ranking", "daily-week", "daily-exp-mana", "tower-dungeon", "expert-single", "solo-time-attack", "hard-multi"].includes(kind) ? "direct_rewards" : "no_event_shop_definition"
        const economyEvidence = currencyFree ? { anchor: `core/personal-service/assets/cn-activity-masters/rush_event.orderedmap#/${activityId.split(":")[1]}`, string_id: assets.rushMaster[activityId.split(":")[1]][0], drop_currency_fields: assets.rushMaster[activityId.split(":")[1]].slice(10, 14), exchange_source: "inventory_from_original_event" } : null
        return { activity_id: activityId, name: definitions.get(activityId)?.name ?? null, kind, classification, economy_evidence: economyEvidence, quest_count: questKeys.length, quest_keys: questKeys, source_currency_ids: currencyIds, regional_period: assets.regional[activityId] ? { start_at_ms: assets.regional[activityId].start_at_ms, end_at_ms: assets.regional[activityId].end_at_ms } : null }
    })
}

// //// 生成完整货币来源报告 [@x380kkm 2026-09-29] ////
export function auditActivityEconomy(root = ROOT) {
    const assets = readAssets(root)
    const shops = shopCatalog(assets)
    const currencyIds = new Set(shops.flatMap((shop) => Object.values(shop.items).flatMap((item) => item.costs.map((cost) => cost.id))))
    for (const box of Object.values(assets.boxes)) currencyIds.add(box.itemId)
    const sources = new Map()
    const context = { ...assets, currencyIds, issues: [], additionalCoverage: [], addSource(id, source) { if (currencyIds.has(id)) { if (!sources.has(id)) sources.set(id, []); sources.get(id).push(source) } } }
    recordBattleSources(context)
    recordAdditionalSources(context)
    recordPeriodicSources(context)
    recordEventSources(context)
    compactSources(sources)
    const shopResults = auditShops(shops, sources, assets)
    const currencyVariants = auditCurrencyVariants(shops, sources, assets.items)
    const activities = activityCoverage(assets, shops, sources)
    const unresolved = shopResults.flatMap((shop) => shop.currencies.filter((currency) => currency.status === "unresolved").map((currency) => ({ activity_id: shop.activity_id, shop_anchor: shop.anchor, ...currency })))
    const classifications = Object.fromEntries([...new Set(activities.map((activity) => activity.classification))].map((kind) => [kind, activities.filter((activity) => activity.classification === kind).length]))
    return { scope: { probability: "positive_probability_or_conditional_reward", quantity: "currency_identity_reachability", time: "runtime_calendar_and_player_prerequisites_apply" }, summary: { activity_count: activities.length, quest_count: Object.keys(assets.battle.quests).length, event_quest_count: activities.reduce((sum, activity) => sum + activity.quest_count, 0), shop_count: shops.length, shop_product_count: shops.reduce((sum, shop) => sum + Object.keys(shop.items).length, 0), shop_currency_count: new Set(shopResults.flatMap((shop) => shop.currencies.map((currency) => currency.item_id))).size, shop_currency_relation_count: shopResults.reduce((sum, shop) => sum + shop.currencies.length, 0), unresolved_currency_relation_count: unresolved.length, currency_variant_relation_count: currencyVariants.length, issue_count: context.issues.length, classifications }, unresolved, currency_variants: currencyVariants, issues: context.issues, additional_coverage: context.additionalCoverage, shops: shopResults, activities }
}

// //// 将审计结果写入指定文件并打印覆盖统计 [@x380kkm 2026-09-29] ////
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const argument = (name, fallback) => { const index = process.argv.indexOf(name); return index < 0 ? fallback : process.argv[index + 1] }
    const root = path.resolve(argument("--root", ROOT))
    const output = path.resolve(argument("--output", path.join(root, "archive", "2026-09-29-regional-economy", "currency-audit.json")))
    const report = auditActivityEconomy(root)
    fs.mkdirSync(path.dirname(output), { recursive: true })
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8")
    console.log(JSON.stringify({ output, ...report.summary }))
    if (report.unresolved.length || report.currency_variants.length || report.issues.length) process.exitCode = 1
}
