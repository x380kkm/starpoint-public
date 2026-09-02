// audience: internal
// # generate-cn-item-catalog
// 此脚本从 CN item orderedmap 生成邮件发放目录. 字段位置对应 iOS 1.8.4 使用的 CN master 记录.

import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"

const ITEM_MASTER_SCHEMA = {
    name: "item",
    logicalPath: "master/item/item.orderedmap",
}

const ITEM_KIND_BY_EFFECT = new Map([
    [0, "character-growth"],
    [1, "equipment-growth"],
    [2, "stamina"],
    [4, "character-growth"],
    [5, "character-growth"],
    [6, "equipment-growth"],
    [7, "equipment-growth"],
    [8, "ticket"],
    [9, "event"],
    [11, "ability-soul"],
    [12, "exchange"],
    [13, "exchange"],
    [15, "exchange"],
    [16, "craft"],
    [18, "quest"],
    [19, "quest"],
    [20, "star-grain"],
    [21, "character-growth"],
    [22, "character-growth"],
])

// //// 解析生成器参数 [@x380kkm 2026-08-24] ////
function parseArgs(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index]
        if (!argument.startsWith("--")) throw new Error(`unexpected argument: ${argument}`)
        const name = argument.slice(2)
        const value = argv[index + 1]
        if (value === undefined || value.startsWith("--")) throw new Error(`missing value for --${name}`)
        args[name] = value
        index += 1
    }
    return args
}
// //// /解析生成器参数 ////

// //// 读取物品记录中的整数和可选文本 [@x380kkm 2026-08-24] ////
function requiredInteger(value, field, itemId) {
    const number = Number(value)
    if (!Number.isSafeInteger(number) || number < 0) {
        throw new Error(`CN item ${itemId} has an invalid ${field}`)
    }
    return number
}

function optionalInteger(value, field, itemId) {
    if (value === "" || value === "(None)") return null
    return requiredInteger(value, field, itemId)
}

function optionalText(value) {
    return value === "" || value === "(None)" ? null : value
}
// //// /读取物品记录中的整数和可选文本 ////

// //// 映射 CN item master 记录 [@x380kkm 2026-08-24] ////
function buildItem(id, row) {
    if (!Array.isArray(row) || row.length !== 23) {
        throw new Error(`CN item ${id} has an unexpected field count`)
    }
    if (!/^\d+$/.test(id) || Number(id) < 1) {
        throw new Error(`CN item ${id} has an invalid identifier`)
    }
    const name = optionalText(row[2])
    if (name === null) throw new Error(`CN item ${id} has no name`)
    const effectKind = requiredInteger(row[6], "effect kind", id)
    return {
        id,
        string_id: optionalText(row[0]),
        name,
        thumbnail_id: optionalText(row[3]),
        description: optionalText(row[5]),
        effect_kind: effectKind,
        category: requiredInteger(row[14], "category", id),
        group: optionalInteger(row[15], "group", id),
        kind: ITEM_KIND_BY_EFFECT.get(effectKind) ?? "other",
    }
}
// //// /映射 CN item master 记录 ////

// //// 生成邮件物品目录 [@x380kkm 2026-08-24] ////
async function generate(args) {
    const cdnRoot = path.resolve(args["cdn-root"] ?? path.join(process.cwd(), ".cdn", "cn"))
    const outputPath = path.resolve(args.output ?? path.join(process.cwd(), "assets", "cn_item_catalog.json"))
    const entityRecords = readEntityRecords(cdnRoot)
    const masterBuffers = await readMasterBuffers(cdnRoot, entityRecords, [ITEM_MASTER_SCHEMA])
    const masterBuffer = masterBuffers.get(ITEM_MASTER_SCHEMA.name)
    const rows = decodeOrderedMap(masterBuffer)
    const items = Object.entries(rows)
        .map(([id, row]) => buildItem(id, row))
        .sort((left, right) => Number(left.id) - Number(right.id))
    const catalog = {
        format_version: 1,
        region: "cn",
        logical_path: ITEM_MASTER_SCHEMA.logicalPath,
        source_sha256: crypto.createHash("sha256").update(masterBuffer).digest("hex"),
        row_count: items.length,
        items,
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, `${JSON.stringify(catalog, null, 2)}\n`, "utf8")
    return { output: outputPath, row_count: items.length, source_sha256: catalog.source_sha256 }
}
// //// /生成邮件物品目录 ////

// //// 运行邮件物品目录生成器 [@x380kkm 2026-08-24] ////
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
    generate(parseArgs(process.argv.slice(2)))
        .then((result) => process.stdout.write(`${JSON.stringify(result)}\n`))
        .catch((error) => {
            process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
            process.exitCode = 1
        })
}
// //// /运行邮件物品目录生成器 ////
