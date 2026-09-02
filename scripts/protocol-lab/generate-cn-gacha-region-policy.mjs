// audience: internal
// # generate-cn-gacha-region-policy
// 此脚本从 CN 扭蛋运行时资产生成地区别名和客户端临时入口映射. 地区别名只在普通卡池满足完整行为等价时成立.

import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import { FEATURE_BANNER_RELATION_SCHEMAS } from "./cn-activity-master-schema.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")
const DEFAULT_INPUT_PATH = path.join(REPOSITORY_ROOT, "assets", "gacha.json")
const DEFAULT_OUTPUT_PATH = path.join(REPOSITORY_ROOT, "assets", "gacha-region-policy.json")
const DEFAULT_BANNER_RESOLUTION_PATH = path.join(REPOSITORY_ROOT, "assets", "gacha-banner-resolution.json")
const DEFAULT_RAW_ODDS_SIGNATURES_PATH = path.join(REPOSITORY_ROOT, "assets", "gacha-raw-odds-signatures.json")
const DEFAULT_FEATURE_LINK_EVIDENCE_PATH = path.join(REPOSITORY_ROOT, "assets", "gacha-feature-link-evidence.json")
const TEMPORARY_ALIAS_OFFSET = 1_000_000
const NORMALIZED_COVERAGE_ALIASES = new Map([["61", "1"]])
const TIME_AND_PRESENTATION_FIELDS = new Set([
    "bannerImage",
    "endAtMs",
    "endDate",
    "listOrder",
    "name",
    "startAtMs",
    "startDate",
    "stringId",
    "ticketExpiryAtMs",
    "title",
])

// //// 规范化行为数据以比较卡池 [@x380kkm 2026-08-24] ////
function stableValue(value) {
    if (Array.isArray(value)) return value.map(stableValue)
    if (value === null || typeof value !== "object") return value
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stableValue(value[key])]))
}

function compareCandidates(left, right) {
    const fields = ["id", "rank", "odds", "isRateUp", "rarity"]
    for (const field of fields) {
        const comparison = JSON.stringify(left[field]).localeCompare(JSON.stringify(right[field]), "en", { numeric: true })
        if (comparison !== 0) return comparison
    }
    return JSON.stringify(stableValue(left)).localeCompare(JSON.stringify(stableValue(right)))
}

function normalizedPool(pool) {
    if (pool === null || typeof pool !== "object" || Array.isArray(pool)) {
        throw new Error("CN gacha pool is invalid")
    }
    return Object.fromEntries(Object.keys(pool).sort((left, right) => left.localeCompare(right, "en", { numeric: true }))
        .map((rank) => {
            if (!Array.isArray(pool[rank])) throw new Error(`CN gacha rank ${rank} is invalid`)
            const candidates = pool[rank].map((candidate) => {
                if (candidate === null || typeof candidate !== "object" || Array.isArray(candidate)) {
                    throw new Error(`CN gacha rank ${rank} contains an invalid candidate`)
                }
                for (const field of ["id", "rank", "odds", "isRateUp", "rarity"]) {
                    if (!Object.hasOwn(candidate, field)) throw new Error(`CN gacha candidate is missing ${field}`)
                }
                return stableValue(candidate)
            }).sort(compareCandidates)
            return [rank, candidates]
        }))
}

export function gachaBehaviorFingerprint(pool) {
    const behavior = {}
    for (const [key, value] of Object.entries(pool)) {
        if (TIME_AND_PRESENTATION_FIELDS.has(key)) continue
        behavior[key] = key === "pool" ? normalizedPool(value) : value
    }
    return crypto.createHash("sha256").update(JSON.stringify(stableValue(behavior))).digest("hex")
}

export function gachaContentFingerprint(pool) {
    const content = { type: pool.type, pool: normalizedPool(pool.pool) }
    return crypto.createHash("sha256").update(JSON.stringify(stableValue(content))).digest("hex")
}
// //// /规范化行为数据以比较卡池 ////

