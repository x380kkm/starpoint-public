// audience: internal
// # audit-cn-shop-master-contract
//
// 该脚本核对个人服务商店目录与客户端 CN master 使用相同的商品成本和奖励.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"
import {
    decodeShopContracts,
    projectTreasureItem,
    resolveServerShopItem,
    SHOP_CONTRACT_SCHEMAS,
} from "./generate-cn-shop-contracts.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")

// //// 解析审计参数 [@x380kkm 2026-08-31] ////
function parseArguments(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 2) {
        const option = argv[index]
        const value = argv[index + 1]
        if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
            throw new Error("usage: audit-cn-shop-master-contract.mjs --cdn-root <path> [--asset-root <path>]")
        }
        args[option.slice(2)] = value
    }
    if (!args["cdn-root"]) {
        throw new Error("usage: audit-cn-shop-master-contract.mjs --cdn-root <path> [--asset-root <path>]")
    }
    return args
}
// //// /解析审计参数 ////

// //// 比较 JSON 字段 [@x380kkm 2026-08-31] ////
function sameJson(left, right) {
    return JSON.stringify(left) === JSON.stringify(right)
}
// //// /比较 JSON 字段 ////

// //// 收集服务端商店商品编号 [@x380kkm 2026-08-31] ////
function serverShopItemIds(document, schema) {
    if (schema.eventTypeIndex !== undefined) {
        return new Set(Object.values(document).flatMap((events) =>
            Object.values(events).flatMap((catalog) => Object.keys(catalog))))
    }
    if (schema.categoryIndex !== undefined) {
        return new Set(Object.values(document).flatMap((catalog) => Object.keys(catalog)))
    }
    return new Set(Object.keys(document))
}
// //// /收集服务端商店商品编号 ////

// //// 核对一个商店目录的商品契约 [@x380kkm 2026-08-31] ////
function auditCatalog(document, contracts, schema) {
    const issues = []
    const clientIds = new Set(Object.keys(contracts))
    const serverIds = serverShopItemIds(document, schema)
    for (const [shopItemId, contract] of Object.entries(contracts)) {
        const item = resolveServerShopItem(document, shopItemId, contract.row, schema)
        if (item === undefined) {
            issues.push({ kind: "missing_server_item", shop_type: schema.shopType, shop_item_id: Number(shopItemId) })
            continue
        }
        if (!Array.isArray(item.rewards) || item.rewards.length === 0) {
            issues.push({ kind: "empty_server_rewards", shop_type: schema.shopType, shop_item_id: Number(shopItemId) })
            continue
        }
        const expected = schema.shopType === 2 ? projectTreasureItem(contract, shopItemId) : contract
        const fields = schema.shopType === 2
            ? ["costs", "rewards", "userCost", "availableFrom", "availableUntil", "stock"]
            : ["costs", "rewards"]
        for (const field of fields) {
            if (!sameJson(item[field], expected[field])) {
                issues.push({
                    kind: `${field.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`)}_mismatch`,
                    shop_type: schema.shopType,
                    shop_item_id: Number(shopItemId),
                })
            }
        }
    }
    for (const shopItemId of serverIds) {
        if (!clientIds.has(shopItemId)) {
            issues.push({ kind: "server_only_item", shop_type: schema.shopType, shop_item_id: Number(shopItemId) })
        }
    }
    return issues
}
// //// /核对一个商店目录的商品契约 ////

// //// 汇总审计问题类型 [@x380kkm 2026-08-31] ////
function countIssueKinds(issues) {
    const counts = {}
    for (const issue of issues) counts[issue.kind] = (counts[issue.kind] ?? 0) + 1
    return counts
}
// //// /汇总审计问题类型 ////

// //// 核对全部客户端商店契约 [@x380kkm 2026-08-31] ////
export async function auditShopMasterContract(cdnRoot, assetRoot) {
    const buffers = await readMasterBuffers(cdnRoot, readEntityRecords(cdnRoot), SHOP_CONTRACT_SCHEMAS)
    const issues = []
    const shopCounts = {}
    for (const schema of SHOP_CONTRACT_SCHEMAS) {
        const contracts = decodeShopContracts(buffers.get(schema.name), schema)
        const document = JSON.parse(fs.readFileSync(path.join(assetRoot, schema.assetFile), "utf8"))
        const catalogIssues = auditCatalog(document, contracts, schema)
        issues.push(...catalogIssues)
        shopCounts[String(schema.shopType)] = {
            client_item_count: Object.keys(contracts).length,
            issue_count: catalogIssues.length,
        }
    }
    return {
        summary: {
            issue_count: issues.length,
            issue_kinds: countIssueKinds(issues),
        },
        shop_counts: shopCounts,
        issues,
    }
}
// //// /核对全部客户端商店契约 ////

// //// 执行商店 master 契约审计 [@x380kkm 2026-08-31] ////
const entryPath = process.argv[1] ? path.resolve(process.argv[1]) : ""
if (entryPath === fileURLToPath(import.meta.url)) {
    const args = parseArguments(process.argv.slice(2))
    const assetRoot = path.resolve(args["asset-root"] ?? path.join(REPOSITORY_ROOT, "assets"))
    const report = await auditShopMasterContract(path.resolve(args["cdn-root"]), assetRoot)
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`)
    if (report.summary.issue_count > 0) process.exitCode = 1
}
// //// /执行商店 master 契约审计 ////
