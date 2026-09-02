// audience: internal
// # audit-cn-activity-schedules
//
// 该脚本按 CN 活动目录、orderedmap master 和关联静态目录核查时间窗口与依赖关系.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")
const DEFAULT_SAMPLE_TIMES = [
    "2019-01-01T00:00:00Z",
    "2020-07-01T00:00:00Z",
    "2021-07-01T00:00:00Z",
    "2022-07-01T00:00:00Z",
    "2024-07-23T12:00:00Z",
    "2024-08-14T12:00:00Z",
    "2026-08-30T00:00:00Z",
]
const PERMANENT_ACTIVITY_PREFIXES = ["daily-week:", "daily-exp-mana:", "challenge-dungeon:"]
const PERMANENT_END_MS = 253402300799000

// //// 解析审计输入 [@x380kkm 2026-08-30] ////
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

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, "utf8"))
}

function parseSampleTimes(value) {
    const labels = value ? value.split(",").filter(Boolean) : DEFAULT_SAMPLE_TIMES
    return labels.map((label) => {
        const timestamp = Date.parse(label)
        if (!Number.isSafeInteger(timestamp)) throw new Error(`invalid sample timestamp: ${label}`)
        return [label, timestamp]
    })
}
// //// /解析审计输入 ////

// //// 枚举 orderedmap 行 [@x380kkm 2026-08-30] ////
function enumerateRows(value, location = [], result = []) {
    if (Array.isArray(value)) {
        if (value.length === 0 || !Array.isArray(value[0])) {
            result.push({ location, row: value })
        } else {
            value.forEach((child, index) => enumerateRows(child, [...location, String(index)], result))
        }
        return result
    }
    if (!value || typeof value !== "object") return result
    for (const [key, child] of Object.entries(value)) enumerateRows(child, [...location, key], result)
    return result
}

function isMasterTimestamp(value) {
    return typeof value === "string" && /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)
}

function parseTimestamp(value) {
    if (!isMasterTimestamp(value)) return null
    return parseCnMasterTimestamp(value)
}

function parseCompositeValue(value, separator) {
    if (typeof value !== "string") return null
    const parts = value.split(separator)
    if (parts.length < 2 || !parts.every((part) => isMasterTimestamp(part))) return null
    return parts.map((part) => parseTimestamp(part))
}

function collectWindows(master, rows) {
    const windows = []
    const malformed = []
    for (const { location, row } of rows) {
        if (Number.isInteger(master.start_index) && Number.isInteger(master.end_index)) {
            const startValue = row[master.start_index]
            const endValue = row[master.end_index]
            const start = parseTimestamp(startValue)
            const end = parseTimestamp(endValue)
            if (start !== null && end !== null && end > start) {
                windows.push({ start_at_ms: start, end_at_ms: end, source: "scalar", location })
            } else if (startValue !== undefined || endValue !== undefined) {
                if (startValue !== "(None)" && endValue !== "(None)" && startValue !== "" && endValue !== "") {
                    malformed.push({ location, start: startValue ?? null, end: endValue ?? null })
                }
            }
        }
        for (const composite of master.composite_schedules ?? []) {
            const values = parseCompositeValue(row[composite.index], composite.separator)
            if (!values) continue
            const names = composite.components ?? []
            const start = values[0]
            const end = values[1]
            if (start !== null && end !== null && end > start) {
                windows.push({
                    start_at_ms: start,
                    end_at_ms: end,
                    source: "composite",
                    location,
                    components: Object.fromEntries(values.map((value, index) => [names[index] ?? `time_${index}`, value])),
                })
            }
        }
    }
    return { windows, malformed }
}

function uniqueWindows(windows) {
    const seen = new Set()
    return windows.filter((window) => {
        const key = `${window.start_at_ms}:${window.end_at_ms}`
        if (seen.has(key)) return false
        seen.add(key)
        return true
    })
}

function intervalOverlaps(left, right) {
    return left.start_at_ms < right.end_at_ms && right.start_at_ms < left.end_at_ms
}

function formatWindow(window) {
    return window
        ? { start_at_ms: window.start_at_ms, end_at_ms: window.end_at_ms }
        : null
}
// //// /枚举 orderedmap 行 ////

