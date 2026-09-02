// audience: internal
// # audit-cn-shop-reward-closure
//
// 该脚本核对 CN 商店映射, 客户端 master 覆盖和商品费用与奖励引用形成闭包.

import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")
const SHOP_TYPES = [2, 4, 7, 8, 9, 10]
const GENERIC_SHOP_FILES = new Map([
    [2, "treasure_shop.json"],
    [8, "general_shop.json"],
    [9, "star_grain_shop.json"],
    [10, "equipment_enhancement_shop.json"],
])

// //// 读取审计输入并生成 JSON 锚点 [@x380kkm 2026-08-28] ////
function readOption(args, name, fallback) {
    const index = args.indexOf(name)
    if (index < 0) return fallback
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`)
    return path.resolve(value)
}

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, "utf8"))
}

function requiredObject(value, name) {
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
        throw new Error(`${name} must be an object`)
    }
    return value
}

function positiveInteger(value) {
    const parsed = Number(value)
    return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null
}

function jsonAnchor(filePath, ...segments) {
    const relativePath = path.relative(REPOSITORY_ROOT, filePath).replaceAll(path.sep, "/")
    if (segments.length === 0) return relativePath
    const pointer = segments.map((segment) => String(segment)
        .replaceAll("~", "~0")
        .replaceAll("/", "~1"))
        .join("/")
    return `${relativePath}#/${pointer}`
}
// //// /读取审计输入并生成 JSON 锚点 ////

// //// 建立服务端商品目录并保留来源锚点 [@x380kkm 2026-08-28] ////
function registerCatalogEntry(entriesByType, issues, entry) {
    const shopItemId = positiveInteger(entry.shopItemId)
    if (shopItemId === null) {
        issues.push({ kind: "invalid_shop_item_id", anchor: entry.anchor, value: entry.shopItemId })
        return
    }
    const entries = entriesByType.get(entry.shopType)
    const existing = entries.get(shopItemId)
    if (existing !== undefined) {
        issues.push({
            kind: "duplicate_shop_item_id",
            shop_type: entry.shopType,
            shop_item_id: shopItemId,
            anchors: [existing.anchor, entry.anchor],
        })
        return
    }
    entries.set(shopItemId, {
        ...entry,
        shopItemId,
        item: requiredObject(entry.item, entry.anchor),
    })
}

function buildCatalog(assetRoot, issues) {
    const entriesByType = new Map(SHOP_TYPES.map((shopType) => [shopType, new Map()]))
    const documents = new Map()
    for (const [shopType, fileName] of GENERIC_SHOP_FILES) {
        const filePath = path.join(assetRoot, fileName)
        const document = requiredObject(readJson(filePath), fileName)
        documents.set(shopType, { document, filePath })
        for (const [shopItemId, item] of Object.entries(document)) {
            registerCatalogEntry(entriesByType, issues, {
                shopType,
                shopItemId,
                item,
                anchor: jsonAnchor(filePath, shopItemId),
            })
        }
    }

    const eventFilePath = path.join(assetRoot, "event_item_shop.json")
    const eventShop = requiredObject(readJson(eventFilePath), "event item shop")
    documents.set(4, { document: eventShop, filePath: eventFilePath })
    for (const [eventType, events] of Object.entries(eventShop)) {
        for (const [eventId, catalog] of Object.entries(requiredObject(events, `event shop type ${eventType}`))) {
            for (const [shopItemId, item] of Object.entries(requiredObject(catalog, `event shop ${eventType}:${eventId}`))) {
                registerCatalogEntry(entriesByType, issues, {
                    shopType: 4,
                    shopItemId,
                    item,
                    anchor: jsonAnchor(eventFilePath, eventType, eventId, shopItemId),
                })
            }
        }
    }

    const bossFilePath = path.join(assetRoot, "boss_coin_shop.json")
    const bossShop = requiredObject(readJson(bossFilePath), "boss coin shop")
    documents.set(7, { document: bossShop, filePath: bossFilePath })
    for (const [categoryId, catalog] of Object.entries(bossShop)) {
        for (const [shopItemId, item] of Object.entries(requiredObject(catalog, `boss coin shop ${categoryId}`))) {
            registerCatalogEntry(entriesByType, issues, {
                shopType: 7,
                shopItemId,
                item,
                anchor: jsonAnchor(bossFilePath, categoryId, shopItemId),
            })
        }
    }
    return { documents, entriesByType }
}
// //// /建立服务端商品目录并保留来源锚点 ////

