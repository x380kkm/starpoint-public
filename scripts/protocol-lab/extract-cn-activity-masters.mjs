// audience: internal
// # extract-cn-activity-masters
// 此脚本从 CN CDN 的 EntityLists 和归档提取活动 master, 为运行时投影保留原始 orderedmap 字节.

import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import unzipper from "unzipper"
import {
    EVENT_LIST_SCHEMA,
    EVENT_MASTER_SCHEMAS,
    INDEPENDENT_ACTIVITY_SCHEMAS,
} from "./cn-activity-master-schema.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { encodeOrderedMap } from "./encode-cn-orderedmap.mjs"
import { assetEntryPaths, hashCnAssetPath } from "./cn-asset-paths.mjs"

const DEFAULT_CDN_ROOT = path.resolve(process.cwd(), "..", "starpoint", ".cdn", "cn")
const DEFAULT_OUTPUT = path.resolve(process.cwd(), "core", "personal-service", "assets", "cn-activity-master-projection.json")
const DEFAULT_BINARY_ROOT = path.resolve(process.cwd(), "core", "personal-service", "assets", "cn-activity-masters")
const REQUIRED_NAMES = new Set([
    "event_list",
    "ranking_event",
    "ranking_event_single_quest",
    "rush_event",
    "rush_event_quest",
    "rush_event_quest_folder",
    "solo_time_attack_event",
    "solo_time_attack_event_quest",
    "score_attack_event",
    "score_attack_event_quest",
])

const QUEST_MASTER_DEFINITIONS = Object.freeze([
    { name: "advent_event_quest", parent: "advent_event", category: 7, startIndex: 5, endIndex: 6 },
    { name: "story_event_single_quest", parent: "story_event", category: 10, startIndex: 5, endIndex: 6 },
    { name: "daily_week_event_quest", parent: "daily_week_event", category: 6, startIndex: 4, endIndex: 5 },
    { name: "daily_exp_mana_event_quest", parent: "daily_exp_mana_event", category: 14, startIndex: 5, endIndex: 6 },
    { name: "challenge_dungeon_event_quest", parent: "challenge_dungeon_event", category: 13, startIndex: 5, endIndex: 6 },
    { name: "world_story_event_quest", parent: "world_story_event", category: 18, startIndex: 5, endIndex: 6 },
    { name: "world_story_event_boss_battle_quest", parent: "world_story_event", category: 19, startIndex: 5, endIndex: 6 },
    { name: "tower_dungeon_event_quest", parent: "tower_dungeon_event", category: 20, startIndex: 5, endIndex: 6 },
    { name: "expert_single_event_quest", parent: "expert_single_event", category: 21, startIndex: 7, endIndex: 8 },
    { name: "carnival_event_quest", parent: "carnival_event", category: 22, startIndex: 7, endIndex: 8 },
    { name: "raid_event_quest", parent: "raid_event", category: 23, startIndex: 7, endIndex: 8 },
    { name: "ranking_event_single_quest", parent: "ranking_event", category: 11, startIndex: 5, endIndex: 6 },
    { name: "rush_event_quest", parent: "rush_event", category: 24, startIndex: 7, endIndex: 8 },
    { name: "solo_time_attack_event_quest", parent: "solo_time_attack_event", category: 25, startIndex: 6, endIndex: 7 },
    { name: "hard_multi_event_quest", parent: "hard_multi_event", category: 26, startIndex: 7, endIndex: 8 },
    { name: "score_attack_event_quest", parent: "score_attack_event", category: 27, startIndex: 7, endIndex: 8 },
    { name: "rush_event_quest_folder", parent: "rush_event", category: 24, startIndex: null, endIndex: null },
])

const EVENT_SCHEDULE_INDEXES = Object.freeze({
    ranking_event: [3, 11, 12, 18, 19, 20],
    rush_event: [2, 11, 12, 13, 15, 16, 17],
    raid_event: [2, 19, 22, 23, 24],
})