// //// 读取 master 记录 [@x380kkm 2026-08-30] ////
function readMasterRecords(projectionPath, projection) {
    const records = new Map()
    const issues = []
    for (const master of projection.masters ?? []) {
        const binaryPath = path.resolve(path.dirname(projectionPath), master.binary_path)
        if (!fs.existsSync(binaryPath)) {
            issues.push({ type: "missing_master_binary", master: master.name, path: binaryPath })
            continue
        }
        let decoded
        try {
            decoded = decodeOrderedMap(fs.readFileSync(binaryPath))
        } catch (error) {
            issues.push({ type: "invalid_master_binary", master: master.name, message: String(error) })
            continue
        }
        const roots = new Map()
        for (const [key, value] of Object.entries(decoded ?? {})) {
            const activityId = master.activity_id_prefix ? `${master.activity_id_prefix}${key}` : null
            const rows = enumerateRows(value)
            const collected = collectWindows(master, rows)
            roots.set(key, {
                key,
                activity_id: activityId,
                row_count: rows.length,
                windows: uniqueWindows(collected.windows),
                malformed_windows: collected.malformed,
            })
        }
        records.set(master.name, { master, decoded, roots })
    }
    return { records, issues }
}
// //// /读取 master 记录 ////

// //// 核对目录与 master 窗口 [@x380kkm 2026-08-30] ////
function auditCatalog(catalog, projection, masterRecords) {
    const issues = []
    const entries = []
    const masterByLogicalPath = new Map((projection.masters ?? []).map((master) => [master.logical_path, master]))
    for (const activity of catalog.activities ?? []) {
        const externalProjection = activity.kind === "gacha" || activity.kind === "gacha-campaign"
        const master = masterByLogicalPath.get(activity.source_table)
        const suffix = master?.activity_id_prefix && activity.activity_id.startsWith(master.activity_id_prefix)
            ? activity.activity_id.slice(master.activity_id_prefix.length)
            : null
        const record = master && suffix !== null ? masterRecords.get(master.name)?.roots.get(suffix) : null
        const catalogWindow = activity.default_start_at_ms !== undefined && activity.default_end_at_ms !== undefined
            ? { start_at_ms: activity.default_start_at_ms, end_at_ms: activity.default_end_at_ms }
            : null
        const masterWindow = record?.windows[0] ?? null
        const windowMatches = catalogWindow === null && masterWindow === null
            || catalogWindow !== null && record?.windows.some((window) => window.start_at_ms === catalogWindow.start_at_ms && window.end_at_ms === catalogWindow.end_at_ms)
        const entry = {
            activity_id: activity.activity_id,
            kind: activity.kind,
            source_table: activity.source_table ?? null,
            catalog_window: formatWindow(catalogWindow),
            master_windows: record?.windows.map(formatWindow) ?? [],
            master_row_count: record?.row_count ?? 0,
            permanent: PERMANENT_ACTIVITY_PREFIXES.some((prefix) => activity.activity_id.startsWith(prefix)),
            projection_owner: externalProjection ? "gacha" : "activity",
            window_matches: Boolean(windowMatches),
        }
        entries.push(entry)
        if (externalProjection) continue
        if (!master) issues.push({ type: "catalog_master_missing", activity_id: activity.activity_id, source_table: activity.source_table ?? null })
        else if (!record) issues.push({ type: "catalog_master_row_missing", activity_id: activity.activity_id, master: master.name, suffix })
        else if (!windowMatches) issues.push({ type: "catalog_master_window_mismatch", activity_id: activity.activity_id, ...entry })
        if (record?.malformed_windows.length) issues.push({ type: "malformed_master_window", activity_id: activity.activity_id, master: master.name, rows: record.malformed_windows })
    }
    return { entries, issues }
}

function auditUnlistedMasterRoots(catalog, projection, masterRecords) {
    const catalogIds = new Set((catalog.activities ?? []).map((activity) => activity.activity_id))
    const unlisted = []
    for (const { master, roots } of masterRecords.values()) {
        if (!master.activity_id_prefix || master.name === "event_list" || master.parent_activity_master) continue
        for (const record of roots.values()) {
            if (!record.activity_id || catalogIds.has(record.activity_id)) continue
            unlisted.push({
                activity_id: record.activity_id,
                master: master.name,
                row_count: record.row_count,
                windows: record.windows.map(formatWindow),
                malformed_windows: record.malformed_windows,
            })
        }
    }
    return unlisted.sort((left, right) => left.activity_id.localeCompare(right.activity_id, "en", { numeric: true }))
}