// //// 识别严格等价的 CN 地区别名 [@x380kkm 2026-08-24] ////
function indexPoolsByStringId(document) {
    const pools = new Map()
    for (const [id, pool] of Object.entries(document)) {
        const stringId = pool?.stringId
        if (typeof stringId !== "string" || stringId.length === 0) {
            throw new Error(`CN gacha ${id} has an invalid stringId`)
        }
        if (pools.has(stringId)) throw new Error(`CN gacha stringId is duplicated: ${stringId}`)
        pools.set(stringId, { id, pool })
    }
    return pools
}

function findRegionalAliasPairs(document, rawOddsSignatures) {
    const poolsByStringId = indexPoolsByStringId(document)
    const pairs = []
    for (const [canonicalId, canonical] of Object.entries(document)) {
        if (canonical?.pageKind !== 0 || !canonical.stringId.endsWith("_1")) continue
        const stringIdBase = canonical.stringId.slice(0, -2)
        const alias = poolsByStringId.get(stringIdBase)
        if (!alias || alias.pool?.pageKind !== 0) continue
        if (alias.pool.bannerImage !== canonical.bannerImage) continue
        if (rawOddsSignatures[alias.id] !== rawOddsSignatures[canonicalId]) continue
        const fingerprint = gachaBehaviorFingerprint(canonical)
        if (gachaBehaviorFingerprint(alias.pool) !== fingerprint) continue
        pairs.push({
            aliasId: alias.id,
            canonicalId,
            fingerprint,
            stringIdBase,
            type: canonical.type,
        })
    }
    return pairs.sort((left, right) => Number(left.aliasId) - Number(right.aliasId))
}
// //// /识别严格等价的 CN 地区别名 ////

function buildNormalizedCoverageAliases(document, rawOddsSignatures) {
    const aliases = []
    for (const [aliasId, canonicalId] of NORMALIZED_COVERAGE_ALIASES) {
        const alias = document[aliasId]
        const canonical = document[canonicalId]
        const rankRatesMatch = JSON.stringify(stableValue(alias?.rankRates))
            === JSON.stringify(stableValue(canonical?.rankRates))
        if (!alias || !canonical || alias.pageKind !== 0 || canonical.pageKind !== 0
            || alias.bannerImage !== canonical.bannerImage
            || alias.guaranteeNumber !== canonical.guaranteeNumber
            || !rankRatesMatch
            || gachaContentFingerprint(alias) !== gachaContentFingerprint(canonical)
            || rawOddsSignatures[aliasId] !== rawOddsSignatures[canonicalId]) {
            throw new Error(`normalized CN gacha coverage alias is invalid: ${aliasId}`)
        }
        aliases.push([aliasId, Number(canonicalId)])
    }
    return numericObject(aliases)
}

// //// 将首页地区别名跳转投影到规范卡池 [@x380kkm 2026-08-24] ////
function buildFeatureLinkProjections(document, excludedRegionalAliases, featureLinkEvidence) {
    if (featureLinkEvidence?.discriminator !== 4 || !Array.isArray(featureLinkEvidence.tables)
        || featureLinkEvidence.tables.length !== FEATURE_BANNER_RELATION_SCHEMAS.length) {
        throw new Error("CN gacha feature link evidence is invalid")
    }
    const expectedSchemas = new Map(FEATURE_BANNER_RELATION_SCHEMAS.map((schema) => [schema.name, schema]))
    const projections = {}
    for (const table of featureLinkEvidence.tables) {
        const schema = expectedSchemas.get(table?.name)
        const target = schema?.targetsByDiscriminator.get("4")
        if (!schema || !target || table.logicalPath !== schema.logicalPath
            || table.discriminatorIndex !== schema.discriminatorIndex || table.targetIdIndex !== target.idIndex
            || !/^[a-f0-9]{64}$/.test(table.sourceSha256 ?? "")
            || !Number.isSafeInteger(table.rowCount) || table.rowCount < 0
            || !Number.isSafeInteger(table.ordinaryLinkCount) || table.ordinaryLinkCount < 0
            || !Number.isSafeInteger(table.ordinaryTargetCount) || table.ordinaryTargetCount < 0
            || !Array.isArray(table.ordinaryGachaIds)
            || table.ordinaryTargetCount !== table.ordinaryGachaIds.length) {
            throw new Error("CN gacha feature link table evidence is invalid")
        }
        expectedSchemas.delete(table.name)
        const tableProjections = []
        const seenIds = new Set()
        for (const aliasId of table.ordinaryGachaIds) {
            const key = String(aliasId)
            if (!Number.isSafeInteger(aliasId) || aliasId < 1 || seenIds.has(aliasId) || !document[key]) {
                throw new Error(`feature-linked gacha is invalid: ${aliasId}`)
            }
            seenIds.add(aliasId)
            const canonicalId = excludedRegionalAliases[key]
            if (canonicalId === undefined) continue
            const alias = document[key]
            const canonical = document[String(canonicalId)]
            if (!alias || !canonical || alias.pageKind !== 0 || canonical.pageKind !== 0
                || alias.isComeback !== false || canonical.isComeback !== false
                || alias.isStarsGacha !== false || canonical.isStarsGacha !== false) {
                throw new Error(`feature-linked regional gacha alias is not ordinary: ${aliasId}`)
            }
            tableProjections.push([key, Number(canonicalId)])
        }
        projections[table.name] = numericObject(tableProjections)
    }
    if (expectedSchemas.size !== 0) throw new Error("CN gacha feature link table evidence is incomplete")
    return projections
}
// //// /将首页地区别名跳转投影到规范卡池 ////