// //// 校验活动商店与讨伐币商店映射 [@x380kkm 2026-08-28] ////
function parseEventIdentity(value) {
    if (value === null || typeof value !== "object" || Array.isArray(value)) return null
    const eventType = Number(value.eventType)
    const eventId = positiveInteger(value.eventId)
    return Number.isSafeInteger(eventType) && eventType >= 0 && eventId !== null
        ? { eventType, eventId }
        : null
}

function resolveEventItem(eventShop, identity, shopItemId) {
    const events = eventShop[String(identity.eventType)]
    if (events === undefined) return null
    const direct = events[String(identity.eventId)]?.[String(shopItemId)]
    if (direct !== undefined) return direct
    if (identity.eventId >= 700010 && identity.eventId <= 700019) {
        return events[String(identity.eventId - 10)]?.[String(shopItemId)] ?? null
    }
    return null
}

function auditMaps(assetRoot, catalog, issues) {
    const eventMapFile = path.join(assetRoot, "event_item_shop_id_map.json")
    const eventMap = requiredObject(readJson(eventMapFile), "event shop id map")
    const eventShop = catalog.documents.get(4).document
    for (const [shopItemIdText, value] of Object.entries(eventMap)) {
        const shopItemId = positiveInteger(shopItemIdText)
        const identity = parseEventIdentity(value)
        if (shopItemId === null || identity === null) {
            issues.push({ kind: "invalid_event_map_entry", anchor: jsonAnchor(eventMapFile, shopItemIdText) })
            continue
        }
        if (resolveEventItem(eventShop, identity, shopItemId) === null) {
            issues.push({
                kind: "missing_event_map_target",
                shop_item_id: shopItemId,
                anchor: jsonAnchor(eventMapFile, shopItemIdText),
                target_anchor: jsonAnchor(
                    catalog.documents.get(4).filePath,
                    identity.eventType,
                    identity.eventId,
                    shopItemId,
                ),
            })
        }
    }
    for (const entry of catalog.entriesByType.get(4).values()) {
        if (!Object.hasOwn(eventMap, String(entry.shopItemId))) {
            issues.push({
                kind: "missing_event_item_map",
                shop_item_id: entry.shopItemId,
                anchor: entry.anchor,
                target_anchor: jsonAnchor(eventMapFile, entry.shopItemId),
            })
        }
    }

    const bossMapFile = path.join(assetRoot, "boss_coin_shop_item_category_map.json")
    const bossMap = requiredObject(readJson(bossMapFile), "boss coin category map")
    const bossShop = catalog.documents.get(7).document
    for (const [shopItemIdText, categoryValue] of Object.entries(bossMap)) {
        const shopItemId = positiveInteger(shopItemIdText)
        const categoryId = positiveInteger(categoryValue)
        if (shopItemId === null || categoryId === null) {
            issues.push({ kind: "invalid_boss_coin_map_entry", anchor: jsonAnchor(bossMapFile, shopItemIdText) })
            continue
        }
        if (bossShop[String(categoryId)]?.[String(shopItemId)] === undefined) {
            issues.push({
                kind: "missing_boss_coin_map_target",
                shop_item_id: shopItemId,
                anchor: jsonAnchor(bossMapFile, shopItemIdText),
                target_anchor: jsonAnchor(catalog.documents.get(7).filePath, categoryId, shopItemId),
            })
        }
    }
    for (const entry of catalog.entriesByType.get(7).values()) {
        if (!Object.hasOwn(bossMap, String(entry.shopItemId))) {
            issues.push({
                kind: "missing_boss_coin_item_map",
                shop_item_id: entry.shopItemId,
                anchor: entry.anchor,
                target_anchor: jsonAnchor(bossMapFile, entry.shopItemId),
            })
        }
    }
    return { eventMap, bossMap }
}
// //// /校验活动商店与讨伐币商店映射 ////

