// audience: internal
// # cn-shop-contract-generator
//
// 该脚本从客户端 CN master 生成个人服务使用的商店成本和奖励契约.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")

export const SHOP_CONTRACT_SCHEMAS = [
    {
        name: "special_pack_shop",
        shopType: 3,
        logicalPath: "master/shop/special_pack_shop.orderedmap",
        assetFile: "special_pack_shop.json",
        costOffset: 12,
        rewardOffset: 27,
    },
    {
        name: "mana_shop",
        shopType: 5,
        logicalPath: "master/shop/mana_shop.orderedmap",
        assetFile: "mana_shop.json",
        costOffset: 6,
        rewardOffset: 20,
    },
    {
        name: "treasure_shop",
        shopType: 2,
        logicalPath: "master/shop/treasure_shop.orderedmap",
        assetFile: "treasure_shop.json",
        costOffset: 10,
        rewardOffset: 24,
    },
    {
        name: "event_item_shop",
        shopType: 4,
        logicalPath: "master/shop/event_item_shop.orderedmap",
        assetFile: "event_item_shop.json",
        costOffset: 18,
        rewardOffset: 32,
        eventTypeIndex: 2,
        eventIdIndex: 1,
    },
    {
        name: "boss_coin_shop",
        shopType: 7,
        logicalPath: "master/shop/boss_coin_shop.orderedmap",
        assetFile: "boss_coin_shop.json",
        costOffset: 17,
        rewardOffset: 32,
        categoryIndex: 0,
    },
    {
        name: "general_shop",
        shopType: 8,
        logicalPath: "master/shop/general_shop.orderedmap",
        assetFile: "general_shop.json",
        costOffset: 12,
        rewardOffset: 29,
    },
    {
        name: "star_grain_shop",
        shopType: 9,
        logicalPath: "master/shop/star_grain_shop.orderedmap",
        assetFile: "star_grain_shop.json",
        costOffset: 10,
        rewardOffset: 25,
    },
    {
        name: "equipment_enhancement_shop",
        shopType: 10,
        logicalPath: "master/equipment_enhancement/equipment_enhancement_shop.orderedmap",
        assetFile: "equipment_enhancement_shop.json",
        costOffset: 14,
        rewardOffset: 32,
    },
]

// //// 解析生成器参数 [@x380kkm 2026-08-31] ////
function parseArguments(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 2) {
        const option = argv[index]
        const value = argv[index + 1]
        if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
            throw new Error("usage: generate-cn-shop-contracts.mjs --cdn-root <path> [--asset-root <path>]")
        }
        args[option.slice(2)] = value
    }
    if (!args["cdn-root"]) {
        throw new Error("usage: generate-cn-shop-contracts.mjs --cdn-root <path> [--asset-root <path>]")
    }
    return args
}
// //// /解析生成器参数 ////

// //// 判断 master 字段是否为空 [@x380kkm 2026-08-31] ////
function isEmptyField(value) {
    return value === undefined || value === null || value === "" || value === "(None)"
}
// //// /判断 master 字段是否为空 ////

// //// 解析 master 整数字段 [@x380kkm 2026-08-31] ////
function parseInteger(value, label, { minimum = 0 } = {}) {
    const parsed = Number(value)
    if (!Number.isSafeInteger(parsed) || parsed < minimum) {
        throw new Error(`${label} is not an integer greater than or equal to ${minimum}: ${value}`)
    }
    return parsed
}
// //// /解析 master 整数字段 ////