// //// 解析脚本参数 [@x380kkm 2026-08-29] ////
function parseArgs(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index]
        if (!argument.startsWith("--")) throw new Error(`unexpected argument: ${argument}`)
        const name = argument.slice(2)
        if (name === "check") {
            args[name] = true
            continue
        }
        const value = argv[index + 1]
        if (value === undefined || value.startsWith("--")) throw new Error(`missing value for --${name}`)
        args[name] = value
        index += 1
    }
    return args
}
// //// /解析脚本参数 ////

// //// 声明活动 master 来源集合 [@x380kkm 2026-08-29] ////
function activityMasterDefinitions() {
    const prefixByMasterName = new Map([
        ...EVENT_MASTER_SCHEMAS,
        ...INDEPENDENT_ACTIVITY_SCHEMAS,
    ].map((schema) => [schema.name, schema.identityPrefix]))
    const definitions = [
        {
            name: EVENT_LIST_SCHEMA.name,
            logicalPath: EVENT_LIST_SCHEMA.logicalPath,
            activityIdPrefix: "event:",
            kindCode: null,
            required: true,
            startIndex: null,
            endIndex: null,
            parent: null,
            category: null,
        },
        ...EVENT_MASTER_SCHEMAS.map((schema) => ({
            name: schema.name,
            logicalPath: schema.logicalPath,
            activityIdPrefix: `${schema.identityPrefix}:`,
            kindCode: Number(schema.kindCode),
            required: true,
            startIndex: schema.startIndex,
            endIndex: schema.endIndex,
            parent: null,
            category: null,
        })),
        ...INDEPENDENT_ACTIVITY_SCHEMAS
            .filter((schema) => !["gacha", "gacha_campaign"].includes(schema.name))
            .map((schema) => ({
                name: schema.name,
                logicalPath: schema.logicalPath,
                activityIdPrefix: `${schema.identityPrefix}:`,
                kindCode: null,
                required: true,
                startIndex: schema.startIndex,
                endIndex: schema.endIndex,
                parent: null,
                category: null,
            })),
        ...QUEST_MASTER_DEFINITIONS.map((definition) => ({
            name: definition.name,
            logicalPath: `master/quest/event/${definition.name}.orderedmap`,
            activityIdPrefix: `${prefixByMasterName.get(definition.parent) ?? definition.parent.replace(/_event$/, "").replaceAll("_", "-")}:`,
            kindCode: null,
            required: REQUIRED_NAMES.has(definition.name),
            startIndex: definition.startIndex,
            endIndex: definition.endIndex,
            parent: definition.parent,
            category: definition.category,
        })),
    ]
    const seen = new Set()
    return definitions.filter((definition) => {
        if (seen.has(definition.logicalPath)) return false
        seen.add(definition.logicalPath)
        return true
    })
}
// //// /声明活动 master 来源集合 ////

// //// 读取 EntityLists 元数据 [@x380kkm 2026-08-29] ////
function readEntityRecords(cdnRoot) {
    const manifestPath = path.join(cdnRoot, "entities", "PathFile.csv")
    const records = new Map()
    const lines = fs.readFileSync(manifestPath, "utf8").split(/\r?\n/)
    for (const line of lines) {
        if (!line) continue
        const fields = line.split(",")
        if (fields.length !== 5) throw new Error(`EntityLists row has an unexpected field count: ${line}`)
        const [entryPath, version, byteLength, digest, assetKind] = fields
        if (!/^production\/(?:(?:android|ios|medium)_)?upload\/[a-f0-9]{2}\/[a-f0-9]{38}$/.test(entryPath)) {
            throw new Error(`EntityLists contains an invalid asset path: ${entryPath}`)
        }
        const size = Number(byteLength)
        if (!version || !Number.isSafeInteger(size) || size < 1 || !/^[A-Za-z0-9_-]{43}$/.test(digest) || !assetKind) {
            throw new Error(`EntityLists contains invalid metadata: ${entryPath}`)
        }
        records.set(entryPath, { version, byteLength: size, digest, assetKind })
    }
    return records
}
// //// /读取 EntityLists 元数据 ////

