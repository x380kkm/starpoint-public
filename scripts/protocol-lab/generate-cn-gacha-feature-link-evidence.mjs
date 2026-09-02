// audience: internal
// # generate-cn-gacha-feature-link-evidence
// 此脚本从三份 FeatureBanner master 提取普通卡池导航目标及来源摘要.

import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import { FEATURE_BANNER_RELATION_SCHEMAS, enumerateMasterRows } from "./cn-activity-master-schema.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readEntityRecords, readMasterBuffers } from "./generate-cn-activity-catalog.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")
const DEFAULT_OUTPUT_PATH = path.join(REPOSITORY_ROOT, "assets", "gacha-feature-link-evidence.json")

// //// 读取生成器路径参数 [@x380kkm 2026-08-24] ////
function readOption(args, name, fallback) {
    const index = args.indexOf(name)
    if (index < 0) return fallback
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`)
    return path.resolve(value)
}
// //// /读取生成器路径参数 ////

// //// 从 FeatureBanner master 构造普通卡池导航证据 [@x380kkm 2026-08-24] ////
export function buildGachaFeatureLinkEvidence(masterBuffers) {
    const tables = FEATURE_BANNER_RELATION_SCHEMAS.map((schema) => {
        const source = masterBuffers.get(schema.name)
        if (!Buffer.isBuffer(source)) throw new Error(`CN feature banner master is missing: ${schema.name}`)
        const target = schema.targetsByDiscriminator.get("4")
        if (!target) throw new Error(`CN feature banner gacha target is missing: ${schema.name}`)
        const rows = enumerateMasterRows(decodeOrderedMap(source))
        const ordinaryLinks = rows.filter(({ row }) => String(row[schema.discriminatorIndex]) === "4")
        for (const { key, row } of ordinaryLinks) {
            const gachaId = Number(row[target.idIndex])
            if (!Number.isSafeInteger(gachaId) || gachaId < 1) {
                throw new Error(`CN feature banner gacha target is invalid: ${schema.name}/${key}`)
            }
        }
        const ordinaryGachaIds = [...new Set(ordinaryLinks.map(({ row }) => Number(row[target.idIndex])))]
            .sort((left, right) => left - right)
        return {
            name: schema.name,
            logicalPath: schema.logicalPath,
            discriminatorIndex: schema.discriminatorIndex,
            targetIdIndex: target.idIndex,
            sourceSha256: crypto.createHash("sha256").update(source).digest("hex"),
            rowCount: rows.length,
            ordinaryLinkCount: ordinaryLinks.length,
            ordinaryTargetCount: ordinaryGachaIds.length,
            ordinaryGachaIds,
        }
    })
    return { discriminator: 4, tables }
}
// //// /从 FeatureBanner master 构造普通卡池导航证据 ////

// //// 读取 CDN master 并写入导航证据 [@x380kkm 2026-08-24] ////
async function generate(args) {
    const cdnRoot = readOption(args, "--cdn-root", path.join(process.cwd(), ".cdn", "cn"))
    const outputPath = readOption(args, "--output", DEFAULT_OUTPUT_PATH)
    const masterBuffers = await readMasterBuffers(
        cdnRoot,
        readEntityRecords(cdnRoot),
        FEATURE_BANNER_RELATION_SCHEMAS,
    )
    const evidence = buildGachaFeatureLinkEvidence(masterBuffers)
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, `${JSON.stringify(evidence, null, 2)}\n`, "utf8")
    return {
        output: outputPath,
        tableCount: evidence.tables.length,
        ordinaryLinkCount: evidence.tables.reduce((sum, table) => sum + table.ordinaryLinkCount, 0),
    }
}
// //// /读取 CDN master 并写入导航证据 ////

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try {
        process.stdout.write(`${JSON.stringify(await generate(process.argv.slice(2)))}\n`)
    } catch (error) {
        process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
        process.exitCode = 1
    }
}