// //// 生成地区策略和客户端临时入口映射 [@x380kkm 2026-08-24] ////
function numericObject(entries) {
    return Object.fromEntries([...entries].sort(([left], [right]) => Number(left) - Number(right)))
}

function buildBannerPathOverrides(document, retainedIds, resolvedBannerPoolIds) {
    const anchorByBanner = new Map()
    for (const pool of Object.values(document)) {
        const bannerImage = pool.bannerImage
        if (bannerImage.split("/").at(-1) !== pool.stringId) continue
        const contentFingerprint = gachaContentFingerprint(pool)
        const anchor = anchorByBanner.get(bannerImage)
        if (anchor && anchor !== contentFingerprint) {
            throw new Error(`CN gacha banner has ambiguous named content: ${bannerImage}`)
        }
        anchorByBanner.set(bannerImage, contentFingerprint)
    }
    const overrides = []
    for (const id of retainedIds) {
        const pool = document[id]
        const anchor = anchorByBanner.get(pool.bannerImage)
        if (!anchor || anchor === gachaContentFingerprint(pool) || resolvedBannerPoolIds.has(id)) continue
        overrides.push([id, `dynamic/gacha_list_banner/starpoint_generated/${id}`])
    }
    return numericObject(overrides)
}

function usesServerCampaignPeriod(pool) {
    return pool.isComeback === true || pool.isStarsGacha === true
}