function digestForEntity(buffer) {
    return crypto.createHash("sha256").update(buffer).digest("base64")
        .replaceAll("+", "_")
        .replaceAll("/", "-")
        .replace(/=+$/, "")
}

// //// 扫描归档并保留原始 entry [@x380kkm 2026-08-29] ////
async function readArchiveEntries(cdnRoot, targets) {
    const directories = [
        "archive-common-diff",
        "archive-common-full",
        "archive-ios-diff",
        "archive-ios-full",
        "archive-medium-diff",
        "archive-medium-full",
        "archive-android-diff",
        "archive-android-full",
    ]
    const pending = new Map()
    for (const target of targets) {
        pending.set(target.entryPath, target)
    }
    const results = new Map()
    for (const directoryName of directories) {
        if (pending.size === 0) break
        const directory = path.join(cdnRoot, directoryName)
        if (!fs.existsSync(directory)) continue
        const archives = fs.readdirSync(directory).filter((name) => name.endsWith(".zip")).sort()
        for (const archiveName of archives) {
            if (pending.size === 0) break
            const archivePath = path.join(directory, archiveName)
            const archive = await unzipper.Open.file(archivePath)
            for (const entry of archive.files) {
                const target = pending.get(entry.path)
                if (!target || entry.type !== "File") continue
                if (entry.uncompressedSize !== target.entity.byteLength) continue
                const buffer = await entry.buffer()
                if (digestForEntity(buffer) !== target.entity.digest) continue
                results.set(target.name, {
                    buffer,
                    archivePath: path.relative(cdnRoot, archivePath).replaceAll("\\", "/"),
                    entryPath: target.entryPath,
                })
                pending.delete(entry.path)
            }
        }
    }
    if (pending.size > 0) {
        throw new Error(`活动 master 归档缺少 entry: ${[...pending.values()].map((target) => target.logicalPath).join(", ")}`)
    }
    return results
}
// //// /扫描归档并保留原始 entry ////

function firstRow(value) {
    if (Array.isArray(value)) {
        if (value.length > 0 && Array.isArray(value[0])) return firstRow(value[0])
        return value
    }
    if (!value || typeof value !== "object") return null
    for (const child of Object.values(value)) {
        const row = firstRow(child)
        if (row) return row
    }
    return null
}

function allRows(value, rows = []) {
    if (Array.isArray(value)) {
        if (value.length === 0 || !Array.isArray(value[0])) rows.push(value)
        else for (const child of value) allRows(child, rows)
        return rows
    }
    if (!value || typeof value !== "object") return rows
    for (const child of Object.values(value)) allRows(child, rows)
    return rows
}

function isTimestamp(value) {
    return typeof value === "string" && /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(value)
}

function timestampParts(value) {
    if (typeof value !== "string") return []
    const parts = value.split(",")
    return parts.every((part) => isTimestamp(part) || part === "(None)" || part === "") && parts.some(isTimestamp)
        ? parts
        : []
}

function inferTimeIndexes(rows) {
    const counts = new Map()
    for (const row of rows) {
        row.forEach((value, index) => {
            if (isTimestamp(value) || timestampParts(value).length > 0) counts.set(index, (counts.get(index) ?? 0) + 1)
        })
    }
    return [...counts.keys()].sort((left, right) => left - right)
}

function inferCompositeIndexes(rows) {
    const indexes = new Set()
    for (const row of rows) {
        row.forEach((value, index) => {
            if (timestampParts(value).length >= 2) indexes.add(index)
        })
    }
    return [...indexes].sort((left, right) => left - right)
}

function scheduleComponents(name, index, value) {
    const count = timestampParts(value).length
    if (["ranking_event", "rush_event", "raid_event"].includes(name) && count >= 3) {
        return ["start_time", "playable_end_time", "aggregation_end_time", "reward_receive_end_time"].slice(0, count)
    }
    return []
}

