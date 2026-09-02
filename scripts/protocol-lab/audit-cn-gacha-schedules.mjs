// audience: internal
// # audit-cn-gacha-schedules
//
// 该脚本按 JST 解析 CN 卡池时间, 对照地区别名和临时别名, 并报告客户端入口与服务端时间门控可能分叉的记录.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"
import { gachaBehaviorFingerprint } from "./generate-cn-gacha-region-policy.mjs"

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

function parseSampleTimes(value) {
    const labels = value ? value.split(",").filter(Boolean) : DEFAULT_SAMPLE_TIMES
    return labels.map((label) => {
        const timestamp = Date.parse(label)
        if (!Number.isSafeInteger(timestamp)) throw new Error(`invalid sample timestamp: ${label}`)
        return [label, timestamp]
    })
}
// //// /解析审计输入 ////

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, "utf8"))
}

function timestamp(pool, field, dateField) {
    const raw = pool[field]
    if (Number.isSafeInteger(raw)) return raw
    return parseCnMasterTimestamp(pool[dateField])
}

function classify(pool) {
    const categories = []
    if (pool.isComeback === true) categories.push("comeback")
    if (pool.isStarsGacha === true) categories.push("special")
    if (pool.paymentType > 0) categories.push("payment")
    if (pool.ticketExpiryAtMs !== null && pool.ticketExpiryAtMs !== undefined) categories.push("ticket")
    categories.push(`pageKind:${pool.pageKind}`)
    return categories
}

function openAtWindow(startAtMs, endAtMs, at) {
    return startAtMs !== null && endAtMs !== null && startAtMs <= at && at < endAtMs
}

function patchedClientWindow(id, pool, policy, document) {
    const canonicalId = policy.excludedRegionalAliases?.[String(id)]
        ?? policy.temporaryAliases?.[String(id)]
        ?? id
    const source = document[String(canonicalId)] ?? pool
    return {
        startAtMs: timestamp(source, "startAtMs", "startDate"),
        endAtMs: timestamp(source, "endAtMs", "endDate"),
    }
}

function countBy(items, selector) {
    return Object.fromEntries([...items.reduce((counts, item) => {
        const key = selector(item)
        counts.set(key, (counts.get(key) ?? 0) + 1)
        return counts
    }, new Map())].sort(([left], [right]) => left.localeCompare(right, "en", { numeric: true })))
}

