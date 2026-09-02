// audience: internal
// # cn-shop-limits-generator
// 此脚本从 CN 商店 master 生成购买上限和周期库存元数据.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"

const SHOP_MASTER_SCHEMAS = [
    { name: "treasure_shop", shopType: 2, logicalPath: "master/shop/treasure_shop.orderedmap", indices: { buyMaxCount: 21, maxFrequency: 22, dailyStock: 23, monthlyStock: null } },
    { name: "event_item_shop", shopType: 4, logicalPath: "master/shop/event_item_shop.orderedmap", indices: { buyMaxCount: 29, maxFrequency: 30, dailyStock: 31, monthlyStock: null } },
    { name: "boss_coin_shop", shopType: 7, logicalPath: "master/shop/boss_coin_shop.orderedmap", indices: { buyMaxCount: 27, maxFrequency: 29, dailyStock: 30, monthlyStock: 31 } },
    { name: "general_shop", shopType: 8, logicalPath: "master/shop/general_shop.orderedmap", indices: { buyMaxCount: 23, maxFrequency: 24, dailyStock: 25, monthlyStock: 26 } },
    { name: "star_grain_shop", shopType: 9, logicalPath: "master/shop/star_grain_shop.orderedmap", indices: { buyMaxCount: 21, maxFrequency: 22, dailyStock: 23, monthlyStock: 24 } },
    { name: "equipment_enhancement_shop", shopType: 10, logicalPath: "master/equipment_enhancement/equipment_enhancement_shop.orderedmap", indices: { buyMaxCount: null, maxFrequency: null, dailyStock: null, monthlyStock: null } },
]

// //// 解析脚本参数 ////
function parseArguments(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 2) {
        const option = argv[index]
        const value = argv[index + 1]
        if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
            throw new Error("usage: generate-cn-shop-limits.mjs --cdn-root <path> --output <path>")
        }
        args[option.slice(2)] = value
    }
    if (!args["cdn-root"] || !args.output) {
        throw new Error("usage: generate-cn-shop-limits.mjs --cdn-root <path> --output <path>")
    }
    return args
}
// //// /解析脚本参数 ////

// //// 解析商店 master 的可选整数 ////
function parseOptionalInteger(value, logicalPath, index) {
    if (value === undefined || value === null || value === "" || value === "(None)") return null
    const parsed = Number(value)
    if (!Number.isSafeInteger(parsed) || parsed < 0) {
        throw new Error(`CN shop master has an invalid integer at ${logicalPath}[${index}]: ${value}`)
    }
    return parsed
}
// //// /解析商店 master 的可选整数 ////

// //// 从商店 master 生成周期库存元数据 ////
function parseShopLimits(buffer, schema) {
    const master = decodeOrderedMap(buffer)
    const limits = {}
    for (const [shopItemId, row] of Object.entries(master)) {
        if (!Array.isArray(row)) throw new Error(`CN shop master row is not an array: ${schema.logicalPath}:${shopItemId}`)
        const values = Object.fromEntries(Object.entries(schema.indices).map(([name, index]) => [
            name,
            index === null ? null : parseOptionalInteger(row[index], schema.logicalPath, index),
        ]))
        limits[shopItemId] = values
    }
    return limits
}
// //// /从商店 master 生成周期库存元数据 ////

// //// 生成商店周期库存资产 ////
export async function generateShopLimits(cdnRoot) {
    const entityRecords = readEntityRecords(cdnRoot)
    const masterBuffers = await readMasterBuffers(cdnRoot, entityRecords, SHOP_MASTER_SCHEMAS)
    return Object.fromEntries(SHOP_MASTER_SCHEMAS.map((schema) => [
        schema.shopType,
        parseShopLimits(masterBuffers.get(schema.name), schema),
    ]))
}
// //// /生成商店周期库存资产 ////

const entryPath = process.argv[1] ? path.resolve(process.argv[1]) : ""
if (entryPath === fileURLToPath(import.meta.url)) {
    const args = parseArguments(process.argv.slice(2))
    const limits = await generateShopLimits(path.resolve(args["cdn-root"]))
    fs.writeFileSync(path.resolve(args.output), `${JSON.stringify(limits, null, 2)}\n`, "utf8")
}
