// audience: internal
// # cn-shop-master-whitelist-generator
// 此脚本从 CN CDN 中的客户端 master 生成商店商品 ID 白名单.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"

const SHOP_MASTER_SCHEMAS = [
    { name: "treasure_shop", shopType: 2, logicalPath: "master/shop/treasure_shop.orderedmap" },
    { name: "event_item_shop", shopType: 4, logicalPath: "master/shop/event_item_shop.orderedmap" },
    { name: "boss_coin_shop", shopType: 7, logicalPath: "master/shop/boss_coin_shop.orderedmap" },
    { name: "general_shop", shopType: 8, logicalPath: "master/shop/general_shop.orderedmap" },
    { name: "star_grain_shop", shopType: 9, logicalPath: "master/shop/star_grain_shop.orderedmap" },
    {
        name: "equipment_enhancement_shop",
        shopType: 10,
        logicalPath: "master/equipment_enhancement/equipment_enhancement_shop.orderedmap",
    },
]

// //// 解析生成器参数 [@x380kkm 2026-08-23] ////
function parseArguments(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 2) {
        const option = argv[index]
        const value = argv[index + 1]
        if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
            throw new Error("usage: generate-cn-shop-master-whitelists.mjs --cdn-root <path> --output <path>")
        }
        args[option.slice(2)] = value
    }
    if (!args["cdn-root"] || !args.output) {
        throw new Error("usage: generate-cn-shop-master-whitelists.mjs --cdn-root <path> --output <path>")
    }
    return args
}
// //// /解析生成器参数 ////

// //// 从客户端 master 提取商店商品 ID [@x380kkm 2026-08-23] ////
function parseShopItemIds(buffer, logicalPath) {
    const master = decodeOrderedMap(buffer)
    return Object.keys(master).map((key) => {
        const shopItemId = Number(key)
        if (!Number.isSafeInteger(shopItemId) || shopItemId <= 0 || String(shopItemId) !== key) {
            throw new Error(`CN shop master contains an invalid item id: ${logicalPath}:${key}`)
        }
        return shopItemId
    }).sort((left, right) => left - right)
}
// //// /从客户端 master 提取商店商品 ID ////

// //// 生成客户端商店白名单资产 [@x380kkm 2026-08-23] ////
export async function generateShopMasterWhitelists(cdnRoot) {
    const entityRecords = readEntityRecords(cdnRoot)
    const masterBuffers = await readMasterBuffers(cdnRoot, entityRecords, SHOP_MASTER_SCHEMAS)
    return Object.fromEntries(SHOP_MASTER_SCHEMAS.map((schema) => [
        schema.shopType,
        parseShopItemIds(masterBuffers.get(schema.name), schema.logicalPath),
    ]))
}
// //// /生成客户端商店白名单资产 ////

// //// 格式化商店白名单资产 [@x380kkm 2026-08-23] ////
function formatShopMasterWhitelists(whitelists) {
    const shopTypes = Object.entries(whitelists).map(([shopType, itemIds]) => {
        const rows = []
        for (let index = 0; index < itemIds.length; index += 25) {
            rows.push(`    ${itemIds.slice(index, index + 25).join(", ")}`)
        }
        return `  "${shopType}": [\n${rows.join(",\n")}\n  ]`
    })
    return `{\n${shopTypes.join(",\n")}\n}\n`
}
// //// /格式化商店白名单资产 ////

const entryPath = process.argv[1] ? path.resolve(process.argv[1]) : ""
if (entryPath === fileURLToPath(import.meta.url)) {
    const args = parseArguments(process.argv.slice(2))
    const whitelists = await generateShopMasterWhitelists(path.resolve(args["cdn-root"]))
    fs.writeFileSync(path.resolve(args.output), formatShopMasterWhitelists(whitelists), "utf8")
}