export function buildCnGachaRegionPolicy(
    document,
    sourceBytes = Buffer.from(JSON.stringify(document)),
    bannerResolution = { resolvedPoolIds: [], sourceCatalogSha256: null },
    rawOddsEvidence,
    featureLinkEvidence,
) {
    if (document === null || typeof document !== "object" || Array.isArray(document)) {
        throw new Error("CN gacha asset root is invalid")
    }
    const sourceIds = Object.keys(document)
    if (!sourceIds.every((id) => /^\d+$/.test(id) && Number(id) > 0)) {
        throw new Error("CN gacha asset contains an invalid ID")
    }
    if (rawOddsEvidence?.sourceRegion !== "cn" || rawOddsEvidence.poolCount !== sourceIds.length
        || rawOddsEvidence.signatures === null || typeof rawOddsEvidence.signatures !== "object"
        || !sourceIds.every((id) => /^[a-f0-9]{64}$/.test(rawOddsEvidence.signatures[id] ?? ""))) {
        throw new Error("CN gacha raw odds evidence is invalid")
    }
    const pairs = findRegionalAliasPairs(document, rawOddsEvidence.signatures)
    const excludedRegionalAliases = numericObject(pairs.map((pair) => [pair.aliasId, Number(pair.canonicalId)]))
    const normalizedCoverageAliases = buildNormalizedCoverageAliases(
        document,
        rawOddsEvidence.signatures,
    )
    const featureLinkProjections = buildFeatureLinkProjections(
        document,
        excludedRegionalAliases,
        featureLinkEvidence,
    )
    const featureLinkSources = Object.fromEntries(featureLinkEvidence.tables
        .map((table) => [table.name, table.sourceSha256]))
    const excludedIds = new Set([
        ...Object.keys(excludedRegionalAliases),
        ...Object.keys(normalizedCoverageAliases),
    ])
    const retainedIds = sourceIds.filter((id) => !excludedIds.has(id)).sort((left, right) => Number(left) - Number(right))
    const occupiedIds = new Set(sourceIds)
    const temporaryAliases = {}
    const temporaryAliasIds = retainedIds.filter((id) => !usesServerCampaignPeriod(document[id]))
    for (const canonicalId of temporaryAliasIds) {
        const temporaryId = String(TEMPORARY_ALIAS_OFFSET + Number(canonicalId))
        if (!Number.isSafeInteger(Number(temporaryId)) || occupiedIds.has(temporaryId) || Object.hasOwn(temporaryAliases, temporaryId)) {
            throw new Error(`CN gacha temporary alias collides: ${temporaryId}`)
        }
        temporaryAliases[temporaryId] = Number(canonicalId)
    }
    const canonicalPairIds = new Set(pairs.map((pair) => pair.canonicalId))
    const resolvedBannerPoolIds = new Set(bannerResolution.resolvedPoolIds.map(String))
    const bannerPathOverrides = buildBannerPathOverrides(document, retainedIds, resolvedBannerPoolIds)
    const retainedSpecialIds = retainedIds.filter((id) => document[id].pageKind !== 0).map(Number)
    const retainedUnclassifiedIds = retainedIds
        .filter((id) => document[id].pageKind === 0 && !canonicalPairIds.has(id))
        .map(Number)
    const fingerprints = new Set(pairs.map((pair) => pair.fingerprint))
    return {
        sourceRegion: "cn",
        sourceSha256: crypto.createHash("sha256").update(sourceBytes).digest("hex"),
        sourceCatalogSha256: bannerResolution.sourceCatalogSha256,
        rawOddsSourceSha256: rawOddsEvidence.sourceSha256,
        sourcePoolCount: sourceIds.length,
        excludedRegionalAliases,
        normalizedCoverageAliases,
        featureLinkProjections,
        featureLinkSources,
        bannerPathOverrides,
        temporaryAliases: numericObject(Object.entries(temporaryAliases)),
        retainedSpecialIds,
        retainedUnclassifiedIds,
        evidence: {
            aliasPairCount: pairs.length,
            behaviorGroupCount: fingerprints.size,
            characterAliasCount: pairs.filter((pair) => pair.type === 0).length,
            equipmentAliasCount: pairs.filter((pair) => pair.type === 1).length,
            normalizedCoverageAliasCount: Object.keys(normalizedCoverageAliases).length,
            projectedFeatureLinkCount: Object.values(featureLinkProjections)
                .reduce((sum, table) => sum + Object.keys(table).length, 0),
            retainedPoolCount: retainedIds.length,
            temporaryAliasCount: Object.keys(temporaryAliases).length,
            bannerPathOverrideCount: Object.keys(bannerPathOverrides).length,
        },
    }
}

export function buildCnGachaBannerResolution(catalog, sourceBytes) {
    if (!Array.isArray(catalog?.activities)) throw new Error("CN activity catalog is invalid")
    const resolvedPoolIds = catalog.activities
        .filter((activity) => typeof activity?.activity_id === "string"
            && /^gacha:\d+$/.test(activity.activity_id)
            && Array.isArray(activity.image_candidates)
            && activity.image_candidates.some((candidate) => candidate?.source_type === "activity_banner"
                && candidate.width === 510 && candidate.height === 180))
        .map((activity) => Number(activity.activity_id.slice("gacha:".length)))
        .sort((left, right) => left - right)
    return {
        sourceCatalogSha256: crypto.createHash("sha256").update(sourceBytes).digest("hex"),
        resolvedPoolIds,
    }
}

