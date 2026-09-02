// audience: internal
// # cn-activity-image-relations
// 此模块解析 CN master 图片引用, 连接活动记录并生成关系覆盖信息.

import { assetEntryPaths, hashCnAssetPath } from "./cn-asset-paths.mjs"
import {
    ACTIVITY_SCHEMA_BY_NAME,
    ACTIVITY_MASTER_SCHEMAS,
    collectMasterRows,
    enumerateMasterRows,
    EVENT_SCHEMA_BY_KIND,
    PENDING_ACTIVITY_RELATIONS,
} from "./cn-activity-master-schema.mjs"

const IMAGE_SOURCE_PRIORITY = new Map([
    ["activity_banner", 0],
    ["home_banner", 1],
    ["notice_banner", 2],
    ["activity_entry", 3],
    ["boss_cover", 4],
    ["quest_cover", 5],
    ["shop_exchange", 6],
    ["activity_logo", 7],
    ["activity_background", 8],
    ["master_image", 9],
])

// //// 解析 master 行引用的图片资源 [@x380kkm 2026-08-19] ////
function splitMasterList(value) {
    if (typeof value !== "string") return []
    return value.split(",").map((item) => item.trim()).filter((item) => item && item !== "(None)")
}

function imageSourceType(logicalPath) {
    const value = logicalPath.toLowerCase()
    if (value.includes("home") || value.includes("top_banner")) return "home_banner"
    if (value.includes("notice") || value.includes("news")) return "notice_banner"
    if (value.includes("thumbnail") || value.includes("boss")) return "boss_cover"
    if (value.includes("quest")) return "quest_cover"
    if (value.includes("shop") || value.includes("exchange")) return "shop_exchange"
    if (value.includes("logo")) return "activity_logo"
    if (value.includes("background")) return "activity_background"
    if (value.includes("banner")) return "activity_banner"
    return "master_image"
}

function canonicalImageLogicalPath(logicalPath) {
    return logicalPath.endsWith(".png") ? logicalPath : `${logicalPath}.png`
}

function resolveImageCandidate(reference, entityRecords) {
    const imagePath = canonicalImageLogicalPath(reference.logicalPath)
    const sourceHash = hashCnAssetPath(imagePath)
    const sourceEntry = assetEntryPaths(sourceHash).find((entryPath) => entityRecords.has(entryPath))
    if (!sourceEntry) return null
    const record = entityRecords.get(sourceEntry)
    return {
        source_hash: sourceHash,
        source_type: reference.sourceType,
        logical_path: imagePath,
        source_entry: sourceEntry,
        source_version: record.version,
        source_byte_length: record.byteLength,
        source_digest: record.digest,
        source_asset_kind: record.assetKind,
        width: null,
        height: null,
        association_confidence: reference.confidence,
        evidence: reference.evidence,
    }
}

function sortImageCandidates(candidates) {
    return candidates.sort((left, right) =>
        (IMAGE_SOURCE_PRIORITY.get(left.source_type) ?? 99) - (IMAGE_SOURCE_PRIORITY.get(right.source_type) ?? 99)
        || left.logical_path.localeCompare(right.logical_path))
}

export function collectImageCandidates(schema, rows, entityRecords) {
    const candidates = new Map()
    for (const row of rows) {
        for (const [index, declaredType] of schema.imageFields) {
            for (const logicalPath of splitMasterList(row[index])) {
                if (!logicalPath.includes("/")) continue
                const candidate = resolveImageCandidate({
                    logicalPath,
                    sourceType: declaredType === "auto" ? imageSourceType(logicalPath) : declaredType,
                    confidence: "direct-field",
                    evidence: `master:${schema.name}:field:${index}`,
                }, entityRecords)
                if (candidate && !candidates.has(candidate.source_hash)) candidates.set(candidate.source_hash, candidate)
            }
        }
    }
    return sortImageCandidates([...candidates.values()])
}

export function legacyBannerProjection(imageCandidates) {
    const candidate = imageCandidates[0]
    if (!candidate) return {}
    return {
        banner_candidate: candidate.source_hash,
        banner_logical_path: candidate.logical_path,
        banner_source_entry: candidate.source_entry,
        banner_source_version: candidate.source_version,
        banner_source_byte_length: candidate.source_byte_length,
        banner_source_digest: candidate.source_digest,
    }
}
// //// /解析 master 行引用的图片资源 ////