// //// 读取参考目录并核对费用和奖励类型 [@x380kkm 2026-08-28] ////
function readReferenceIdSet(filePath, name, issues) {
    const values = readJson(filePath)
    if (!Array.isArray(values)) throw new Error(`${name} must be an array`)
    const ids = new Set()
    for (let index = 0; index < values.length; index += 1) {
        const id = positiveInteger(values[index])
        if (id === null) {
            issues.push({ kind: "invalid_reference_id", reference: name, anchor: jsonAnchor(filePath, index), value: values[index] })
        } else if (ids.has(id)) {
            issues.push({ kind: "duplicate_reference_id", reference: name, anchor: jsonAnchor(filePath, index), value: id })
        } else {
            ids.add(id)
        }
    }
    return ids
}

function readCharacterIdSet(filePath, issues) {
    const characters = requiredObject(readJson(filePath), "character catalog")
    const ids = new Set()
    for (const characterId of Object.keys(characters)) {
        const id = positiveInteger(characterId)
        if (id === null) issues.push({ kind: "invalid_character_id", anchor: jsonAnchor(filePath, characterId) })
        else ids.add(id)
    }
    return ids
}

function auditItemReferences(entry, references, issues, counts) {
    if (!Array.isArray(entry.item.costs)) {
        issues.push({ kind: "invalid_cost_list", anchor: `${entry.anchor}/costs` })
    } else {
        for (let index = 0; index < entry.item.costs.length; index += 1) {
            counts.cost_reference_count += 1
            const cost = entry.item.costs[index]
            const itemId = positiveInteger(cost?.id)
            const amount = positiveInteger(cost?.amount)
            if (itemId === null || !references.itemIds.has(itemId)) {
                issues.push({
                    kind: "missing_cost_item",
                    shop_type: entry.shopType,
                    shop_item_id: entry.shopItemId,
                    item_id: cost?.id ?? null,
                    anchor: `${entry.anchor}/costs/${index}`,
                })
            }
            if (amount === null) {
                issues.push({
                    kind: "invalid_cost_amount",
                    shop_type: entry.shopType,
                    shop_item_id: entry.shopItemId,
                    amount: cost?.amount ?? null,
                    anchor: `${entry.anchor}/costs/${index}/amount`,
                })
            }
        }
    }

    if (entry.item.userCost !== undefined) {
        counts.user_cost_count += 1
        if (![0, 1, 2].includes(Number(entry.item.userCost?.type))) {
            issues.push({
                kind: "invalid_user_cost_type",
                shop_type: entry.shopType,
                shop_item_id: entry.shopItemId,
                user_cost_type: entry.item.userCost?.type ?? null,
                anchor: `${entry.anchor}/userCost/type`,
            })
        }
    }

    if (entry.shopType === 10) {
        counts.enhancement_equipment_reference_count += 1
        const equipmentId = positiveInteger(entry.item.equipmentId)
        if (equipmentId === null || !references.equipmentIds.has(equipmentId)) {
            issues.push({
                kind: "missing_enhancement_equipment",
                shop_type: entry.shopType,
                shop_item_id: entry.shopItemId,
                equipment_id: entry.item.equipmentId ?? null,
                anchor: `${entry.anchor}/equipmentId`,
            })
        }
    }

    if (!Array.isArray(entry.item.rewards)) {
        issues.push({ kind: "invalid_reward_list", anchor: `${entry.anchor}/rewards` })
        return
    }
    if (entry.item.rewards.length === 0 && entry.shopType !== 10) {
        issues.push({
            kind: "empty_reward_list",
            shop_type: entry.shopType,
            shop_item_id: entry.shopItemId,
            anchor: `${entry.anchor}/rewards`,
        })
        return
    }
    for (let index = 0; index < entry.item.rewards.length; index += 1) {
        counts.reward_reference_count += 1
        const reward = entry.item.rewards[index]
        const rewardType = Number(reward?.type)
        const rewardId = positiveInteger(reward?.id)
        const rewardCount = positiveInteger(reward?.count)
        const exists = rewardType === 0
            ? rewardId !== null && references.itemIds.has(rewardId)
            : rewardType === 3
                ? rewardId !== null && references.characterIds.has(rewardId)
                : rewardType === 4
                    ? rewardId !== null && references.equipmentIds.has(rewardId)
                    : rewardType === 1 || rewardType === 2
        if (!exists) {
            issues.push({
                kind: "invalid_reward_reference",
                shop_type: entry.shopType,
                shop_item_id: entry.shopItemId,
                reward_type: reward?.type ?? null,
                reward_id: reward?.id ?? null,
                anchor: `${entry.anchor}/rewards/${index}`,
            })
        }
        if (rewardCount === null) {
            issues.push({
                kind: "invalid_reward_count",
                shop_type: entry.shopType,
                shop_item_id: entry.shopItemId,
                reward_type: reward?.type ?? null,
                count: reward?.count ?? null,
                anchor: `${entry.anchor}/rewards/${index}/count`,
            })
        }
    }
}
// //// /读取参考目录并核对费用和奖励类型 ////

