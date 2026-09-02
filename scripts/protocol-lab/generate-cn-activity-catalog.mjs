// audience: internal
// # generate-cn-activity-catalog
// 此脚本从 CN orderedmap 和 EntityLists 生成活动目录源文件. 活动身份只来自 master 记录, 图片必须同时具有 master 引用和 EntityLists 记录.

import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import unzipper from "unzipper"
import {
    BANNER_IMAGE_RELATION_SCHEMA,
    CATALOG_MASTER_SCHEMAS,
    collectMasterRows,
    enumerateEventReferences,
    EVENT_MASTER_SCHEMAS,
    FEATURE_BANNER_RELATION_SCHEMAS,
    INDEPENDENT_ACTIVITY_SCHEMAS,
} from "./cn-activity-master-schema.mjs"
import {
    appendActivityIdRelationCandidates,
    appendExactPathRelationCandidates,
    buildActivityCatalogCoverage,
    collectImageCandidates,
    legacyBannerProjection,
} from "./cn-activity-image-relations.mjs"
import { assetEntryPaths, hashCnAssetPath } from "./cn-asset-paths.mjs"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { readCnGachaRegionPolicy } from "./generate-cn-gacha-region-policy.mjs"

const COMMON_ARCHIVE_DIRECTORIES = ["archive-common-full", "archive-common-diff"]

export { assetEntryPaths, hashCnAssetPath }

// //// 解析生成器参数 [@x380kkm 2026-08-19] ////
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

// //// 读取当前 EntityLists 和资产版本 [@x380kkm 2026-08-19] ////
export function readEntityRecords(cdnRoot) {
    const entityPath = path.join(cdnRoot, "entities", "PathFile.csv")
    const records = new Map()
    const lines = fs.readFileSync(entityPath, "utf8").split(/\r?\n/)
    for (const line of lines) {
        if (!line) continue
        const fields = line.split(",")
        if (fields.length !== 5) throw new Error("CN EntityLists row has an unexpected field count")
        const [entryPath, version, byteLength, digest, assetKind] = fields
        const parsedByteLength = Number(byteLength)
        if (!/^production\/(?:(?:android|ios|medium)_)?upload\/[a-f0-9]{2}\/[a-f0-9]{38}$/.test(entryPath)) {
            throw new Error(`CN EntityLists contains an invalid asset path: ${entryPath}`)
        }
        if (!version || !Number.isSafeInteger(parsedByteLength) || parsedByteLength < 1
            || !/^[A-Za-z0-9_-]{43}$/.test(digest) || !assetKind) {
            throw new Error(`CN EntityLists contains invalid asset metadata: ${entryPath}`)
        }
        records.set(entryPath, { assetKind, byteLength: parsedByteLength, digest, version })
    }
    return records
}

function readAssetVersion(cdnRoot) {
    const pathManifest = JSON.parse(fs.readFileSync(path.join(cdnRoot, "path"), "utf8"))
    const version = pathManifest?.info?.target_asset_version
    return typeof version === "string" && version.length > 0 ? version : null
}
// //// /读取当前 EntityLists 和资产版本 ////

// //// 匹配 EntityLists 原始资源记录 [@x380kkm 2026-08-19] ////
export function computeCnEntityDigest(buffer) {
    return crypto.createHash("sha256").update(buffer).digest("base64")
        .replaceAll("+", "_")
        .replaceAll("/", "-")
        .replace(/=+$/, "")
}

export function matchesEntityRecord(buffer, record) {
    return buffer.byteLength === record.byteLength
        && computeCnEntityDigest(buffer) === record.digest
}
// //// /匹配 EntityLists 原始资源记录 ////