// //// 通过完全相等的逻辑路径连接图片关系行 [@x380kkm 2026-08-19] ////
function collectRelationImageReferences(schema, row) {
    const references = []
    for (const [fieldIndex, sourceType] of schema.pathFields) {
        for (const logicalPath of splitMasterList(row[fieldIndex])) {
            if (!logicalPath.includes("/")) continue
            references.push({
                fieldIndex,
                logicalPath: canonicalImageLogicalPath(logicalPath),
                sourceType,
            })
        }
    }
    return references
}

function indexExactPathRelations(schema, masterMap) {
    const relationsByPath = new Map()
    const relationsByKey = new Map()
    for (const row of collectMasterRows(masterMap)) {
        const stringId = row[0]
        if (typeof stringId !== "string" || !stringId || stringId === "(None)") continue
        const paths = collectRelationImageReferences(schema, row)
        if (paths.length === 0) continue
        const relationKey = JSON.stringify([
            stringId,
            paths.map((reference) => `${reference.fieldIndex}:${reference.logicalPath}`).sort(),
        ])
        const relation = relationsByKey.get(relationKey) ?? { paths, stringId }
        relationsByKey.set(relationKey, relation)
        for (const reference of paths) {
            const relations = relationsByPath.get(reference.logicalPath) ?? []
            if (!relations.includes(relation)) relations.push(relation)
            relationsByPath.set(reference.logicalPath, relations)
        }
    }
    return relationsByPath
}

export function appendExactPathRelationCandidates(activities, schema, masterMap, entityRecords) {
    const coverage = {
        name: schema.name,
        source_table: schema.logicalPath,
        status: masterMap ? "included" : "missing",
        matched_activity_count: 0,
        candidate_count: 0,
        ambiguous_path_count: 0,
        unresolved_count: activities.length,
    }
    if (!masterMap) return coverage

    const relationsByPath = indexExactPathRelations(schema, masterMap)
    const ambiguousPaths = new Set()
    for (const activity of activities) {
        const matchedRelations = new Set()
        for (const candidate of activity.image_candidates) {
            const relations = relationsByPath.get(candidate.logical_path) ?? []
            if (relations.length > 1) {
                ambiguousPaths.add(candidate.logical_path)
                continue
            }
            if (relations.length === 1) matchedRelations.add(relations[0])
        }
        if (matchedRelations.size === 0) continue

        coverage.matched_activity_count += 1
        const candidatesByHash = new Map(activity.image_candidates.map((candidate) => [candidate.source_hash, candidate]))
        for (const relation of matchedRelations) {
            for (const reference of relation.paths) {
                const candidate = resolveImageCandidate({
                    logicalPath: reference.logicalPath,
                    sourceType: reference.sourceType,
                    confidence: "exact-logical-path",
                    evidence: `master:${schema.name}:string_id:${relation.stringId}:field:${reference.fieldIndex}`,
                }, entityRecords)
                if (!candidate || candidatesByHash.has(candidate.source_hash)) continue
                candidatesByHash.set(candidate.source_hash, candidate)
                coverage.candidate_count += 1
            }
        }
        activity.image_candidates = sortImageCandidates([...candidatesByHash.values()])
        Object.assign(activity, legacyBannerProjection(activity.image_candidates))
    }
    coverage.ambiguous_path_count = ambiguousPaths.size
    coverage.unresolved_count -= coverage.matched_activity_count
    return coverage
}
// //// /通过完全相等的逻辑路径连接图片关系行 ////

// //// 通过客户端声明的活动 ID 连接图片关系行 [@x380kkm 2026-08-19] ////
function resolveActivityRelationTarget(schema, row) {
    const discriminator = String(row[schema.discriminatorIndex] ?? "")
    const target = schema.targetsByDiscriminator.get(discriminator)
    if (!target) return { status: "unsupported" }
    const recordId = String(row[target.idIndex] ?? "")
    if (!/^\d+$/.test(recordId) || Number(recordId) < 1) return { status: "invalid" }
    const activitySchema = target.activitySchemaName
        ? ACTIVITY_SCHEMA_BY_NAME.get(target.activitySchemaName)
        : EVENT_SCHEMA_BY_KIND.get(String(row[target.eventKindIndex] ?? ""))
    if (!activitySchema) return { status: "invalid" }
    return { activityId: `${activitySchema.identityPrefix}:${recordId}`, discriminator, status: "resolved" }
}