// //// 检查原始历史目录对客户端 master 的覆盖 [@x380kkm 2026-08-28] ////
function readRawBossShop(referenceAssetRoot) {
    const filePath = path.join(referenceAssetRoot, "cdndata", "boss_coin_shop.json")
    if (!fs.existsSync(filePath)) return null
    const document = requiredObject(readJson(filePath), "raw boss coin shop")
    const ids = new Set()
    const invalidEntries = []
    for (const [shopItemId, rows] of Object.entries(document)) {
        const row = Array.isArray(rows) ? rows[0] : null
        const valid = positiveInteger(shopItemId) !== null
            && Array.isArray(row)
            && row.length >= 35
            && positiveInteger(row[0]) !== null
            && positiveInteger(row[17]) !== null
            && positiveInteger(row[18]) !== null
            && ["0", "1", "2", "3", "4", "5"].includes(String(row[32]))
            && positiveInteger(row[34]) !== null
        if (!valid) {
            invalidEntries.push(jsonAnchor(filePath, shopItemId))
            continue
        }
        ids.add(Number(shopItemId))
    }
    return { filePath, ids, invalidEntries }
}

function classifyWhitelistGaps(assetRoot, referenceAssetRoot, catalog, maps, issues) {
    const whitelistFile = path.join(assetRoot, "cdn_shop_master_whitelists.json")
    const whitelists = requiredObject(readJson(whitelistFile), "shop master whitelists")
    const rawBossShop = readRawBossShop(referenceAssetRoot)
    const gapEntries = []
    const whitelistIdsByType = new Map()
    let whitelistItemCount = 0
    let resolvableWhitelistItemCount = 0

    for (const shopType of SHOP_TYPES) {
        const values = whitelists[String(shopType)]
        if (!Array.isArray(values)) {
            issues.push({ kind: "missing_whitelist_shop_type", shop_type: shopType, anchor: jsonAnchor(whitelistFile, shopType) })
            whitelistIdsByType.set(shopType, new Set())
            continue
        }
        const ids = new Set()
        for (let index = 0; index < values.length; index += 1) {
            whitelistItemCount += 1
            const shopItemId = positiveInteger(values[index])
            const anchor = jsonAnchor(whitelistFile, shopType, index)
            if (shopItemId === null) {
                issues.push({ kind: "invalid_whitelist_item_id", shop_type: shopType, anchor, value: values[index] })
                continue
            }
            if (ids.has(shopItemId)) {
                issues.push({ kind: "duplicate_whitelist_item_id", shop_type: shopType, shop_item_id: shopItemId, anchor })
                continue
            }
            ids.add(shopItemId)
            const resolved = shopType === 4
                ? parseEventIdentity(maps.eventMap[String(shopItemId)]) !== null
                    && resolveEventItem(
                        catalog.documents.get(4).document,
                        parseEventIdentity(maps.eventMap[String(shopItemId)]),
                        shopItemId,
                    ) !== null
                : shopType === 7
                    ? positiveInteger(maps.bossMap[String(shopItemId)]) !== null
                        && catalog.documents.get(7).document[String(maps.bossMap[String(shopItemId)])]?.[String(shopItemId)] !== undefined
                    : catalog.entriesByType.get(shopType).has(shopItemId)
            if (resolved) {
                resolvableWhitelistItemCount += 1
                continue
            }

            const rawHistorical = shopType === 7 && rawBossShop?.ids.has(shopItemId)
            gapEntries.push({
                kind: rawHistorical ? "missing_server_catalog_entry" : "client_master_only_entry",
                shop_type: shopType,
                shop_item_id: shopItemId,
                whitelist_anchor: jsonAnchor(whitelistFile, shopType),
                item_anchor: anchor,
                historical_source_anchor: rawHistorical
                    ? jsonAnchor(rawBossShop.filePath, shopItemId)
                    : undefined,
            })
            if (rawHistorical) {
                issues.push({
                    kind: "missing_server_catalog_entry",
                    shop_type: shopType,
                    shop_item_id: shopItemId,
                    anchor,
                    historical_source_anchor: jsonAnchor(rawBossShop.filePath, shopItemId),
                })
            }
        }
        whitelistIdsByType.set(shopType, ids)
    }

    const serverCatalogOnly = []
    const shopTypeCounts = {}
    for (const shopType of SHOP_TYPES) {
        const entries = catalog.entriesByType.get(shopType)
        const whitelistIds = whitelistIdsByType.get(shopType)
        const missingFromWhitelist = [...entries.keys()].filter((shopItemId) => !whitelistIds.has(shopItemId))
        if (missingFromWhitelist.length > 0) {
            serverCatalogOnly.push({
                shop_type: shopType,
                count: missingFromWhitelist.length,
                catalog_anchor: jsonAnchor(catalog.documents.get(shopType).filePath),
                whitelist_anchor: jsonAnchor(whitelistFile, shopType),
                shop_item_ids: missingFromWhitelist.sort((left, right) => left - right),
            })
        }
        shopTypeCounts[String(shopType)] = {
            catalog_item_count: entries.size,
            whitelist_item_count: whitelistIds.size,
            missing_client_master_count: gapEntries.filter((gap) => gap.shop_type === shopType).length,
            server_catalog_only_count: missingFromWhitelist.length,
        }
    }
    return {
        whitelistItemCount,
        resolvableWhitelistItemCount,
        gapEntries,
        serverCatalogOnly,
        shopTypeCounts,
    }
}
// //// /检查原始历史目录对客户端 master 的覆盖 ////