// //// 从本地 archive 批量读取当前存在的活动 master [@x380kkm 2026-08-19] ////
export async function readMasterBuffers(cdnRoot, entityRecords, schemas = CATALOG_MASTER_SCHEMAS) {
    const targets = new Map()
    for (const schema of schemas) {
        const hash = hashCnAssetPath(schema.logicalPath)
        const sourceEntry = assetEntryPaths(hash).find((entryPath) => entityRecords.has(entryPath))
        if (!sourceEntry) continue
        targets.set(sourceEntry, {
            logicalPath: schema.logicalPath,
            name: schema.name,
            record: entityRecords.get(sourceEntry),
        })
    }
    const buffers = new Map()

    for (const directoryName of COMMON_ARCHIVE_DIRECTORIES) {
        const directory = path.join(cdnRoot, directoryName)
        if (!fs.existsSync(directory)) continue
        const archiveNames = fs.readdirSync(directory).filter((name) => name.endsWith(".zip")).sort()
        for (const archiveName of archiveNames) {
            if (buffers.size === targets.size) break
            const archive = await unzipper.Open.file(path.join(directory, archiveName))
            for (const entry of archive.files) {
                const target = targets.get(entry.path)
                if (!target || buffers.has(target.name)) continue
                if (entry.type !== "File" || entry.uncompressedSize > 64 * 1024 * 1024) {
                    throw new Error(`CN master entry is invalid: ${target.logicalPath}`)
                }
                if (entry.uncompressedSize !== target.record.byteLength) continue
                const buffer = await entry.buffer()
                if (!matchesEntityRecord(buffer, target.record)) continue
                buffers.set(target.name, buffer)
            }
        }
    }

    for (const target of targets.values()) {
        if (!buffers.has(target.name)) throw new Error(`CN master asset is missing: ${target.logicalPath}`)
    }
    return buffers
}
// //// /从本地 archive 批量读取当前存在的活动 master ////

export { parseCnMasterTimestamp }

// //// 读取客户端活动时间窗口 [@x380kkm 2026-08-24] ////
function parseActivityWindow(schema, rows) {
    if (!Number.isInteger(schema.startIndex) || !Number.isInteger(schema.endIndex)) return {}
    for (const row of rows) {
        const defaultStartAt = parseCnMasterTimestamp(row[schema.startIndex])
        const defaultEndAt = parseCnMasterTimestamp(row[schema.endIndex])
        if (defaultStartAt !== null && defaultEndAt !== null && defaultEndAt > defaultStartAt) {
            return { default_start_at_ms: defaultStartAt, default_end_at_ms: defaultEndAt }
        }
    }
    return {}
}
// //// /读取客户端活动时间窗口 ////

// //// 构造所有已识别 master 的活动记录 [@x380kkm 2026-08-19] ////
function activityName(schema, id, rows) {
    for (const index of schema.nameIndexes) {
        for (const row of rows) {
            const value = row[index]
            if (typeof value === "string" && value && value !== "(None)" && value !== "活动名") return value
        }
    }
    return `未命名活动 ${id}`
}

const GACHA_PAGE_LABELS = new Map([
    ["0", "普通卡池"],
    ["1", "每账号一次十连"],
    ["2", "单抽和十连票券"],
    ["3", "单抽票券"],
    ["4", "十连票券"],
    ["5", "疯狂十连票券"],
    ["6", "单抽卡池"],
    ["7", "十连卡池"],
    ["8", "无每日优惠卡池"],
])

// //// 描述管理页中的卡池类型与 banner 状态 [@x380kkm 2026-08-24] ////
function isMasterTrue(value) {
    return String(value).toLowerCase() === "true"
}

function activityPresentation(schema, rows, imageCandidates) {
    if (schema.name !== "gacha") return { tags: ["CN", schema.label], description: "" }
    const row = rows[0] ?? []
    const pageKind = String(row[4] ?? "")
    const pageLabel = GACHA_PAGE_LABELS.get(pageKind) ?? `未知卡池类型 ${pageKind}`
    const tags = ["CN", schema.label, `gacha:page-${pageKind}`]
    if (isMasterTrue(row[38])) tags.push("gacha:tutorial")
    if (isMasterTrue(row[43])) tags.push("gacha:comeback")
    if (isMasterTrue(row[46])) tags.push("gacha:stars")
    if (imageCandidates.length === 0) tags.push("banner:unresolved")
    const bannerImage = String(row[3] ?? "")
    const resourceStatus = imageCandidates.length === 0 ? " 当前包内未解析到对应纹理." : ""
    return {
        tags,
        description: `卡池类型: ${pageLabel}. Banner: ${bannerImage}.${resourceStatus}`,
    }
}
// //// /描述管理页中的卡池类型与 banner 状态 ////

function buildActivity(schema, id, rows, entityRecords) {
    const imageCandidates = collectImageCandidates(schema, rows, entityRecords)
    const presentation = activityPresentation(schema, rows, imageCandidates)
    return {
        activity_id: `${schema.identityPrefix}:${id}`,
        name: activityName(schema, id, rows),
        kind: schema.kind,
        tags: presentation.tags,
        description: presentation.description,
        image_candidates: imageCandidates,
        ...legacyBannerProjection(imageCandidates),
        ...parseActivityWindow(schema, rows),
        source_table: schema.logicalPath,
    }
}

