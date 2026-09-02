// audience: internal
// # cn-m4-summary-tests
//
// 该脚本验证 CN M4 聚合摘要只返回统计状态和安全路由元数据.

import assert from "node:assert/strict"
import { mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { createRequire } from "node:module"
import { join } from "node:path"
import { tmpdir } from "node:os"

import { compareCnDatabaseSummaries, readCnDatabaseSummary, summarizeCnMetadata } from "./cn-m4-summary.mjs"

const require = createRequire(import.meta.url)
const Database = require("better-sqlite3")

function createFixture(databasePath, values) {
    const database = new Database(databasePath)
    database.exec(`
        CREATE TABLE accounts (id INTEGER PRIMARY KEY);
        CREATE TABLE players (
            id INTEGER PRIMARY KEY,
            free_vmoney INTEGER NOT NULL,
            vmoney INTEGER NOT NULL,
            free_mana INTEGER NOT NULL,
            paid_mana INTEGER NOT NULL,
            exp_pool INTEGER NOT NULL,
            tutorial_step INTEGER,
            tutorial_skip_flag INTEGER
        );
        CREATE TABLE players_characters (id INTEGER, entry_count INTEGER);
        CREATE TABLE players_items (id INTEGER, amount INTEGER);
        CREATE TABLE players_equipment (id INTEGER, stack INTEGER);
        CREATE TABLE players_gacha_info (gacha_id INTEGER, gacha_exchange_point INTEGER);
        CREATE TABLE player_mails (id INTEGER, received_at INTEGER, expires_at INTEGER);
    `)
    database.prepare("INSERT INTO accounts (id) VALUES (1)").run()
    database.prepare("INSERT INTO players VALUES (1, ?, ?, ?, ?, ?, ?, ?)").run(
        values.freeVmoney,
        values.vmoney,
        values.freeMana,
        values.paidMana,
        values.expPool,
        values.tutorialStep,
        values.tutorialSkipFlag,
    )
    database.prepare("INSERT INTO players_characters VALUES (100, ?)").run(values.characterEntries)
    database.prepare("INSERT INTO players_items VALUES (200, ?)").run(values.itemAmount)
    database.prepare("INSERT INTO players_equipment VALUES (300, ?)").run(values.equipmentStack)
    database.prepare("INSERT INTO players_gacha_info VALUES (1, ?)").run(values.exchangePoints)
    database.prepare("INSERT INTO player_mails VALUES (1, NULL, NULL)").run()
    database.close()
}

const temporaryRoot = mkdtempSync(join(tmpdir(), "starpoint-cn-m4-summary-"))
try {
    const beforePath = join(temporaryRoot, "before.sqlite")
    const afterPath = join(temporaryRoot, "after.sqlite")
    createFixture(beforePath, {
        freeVmoney: 100,
        vmoney: 5,
        freeMana: 20,
        paidMana: 0,
        expPool: 10,
        tutorialStep: 2,
        tutorialSkipFlag: 0,
        characterEntries: 1,
        itemAmount: 3,
        equipmentStack: 1,
        exchangePoints: 0,
    })
    createFixture(afterPath, {
        freeVmoney: 90,
        vmoney: 5,
        freeMana: 20,
        paidMana: 0,
        expPool: 10,
        tutorialStep: 2,
        tutorialSkipFlag: 0,
        characterEntries: 2,
        itemAmount: 3,
        equipmentStack: 1,
        exchangePoints: 1,
    })

    const before = readCnDatabaseSummary(beforePath)
    const after = readCnDatabaseSummary(afterPath)
    assert.equal(before.integrity, "ok")
    assert.equal(after.characters.rowCount, 1)
    const transition = compareCnDatabaseSummaries(before, after)
    assert.equal(transition.resourceDeltas.freeVmoneyTotal, -10)
    assert.equal(transition.characterDeltas.entryCountTotal, 1)
    assert.equal(transition.gachaDeltas.exchangePointTotal, 1)
    assert.equal(transition.tutorial.stable, true)

    const metadataPath = join(temporaryRoot, "metadata.jsonl")
    writeFileSync(metadataPath, [
        JSON.stringify({ observedAtUtc: "2026-08-13T00:00:00.000Z", method: "POST", path: "/api/index.php/gacha/exec", status: 200, contentType: "application/x-msgpack" }),
        JSON.stringify({ observedAtUtc: "2026-08-13T00:00:01.000Z", method: "POST", path: "/api/index.php/gacha/exec", status: 200, contentType: "application/x-msgpack" }),
    ].join("\n") + "\n", "utf8")
    const http = summarizeCnMetadata(metadataPath, ["POST /api/index.php/gacha/exec"])
    assert.equal(http.recordCount, 2)
    assert.equal(http.routeCounts["POST /api/index.php/gacha/exec"], 2)
    assert.throws(
        () => summarizeCnMetadata(metadataPath, ["POST /api/index.php/gacha/exec?viewer_id=private"]),
        /invalid CN evidence/,
    )
    const serialized = JSON.stringify({ transition, http })
    assert.doesNotMatch(serialized, /player_id|viewer_id|token|payload|character_id/)
    assert.doesNotMatch(serialized, /gacha\/exec[^"}]*[?#\s]/)
} finally {
    rmSync(temporaryRoot, { recursive: true, force: true })
}

process.stdout.write("CN M4 summary test passed.\n")
