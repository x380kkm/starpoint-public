// audience: internal
// # sync-cn-gacha-asset
//
// 此脚本从 CN 客户端 CDN 归档读取扭蛋 master 和候选表, 并生成服务端运行时资产.

import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import unzipper from "unzipper"
import { assetEntryPaths, hashCnAssetPath } from "./cn-asset-paths.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { buildGachaAsset, collectGachaOrderedMapIds } from "./generate-cn-gacha-fixture.mjs"

const GACHA_MASTER_PATH = "master/gacha/gacha.orderedmap"

// //// 解析 CN 卡池同步输入 [@x380kkm 2026-08-24] ////
function parseArgs(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 2) {
        const argument = argv[index]
        const value = argv[index + 1]
        if (!argument?.startsWith("--") || value === undefined || value.startsWith("--")) {
            throw new Error(`invalid argument: ${argument ?? ""}`)
        }
        args[argument.slice(2)] = value
    }
    return args
}

function resolveManifestPath(cdnRoot, requestedPath) {
    if (requestedPath) return path.resolve(requestedPath)
    const entityRoot = path.join(cdnRoot, "entities")
    const candidates = fs.readdirSync(entityRoot)
        .filter((name) => /-ios_medium\.csv$/.test(name))
        .sort()
    if (candidates.length !== 1) {
        throw new Error(`expected one iOS EntityLists manifest, found ${candidates.length}`)
    }
    return path.join(entityRoot, candidates[0])
}

function readEntityManifest(manifestPath) {
    const records = new Map()
    for (const line of fs.readFileSync(manifestPath, "utf8").split(/\r?\n/)) {
        if (!line) continue
        const fields = line.split(",")
        if (fields.length !== 5) throw new Error(`invalid EntityLists row: ${line}`)
        const byteLength = Number(fields[2])
        if (!Number.isSafeInteger(byteLength) || byteLength < 0) {
            throw new Error(`invalid EntityLists byte length: ${fields[2]}`)
        }
        records.set(fields[0], {
            entryPath: fields[0],
            byteLength,
            digest: fields[3],
            assetKind: fields[4],
        })
    }
    return records
}

function getLogicalAssetRecord(manifest, logicalPath) {
    const matches = assetEntryPaths(hashCnAssetPath(logicalPath))
        .filter((entryPath) => manifest.has(entryPath))
        .map((entryPath) => manifest.get(entryPath))
    if (matches.length !== 1) {
        throw new Error(`expected one EntityLists record for ${logicalPath}, found ${matches.length}`)
    }
    return matches[0]
}

function calculateEntityDigest(bytes) {
    return crypto.createHash("sha256")
        .update(bytes)
        .digest("base64")
        .replace(/=+$/, "")
        .replaceAll("+", "_")
        .replaceAll("/", "-")
}

function listArchivePaths(cdnRoot, assetKinds) {
    const archivePaths = []
    for (const assetKind of [...assetKinds].sort()) {
        for (const suffix of ["full", "diff"]) {
            const archiveRoot = path.join(cdnRoot, `archive-${assetKind}-${suffix}`)
            if (!fs.existsSync(archiveRoot)) continue
            archivePaths.push(...fs.readdirSync(archiveRoot, { withFileTypes: true })
                .filter((entry) => entry.isFile() && entry.name.endsWith(".zip"))
                .map((entry) => path.join(archiveRoot, entry.name))
                .sort())
        }
    }
    return archivePaths
}
// //// /解析 CN 卡池同步输入 ////