function main() {
    const args = parseArgs(process.argv.slice(2))
    const root = path.resolve(args.root ?? REPOSITORY_ROOT)
    const gachaPath = path.resolve(args.gacha ?? path.join(root, "assets", "gacha.json"))
    const policyPath = path.resolve(args.policy ?? path.join(root, "assets", "gacha-region-policy.json"))
    const campaignPath = path.resolve(args.campaign ?? path.join(root, "assets", "gacha_campaign.json"))
    const document = readJson(gachaPath)
    const policy = readJson(policyPath)
    const campaigns = readJson(campaignPath)
    const rows = Object.entries(document).map(([id, pool]) => {
        const numericId = Number(id)
        const clientWindow = patchedClientWindow(numericId, pool, policy, document)
        return {
            id: numericId,
            pool,
            rawStartAtMs: timestamp(pool, "startAtMs", "startDate"),
            rawEndAtMs: timestamp(pool, "endAtMs", "endDate"),
            ...clientWindow,
            categories: classify(pool),
            behaviorFingerprint: gachaBehaviorFingerprint(pool),
            canonicalId: Number(policy.excludedRegionalAliases?.[id] ?? policy.normalizedCoverageAliases?.[id] ?? id),
            isRegionalAlias: Object.hasOwn(policy.excludedRegionalAliases ?? {}, id),
            isCoverageAlias: Object.hasOwn(policy.normalizedCoverageAliases ?? {}, id),
            isTemporaryAlias: Object.hasOwn(policy.temporaryAliases ?? {}, id),
            campaignId: campaigns[id] ?? null,
        }
    })
    const temporaryRows = Object.entries(policy.temporaryAliases ?? {}).map(([id, canonicalId]) => ({
        id: Number(id),
        pool: document[String(canonicalId)],
        rawStartAtMs: timestamp(document[String(canonicalId)], "startAtMs", "startDate"),
        rawEndAtMs: timestamp(document[String(canonicalId)], "endAtMs", "endDate"),
        ...patchedClientWindow(Number(id), document[String(canonicalId)], policy, document),
        categories: ["temporary", `pageKind:${document[String(canonicalId)]?.pageKind}`],
        behaviorFingerprint: document[String(canonicalId)]
            ? gachaBehaviorFingerprint(document[String(canonicalId)])
            : null,
        canonicalId: Number(canonicalId),
        isRegionalAlias: false,
        isCoverageAlias: false,
        isTemporaryAlias: true,
        campaignId: campaigns[String(canonicalId)] ?? null,
    }))
    const sampleTimes = parseSampleTimes(args.at)
    const canonicalRows = rows.filter((row) => !row.isRegionalAlias && !row.isCoverageAlias && !row.isTemporaryAlias)
    const canonicalById = new Map(canonicalRows.map((row) => [row.id, row]))
    const behaviorGroups = new Map()
    for (const row of rows.filter((entry) => !entry.isTemporaryAlias)) {
        const key = `${row.pool.type}:${row.behaviorFingerprint}`
        const group = behaviorGroups.get(key) ?? []
        group.push(row.id)
        behaviorGroups.set(key, group)
    }
    const duplicateWindowGroups = [...behaviorGroups.values()]
        .filter((ids) => ids.length > 1)
        .map((ids) => ids.sort((left, right) => left - right))
    const overlappingWindows = duplicateWindowGroups.map((ids) => ({
        ids,
        windows: ids.map((id) => {
            const row = rows.find((entry) => entry.id === id)
            return { id, startAtMs: row.startAtMs, endAtMs: row.endAtMs }
        }),
    })).filter((group) => group.windows.some((left) => group.windows.some((right) => left.id !== right.id && left.startAtMs < right.endAtMs && right.startAtMs < left.endAtMs)))
    const forkRecords = [...rows.filter((row) => row.isRegionalAlias || row.isCoverageAlias), ...temporaryRows].map((row) => ({
        id: row.id,
        canonicalId: row.canonicalId,
        kind: row.isTemporaryAlias ? "temporary" : row.isCoverageAlias ? "coverage" : "regional",
        rawStartAtMs: row.rawStartAtMs,
        rawEndAtMs: row.rawEndAtMs,
        clientStartAtMs: row.startAtMs,
        clientEndAtMs: row.endAtMs,
        canonicalStartAtMs: canonicalById.get(row.canonicalId)?.startAtMs ?? null,
        canonicalEndAtMs: canonicalById.get(row.canonicalId)?.endAtMs ?? null,
        campaignId: row.campaignId,
        serviceCanonicalGate: canonicalById.has(row.canonicalId),
    }))
    const visibleBySample = Object.fromEntries(sampleTimes.map(([label, at]) => [label, {
        clientMasterOpen: rows.filter((row) => openAtWindow(row.startAtMs, row.endAtMs, at)).length,
        serviceCanonicalOpen: canonicalRows.filter((row) => openAtWindow(row.startAtMs, row.endAtMs, at)).length,
        temporaryAliasCanonicalWindowOpen: temporaryRows.filter((row) => openAtWindow(row.startAtMs, row.endAtMs, at)).length,
    }]))
    const aliasWindowDifferences = forkRecords.filter((row) => row.kind !== "temporary"
        && (row.clientStartAtMs !== row.canonicalStartAtMs || row.clientEndAtMs !== row.canonicalEndAtMs))
    const missingCanonicalTargets = forkRecords.filter((row) => !row.serviceCanonicalGate)
    const output = {
        source: {
            gacha: path.relative(root, gachaPath).replaceAll(path.sep, "/"),
            policy: path.relative(root, policyPath).replaceAll(path.sep, "/"),
            campaign: path.relative(root, campaignPath).replaceAll(path.sep, "/"),
            timezone: "JST",
        },
        counts: {
            total: rows.length,
            canonical: canonicalRows.length,
            regionalAliases: rows.filter((row) => row.isRegionalAlias).length,
            coverageAliases: rows.filter((row) => row.isCoverageAlias).length,
            temporaryAliases: temporaryRows.length,
            campaignMappings: rows.filter((row) => row.campaignId !== null).length,
        },
        categories: countBy(rows, (row) => row.categories.join(",")),
        dateRange: {
            earliestStartAtMs: Math.min(...rows.map((row) => row.startAtMs).filter(Number.isFinite)),
            latestEndAtMs: Math.max(...rows.map((row) => row.endAtMs).filter(Number.isFinite)),
        },
        duplicateBehaviorGroups: duplicateWindowGroups,
        overlappingWindows,
        visibleBySample,
        forkRecords,
        aliasWindowDifferences,
        missingCanonicalTargets,
    }
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`)
}

main()