function eventReferences(eventList) {
    const references = []
    for (const [key, row] of Object.entries(eventList)) {
        if (!Array.isArray(row) || row.length < 2) continue
        const kindCode = Number(row[0])
        const eventId = Number(row[1])
        if (!Number.isSafeInteger(kindCode) || !Number.isSafeInteger(eventId)) continue
        references.push({ key, kindCode, eventId, questId: row[2] ?? null })
    }
    return references
}

function nestedRowsForEvent(value, eventId) {
    const group = value?.[String(eventId)]
    if (!group || typeof group !== "object" || Array.isArray(group)) return []
    return allRows(group)
}

function findQuestIdForEvent(value, eventId, expectedCategory) {
    for (const row of nestedRowsForEvent(value, eventId)) {
        const questId = Number(row[0])
        if (Number.isSafeInteger(questId) && questId > 0) {
            if (expectedCategory === undefined || Number(row[1]) === expectedCategory || row.length > 2) return questId
        }
    }
    return null
}

// //// 构造活动 master 投影清单 [@x380kkm 2026-08-29] ////
function buildProjection(definition, target, decoded, binaryPath) {
    const rows = allRows(decoded)
    const sample = firstRow(decoded) ?? []
    const compositeIndexes = inferCompositeIndexes(rows)
    const explicitScheduleIndexes = EVENT_SCHEDULE_INDEXES[definition.name] ?? []
    const timeIndexes = inferTimeIndexes(rows)
    const synchronizedIndexes = [...new Set([
        ...(Number.isInteger(definition.startIndex) ? [definition.startIndex] : []),
        ...(Number.isInteger(definition.endIndex) ? [definition.endIndex] : []),
        ...timeIndexes,
        ...explicitScheduleIndexes,
    ])].sort((left, right) => left - right)
    const compositeSchedules = compositeIndexes.map((index) => ({
        index,
        separator: ",",
        components: scheduleComponents(definition.name, index, sample[index] ?? ""),
    }))
    return {
        name: definition.name,
        activity_id_prefix: definition.activityIdPrefix,
        logical_path: definition.logicalPath,
        entry_path: target.entryPath,
        source_archive: target.archivePath,
        source_version: target.entity.version,
        source_asset_kind: target.entity.assetKind,
        source_byte_length: target.entity.byteLength,
        source_entity_digest: target.entity.digest,
        binary_path: binaryPath,
        row_count: rows.length,
        root_key_count: Object.keys(decoded).length,
        start_index: Number.isInteger(definition.startIndex) ? definition.startIndex : null,
        end_index: Number.isInteger(definition.endIndex) ? definition.endIndex : null,
        time_indexes: timeIndexes,
        scalar_schedule_indexes: synchronizedIndexes.filter((index) => !compositeIndexes.includes(index)),
        composite_schedule_indexes: compositeIndexes,
        composite_schedules: compositeSchedules,
        synchronized_field_indexes: synchronizedIndexes,
        parent_activity_master: definition.parent,
        quest_category: definition.category,
    }
}
// //// /构造活动 master 投影清单 ////

function assertRoundTrip(name, buffer, decoded) {
    const encoded = encodeOrderedMap(decoded)
    const decodedAgain = decodeOrderedMap(encoded)
    if (JSON.stringify(decodedAgain) !== JSON.stringify(decoded)) {
        throw new Error(`orderedmap decode/encode changed records: ${name}`)
    }
    return { encodedByteLength: encoded.length, sourceByteLength: buffer.length }
}

function readAssetVersion(cdnRoot) {
    const manifest = JSON.parse(fs.readFileSync(path.join(cdnRoot, "path"), "utf8"))
    return {
        client: manifest?.info?.client_asset_version ?? null,
        target: manifest?.info?.target_asset_version ?? null,
        eventualTarget: manifest?.info?.eventual_target_asset_version ?? null,
    }
}