// //// 读取 EntityLists 指定的 CN 客户端资产 [@x380kkm 2026-08-24] ////
async function readCnLogicalAssets(cdnRoot, manifest, logicalPaths) {
    const targets = new Map()
    for (const logicalPath of logicalPaths) {
        const record = getLogicalAssetRecord(manifest, logicalPath)
        if (targets.has(record.entryPath)) {
            throw new Error(`CN logical assets share one entry path: ${logicalPath}`)
        }
        targets.set(record.entryPath, { logicalPath, record })
    }

    const resolved = new Map()
    const assetKinds = new Set([...targets.values()].map((target) => target.record.assetKind))
    archives: for (const archivePath of listArchivePaths(cdnRoot, assetKinds)) {
        const archive = await unzipper.Open.file(archivePath)
        for (const entry of archive.files) {
            const target = targets.get(entry.path)
            if (!target || resolved.has(target.logicalPath)) continue
            if (entry.uncompressedSize !== target.record.byteLength) continue
            const bytes = await entry.buffer()
            if (calculateEntityDigest(bytes) !== target.record.digest) continue
            resolved.set(target.logicalPath, bytes)
            if (resolved.size === targets.size) break archives
        }
    }

    if (resolved.size !== targets.size) {
        const missing = [...targets.values()]
            .map((target) => target.logicalPath)
            .filter((logicalPath) => !resolved.has(logicalPath))
        throw new Error(`CN archives are missing ${missing.length} gacha assets: ${missing.slice(0, 8).join(", ")}`)
    }
    return resolved
}
// //// /读取 EntityLists 指定的 CN 客户端资产 ////

// //// 同步服务端使用的 CN 扭蛋资产 [@x380kkm 2026-08-24] ////
function writeJson(outputPath, value) {
    const contents = `${JSON.stringify(value, null, 4)}\n`
    if (fs.existsSync(outputPath) && fs.readFileSync(outputPath, "utf8").replaceAll("\r\n", "\n") === contents) {
        return
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, contents, "utf8")
}

export async function syncCnGachaAsset({ cdnRoot, manifestPath, outputPath, baselineOutputPath }) {
    const resolvedCdnRoot = path.resolve(cdnRoot)
    const resolvedManifestPath = resolveManifestPath(resolvedCdnRoot, manifestPath)
    const manifest = readEntityManifest(resolvedManifestPath)
    const masterAssets = await readCnLogicalAssets(resolvedCdnRoot, manifest, [GACHA_MASTER_PATH])
    const gachaMap = decodeOrderedMap(masterAssets.get(GACHA_MASTER_PATH))
    const orderedMapIds = collectGachaOrderedMapIds(gachaMap)
    const logicalPathById = new Map(orderedMapIds.map((mapId) => [
        mapId,
        `master/gacha_odds/${mapId}.orderedmap`,
    ]))
    const orderedMapAssets = await readCnLogicalAssets(
        resolvedCdnRoot,
        manifest,
        [...logicalPathById.values()],
    )
    const orderedMaps = Object.fromEntries([...logicalPathById].map(([mapId, logicalPath]) => [
        mapId,
        decodeOrderedMap(orderedMapAssets.get(logicalPath)),
    ]))
    const gachaAsset = buildGachaAsset(gachaMap, orderedMaps)
    const baselineGacha = gachaAsset["1"]
    if (!baselineGacha) throw new Error("CN gacha master is missing baseline pool 1")

    writeJson(path.resolve(outputPath), gachaAsset)
    writeJson(path.resolve(baselineOutputPath), { 1: baselineGacha })
    return {
        manifest: resolvedManifestPath,
        gachas: Object.keys(gachaAsset).length,
        orderedMaps: orderedMapIds.length,
        output: path.resolve(outputPath),
        baselineOutput: path.resolve(baselineOutputPath),
    }
}
// //// /同步服务端使用的 CN 扭蛋资产 ////

// //// 执行 CN 卡池同步命令 [@x380kkm 2026-08-24] ////
async function main() {
    const args = parseArgs(process.argv.slice(2))
    if (!args["cdn-root"] || !args.output || !args["baseline-output"]) {
        throw new Error("usage: --cdn-root DIRECTORY --output FILE --baseline-output FILE [--manifest FILE]")
    }
    const result = await syncCnGachaAsset({
        cdnRoot: args["cdn-root"],
        manifestPath: args.manifest,
        outputPath: args.output,
        baselineOutputPath: args["baseline-output"],
    })
    process.stdout.write(`${JSON.stringify(result)}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    main().catch((error) => {
        console.error(error.message)
        process.exitCode = 1
    })
}
// //// /执行 CN 卡池同步命令 ////
