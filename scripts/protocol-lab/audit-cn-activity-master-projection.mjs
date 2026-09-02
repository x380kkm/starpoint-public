// audience: internal
// # audit-cn-activity-master-projection
// 此脚本核查活动 master 投影、CN EntityLists 记录、竞速活动引用链和 ZIP 归档格式.

import assert from "node:assert/strict"
import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import unzipper from "unzipper"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { encodeOrderedMap } from "./encode-cn-orderedmap.mjs"

const DEFAULT_PROJECTION = path.resolve(process.cwd(), "core", "personal-service", "assets", "cn-activity-master-projection.json")
const DEFAULT_CDN_ROOT = path.resolve(process.cwd(), "..", "starpoint", ".cdn", "cn")
const DEFAULT_CATALOG = path.resolve(process.cwd(), "assets", "cn-activity-catalog-source.json")
const DEFAULT_SINGLE_BATTLE = path.resolve(process.cwd(), "core", "personal-service", "assets", "cn-single-battle.json")

// //// 解析脚本参数 [@x380kkm 2026-08-29] ////
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
// //// /解析脚本参数 ////

function digestForEntity(buffer) {
    return crypto.createHash("sha256").update(buffer).digest("base64")
        .replaceAll("+", "_")
        .replaceAll("/", "-")
        .replace(/=+$/, "")
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

function readEntityManifest(cdnRoot) {
    const manifestPath = path.join(cdnRoot, "entities", "PathFile.csv")
    const records = new Map()
    for (const line of fs.readFileSync(manifestPath, "utf8").split(/\r?\n/)) {
        if (!line) continue
        const [entryPath, version, byteLength, digest, assetKind] = line.split(",")
        if (!entryPath || !version || !byteLength || !digest || !assetKind) throw new Error(`invalid EntityLists row: ${line}`)
        records.set(entryPath, { version, byteLength: Number(byteLength), digest, assetKind })
    }
    return records
}

function assertRoundTrip(name, buffer) {
    const decoded = decodeOrderedMap(buffer)
    const encoded = encodeOrderedMap(decoded)
    const decodedAgain = decodeOrderedMap(encoded)
    assert.deepEqual(decodedAgain, decoded, `orderedmap records changed: ${name}`)
    return { source_byte_length: buffer.length, encoded_byte_length: encoded.length, row_count: allRows(decoded).length }
}

function assertLegacyZip(zipPath) {
    const bytes = fs.readFileSync(zipPath)
    const endOffset = bytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]))
    assert.ok(endOffset >= 0 && endOffset + 22 <= bytes.length, `ZIP end record missing: ${zipPath}`)
    const zip64Locator = bytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x06, 0x07]))
    const zip64End = bytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x06, 0x06]))
    assert.ok(zip64Locator < 0 && zip64End < 0, `ZIP64 records are not supported: ${zipPath}`)
    assert.notEqual(bytes.readUInt16LE(endOffset + 8), 0xffff, `ZIP64 entry count: ${zipPath}`)
    assert.notEqual(bytes.readUInt16LE(endOffset + 10), 0xffff, `ZIP64 entry count: ${zipPath}`)
    assert.notEqual(bytes.readUInt32LE(endOffset + 12), 0xffffffff, `ZIP64 directory size: ${zipPath}`)
    assert.notEqual(bytes.readUInt32LE(endOffset + 16), 0xffffffff, `ZIP64 directory offset: ${zipPath}`)
}

async function assertArchiveContainsEntry(cdnRoot, archiveName, entryPath) {
    const archivePath = path.join(cdnRoot, archiveName)
    assertLegacyZip(archivePath)
    const archive = await unzipper.Open.file(archivePath)
    const entry = archive.files.find((candidate) => candidate.path === entryPath)
    assert.ok(entry && entry.type === "File", `archive entry missing: ${archiveName}/${entryPath}`)
    return { archive: archiveName, entry_path: entryPath, uncompressed_size: entry.uncompressedSize }
}

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, "utf8"))
}

function assertCatalogCoverage(catalog, masters, decodedMasters) {
    const sourceTables = new Set(masters.map((master) => master.logical_path))
    const excludedKinds = new Set(["gacha", "gacha-campaign"])
    const missing = []
    const missingRows = []
    for (const activity of catalog.activities ?? []) {
        if (excludedKinds.has(activity.kind)) continue
        if (!activity.source_table || !sourceTables.has(activity.source_table)) {
            missing.push(activity.activity_id ?? "(unknown)")
            continue
        }
        const masterName = masters.find((master) => master.logical_path === activity.source_table)?.name
        const activityKey = String(activity.activity_id ?? "").split(":").slice(1).join(":")
        if (!masterName || !activityKey || !decodedMasters.get(masterName)?.[activityKey]) {
            missingRows.push(activity.activity_id ?? "(unknown)")
        }
    }
    assert.deepEqual(missing, [], `catalog activities are not backed by a seed master: ${missing.join(", ")}`)
    assert.deepEqual(missingRows, [], `catalog activities do not map to a seed master row: ${missingRows.join(", ")}`)
    return catalog.activities.filter((activity) => !excludedKinds.has(activity.kind)).length
}