// //// 在关系表补充候选后同步活动图片投影 [@x380kkm 2026-08-24] ////
function synchronizeActivityPresentation(activity) {
    Object.assign(activity, legacyBannerProjection(activity.image_candidates))
    if (activity.kind !== "gacha") return

    const unresolvedTag = "banner:unresolved"
    const unresolvedSentence = " 当前包内未解析到对应纹理."
    const hasImage = activity.image_candidates.length > 0
    activity.tags = activity.tags.filter((tag) => tag !== unresolvedTag)
    activity.description = activity.description.replace(unresolvedSentence, "")
    if (!hasImage) {
        activity.tags.push(unresolvedTag)
        activity.description += unresolvedSentence
    }
}
// //// /在关系表补充候选后同步活动图片投影 ////

function requireUniqueActivityIds(activities) {
    const activityIds = new Set()
    for (const activity of activities) {
        if (activityIds.has(activity.activity_id)) {
            throw new Error(`duplicate CN activity ID: ${activity.activity_id}`)
        }
        activityIds.add(activity.activity_id)
    }
}

function gachaRegionPolicyView(regionPolicy) {
    if (!regionPolicy) return null
    const excludedRegionalAliases = new Set(Object.keys(regionPolicy.excludedRegionalAliases))
    const normalizedCoverageAliases = new Set(Object.keys(regionPolicy.normalizedCoverageAliases))
    const temporaryAliases = new Set(Object.keys(regionPolicy.temporaryAliases))
    return {
        bannerPathOverrides: regionPolicy.bannerPathOverrides,
        excludedRegionalAliases,
        normalizedCoverageAliases,
        temporaryAliases,
    }
}

function applyGachaCatalogPolicy(schema, recordId, rows, policy) {
    if (schema.name !== "gacha" || !policy) return rows
    const bannerImage = policy.bannerPathOverrides[recordId]
    if (!bannerImage) return rows
    return rows.map((row) => {
        const patched = [...row]
        patched[3] = bannerImage
        return patched
    })
}