function auditParentLinks(projection, masterRecords) {
    const links = []
    const issues = []
    for (const master of projection.masters ?? []) {
        if (!master.parent_activity_master) continue
        const child = masterRecords.get(master.name)
        const parent = masterRecords.get(master.parent_activity_master)
        if (!child || !parent) continue
        for (const [key, childRecord] of child.roots) {
            const parentRecord = parent.roots.get(key)
            const childWindows = childRecord.windows
            const parentWindows = parentRecord?.windows ?? []
            const overlap = childWindows.some((childWindow) => parentWindows.some((parentWindow) => intervalOverlaps(childWindow, parentWindow)))
            links.push({
                master: master.name,
                parent_master: master.parent_activity_master,
                activity_id: childRecord.activity_id,
                child_windows: childWindows.map(formatWindow),
                parent_windows: parentWindows.map(formatWindow),
                overlap,
            })
            if (!parentRecord) issues.push({ type: "child_parent_row_missing", master: master.name, parent: master.parent_activity_master, key })
            else if (childWindows.length > 0 && parentWindows.length > 0 && !overlap) {
                issues.push({ type: "child_parent_window_disjoint", master: master.name, parent: master.parent_activity_master, key, child_windows: childWindows.map(formatWindow), parent_windows: parentWindows.map(formatWindow) })
            }
        }
    }
    return { links, issues }
}
// //// /核对目录与 master 窗口 ////

// //// 核对活动商店和箱池依赖 [@x380kkm 2026-08-30] ////
function eventActivityId(eventType, eventId) {
    const prefixes = ["advent", "ranking", "story", "daily-week", "challenge-dungeon", "daily-exp-mana", "world-story", "tower-dungeon", "expert-single", "collect-item", "carnival", "rush", "score-attack"]
    return prefixes[eventType] ? `${prefixes[eventType]}:${eventId}` : null
}

function relatedWorldStoryActivity(masterRecords, eventId, catalogIds) {
    const row = masterRecords.get("world_story_event")?.decoded?.[String(eventId)]
    const relatedId = Array.isArray(row) && /^\d+$/.test(String(row[1])) ? `world-story:${row[1]}` : null
    return relatedId && catalogIds.has(relatedId) ? relatedId : null
}

function auditShopAndBoxDependencies(root, catalog, masterRecords) {
    const issues = []
    const catalogIds = new Set((catalog.activities ?? []).map((activity) => activity.activity_id))
    const eventShops = readJson(path.join(root, "assets", "event_item_shop.json"))
    const eventShopIds = readJson(path.join(root, "assets", "event_item_shop_id_map.json"))
    const shopActivityIds = new Set()
    const unmatchedShopActivities = []
    let eventShopItemCount = 0
    for (const [eventTypeValue, events] of Object.entries(eventShops)) {
        const eventType = Number(eventTypeValue)
        for (const [eventIdValue, shop] of Object.entries(events)) {
            const eventId = Number(eventIdValue)
            const activityId = eventActivityId(eventType, eventId)
            if (!activityId) continue
            const itemCount = Object.keys(shop).length
            eventShopItemCount += itemCount
            shopActivityIds.add(activityId)
            if (catalogIds.has(activityId)) continue
            const masterName = `${activityId.split(":")[0].replaceAll("-", "_")}_event`
            const masterRecord = masterRecords.get(masterName)?.roots.get(String(eventId))
            const relatedActivityId = eventType === 6
                ? relatedWorldStoryActivity(masterRecords, eventId, catalogIds)
                : null
            unmatchedShopActivities.push({
                activity_id: activityId,
                event_type: eventType,
                event_id: eventId,
                item_count: itemCount,
                master_exists: Boolean(masterRecord),
                master_windows: masterRecord?.windows.map(formatWindow) ?? [],
                related_activity_id: relatedActivityId,
            })
            if (!relatedActivityId) {
                issues.push({ type: "shop_activity_without_catalog", activity_id: activityId, event_type: eventType, event_id: eventId })
            }
        }
    }
    const boxGacha = readJson(path.join(root, "assets", "box_gacha.json"))
    const boxActivityIds = Object.keys(boxGacha).map((id) => `box-gacha:${id}`)
    for (const activityId of boxActivityIds) {
        if (!catalogIds.has(activityId)) issues.push({ type: "box_without_catalog", activity_id: activityId })
    }
    return {
        event_shop_identity_count: Object.keys(eventShopIds).length,
        event_shop_item_count: eventShopItemCount,
        event_shop_activity_count: shopActivityIds.size,
        unmatched_event_shop_activities: unmatchedShopActivities,
        box_gacha_count: boxActivityIds.length,
        issues,
    }
}
// //// /核对活动商店和箱池依赖 ////

// //// 统计代表日期的活动覆盖 [@x380kkm 2026-08-30] ////
function isOpen(activity, at) {
    if (PERMANENT_ACTIVITY_PREFIXES.some((prefix) => activity.activity_id.startsWith(prefix))) return true
    return Number.isSafeInteger(activity.default_start_at_ms)
        && Number.isSafeInteger(activity.default_end_at_ms)
        && activity.default_start_at_ms <= at
        && at < activity.default_end_at_ms
}

