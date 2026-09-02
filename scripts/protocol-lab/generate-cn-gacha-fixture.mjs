// audience: internal
// # generate-cn-gacha-fixture
// 此脚本把选定的 CN 扭蛋 orderedmap 转换为服务端使用的 Gacha 结构.

import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"
import { decodeOrderedMapFile } from "./decode-cn-orderedmap.mjs"

const CHARACTER_POOL_FIELDS = [14, 15, 16]
const EQUIPMENT_POOL_FIELDS = [22, 23, 24]

// //// 生成规范化 CN 扭蛋资产 [@x380kkm 2026-07-23] ////
function parseArgs(argv) {
    const args = {}
    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index]
        if (!argument.startsWith("--")) throw new Error(`unexpected argument: ${argument}`)
        const name = argument.slice(2)
        const value = argv[index + 1]
        if (value === undefined || value.startsWith("--")) throw new Error(`missing value for --${name}`)
        if (args[name] === undefined) args[name] = value
        else args[name] = Array.isArray(args[name]) ? [...args[name], value] : [args[name], value]
        index += 1
    }
    return args
}

function toNumber(value, field) {
    const number = Number(value)
    if (!Number.isFinite(number)) throw new Error(`${field} is not numeric: ${value}`)
    return number
}

function toOptionalNumber(value, field) {
    if (value === undefined || value === null || value === "" || value === "(None)") return null
    return toNumber(value, field)
}

function toOptionalString(value) {
    if (value === undefined || value === null || value === "" || value === "(None)") return null
    return String(value)
}

function toBoolean(value, field) {
    if (["true", "True", "TRUE"].includes(value)) return true
    if (["false", "False", "FALSE"].includes(value)) return false
    throw new Error(`${field} is not boolean: ${value}`)
}

function requireString(value, field) {
    if (typeof value !== "string" || value.length === 0 || value === "(None)") {
        throw new Error(`${field} is missing`)
    }
    return value
}

function normalizeWeights(entries) {
    const total = entries.reduce((sum, entry) => sum + entry.weight, 0)
    if (total <= 0) throw new Error("gacha rarity weights must be positive")
    const weights = entries.map((entry) => Math.round(entry.weight * 1000 / total))
    const difference = 1000 - weights.reduce((sum, weight) => sum + weight, 0)
    weights[weights.length - 1] += difference
    return weights
}

function getNestedMap(map, key) {
    const value = map[key]
    if (value === undefined || Array.isArray(value)) throw new Error(`missing nested orderedmap entry: ${key}`)
    return value
}

function getRankEntries(rarityMap, rarityMinimum = 1) {
    return Object.values(rarityMap)
        .map((row) => ({ rarity: toNumber(row[0], "rarity"), weight: toNumber(row[1], "rarity weight") }))
        .filter((entry) => entry.rarity >= rarityMinimum)
        .sort((left, right) => right.rarity - left.rarity)
}

function getPool(oddsMap, oddsId) {
    const rows = Object.values(getNestedMap(oddsMap, oddsId))
    const items = rows.map((row) => {
        const weight = toNumber(row[2], "pool weight")
        return {
            id: toNumber(row[0], "pool id"),
            rank: toNumber(row[1], "pool rank"),
            odds: weight,
            isRateUp: toBoolean(row[3], "pool rate-up flag"),
        }
    })
    const totalOdds = items.reduce((sum, item) => sum + item.odds, 0)
    if (totalOdds <= 0) throw new Error(`gacha pool weights must be positive: ${oddsId}`)
    return items.map((item) => ({
        ...item,
        rarity: Math.round(item.odds * 100000 / totalOdds) / 100,
    }))
}

function getPoolFields(gachaType) {
    if (gachaType === 0) return CHARACTER_POOL_FIELDS
    if (gachaType === 1) return EQUIPMENT_POOL_FIELDS
    throw new Error(`unsupported gacha prize kind: ${gachaType}`)
}