// //// 读取客户端商品的成本和奖励 [@x380kkm 2026-08-31] ////
export function parseShopContract(row, schema, shopItemId) {
    if (!Array.isArray(row)) {
        throw new Error(`CN shop master row is not an array: ${schema.logicalPath}:${shopItemId}`)
    }
    const costs = []
    for (let offset = schema.costOffset; offset < schema.costOffset + 8; offset += 2) {
        if (isEmptyField(row[offset])) continue
        costs.push({
            id: parseInteger(row[offset], `${schema.logicalPath}:${shopItemId}:cost-id`, { minimum: 1 }),
            amount: parseInteger(row[offset + 1], `${schema.logicalPath}:${shopItemId}:cost-amount`, { minimum: 1 }),
        })
    }
    const rewards = []
    const userCostOffset = schema.costOffset - 3
    const userCost = isEmptyField(row[userCostOffset]) ? undefined : {
        type: parseInteger(row[userCostOffset], `${schema.name}:${shopItemId}:user-cost-type`),
        amount: parseInteger(row[userCostOffset + 1], `${schema.name}:${shopItemId}:user-cost-amount`, { minimum: 1 }),
    }
    const contract = {
        costs, rewards, userCost,
        availableFrom: parseOptionalTime(row[schema.costOffset + 8]),
        availableUntil: parseOptionalTime(row[schema.costOffset + 9]),
    }
    if (schema.shopType === 3) contract.paidOnly = true
    if (schema.shopType === 5) {
        rewards.push({ type: 2, count: parseInteger(row[20], `${schema.name}:${shopItemId}:purchase-count`) + parseInteger(row[21], `${schema.name}:${shopItemId}:additional-count`) })
        return contract
    }
    if (schema.shopType === 10 && row[4] === "1") return contract
    for (let offset = schema.rewardOffset; offset < schema.rewardOffset + 18; offset += 3) {
        if (isEmptyField(row[offset])) continue
        const reward = {
            type: parseInteger(row[offset], `${schema.logicalPath}:${shopItemId}:reward-type`),
        }
        if (reward.type > 4 && reward.type !== 6) {
            throw new Error(`CN shop master reward type is unsupported: ${schema.logicalPath}:${shopItemId}:${reward.type}`)
        }
        if (!isEmptyField(row[offset + 1])) {
            reward.id = parseInteger(row[offset + 1], `${schema.logicalPath}:${shopItemId}:reward-id`, { minimum: 1 })
        }
        if (!isEmptyField(row[offset + 2])) {
            reward.count = parseInteger(row[offset + 2], `${schema.logicalPath}:${shopItemId}:reward-count`, { minimum: 1 })
        }
        rewards.push(reward)
    }
    if (rewards.length === 0) {
        throw new Error(`CN shop master reward list is empty: ${schema.logicalPath}:${shopItemId}`)
    }
    return contract
}
// //// /读取客户端商品的成本和奖励 ////

// //// 解码客户端商店 master [@x380kkm 2026-08-31] ////
export function decodeShopContracts(buffer, schema) {
    const rows = decodeOrderedMap(buffer)
    return Object.fromEntries(Object.entries(rows).map(([shopItemId, row]) => [
        shopItemId,
        {
            row,
            ...parseShopContract(row, schema, shopItemId),
        },
    ]))
}
// //// /解码客户端商店 master ////

// //// 定位服务端商店商品 [@x380kkm 2026-08-31] ////
export function resolveServerShopItem(document, shopItemId, row, schema) {
    if (schema.eventTypeIndex !== undefined) {
        return document[String(row[schema.eventTypeIndex])]?.[String(row[schema.eventIdIndex])]?.[shopItemId]
    }
    if (schema.categoryIndex !== undefined) {
        return document[String(row[schema.categoryIndex])]?.[shopItemId]
    }
    return document[shopItemId]
}
// //// /定位服务端商店商品 ////

// //// 投影装备强化阶段和逐级消耗 [@x380kkm 2026-09-21] ////
export function projectEquipmentEnhancementItem(contract, shopItemId) {
    const { row } = contract
    if (row[4] !== "1") throw new Error(`CN equipment enhancement kind is unsupported: ${shopItemId}:${row[4]}`)
    const item = {
        costs: contract.costs,
        rewards: contract.rewards,
        availableFrom: parseOptionalTime(row[22]),
        availableUntil: parseOptionalTime(row[23]),
        stock: -1,
        shopCategoryId: parseInteger(row[0], `equipment-enhancement:${shopItemId}:category`),
        groupId: parseInteger(row[2], `equipment-enhancement:${shopItemId}:group`),
        stage: parseInteger(row[3], `equipment-enhancement:${shopItemId}:stage`),
        equipmentId: parseInteger(row[29], `equipment-enhancement:${shopItemId}:equipment`),
        enhancementMaxLevel: parseInteger(row[30], `equipment-enhancement:${shopItemId}:max-level`),
        requireAwakeningLevel: parseInteger(row[31], `equipment-enhancement:${shopItemId}:awakening`),
    }
    if (contract.userCost !== undefined) item.userCost = contract.userCost
    return item
}
// //// /投影装备强化阶段和逐级消耗 ////