function countByKind(activities) {
    return Object.fromEntries([...activities.reduce((counts, activity) => {
        counts.set(activity.kind || "(missing)", (counts.get(activity.kind || "(missing)") ?? 0) + 1)
        return counts
    }, new Map())].sort(([left], [right]) => left.localeCompare(right, "en", { numeric: true })))
}

function sampleCoverage(catalog, sampleTimes) {
    return Object.fromEntries(sampleTimes.map(([label, at]) => {
        const open = (catalog.activities ?? []).filter((activity) => isOpen(activity, at))
        return [label, {
            total: open.length,
            non_gacha_total: open.filter((activity) => activity.kind !== "gacha").length,
            by_kind: countByKind(open),
        }]
    }))
}
// //// /统计代表日期的活动覆盖 ////

// //// 执行活动时间审计 [@x380kkm 2026-08-30] ////
function main() {
    const args = parseArgs(process.argv.slice(2))
    const root = path.resolve(args.root ?? REPOSITORY_ROOT)
    const catalogPath = path.resolve(args.catalog ?? path.join(root, "assets", "cn-activity-catalog-source.json"))
    const projectionPath = path.resolve(args.projection ?? path.join(root, "core", "personal-service", "assets", "cn-activity-master-projection.json"))
    const catalog = readJson(catalogPath)
    const projection = readJson(projectionPath)
    const { records: masterRecords, issues: masterIssues } = readMasterRecords(projectionPath, projection)
    const catalogAudit = auditCatalog(catalog, projection, masterRecords)
    const unlistedMasterRoots = auditUnlistedMasterRoots(catalog, projection, masterRecords)
    const parentAudit = auditParentLinks(projection, masterRecords)
    const dependencyAudit = auditShopAndBoxDependencies(root, catalog, masterRecords)
    const activitiesWithFiniteWindows = catalog.activities.filter((activity) => Number.isSafeInteger(activity.default_start_at_ms) && Number.isSafeInteger(activity.default_end_at_ms))
    const anomalousWindows = activitiesWithFiniteWindows
        .filter((activity) => activity.kind !== "gacha"
            && !PERMANENT_ACTIVITY_PREFIXES.some((prefix) => activity.activity_id.startsWith(prefix))
            && (activity.default_end_at_ms >= PERMANENT_END_MS || activity.default_start_at_ms < Date.parse("2019-12-01T00:00:00Z")))
        .map((activity) => ({ activity_id: activity.activity_id, kind: activity.kind, start_at_ms: activity.default_start_at_ms, end_at_ms: activity.default_end_at_ms }))
    const output = {
        source: {
            catalog: path.relative(root, catalogPath).replaceAll(path.sep, "/"),
            projection: path.relative(root, projectionPath).replaceAll(path.sep, "/"),
            region: catalog.region ?? null,
            timezone: "JST",
        },
        counts: {
            catalog_activities: catalog.activities?.length ?? 0,
            master_files: masterRecords.size,
            activities_with_finite_windows: activitiesWithFiniteWindows.length,
            activities_without_finite_windows: (catalog.activities?.length ?? 0) - activitiesWithFiniteWindows.length,
            permanent_activities: catalog.activities.filter((activity) => PERMANENT_ACTIVITY_PREFIXES.some((prefix) => activity.activity_id.startsWith(prefix))).length,
            activities_without_cn_tag: catalog.activities.filter((activity) => !activity.tags?.includes("CN")).length,
            unlisted_master_roots: unlistedMasterRoots.length,
        },
        by_kind: countByKind(catalog.activities ?? []),
        sample_coverage: sampleCoverage(catalog, parseSampleTimes(args.at)),
        anomalous_windows: anomalousWindows,
        masters: [...masterRecords.values()].map(({ master, roots }) => ({
            name: master.name,
            logical_path: master.logical_path,
            parent_activity_master: master.parent_activity_master ?? null,
            quest_category: master.quest_category ?? null,
            root_count: roots.size,
            roots_with_windows: [...roots.values()].filter((record) => record.windows.length > 0).length,
            roots_with_multiple_windows: [...roots.values()].filter((record) => record.windows.length > 1).length,
            malformed_window_rows: [...roots.values()].reduce((count, record) => count + record.malformed_windows.length, 0),
        })),
        catalog_entries: catalogAudit.entries,
        unlisted_master_roots: unlistedMasterRoots,
        parent_links: parentAudit.links,
        dependencies: dependencyAudit,
        issues: [...masterIssues, ...catalogAudit.issues, ...parentAudit.issues, ...dependencyAudit.issues],
    }
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`)
}
// //// /执行活动时间审计 ////

main()