export function buildActivityCatalogArtifacts({
    assetVersion,
    clientVersion,
    entityRecords,
    masterMaps,
    raidMap,
    rankingMap,
    regionPolicy,
}) {
    const maps = masterMaps instanceof Map ? new Map(masterMaps) : new Map(Object.entries(masterMaps ?? {}))
    if (raidMap) maps.set("raid_event", raidMap)
    if (rankingMap) maps.set("ranking_event", rankingMap)
    const activities = []
    const gachaPolicy = gachaRegionPolicyView(regionPolicy)
    let excludedRegionalGachaCount = 0
    let excludedNormalizedCoverageGachaCount = 0
    let excludedTemporaryGachaCount = 0
    let duplicateEventReferenceCount = 0

    const eventListMap = maps.get("event_list")
    let additionalEventMasterCount = 0
    if (eventListMap) {
        const eventReferences = new Set()
        for (const reference of enumerateEventReferences(eventListMap)) {
            const referenceKey = `${reference.schema.name}:${reference.eventId}`
            if (eventReferences.has(referenceKey)) {
                duplicateEventReferenceCount += 1
                continue
            }
            eventReferences.add(referenceKey)
            const rootRow = maps.get(reference.schema.name)?.[reference.eventId]
            const rows = collectMasterRows(rootRow)
            if (rows.length === 0) {
                throw new Error(`CN EventList root row is missing: ${reference.schema.name}:${reference.eventId}`)
            }
            activities.push(buildActivity(reference.schema, reference.eventId, rows, entityRecords))
        }
        for (const schema of EVENT_MASTER_SCHEMAS) {
            const masterMap = maps.get(schema.name)
            if (!masterMap) continue
            for (const [eventId, rootRow] of Object.entries(masterMap)) {
                if (!/^\d+$/.test(eventId) || Number(eventId) < 1) continue
                const referenceKey = `${schema.name}:${eventId}`
                if (eventReferences.has(referenceKey)) continue
                const rows = collectMasterRows(rootRow)
                if (rows.length === 0) continue
                activities.push(buildActivity(schema, eventId, rows, entityRecords))
                additionalEventMasterCount += 1
            }
        }
    } else {
        for (const schema of EVENT_MASTER_SCHEMAS) {
            const masterMap = maps.get(schema.name)
            if (!masterMap) continue
            for (const [eventId, rootRow] of Object.entries(masterMap)) {
                if (!/^\d+$/.test(eventId) || Number(eventId) < 1) continue
                const rows = collectMasterRows(rootRow)
                if (rows.length > 0) activities.push(buildActivity(schema, eventId, rows, entityRecords))
            }
        }
    }
    for (const schema of INDEPENDENT_ACTIVITY_SCHEMAS) {
        const masterMap = maps.get(schema.name)
        if (!masterMap) continue
        for (const [recordId, rootRow] of Object.entries(masterMap)) {
            if (!/^\d+$/.test(recordId) || Number(recordId) < 1) continue
            if (schema.name === "gacha" && gachaPolicy?.excludedRegionalAliases.has(recordId)) {
                excludedRegionalGachaCount += 1
                continue
            }
            if (schema.name === "gacha" && gachaPolicy?.normalizedCoverageAliases.has(recordId)) {
                excludedNormalizedCoverageGachaCount += 1
                continue
            }
            if (schema.name === "gacha" && gachaPolicy?.temporaryAliases.has(recordId)) {
                excludedTemporaryGachaCount += 1
                continue
            }
            const rows = applyGachaCatalogPolicy(
                schema,
                recordId,
                collectMasterRows(rootRow),
                gachaPolicy,
            )
            if (rows.length > 0) activities.push(buildActivity(schema, recordId, rows, entityRecords))
        }
    }
    requireUniqueActivityIds(activities)
    const relationTables = [
        appendExactPathRelationCandidates(
            activities,
            BANNER_IMAGE_RELATION_SCHEMA,
            maps.get(BANNER_IMAGE_RELATION_SCHEMA.name),
            entityRecords,
        ),
        ...FEATURE_BANNER_RELATION_SCHEMAS.map((schema) => appendActivityIdRelationCandidates(
            activities,
            schema,
            maps.get(schema.name),
            entityRecords,
        )),
    ]
    for (const activity of activities) synchronizeActivityPresentation(activity)
    activities.sort((left, right) => left.kind.localeCompare(right.kind)
        || left.activity_id.localeCompare(right.activity_id, "en", { numeric: true }))
    const coverage = buildActivityCatalogCoverage(maps, activities, relationTables, duplicateEventReferenceCount)
    coverage.additional_event_master_count = additionalEventMasterCount
    coverage.gacha_region_policy = {
        excluded_regional_alias_count: excludedRegionalGachaCount,
        excluded_normalized_coverage_alias_count: excludedNormalizedCoverageGachaCount,
        excluded_temporary_alias_count: excludedTemporaryGachaCount,
        retained_gacha_activity_count: activities.filter((activity) => activity.kind === "gacha").length,
    }
    return {
        catalog: {
            format_version: 1,
            region: "cn",
            client_version: clientVersion,
            asset_version: assetVersion,
            generated_at: new Date().toISOString(),
            activities,
        },
        coverage,
    }
}
// //// /构造所有已识别 master 的活动记录 ////

// //// 构造运行时活动目录 [@x380kkm 2026-08-24] ////
export function buildActivityCatalogSource(options) {
    return buildActivityCatalogArtifacts(options).catalog
}

// //// 生成本地活动目录源文件 [@x380kkm 2026-08-19] ////
async function generate(args) {
    const cdnRoot = path.resolve(args["cdn-root"] ?? path.join(process.cwd(), ".cdn", "cn"))
    const outputPath = path.resolve(args.output ?? path.join(cdnRoot, "activity-catalog-source.json"))
    const clientVersion = args["client-version"] ?? "1.8.1"
    const entityRecords = readEntityRecords(cdnRoot)
    const masterBuffers = await readMasterBuffers(cdnRoot, entityRecords)
    const masterMaps = new Map([...masterBuffers].map(([name, buffer]) => [name, decodeOrderedMap(buffer)]))
    const source = buildActivityCatalogSource({
        assetVersion: readAssetVersion(cdnRoot),
        clientVersion,
        entityRecords,
        masterMaps,
        regionPolicy: readCnGachaRegionPolicy(),
    })
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, `${JSON.stringify(source, null, 2)}\n`, "utf8")
    return {
        activity_count: source.activities.length,
        master_count: masterMaps.size,
        output: outputPath,
    }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
    generate(parseArgs(process.argv.slice(2)))
        .then((result) => process.stdout.write(`${JSON.stringify(result)}\n`))
        .catch((error) => {
            process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
            process.exitCode = 1
        })
}
// //// /生成本地活动目录源文件 ////
