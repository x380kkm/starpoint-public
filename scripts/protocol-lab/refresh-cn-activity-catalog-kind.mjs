// audience: internal
// # refresh-cn-activity-catalog-kind
// 此脚本从 CN master 和 EntityLists 重新生成指定类型的活动, 其余活动沿用目录中的记录.

import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { parseArgs } from "node:util"
import { fileURLToPath } from "node:url"
import {
    buildActivityCatalogSource,
    readEntityRecords,
    readMasterBuffers,
} from "./generate-cn-activity-catalog.mjs"
import {
    BANNER_IMAGE_RELATION_SCHEMA,
    FEATURE_BANNER_RELATION_SCHEMAS,
    EVENT_MASTER_SCHEMAS,
    INDEPENDENT_ACTIVITY_SCHEMAS,
} from "./cn-activity-master-schema.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readCnGachaRegionPolicy } from "./generate-cn-gacha-region-policy.mjs"

// //// 按活动身份合并指定类型的目录记录 [@x380kkm 2026-09-21] ////
export function replaceActivityKind(source, generated, kind) {
    const replacements = new Map(generated.activities.map((activity) => [activity.activity_id, activity]))
    const existing = source.activities.filter((activity) => activity.kind === kind)
    if (generated.activities.some((activity) => activity.kind !== kind)
        || (existing.length > 0 && (existing.length !== replacements.size
            || existing.some((activity) => !replacements.has(activity.activity_id))))) {
        throw new Error(`activity master and catalog IDs differ for ${kind}`)
    }
    return {
        ...source,
        asset_version: generated.asset_version,
        generated_at: generated.generated_at,
        activities: existing.length === 0
            ? [...source.activities, ...generated.activities]
            : source.activities.map((activity) => replacements.get(activity.activity_id) ?? activity),
    }
}
// //// /按活动身份合并指定类型的目录记录 ////

// //// 读取活动主表并保存指定类型的目录记录 [@x380kkm 2026-09-21] ////
async function refresh(args) {
    const kind = args.kind
    const activitySchemas = [...EVENT_MASTER_SCHEMAS, ...INDEPENDENT_ACTIVITY_SCHEMAS]
        .filter((schema) => schema.kind === kind)
    if (activitySchemas.length === 0) throw new Error(`unknown activity kind: ${kind}`)
    const cdnRoot = path.resolve(args["cdn-root"] ?? path.join(process.cwd(), ".cdn", "cn"))
    const catalogPath = path.resolve(args.catalog ?? path.join(process.cwd(), "assets", "cn-activity-catalog-source.json"))
    const source = JSON.parse(fs.readFileSync(catalogPath, "utf8"))
    const entityRecords = readEntityRecords(cdnRoot)
    const masterBuffers = await readMasterBuffers(cdnRoot, entityRecords, [
        ...activitySchemas,
        BANNER_IMAGE_RELATION_SCHEMA,
        ...FEATURE_BANNER_RELATION_SCHEMAS,
    ])
    const masterMaps = new Map([...masterBuffers].map(([name, buffer]) => [name, decodeOrderedMap(buffer)]))
    const assetVersion = JSON.parse(fs.readFileSync(path.join(cdnRoot, "path"), "utf8")).info.target_asset_version
    const generated = buildActivityCatalogSource({
        assetVersion,
        clientVersion: source.client_version,
        entityRecords,
        masterMaps,
        regionPolicy: readCnGachaRegionPolicy(),
    })
    const catalog = replaceActivityKind(source, generated, kind)
    fs.writeFileSync(catalogPath, `${JSON.stringify(catalog, null, 2)}\n`, "utf8")
    return {
        output: catalogPath,
        activity_count: catalog.activities.length,
        refreshed_activity_count: generated.activities.length,
        preserved_activity_count: catalog.activities.length - generated.activities.length,
    }
}
// //// /读取活动主表并保存指定类型的目录记录 ////

// //// 解析命令参数并输出目录生成结果 [@x380kkm 2026-09-21] ////
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
    const { values } = parseArgs({
        options: {
            kind: { type: "string" },
            "cdn-root": { type: "string" },
            catalog: { type: "string" },
        },
    })
    refresh(values)
        .then((result) => process.stdout.write(`${JSON.stringify(result)}\n`))
        .catch((error) => {
            process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
            process.exitCode = 1
        })
}
// //// /解析命令参数并输出目录生成结果 ////