function buildGacha(gachaRow, rarityMap, oddsMaps) {
    const stringId = requireString(gachaRow[0], "gacha string id")
    const title = requireString(gachaRow[1], "gacha title")
    const bannerImage = requireString(gachaRow[3], "gacha banner image")
    const type = toNumber(gachaRow[13], "gacha prize kind")
    const pageKind = toNumber(gachaRow[4], "gacha page kind")
    if (!Number.isInteger(pageKind) || pageKind < 0 || pageKind > 8) {
        throw new Error(`unsupported gacha page kind: ${gachaRow[4]}`)
    }
    const rarityTable = getNestedMap(rarityMap, gachaRow[11])
    const rarityEntries = getRankEntries(rarityTable)
    const guaranteeRarity = toNumber(gachaRow[10], "gacha guarantee rarity")
    const pool = {}
    for (const field of getPoolFields(type)) {
        const oddsId = gachaRow[field]
        if (oddsId === "") continue
        const odds = getPool(oddsMaps[oddsId], oddsId)
        const rank = odds[0]?.rank
        if (rank === undefined) throw new Error(`empty gacha pool: ${oddsId}`)
        pool[String(6 - rank)] = odds
    }

    const normalEntries = rarityEntries
    const guaranteeEntries = getRankEntries(rarityTable, guaranteeRarity)
    const startDate = gachaRow[29]
    const endDate = gachaRow[30]
    const startAtMs = parseCnMasterTimestamp(startDate)
    const endAtMs = parseCnMasterTimestamp(endDate)
    const ticketExpiryAtMs = parseCnMasterTimestamp(gachaRow[31])
    if (startAtMs === null || endAtMs === null || endAtMs <= startAtMs) {
        throw new Error("gacha time window is invalid")
    }
    return {
        stringId,
        title,
        name: title,
        listOrder: toNumber(gachaRow[2], "gacha list order"),
        bannerImage,
        pageKind,
        type,
        paymentType: 0,
        singleCost: toNumber(gachaRow[5], "single cost"),
        multiCost: toNumber(gachaRow[6], "multi cost"),
        discountCost: toNumber(gachaRow[7], "discount cost"),
        tenTimesPerAccountCost: toOptionalNumber(gachaRow[8], "ten times per account cost"),
        guaranteeNumber: toNumber(gachaRow[9], "guarantee number"),
        onceTicketItemId: toOptionalNumber(gachaRow[27], "once ticket item id"),
        tenTimesTicketItemId: toOptionalNumber(gachaRow[28], "ten times ticket item id"),
        crazyTenTimesTicketItemId: toOptionalNumber(gachaRow[45], "crazy ten times ticket item id"),
        wildcardCharacterTicketAvailable: type === 0
            ? toBoolean(gachaRow[20], "wildcard character ticket flag")
            : false,
        canBeStartDashExchange: type === 0
            ? toBoolean(gachaRow[21], "start dash exchange flag")
            : false,
        wildcardEquipmentTicketAvailable: type === 1
            ? toBoolean(gachaRow[26], "wildcard equipment ticket flag")
            : false,
        ticketExpiryAtMs,
        showPeriod: toBoolean(gachaRow[32], "gacha show period flag"),
        isComeback: toBoolean(gachaRow[43], "gacha comeback flag"),
        isStarsGacha: toBoolean(gachaRow[46], "gacha stars flag"),
        freemiumGuaranteeAvailable: toBoolean(gachaRow[44], "gacha freemium guarantee flag"),
        canBeTutorial: toBoolean(gachaRow[38], "gacha tutorial flag"),
        tutorialOddsRarityStringId: toOptionalString(gachaRow[39]),
        tutorialMovieId: toOptionalString(gachaRow[40]),
        tutorialOnceCost: toOptionalNumber(gachaRow[41], "tutorial once cost"),
        tutorialReasonId: toOptionalNumber(gachaRow[42], "tutorial reason id"),
        movieName: gachaRow[17] ?? "normal",
        guaranteeMovieName: gachaRow[18] ?? "normal_guarantee",
        startDate,
        endDate,
        startAtMs,
        endAtMs,
        rankRates: {
            normal: normalizeWeights(normalEntries),
            multiGuarantee: normalizeWeights(guaranteeEntries),
        },
        pool,
    }
}

export function collectGachaOrderedMapIds(gachaMap) {
    const mapIds = new Set()
    for (const gachaRow of Object.values(gachaMap)) {
        const type = toNumber(gachaRow[13], "gacha prize kind")
        const rarityId = gachaRow[11]
        if (!rarityId) throw new Error("gacha rarity map id is missing")
        mapIds.add(rarityId)
        for (const field of getPoolFields(type)) {
            const oddsId = gachaRow[field]
            if (!oddsId) throw new Error(`gacha pool map id is missing at field ${field}`)
            mapIds.add(oddsId)
        }
    }
    return [...mapIds]
}

export function buildGachaAsset(gachaMap, orderedMaps, gachaIds = Object.keys(gachaMap)) {
    const output = {}
    for (const gachaId of gachaIds) {
        const gachaRow = gachaMap[gachaId]
        if (!gachaRow) throw new Error(`gacha id not found: ${gachaId}`)
        const rarityId = gachaRow[11]
        const rarityMap = orderedMaps[rarityId]
        if (!rarityMap) throw new Error(`gacha rarity map is missing: ${rarityId}`)
        output[gachaId] = buildGacha(gachaRow, rarityMap, orderedMaps)
    }
    return output
}

function main() {
    const args = parseArgs(process.argv.slice(2))
    if (!args.gacha || !args.rarity || !args.map || !args.output) {
        throw new Error("usage: --gacha FILE --rarity FILE --map ID=FILE ... --output FILE [--id ID]")
    }
    const gachaId = args.id ?? "1"
    const gachaMap = decodeOrderedMapFile(args.gacha)
    const gachaRow = gachaMap[gachaId]
    if (!gachaRow) throw new Error(`gacha id not found: ${gachaId}`)
    const rarityMap = decodeOrderedMapFile(args.rarity)
    const orderedMaps = { [gachaRow[11]]: rarityMap }
    const mapArguments = Array.isArray(args.map) ? args.map : [args.map]
    for (const mapArgument of mapArguments) {
        const separator = mapArgument.indexOf("=")
        if (separator <= 0) throw new Error(`invalid --map value: ${mapArgument}`)
        const oddsId = mapArgument.slice(0, separator)
        orderedMaps[oddsId] = decodeOrderedMapFile(mapArgument.slice(separator + 1))
    }

    const output = buildGachaAsset(gachaMap, orderedMaps, [gachaId])
    fs.writeFileSync(args.output, `${JSON.stringify(output, null, 4)}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
// //// /生成规范化 CN 扭蛋资产 ////
