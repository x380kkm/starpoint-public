// audience: internal
// # cn-shop-cost-schedule-generator
// 此脚本从商店商品引用和月度成本主表生成动态兑换费用.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"
import { SHOP_CONTRACT_SCHEMAS } from "./generate-cn-shop-contracts.mjs"

const SCHEDULE_SCHEMA = { name: "shop_cost_item_schedule", logicalPath: "master/shop/shop_cost_item_schedule.orderedmap" }

// //// 汇总全部商品的动态成本引用和有效月份 [@x380kkm 2026-09-21] ////
export async function generateShopCostSchedules(cdnRoot) {
    const buffers = await readMasterBuffers(cdnRoot, readEntityRecords(cdnRoot), [...SHOP_CONTRACT_SCHEMAS, SCHEDULE_SCHEMA])
    const items = {}
    for (const schema of SHOP_CONTRACT_SCHEMAS) {
        for (const [id, row] of Object.entries(decodeOrderedMap(buffers.get(schema.name)))) {
            const group = row[schema.costOffset - 1]
            if (group && group !== "(None)") items[`${schema.shopType}:${id}`] = group
        }
    }
    const schedules = {}
    for (const [group, rows] of Object.entries(decodeOrderedMap(buffers.get(SCHEDULE_SCHEMA.name)))) {
        schedules[group] = Object.values(rows).map(row => {
            const month = Number(row[2])
            if (!Number.isInteger(month) || month < 1 || month > 12) throw new Error(`invalid shop cost month: ${group}:${row[2]}`)
            const costs = []
            for (let index = 3; index < 11; index += 2) {
                if (row[index] === "(None)") continue
                const id = Number(row[index])
                const amount = Number(row[index + 1])
                if (!Number.isSafeInteger(id) || id <= 0 || !Number.isSafeInteger(amount) || amount <= 0) throw new Error(`invalid shop cost: ${group}:${index}`)
                costs.push({ id, amount })
            }
            return { month, availableFrom: row[0], availableUntil: row[1] === "(None)" ? null : row[1], costs }
        })
    }
    for (const group of Object.values(items)) if (!schedules[group]) throw new Error(`missing shop cost group: ${group}`)
    return { items, schedules }
}
// //// /汇总全部商品的动态成本引用和有效月份 ////

const entryPath = process.argv[1] ? path.resolve(process.argv[1]) : ""
if (entryPath === fileURLToPath(import.meta.url)) {
    const args = Object.fromEntries(process.argv.slice(2).reduce((pairs, value, index, values) => index % 2 === 0 ? [...pairs, [value, values[index + 1]]] : pairs, []))
    if (!args["--cdn-root"] || !args["--output"]) throw new Error("usage: generate-cn-shop-cost-schedules.mjs --cdn-root <path> --output <path>")
    const result = await generateShopCostSchedules(path.resolve(args["--cdn-root"]))
    fs.writeFileSync(path.resolve(args["--output"]), `${JSON.stringify(result, null, 2)}\n`, "utf8")
    process.stdout.write(`${JSON.stringify({ item_count: Object.keys(result.items).length, schedule_count: Object.values(result.schedules).reduce((count, rows) => count + rows.length, 0) })}\n`)
}