// //// 生成活动 master 投影和原始字节 [@x380kkm 2026-08-29] ////
async function generate(args) {
    const cdnRoot = path.resolve(args["cdn-root"] ?? DEFAULT_CDN_ROOT)
    const outputPath = path.resolve(args.output ?? DEFAULT_OUTPUT)
    const binaryRoot = path.resolve(args["binary-root"] ?? DEFAULT_BINARY_ROOT)
    const entityRecords = readEntityRecords(cdnRoot)
    const definitions = activityMasterDefinitions()
    const targets = []
    for (const definition of definitions) {
        const hash = hashCnAssetPath(definition.logicalPath)
        const entryPath = assetEntryPaths(hash).find((candidate) => entityRecords.has(candidate))
        if (!entryPath) {
            if (definition.required) throw new Error(`EntityLists 缺少活动 master: ${definition.logicalPath}`)
            continue
        }
        targets.push({
            name: definition.name,
            logicalPath: definition.logicalPath,
            entryPath,
            entity: entityRecords.get(entryPath),
        })
    }
    const archives = await readArchiveEntries(cdnRoot, targets)
    fs.mkdirSync(binaryRoot, { recursive: true })
    const masters = []
    const roundTrips = []
    for (const definition of definitions) {
        const target = targets.find((candidate) => candidate.name === definition.name)
        if (!target) continue
        const archive = archives.get(definition.name)
        const binaryName = `${definition.name}.orderedmap`
        const binaryPath = path.join(binaryRoot, binaryName)
        fs.writeFileSync(binaryPath, archive.buffer)
        const decoded = decodeOrderedMap(archive.buffer)
        roundTrips.push({ name: definition.name, ...assertRoundTrip(definition.name, archive.buffer, decoded) })
        masters.push(buildProjection(definition, { ...target, archivePath: archive.archivePath }, decoded, path.relative(path.dirname(outputPath), binaryPath).replaceAll("\\", "/")))
    }
    const eventListMaster = masters.find((master) => master.name === "event_list")
    const eventListBuffer = archives.get("event_list")?.buffer
    const eventList = eventListBuffer ? decodeOrderedMap(eventListBuffer) : {}
    const references = eventReferences(eventList)
    const rankingReference = references.find((reference) => reference.kindCode === 1 && reference.eventId === 2)
    const rankingQuestMaster = archives.get("ranking_event_single_quest")?.buffer
        ? decodeOrderedMap(archives.get("ranking_event_single_quest").buffer)
        : null
    const rankingQuestId = rankingQuestMaster ? findQuestIdForEvent(rankingQuestMaster, 2, 1) : null
    if (!rankingReference || rankingQuestId !== 2001) {
        throw new Error("ranking:2 -> quest 2001 reference is missing from event_list")
    }
    const projection = {
        format_version: 1,
        region: "cn",
        client_asset_version: readAssetVersion(cdnRoot).client,
        target_asset_version: readAssetVersion(cdnRoot).target,
        eventual_target_asset_version: readAssetVersion(cdnRoot).eventualTarget,
        entity_manifest: "entities/PathFile.csv",
        binary_root: path.relative(path.dirname(outputPath), binaryRoot).replaceAll("\\", "/"),
        masters,
        references: {
            event_list_count: references.length,
            ranking_2: {
                activity_id: "ranking:2",
                event_kind: rankingReference.kindCode,
                event_id: rankingReference.eventId,
                event_list_target: rankingReference.questId,
                quest_id: rankingQuestId,
                single_battle_quest: `11:${rankingQuestId}`,
            },
        },
        round_trip: roundTrips,
        archive_contract: {
            compression: "zip",
            zip64: false,
            entry_paths: masters.map((master) => master.entry_path),
        },
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, `${JSON.stringify(projection, null, 2)}\n`, "utf8")
    return {
        output: outputPath,
        binary_root: binaryRoot,
        master_count: masters.length,
        event_list_count: references.length,
        ranking_2_quest_id: rankingQuestId,
        round_trip_count: roundTrips.length,
        event_list_master: eventListMaster?.binary_path ?? null,
    }
}
// //// /生成活动 master 投影和原始字节 ////

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
    generate(parseArgs(process.argv.slice(2)))
        .then((result) => process.stdout.write(`${JSON.stringify(result)}\n`))
        .catch((error) => {
            process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
            process.exitCode = 1
        })
}
