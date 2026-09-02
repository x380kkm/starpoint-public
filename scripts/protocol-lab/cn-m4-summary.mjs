// audience: internal
// # cn-m4-summary
//
// 该模块从 CN 实验数据库和 HTTP 元数据生成不含身份与正文的聚合摘要.

import { existsSync, readFileSync, writeFileSync } from "node:fs"
import { createRequire } from "node:module"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { validateCnMetadataRecord, validateEvidenceRoute } from "./cn-evidence.mjs"

const require = createRequire(import.meta.url)
const Database = require("better-sqlite3")

const SNAPSHOT_SCHEMA_VERSION = 1

function fail(message) {
    throw new Error(`invalid CN M4 summary input: ${message}`)
}

function parseArgs(argv) {
    const options = { expectedRoutes: [] }
    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index]
        if (!argument.startsWith("--")) fail(`unexpected argument ${argument}`)
        const name = argument.slice(2)
        if (name === "expected-route") {
            const value = argv[index + 1]
            if (value === undefined) fail(`${argument} requires a value`)
            options.expectedRoutes.push(value)
            index += 1
            continue
        }
        const value = argv[index + 1]
        if (value === undefined || value.startsWith("--")) fail(`${argument} requires a value`)
        if (!["before", "after", "metadata", "output"].includes(name)) fail(`unknown option ${argument}`)
        options[name] = value
        index += 1
    }
    if (options.after === undefined) fail("--after is required")
    return options
}