function indexActivitiesById(activities) {
    const activitiesById = new Map()
    for (const activity of activities) {
        const matches = activitiesById.get(activity.activity_id) ?? []
        matches.push(activity)
        activitiesById.set(activity.activity_id, matches)
    }
    return activitiesById
}

export function appendActivityIdRelationCandidates(activities, schema, masterMap, entityRecords) {
    const coverage = {
        name: schema.name,
        source_table: schema.logicalPath,
        status: masterMap ? "included" : "missing",
        source_row_count: 0,
        relation_count: 0,
        unsupported_link_kind_count: 0,
        invalid_target_count: 0,
        unresolved_target_count: 0,
        matched_relation_count: 0,
        matched_activity_count: 0,
        candidate_count: 0,
        ambiguous_target_count: 0,
        missing_asset_count: 0,
        unresolved_count: 0,
    }
    if (!masterMap) return coverage

    const activitiesById = indexActivitiesById(activities)
    const matchedActivityIds = new Set()
    for (const { key, row } of enumerateMasterRows(masterMap)) {
        coverage.source_row_count += 1
        coverage.relation_count += 1
        const target = resolveActivityRelationTarget(schema, row)
        if (target.status === "unsupported") {
            coverage.unsupported_link_kind_count += 1
            continue
        }
        if (target.status === "invalid") {
            coverage.invalid_target_count += 1
            continue
        }
        const matches = activitiesById.get(target.activityId) ?? []
        if (matches.length === 0) {
            coverage.unresolved_target_count += 1
            coverage.unresolved_count += 1
            continue
        }
        if (matches.length > 1) {
            coverage.ambiguous_target_count += 1
            continue
        }

        coverage.matched_relation_count += 1
        matchedActivityIds.add(target.activityId)
        const activity = matches[0]
        const candidatesByHash = new Map(activity.image_candidates.map((candidate) => [candidate.source_hash, candidate]))
        const references = collectRelationImageReferences(schema, row)
        for (const reference of references) {
            const candidate = resolveImageCandidate({
                logicalPath: reference.logicalPath,
                sourceType: reference.sourceType,
                confidence: "exact-activity-id",
                evidence: `master:${schema.name}:key:${key}:target:${target.activityId}:field:${reference.fieldIndex}`,
            }, entityRecords)
            if (!candidate) {
                coverage.missing_asset_count += 1
                continue
            }
            if (candidatesByHash.has(candidate.source_hash)) continue
            candidatesByHash.set(candidate.source_hash, candidate)
            coverage.candidate_count += 1
        }
        activity.image_candidates = sortImageCandidates([...candidatesByHash.values()])
        Object.assign(activity, legacyBannerProjection(activity.image_candidates))
    }
    coverage.matched_activity_count = matchedActivityIds.size
    return coverage
}
// //// /通过客户端声明的活动 ID 连接图片关系行 ////

// //// 汇总活动 master 和图片关系覆盖信息 [@x380kkm 2026-08-19] ////
export function buildActivityCatalogCoverage(maps, activities, relationTables, duplicateEventReferenceCount) {
    const activityCounts = new Map()
    for (const activity of activities) {
        activityCounts.set(activity.source_table, (activityCounts.get(activity.source_table) ?? 0) + 1)
    }
    const masterTables = ACTIVITY_MASTER_SCHEMAS.map((schema) => ({
        activity_count: activityCounts.get(schema.logicalPath) ?? 0,
        name: schema.name,
        source_table: schema.logicalPath,
        status: maps.has(schema.name) ? "included" : "missing",
    }))
    return {
        master_tables: masterTables,
        missing_master_tables: masterTables
            .filter((table) => table.status === "missing")
            .map((table) => table.source_table),
        duplicate_event_reference_count: duplicateEventReferenceCount,
        pending_relation_tables: PENDING_ACTIVITY_RELATIONS,
        relation_tables: relationTables,
    }
}
// //// /汇总活动 master 和图片关系覆盖信息 ////