function assertRankingChain(projection, projectionPath, singleBattle) {
    const ranking = projection.references?.ranking_2
    assert.deepEqual(ranking, {
        activity_id: "ranking:2",
        event_kind: 1,
        event_id: 2,
        event_list_target: ranking.event_list_target,
        quest_id: 2001,
        single_battle_quest: "11:2001",
    })
    const rankingMaster = projection.masters.find((master) => master.name === "ranking_event")
    const rankingQuestMaster = projection.masters.find((master) => master.name === "ranking_event_single_quest")
    const eventListMaster = projection.masters.find((master) => master.name === "event_list")
    assert.ok(rankingMaster && rankingQuestMaster && eventListMaster, "ranking seed masters are missing")
    const rankingRows = decodeOrderedMap(fs.readFileSync(path.resolve(path.dirname(projectionPath), rankingMaster.binary_path)))
    assert.ok(rankingRows["2"], "ranking_event/2 is missing")
    const rankingQuests = decodeOrderedMap(fs.readFileSync(path.resolve(path.dirname(projectionPath), rankingQuestMaster.binary_path)))
    assert.ok(rankingQuests["2"]?.["1"]?.[0] === "2001", "ranking_event_single_quest/2 does not point at 2001")
    assert.ok(singleBattle.quests?.["11:2001"], "cn-single-battle does not contain category 11 quest 2001")
    return {
        event_list_target: ranking.event_list_target,
        ranking_event_row: rankingRows["2"].length,
        ranking_quest_id: rankingQuests["2"]["1"][0],
        single_battle: singleBattle.quests["11:2001"],
    }
}

// //// 执行活动 master 投影审计 [@x380kkm 2026-08-29] ////
async function audit(args) {
    const projectionPath = path.resolve(args.projection ?? DEFAULT_PROJECTION)
    const cdnRoot = path.resolve(args["cdn-root"] ?? DEFAULT_CDN_ROOT)
    const catalogPath = path.resolve(args.catalog ?? DEFAULT_CATALOG)
    const singleBattlePath = path.resolve(args["single-battle"] ?? DEFAULT_SINGLE_BATTLE)
    const projection = readJson(projectionPath)
    const entityRecords = readEntityManifest(cdnRoot)
    const masters = projection.masters ?? []
    assert.ok(masters.length >= 10, "activity master projection is empty")
    const roundTrips = []
    const archiveTargets = new Map()
    const decodedMasters = new Map()
    for (const master of masters) {
        assert.ok(master.logical_path && master.entry_path && master.binary_path, `master metadata is incomplete: ${master.name}`)
        const entity = entityRecords.get(master.entry_path)
        assert.ok(entity, `EntityLists entry is missing: ${master.entry_path}`)
        assert.equal(entity.byteLength, master.source_byte_length, `EntityLists byte length mismatch: ${master.name}`)
        assert.equal(entity.digest, master.source_entity_digest, `EntityLists digest mismatch: ${master.name}`)
        const binaryPath = path.resolve(path.dirname(projectionPath), master.binary_path)
        assert.ok(fs.existsSync(binaryPath), `seed binary is missing: ${binaryPath}`)
        const buffer = fs.readFileSync(binaryPath)
        assert.equal(buffer.length, master.source_byte_length, `seed binary byte length mismatch: ${master.name}`)
        assert.equal(digestForEntity(buffer), master.source_entity_digest, `seed binary digest mismatch: ${master.name}`)
        roundTrips.push({ name: master.name, ...assertRoundTrip(master.name, buffer) })
        decodedMasters.set(master.name, decodeOrderedMap(buffer))
        if (master.source_archive) archiveTargets.set(master.source_archive, master.entry_path)
    }
    const archiveChecks = []
    for (const [archiveName, entryPath] of archiveTargets) {
        archiveChecks.push(await assertArchiveContainsEntry(cdnRoot, archiveName, entryPath))
    }
    const catalogCoverage = assertCatalogCoverage(readJson(catalogPath), masters, decodedMasters)
    const rankingChain = assertRankingChain(projection, projectionPath, readJson(singleBattlePath))
    if (args.archive) {
        const archivePath = path.resolve(args.archive)
        assertLegacyZip(archivePath)
        archiveChecks.push({ archive: archivePath, zip64: false })
    }
    return {
        projection: projectionPath,
        master_count: masters.length,
        catalog_non_gacha_activity_count: catalogCoverage,
        round_trip_count: roundTrips.length,
        archive_check_count: archiveChecks.length,
        ranking_chain: rankingChain,
        zip64: false,
    }
}
// //// /执行活动 master 投影审计 ////

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
    audit(parseArgs(process.argv.slice(2)))
        .then((result) => process.stdout.write(`${JSON.stringify(result)}\n`))
        .catch((error) => {
            process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
            process.exitCode = 1
        })
}
