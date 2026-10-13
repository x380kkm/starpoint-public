// audience: internal
// # collect-cn-period-resource-references
// 此脚本使用活动 schema 和服务端时间目录提取指定时间段的图片与卡池动画引用.
// 输出保留逻辑路径和来源字段, 供 CDN EntityLists 与实际归档核对.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { ACTIVITY_MASTER_SCHEMAS, enumerateMasterRows } from "./cn-activity-master-schema.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..")

// //// 解析时间范围与资源来源 [@x380kkm 2026-09-20] ////
function parseArgs(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 2) {
        const name = argv[index]
        const value = argv[index + 1]
        if (!name.startsWith("--") || !value || value.startsWith("--")) throw new Error(`invalid argument: ${name}`)
        args[name.slice(2)] = value
    }
    const start = args.start ? Date.parse(args.start) : -8640000000000000
    const end = args.end ? Date.parse(args.end) : 8640000000000000
    if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start >= end) {
        throw new Error("--start and --end must define an increasing ISO date range")
    }
    return { ...args, root: path.resolve(args.root ?? ROOT), start, end }
}

function readJson(file) {
    return JSON.parse(fs.readFileSync(file, "utf8"))
}

function overlaps(start, end, period) {
    return Number.isSafeInteger(start) && Number.isSafeInteger(end) && start < period.end && end > period.start
}
// //// /解析时间范围与资源来源 ////

// //// 保留每个资源的活动和字段来源 [@x380kkm 2026-09-20] ////
function addReference(references, logicalPath, owner, evidence) {
    let alternativePaths
    if (logicalPath.startsWith("bgm/")) logicalPath = logicalPath.replace(/\.png$/, ".mp3")
    else if (logicalPath.includes("/animation_background/")) {
        const base = logicalPath.replace(/\.png$/, "")
        logicalPath = `${base}.parts.amf3.deflate`
        alternativePaths = [`${base}.frame.amf3.deflate`]
    }
    const entry = references.get(logicalPath) ?? { logicalPath, owners: new Set(), evidence: new Set() }
    if (alternativePaths) entry.alternativePaths = alternativePaths
    entry.owners.add(owner)
    entry.evidence.add(evidence)
    references.set(logicalPath, entry)
}

function imagePaths(value) {
    if (typeof value !== "string") return []
    return value.split(",").map((item) => item.trim()).filter((item) => item.includes("/"))
        .flatMap((item) => {
            if (item.startsWith("item_icon/")) return ["item_icon/sprite_sheet.atlas.amf3.deflate", "item_icon/sprite_sheet.png"]
            if (item.startsWith("scene/") && item.includes("/sprite_sheet/")) {
                const sheet = item.slice(0, item.indexOf("/sprite_sheet/") + "/sprite_sheet".length)
                return [`${sheet}.atlas.amf3.deflate`, `${sheet}.png`]
            }
            return [item.endsWith(".png") ? item : `${item}.png`]
        })
}

function collectActivityReferences(root, selected, references) {
    const projectionPath = path.join(root, "core/personal-service/assets/cn-activity-master-projection.json")
    const projection = readJson(projectionPath)
    const schemas = new Map(ACTIVITY_MASTER_SCHEMAS.map((schema) => [schema.name, schema]))
    for (const master of projection.masters) {
        const schema = schemas.get(master.name)
        if (!schema?.imageFields?.size) continue
        const data = decodeOrderedMap(fs.readFileSync(path.resolve(path.dirname(projectionPath), master.binary_path)))
        for (const { key, row } of enumerateMasterRows(data)) {
            const owner = `${schema.identityPrefix}:${key.split("/")[0]}`
            if (!selected.has(owner)) continue
            for (const field of schema.imageFields.keys()) {
                for (const logicalPath of imagePaths(row[field])) {
                    addReference(references, logicalPath, owner, `${master.logical_path}:${key}:field:${field}`)
                }
            }
        }
    }
}

function collectGachaReferences(root, selected, references) {
    const pools = readJson(path.join(root, "assets/gacha.json"))
    for (const [id, pool] of Object.entries(pools)) {
        const owner = `gacha:${id}`
        if (!selected.has(owner)) continue
        for (const logicalPath of imagePaths(pool.bannerImage)) {
            addReference(references, logicalPath, owner, "assets/gacha.json:bannerImage")
        }
        for (const field of ["movieName", "guaranteeMovieName", "tutorialMovieId"]) {
            if (pool[field]) addReference(references, `gacha/${pool[field]}.gacha.amf3.deflate`, owner, `assets/gacha.json:${field}`)
        }
    }
}
// //// /保留每个资源的活动和字段来源 ////

// //// 生成期间资源引用集合 [@x380kkm 2026-09-20] ////
function main() {
    const args = parseArgs(process.argv.slice(2))
    const catalog = readJson(path.join(args.root, "assets/cn-activity-catalog-source.json"))
    const activities = !args.start && !args.end ? catalog.activities
        : catalog.activities.filter((activity) => overlaps(activity.default_start_at_ms, activity.default_end_at_ms, args))
    const selected = new Set(activities.map((activity) => activity.activity_id))
    const references = new Map()
    collectActivityReferences(args.root, selected, references)
    collectGachaReferences(args.root, selected, references)
    const extra = args["extra-references"] ? readJson(args["extra-references"]) : null
    for (const reference of extra?.references ?? []) {
        for (const owner of reference.owners) {
            if ((args.start || args.end) && !selected.has(owner)) continue
            for (const evidence of reference.evidence) addReference(references, reference.logicalPath, owner, evidence)
        }
    }
    const report = {
        scope: { start: new Date(args.start).toISOString(), end: new Date(args.end).toISOString(),
            coverage: ["activity_schema_image_fields", "gacha_banner", "gacha_movie"],
            extra: extra?.scope ?? null,
            activities: activities.map((activity) => ({ id: activity.activity_id, name: activity.name })) },
        references: [...references.values()].map((entry) => ({ ...entry, owners: [...entry.owners], evidence: [...entry.evidence] })),
    }
    const output = `${JSON.stringify(report, null, 2)}\n`
    if (args.output) {
        fs.mkdirSync(path.dirname(path.resolve(args.output)), { recursive: true })
        fs.writeFileSync(args.output, output, "utf8")
        console.log(JSON.stringify({ output: args.output, activityCount: activities.length, referenceCount: references.size }))
    } else process.stdout.write(output)
}

main()
// //// /生成期间资源引用集合 ////