export function readCnGachaRegionPolicy(policyPath = DEFAULT_OUTPUT_PATH) {
    const policy = JSON.parse(fs.readFileSync(policyPath, "utf8"))
    const projectionTables = policy?.featureLinkProjections
    const featureLinkSources = policy?.featureLinkSources
    const projectionsAreValid = projectionTables !== null && typeof projectionTables === "object"
        && Object.keys(projectionTables).length === FEATURE_BANNER_RELATION_SCHEMAS.length
        && FEATURE_BANNER_RELATION_SCHEMAS.every((schema) => {
            const table = projectionTables[schema.name]
            return table !== null && typeof table === "object" && !Array.isArray(table)
                && Object.entries(table).every(([aliasId, canonicalId]) => /^\d+$/.test(aliasId)
                    && Number(aliasId) > 0 && Number.isSafeInteger(canonicalId) && canonicalId > 0)
        })
    const featureLinkSourcesAreValid = featureLinkSources !== null && typeof featureLinkSources === "object"
        && Object.keys(featureLinkSources).length === FEATURE_BANNER_RELATION_SCHEMAS.length
        && FEATURE_BANNER_RELATION_SCHEMAS.every((schema) => /^[a-f0-9]{64}$/.test(featureLinkSources[schema.name] ?? ""))
    if (policy?.sourceRegion !== "cn" || !/^[a-f0-9]{64}$/.test(policy.sourceSha256 ?? "")
        || !/^[a-f0-9]{64}$/.test(policy.rawOddsSourceSha256 ?? "")
        || policy.excludedRegionalAliases === null || typeof policy.excludedRegionalAliases !== "object"
        || policy.normalizedCoverageAliases === null || typeof policy.normalizedCoverageAliases !== "object"
        || !projectionsAreValid
        || !featureLinkSourcesAreValid
        || policy.bannerPathOverrides === null || typeof policy.bannerPathOverrides !== "object"
        || policy.temporaryAliases === null || typeof policy.temporaryAliases !== "object") {
        throw new Error("CN gacha region policy is invalid")
    }
    return policy
}
// //// /生成地区策略和客户端临时入口映射 ////

// //// 读取命令参数并写入策略资产 [@x380kkm 2026-08-24] ////
function readOption(args, name, fallback) {
    const index = args.indexOf(name)
    if (index < 0) return fallback
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`)
    return path.resolve(value)
}

function readOptionalOption(args, name) {
    const index = args.indexOf(name)
    if (index < 0) return null
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`)
    return path.resolve(value)
}

function generate(args) {
    const inputPath = readOption(args, "--input", DEFAULT_INPUT_PATH)
    const outputPath = readOption(args, "--output", DEFAULT_OUTPUT_PATH)
    const bannerResolutionPath = readOption(
        args,
        "--banner-resolution",
        DEFAULT_BANNER_RESOLUTION_PATH,
    )
    const catalogPath = readOptionalOption(args, "--catalog")
    const rawOddsSignaturesPath = readOption(
        args,
        "--raw-odds-signatures",
        DEFAULT_RAW_ODDS_SIGNATURES_PATH,
    )
    const featureLinkEvidencePath = readOption(
        args,
        "--feature-link-evidence",
        DEFAULT_FEATURE_LINK_EVIDENCE_PATH,
    )
    const sourceBytes = fs.readFileSync(inputPath)
    let bannerResolution
    if (catalogPath) {
        const catalogBytes = fs.readFileSync(catalogPath)
        bannerResolution = buildCnGachaBannerResolution(
            JSON.parse(catalogBytes.toString("utf8")),
            catalogBytes,
        )
        fs.writeFileSync(bannerResolutionPath, `${JSON.stringify(bannerResolution, null, 2)}\n`, "utf8")
    } else {
        bannerResolution = JSON.parse(fs.readFileSync(bannerResolutionPath, "utf8"))
    }
    const policy = buildCnGachaRegionPolicy(
        JSON.parse(sourceBytes.toString("utf8")),
        sourceBytes,
        bannerResolution,
        JSON.parse(fs.readFileSync(rawOddsSignaturesPath, "utf8")),
        JSON.parse(fs.readFileSync(featureLinkEvidencePath, "utf8")),
    )
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, `${JSON.stringify(policy, null, 2)}\n`, "utf8")
    return { output: outputPath, ...policy.evidence }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try {
        process.stdout.write(`${JSON.stringify(generate(process.argv.slice(2)))}\n`)
    } catch (error) {
        process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
        process.exitCode = 1
    }
}
// //// /读取命令参数并写入策略资产 ////