function getTableNames(database) {
    return new Set(database.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").all().map((row) => row.name))
}

function countRows(database, tables, table) {
    if (!tables.has(table)) return 0
    return Number(database.prepare(`SELECT COUNT(*) AS count FROM "${table}"`).get().count)
}

function sumColumn(database, tables, table, column) {
    if (!tables.has(table)) return 0
    return Number(database.prepare(`SELECT COALESCE(SUM("${column}"), 0) AS total FROM "${table}"`).get().total)
}

function readTutorialSummary(database, tables) {
    if (!tables.has("players")) return { rows: 0, minStep: null, maxStep: null, skippedCount: 0 }
    const row = database.prepare(
        "SELECT COUNT(*) AS rows, MIN(tutorial_step) AS min_step, MAX(tutorial_step) AS max_step, SUM(CASE WHEN tutorial_skip_flag IS NOT NULL AND tutorial_skip_flag != 0 THEN 1 ELSE 0 END) AS skipped_count FROM players",
    ).get()
    return {
        rows: Number(row.rows),
        minStep: row.min_step === null ? null : Number(row.min_step),
        maxStep: row.max_step === null ? null : Number(row.max_step),
        skippedCount: Number(row.skipped_count ?? 0),
    }
}

function readMailSummary(database, tables) {
    if (!tables.has("player_mails")) return { pendingCount: 0, receivedCount: 0, expiredCount: 0 }
    const row = database.prepare(
        "SELECT SUM(CASE WHEN received_at IS NULL THEN 1 ELSE 0 END) AS pending_count, SUM(CASE WHEN received_at IS NOT NULL THEN 1 ELSE 0 END) AS received_count, SUM(CASE WHEN expires_at IS NOT NULL AND expires_at <= strftime('%s', 'now') AND received_at IS NULL THEN 1 ELSE 0 END) AS expired_count FROM player_mails",
    ).get()
    return {
        pendingCount: Number(row.pending_count ?? 0),
        receivedCount: Number(row.received_count ?? 0),
        expiredCount: Number(row.expired_count ?? 0),
    }
}

function readIntegrity(database) {
    const result = database.pragma("integrity_check")
    return result.length === 1 && Object.values(result[0])[0] === "ok" ? "ok" : "failed"
}

// //// 读取数据库的安全聚合状态 [@x380kkm 2026-08-13] ////
export function readCnDatabaseSummary(databasePath) {
    if (typeof databasePath !== "string" || databasePath.length === 0) fail("database path is required")
    if (!existsSync(databasePath)) fail("database does not exist")
    const database = new Database(databasePath, { readonly: true, fileMustExist: true })
    try {
        database.pragma("query_only = ON")
        const tables = getTableNames(database)
        return {
            integrity: readIntegrity(database),
            accounts: { count: countRows(database, tables, "accounts") },
            players: { count: countRows(database, tables, "players") },
            resources: {
                freeVmoneyTotal: sumColumn(database, tables, "players", "free_vmoney"),
                vmoneyTotal: sumColumn(database, tables, "players", "vmoney"),
                freeManaTotal: sumColumn(database, tables, "players", "free_mana"),
                paidManaTotal: sumColumn(database, tables, "players", "paid_mana"),
                expPoolTotal: sumColumn(database, tables, "players", "exp_pool"),
            },
            characters: {
                rowCount: countRows(database, tables, "players_characters"),
                entryCountTotal: sumColumn(database, tables, "players_characters", "entry_count"),
            },
            items: {
                rowCount: countRows(database, tables, "players_items"),
                amountTotal: sumColumn(database, tables, "players_items", "amount"),
            },
            equipment: {
                rowCount: countRows(database, tables, "players_equipment"),
                stackTotal: sumColumn(database, tables, "players_equipment", "stack"),
            },
            gacha: {
                infoRowCount: countRows(database, tables, "players_gacha_info"),
                exchangePointTotal: sumColumn(database, tables, "players_gacha_info", "gacha_exchange_point"),
            },
            tutorials: readTutorialSummary(database, tables),
            mail: readMailSummary(database, tables),
        }
    } finally {
        database.close()
    }
}
// //// /读取数据库的安全聚合状态 ////

function subtract(after, before) {
    return after - before
}

function compareSection(after, before, fields) {
    return Object.fromEntries(fields.map((field) => [field, subtract(after[field], before[field])]))
}

// //// 比较两个实验快照而不返回身份数据 [@x380kkm 2026-08-13] ////
export function compareCnDatabaseSummaries(before, after) {
    if (before === undefined) return { after }
    return {
        integrity: { before: before.integrity, after: after.integrity },
        accountCountDelta: subtract(after.accounts.count, before.accounts.count),
        playerCountDelta: subtract(after.players.count, before.players.count),
        resourceDeltas: compareSection(after.resources, before.resources, [
            "freeVmoneyTotal", "vmoneyTotal", "freeManaTotal", "paidManaTotal", "expPoolTotal",
        ]),
        characterDeltas: compareSection(after.characters, before.characters, ["rowCount", "entryCountTotal"]),
        itemDeltas: compareSection(after.items, before.items, ["rowCount", "amountTotal"]),
        equipmentDeltas: compareSection(after.equipment, before.equipment, ["rowCount", "stackTotal"]),
        gachaDeltas: compareSection(after.gacha, before.gacha, ["infoRowCount", "exchangePointTotal"]),
        mailDeltas: compareSection(after.mail, before.mail, ["pendingCount", "receivedCount", "expiredCount"]),
        tutorial: {
            stable: JSON.stringify(before.tutorials) === JSON.stringify(after.tutorials),
            before: before.tutorials,
            after: after.tutorials,
        },
    }
}
// //// /比较两个实验快照而不返回身份数据 ////

// //// 汇总安全 HTTP 路由元数据 [@x380kkm 2026-08-13] ////
export function summarizeCnMetadata(metadataPath, expectedRoutes = []) {
    if (typeof metadataPath !== "string" || !existsSync(metadataPath)) fail("metadata file does not exist")
    const records = readFileSync(metadataPath, "utf8").split(/\r?\n/).filter((line) => line.length > 0).map((line, index) => {
        let record
        try {
            record = JSON.parse(line)
        } catch {
            fail(`metadata line ${index + 1} is not JSON`)
        }
        return validateCnMetadataRecord(record, `metadata line ${index + 1}`)
    })
    const counts = new Map()
    for (const record of records) {
        const key = `${record.method} ${record.path}`
        counts.set(key, (counts.get(key) ?? 0) + 1)
    }
    for (const route of expectedRoutes) {
        validateEvidenceRoute(route, "expected route")
        if (!counts.has(route)) fail(`expected route was not observed: ${route}`)
    }
    return {
        recordCount: records.length,
        routeCounts: Object.fromEntries([...counts.entries()].sort(([left], [right]) => left.localeCompare(right))),
        statuses: [...new Set(records.map((record) => record.status))].sort((left, right) => left - right),
        contentTypes: [...new Set(records.map((record) => record.contentType).filter((value) => value !== null))].sort(),
    }
}
// //// /汇总安全 HTTP 路由元数据 ////

export function createCnM4Summary({ beforePath, afterPath, metadataPath, expectedRoutes = [] }) {
    const after = readCnDatabaseSummary(afterPath)
    const before = beforePath === undefined ? undefined : readCnDatabaseSummary(beforePath)
    return {
        schemaVersion: SNAPSHOT_SCHEMA_VERSION,
        database: compareCnDatabaseSummaries(before, after),
        http: metadataPath === undefined ? null : summarizeCnMetadata(metadataPath, expectedRoutes),
    }
}

function main() {
    const options = parseArgs(process.argv.slice(2))
    const summary = createCnM4Summary({
        beforePath: options.before,
        afterPath: options.after,
        metadataPath: options.metadata,
        expectedRoutes: options.expectedRoutes,
    })
    const output = `${JSON.stringify(summary, null, 2)}\n`
    if (options.output === undefined) process.stdout.write(output)
    else writeFileSync(options.output, output, "utf8")
}

if (process.argv[1] !== undefined && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main()