// //// 解析 master 可选时间 [@x380kkm 2026-08-31] ////
function parseOptionalTime(value) {
    return isEmptyField(value) ? null : String(value)
}
// //// /解析 master 可选时间 ////

// //// 投影宝物商店完整商品 [@x380kkm 2026-08-31] ////
export function projectTreasureItem(contract, shopItemId) {
    const item = {
        costs: contract.costs,
        rewards: contract.rewards,
        availableFrom: String(contract.row[18]),
        availableUntil: parseOptionalTime(contract.row[19]),
        stock: parseInteger(contract.row[21], `treasure-shop:${shopItemId}:stock`, { minimum: 1 }),
    }
    const userCost = contract.userCost
    if (userCost !== undefined) item.userCost = userCost
    return item
}
// //// /投影宝物商店完整商品 ////

// //// 生成宝物商店完整目录 [@x380kkm 2026-08-31] ////
function buildTreasureCatalog(contracts) {
    return Object.fromEntries(Object.entries(contracts).map(([shopItemId, contract]) => [
        shopItemId,
        projectTreasureItem(contract, shopItemId),
    ]))
}
// //// /生成宝物商店完整目录 ////

// //// 同步商店成本和奖励 [@x380kkm 2026-08-31] ////
function synchronizeCatalogContracts(document, contracts, schema) {
    for (const [shopItemId, contract] of Object.entries(contracts)) {
        const item = resolveServerShopItem(document, shopItemId, contract.row, schema)
        if (item === undefined) {
            throw new Error(`shop item is missing: ${schema.logicalPath}:${shopItemId}`)
        }
        item.costs = contract.costs
        item.rewards = contract.rewards
        item.availableFrom = contract.availableFrom
        item.availableUntil = contract.availableUntil
        if (contract.userCost === undefined) delete item.userCost
        else item.userCost = contract.userCost
    }
    return document
}
// //// /同步商店成本和奖励 ////

// //// 写入确定性 JSON 资产 [@x380kkm 2026-08-31] ////
function writeJson(filePath, document) {
    fs.writeFileSync(filePath, `${JSON.stringify(document, null, 2)}\n`, "utf8")
}
// //// /写入确定性 JSON 资产 ////

// //// 生成个人服务商店契约资产 [@x380kkm 2026-08-31] ////
export async function generateShopContracts(cdnRoot, assetRoot) {
    const buffers = await readMasterBuffers(cdnRoot, readEntityRecords(cdnRoot), SHOP_CONTRACT_SCHEMAS)
    const counts = {}
    for (const schema of SHOP_CONTRACT_SCHEMAS) {
        const contracts = decodeShopContracts(buffers.get(schema.name), schema)
        const outputPath = path.join(assetRoot, schema.assetFile)
        const document = schema.shopType === 2 ? buildTreasureCatalog(contracts)
            : [3, 5].includes(schema.shopType) ? Object.fromEntries(Object.entries(contracts).map(([id, { row, ...item }]) => [id, item]))
            : synchronizeCatalogContracts(JSON.parse(fs.readFileSync(outputPath, "utf8")), contracts, schema)
        writeJson(outputPath, document)
        counts[schema.shopType] = Object.keys(contracts).length
    }
    return {
        treasure_item_count: counts[2],
        boss_coin_item_count: counts[7],
        star_grain_item_count: counts[9],
        shop_counts: counts,
    }
}
// //// /生成个人服务商店契约资产 ////

// //// 执行商店契约生成器 [@x380kkm 2026-08-31] ////
const entryPath = process.argv[1] ? path.resolve(process.argv[1]) : ""
if (entryPath === fileURLToPath(import.meta.url)) {
    const args = parseArguments(process.argv.slice(2))
    const assetRoot = path.resolve(args["asset-root"] ?? path.join(REPOSITORY_ROOT, "assets"))
    const result = await generateShopContracts(path.resolve(args["cdn-root"]), assetRoot)
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`)
}
// //// /执行商店契约生成器 ////