// //// 汇总 CN 商店奖励闭包审计结果 [@x380kkm 2026-08-28] ////
function countKinds(entries) {
    const counts = {}
    for (const entry of entries) counts[entry.kind] = (counts[entry.kind] ?? 0) + 1
    return counts
}

function groupGaps(gaps) {
    const groups = new Map()
    for (const gap of gaps) {
        const key = `${gap.kind}:${gap.shop_type}`
        const group = groups.get(key) ?? {
            kind: gap.kind,
            shop_type: gap.shop_type,
            whitelist_anchor: gap.whitelist_anchor,
            historical_source_anchor: gap.historical_source_anchor,
            shop_item_ids: [],
        }
        group.shop_item_ids.push(gap.shop_item_id)
        groups.set(key, group)
    }
    return [...groups.values()].map((group) => ({
        ...group,
        count: group.shop_item_ids.length,
        shop_item_ids: group.shop_item_ids.sort((left, right) => left - right),
    }))
}

export function auditShopRewardClosure({ assetRoot, referenceAssetRoot }) {
    const structuralIssues = []
    const catalog = buildCatalog(assetRoot, structuralIssues)
    const maps = auditMaps(assetRoot, catalog, structuralIssues)
    const referenceFiles = {
        items: path.join(referenceAssetRoot, "item_ids.json"),
        equipment: path.join(referenceAssetRoot, "equipment_ids.json"),
        characters: path.join(referenceAssetRoot, "character.json"),
    }
    const references = {
        itemIds: readReferenceIdSet(referenceFiles.items, "item_ids", structuralIssues),
        equipmentIds: readReferenceIdSet(referenceFiles.equipment, "equipment_ids", structuralIssues),
        characterIds: readCharacterIdSet(referenceFiles.characters, structuralIssues),
    }
    const referenceCounts = {
        cost_reference_count: 0,
        reward_reference_count: 0,
        user_cost_count: 0,
        enhancement_equipment_reference_count: 0,
    }
    for (const entries of catalog.entriesByType.values()) {
        for (const entry of entries.values()) {
            auditItemReferences(entry, references, structuralIssues, referenceCounts)
        }
    }
    const whitelist = classifyWhitelistGaps(assetRoot, referenceAssetRoot, catalog, maps, structuralIssues)
    const catalogItemCount = [...catalog.entriesByType.values()]
        .reduce((count, entries) => count + entries.size, 0)
    const report = {
        summary: {
            structural_issue_count: structuralIssues.length,
            catalog_gap_count: whitelist.gapEntries.length,
            missing_server_catalog_entry_count: whitelist.gapEntries
                .filter((gap) => gap.kind === "missing_server_catalog_entry").length,
            client_master_only_entry_count: whitelist.gapEntries
                .filter((gap) => gap.kind === "client_master_only_entry").length,
            server_catalog_only_count: whitelist.serverCatalogOnly
                .reduce((count, group) => count + group.count, 0),
            whitelist_item_count: whitelist.whitelistItemCount,
            resolvable_whitelist_item_count: whitelist.resolvableWhitelistItemCount,
            audited_catalog_item_count: catalogItemCount,
        },
        reference_catalogs: {
            item_count: references.itemIds.size,
            equipment_count: references.equipmentIds.size,
            character_count: references.characterIds.size,
            item_anchor: jsonAnchor(referenceFiles.items),
            equipment_anchor: jsonAnchor(referenceFiles.equipment),
            character_anchor: jsonAnchor(referenceFiles.characters),
        },
        reference_counts: referenceCounts,
        shop_type_counts: whitelist.shopTypeCounts,
        structural_issue_counts: countKinds(structuralIssues),
        structural_issues: structuralIssues,
        catalog_gap_groups: groupGaps(whitelist.gapEntries),
        server_catalog_only_groups: whitelist.serverCatalogOnly,
    }
    return report
}

export function reportHasStructuralIssues(report) {
    return report.summary.structural_issue_count > 0
}
// //// /汇总 CN 商店奖励闭包审计结果 ////

// //// 执行 CN 商店奖励闭包审计 [@x380kkm 2026-08-28] ////
function main() {
    const args = process.argv.slice(2)
    const assetRoot = readOption(args, "--asset-root", path.join(REPOSITORY_ROOT, "assets"))
    const referenceAssetRoot = readOption(
        args,
        "--reference-asset-root",
        path.join(REPOSITORY_ROOT, "..", "startpoint-cn-launcher", "resources", "server", "assets"),
    )
    const report = auditShopRewardClosure({ assetRoot, referenceAssetRoot })
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`)
    if (reportHasStructuralIssues(report)) process.exitCode = 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
// //// /执行 CN 商店奖励闭包审计 ////
