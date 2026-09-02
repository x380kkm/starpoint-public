// audience: internal
// # patch-cn-gacha-master
// 此脚本应用 CN 卡池地区策略, FeatureBanner 导航, 客户端临时入口和免费活动映射, 并输出 orderedmap 文件.

import crypto from "node:crypto"
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { FEATURE_BANNER_RELATION_SCHEMAS } from "./cn-activity-master-schema.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { encodeOrderedMap } from "./encode-cn-orderedmap.mjs"
import { readCnGachaRegionPolicy } from "./generate-cn-gacha-region-policy.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const DEFAULT_CAMPAIGN_MAP_PATH = path.resolve(SCRIPT_DIRECTORY, "..", "..", "assets", "gacha_campaign.json")
const STRING_ID_INDEX = 0
const LIST_ORDER_INDEX = 2
const BANNER_IMAGE_INDEX = 3
const START_TIME_INDEX = 29
const END_TIME_INDEX = 30
const EXTENDED_END_TIME_INDEX = 31
const COMEBACK_INDEX = 43
const CAMPAIGN_START_TIME_INDEX = 3
const CAMPAIGN_END_TIME_INDEX = 4
const CAMPAIGN_GACHA_IDS_INDEX = 5
const TEMPORARY_CAMPAIGN_START_TIME = "1970-01-01 00:00:00"
const TEMPORARY_CAMPAIGN_END_TIME = "2200-12-31 23:59:59"

// //// 应用地区筛选和客户端临时卡池入口 [@x380kkm 2026-08-24] ////
function requiredGachaRow(master, gachaId) {
    const row = master[gachaId]
    if (!Array.isArray(row) || row.length <= COMEBACK_INDEX) {
        throw new Error(`gacha ${gachaId} does not contain the required client columns`)
    }
    return row
}

export function applyCnGachaRegionPolicy(master, policy) {
    const patched = { ...master }
    const temporaryStringIds = new Set()
    for (const [aliasId, canonicalId] of Object.entries(policy.excludedRegionalAliases)) {
        requiredGachaRow(master, String(canonicalId))
        const row = [...requiredGachaRow(master, aliasId)]
        row[COMEBACK_INDEX] = "true"
        patched[aliasId] = row
    }
    for (const [aliasId, canonicalId] of Object.entries(policy.normalizedCoverageAliases)) {
        const alias = requiredGachaRow(master, aliasId)
        const row = [...requiredGachaRow(master, String(canonicalId))]
        row[STRING_ID_INDEX] = `${row[STRING_ID_INDEX]}_coverage_${aliasId}`
        row[LIST_ORDER_INDEX] = alias[LIST_ORDER_INDEX]
        row[START_TIME_INDEX] = alias[START_TIME_INDEX]
        row[END_TIME_INDEX] = alias[END_TIME_INDEX]
        row[EXTENDED_END_TIME_INDEX] = alias[EXTENDED_END_TIME_INDEX]
        row[COMEBACK_INDEX] = "true"
        patched[aliasId] = row
    }
    for (const [gachaId, bannerImage] of Object.entries(policy.bannerPathOverrides)) {
        const row = [...requiredGachaRow(patched, gachaId)]
        row[BANNER_IMAGE_INDEX] = bannerImage
        patched[gachaId] = row
    }
    for (const [temporaryId, canonicalId] of Object.entries(policy.temporaryAliases)) {
        if (Object.hasOwn(master, temporaryId)) throw new Error(`temporary gacha ID collides: ${temporaryId}`)
        const row = [...requiredGachaRow(patched, String(canonicalId))]
        row[STRING_ID_INDEX] = `${row[STRING_ID_INDEX]}_temporary_${canonicalId}`
        if (temporaryStringIds.has(row[STRING_ID_INDEX])) {
            throw new Error(`temporary gacha stringId collides: ${row[STRING_ID_INDEX]}`)
        }
        temporaryStringIds.add(row[STRING_ID_INDEX])
        row[COMEBACK_INDEX] = "true"
        patched[temporaryId] = row
    }
    const allStringIds = new Set()
    for (const gachaId of Object.keys(patched)) {
        const stringId = requiredGachaRow(patched, gachaId)[STRING_ID_INDEX]
        if (allStringIds.has(stringId)) throw new Error(`gacha stringId collides: ${stringId}`)
        allStringIds.add(stringId)
    }
    return patched
}
// //// /应用地区筛选和客户端临时卡池入口 ////

// //// 将 FeatureBanner 导航目标投影到规范卡池 [@x380kkm 2026-08-24] ////
function mapMasterRows(value, transform) {
    if (Array.isArray(value)) return transform([...value])
    if (value === null || typeof value !== "object") return value
    return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, mapMasterRows(child, transform)]))
}

export function applyCnFeatureBannerRegionPolicy(master, tableName, policy) {
    const schema = FEATURE_BANNER_RELATION_SCHEMAS.find((candidate) => candidate.name === tableName)
    const target = schema?.targetsByDiscriminator.get("4")
    const projections = policy.featureLinkProjections?.[tableName]
    if (!schema || !target || projections === null || typeof projections !== "object") {
        throw new Error(`feature banner projection table is invalid: ${tableName}`)
    }
    const appliedCounts = Object.fromEntries(Object.keys(projections).map((aliasId) => [aliasId, 0]))
    const patched = mapMasterRows(master, (row) => {
        if (String(row[schema.discriminatorIndex]) !== "4") return row
        const aliasId = String(row[target.idIndex])
        if (!Object.hasOwn(projections, aliasId)) return row
        row[target.idIndex] = String(projections[aliasId])
        appliedCounts[aliasId] += 1
        return row
    })
    for (const [aliasId, count] of Object.entries(appliedCounts)) {
        if (count !== 1) throw new Error(`feature banner gacha projection count is invalid: ${tableName}/${aliasId}`)
    }
    return patched
}
// //// /将 FeatureBanner 导航目标投影到规范卡池 ////

// //// 为临时卡池生成独立的免费活动入口 [@x380kkm 2026-08-24] ////
function requiredGachaCampaignRow(master, campaignId) {
    const row = master[campaignId]
    if (!Array.isArray(row) || row.length <= CAMPAIGN_GACHA_IDS_INDEX) {
        throw new Error(`gacha campaign ${campaignId} does not contain the required client columns`)
    }
    return row
}

function mappedCampaigns(master, campaignByGachaId) {
    const campaigns = new Map()
    for (const [gachaId, campaignIdValue] of Object.entries(campaignByGachaId)) {
        const campaignId = String(campaignIdValue)
        const row = requiredGachaCampaignRow(master, campaignId)
        if (!row[CAMPAIGN_GACHA_IDS_INDEX].split(",").includes(gachaId)) {
            throw new Error(`gacha campaign mapping is inconsistent: gacha=${gachaId} campaign=${campaignId}`)
        }
        campaigns.set(gachaId, { campaignId, row })
    }
    return campaigns
}

export function applyCnGachaCampaignPolicy(master, policy, campaignByGachaId) {
    const patched = { ...master }
    const canonicalCampaigns = mappedCampaigns(master, campaignByGachaId)
    for (const [temporaryId, canonicalId] of Object.entries(policy.temporaryAliases)) {
        const canonical = canonicalCampaigns.get(String(canonicalId))
        if (!canonical) continue
        if (Object.hasOwn(patched, temporaryId)) {
            throw new Error(`temporary gacha campaign ID collides: ${temporaryId}`)
        }
        const row = [...canonical.row]
        row[STRING_ID_INDEX] = `${row[STRING_ID_INDEX]}_temporary_${canonicalId}`
        row[CAMPAIGN_START_TIME_INDEX] = TEMPORARY_CAMPAIGN_START_TIME
        row[CAMPAIGN_END_TIME_INDEX] = TEMPORARY_CAMPAIGN_END_TIME
        row[CAMPAIGN_GACHA_IDS_INDEX] = temporaryId
        patched[temporaryId] = row
    }
    const stringIds = new Set()
    for (const campaignId of Object.keys(patched)) {
        const stringId = requiredGachaCampaignRow(patched, campaignId)[STRING_ID_INDEX]
        if (stringIds.has(stringId)) throw new Error(`gacha campaign stringId collides: ${stringId}`)
        stringIds.add(stringId)
    }
    return patched
}
// //// /为临时卡池生成独立的免费活动入口 ////

// //// 读取参数并输出补丁后的 orderedmap [@x380kkm 2026-08-22] ////
function readRequiredOption(args, name) {
    const optionIndex = args.indexOf(name)
    const value = args[optionIndex + 1]
    if (optionIndex < 0 || !value || value.startsWith("--")) {
        throw new Error(`missing required option: ${name}`)
    }
    return value
}

function patchGachaMasterFile(inputPath, outputPath, policyPath) {
    const master = decodeOrderedMap(fs.readFileSync(inputPath))
    const policy = readCnGachaRegionPolicy(policyPath)
    const patched = applyCnGachaRegionPolicy(master, policy)
    const encoded = encodeOrderedMap(patched)
    const decoded = decodeOrderedMap(encoded)
    if (!Object.hasOwn(decoded, "1") || !Object.hasOwn(decoded, "3")) {
        throw new Error("patched gacha master does not retain pools 1 and 3")
    }
    for (const gachaId of Object.keys(master)) {
        if (!Object.hasOwn(decoded, gachaId)) throw new Error(`patched gacha master lost pool ${gachaId}`)
    }
    for (const aliasId of Object.keys(policy.excludedRegionalAliases)) {
        if (decoded[aliasId]?.[COMEBACK_INDEX] !== "true") {
            throw new Error(`regional gacha alias remains visible: ${aliasId}`)
        }
    }
    for (const [aliasId, canonicalId] of Object.entries(policy.normalizedCoverageAliases)) {
        if (decoded[aliasId]?.[COMEBACK_INDEX] !== "true"
            || decoded[aliasId]?.[STRING_ID_INDEX]
                !== `${decoded[String(canonicalId)][STRING_ID_INDEX]}_coverage_${aliasId}`
            || decoded[aliasId]?.[START_TIME_INDEX] !== master[aliasId][START_TIME_INDEX]
            || decoded[aliasId]?.[END_TIME_INDEX] !== master[aliasId][END_TIME_INDEX]) {
            throw new Error(`normalized coverage gacha alias is invalid: ${aliasId}`)
        }
    }
    for (const [temporaryId, canonicalId] of Object.entries(policy.temporaryAliases)) {
        if (decoded[temporaryId]?.[COMEBACK_INDEX] !== "true"
            || decoded[temporaryId]?.[STRING_ID_INDEX]
                !== `${decoded[String(canonicalId)][STRING_ID_INDEX]}_temporary_${canonicalId}`) {
            throw new Error(`temporary gacha alias cannot be decoded: ${temporaryId}`)
        }
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, encoded)
    return {
        retainedOriginalCount: Object.keys(master).length,
        excludedRegionalAliasCount: Object.keys(policy.excludedRegionalAliases).length,
        normalizedCoverageAliasCount: Object.keys(policy.normalizedCoverageAliases).length,
        temporaryAliasCount: Object.keys(policy.temporaryAliases).length,
        bytes: encoded.length,
    }
}

function patchFeatureBannerMasterFile(inputPath, outputPath, policyPath, tableName) {
    const source = fs.readFileSync(inputPath)
    const policy = readCnGachaRegionPolicy(policyPath)
    const sourceSha256 = crypto.createHash("sha256").update(source).digest("hex")
    if (sourceSha256 !== policy.featureLinkSources[tableName]) {
        throw new Error(`feature banner source does not match projection evidence: ${tableName}`)
    }
    const master = decodeOrderedMap(source)
    const patched = applyCnFeatureBannerRegionPolicy(master, tableName, policy)
    const encoded = encodeOrderedMap(patched)
    const decoded = decodeOrderedMap(encoded)
    if (JSON.stringify(decoded) !== JSON.stringify(patched)) {
        throw new Error(`patched feature banner master cannot be decoded: ${tableName}`)
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, encoded)
    return {
        projectedLinkCount: Object.keys(policy.featureLinkProjections[tableName]).length,
        bytes: encoded.length,
    }
}

function patchGachaCampaignMasterFile(inputPath, outputPath, policyPath, campaignMapPath) {
    const master = decodeOrderedMap(fs.readFileSync(inputPath))
    const policy = readCnGachaRegionPolicy(policyPath)
    const campaignByGachaId = JSON.parse(fs.readFileSync(campaignMapPath, "utf8"))
    const patched = applyCnGachaCampaignPolicy(master, policy, campaignByGachaId)
    const encoded = encodeOrderedMap(patched)
    const decoded = decodeOrderedMap(encoded)
    for (const campaignId of Object.keys(master)) {
        if (!Object.hasOwn(decoded, campaignId)) {
            throw new Error(`patched gacha campaign master lost campaign ${campaignId}`)
        }
    }
    const canonicalCampaigns = mappedCampaigns(master, campaignByGachaId)
    let temporaryCampaignAliasCount = 0
    for (const [temporaryId, canonicalId] of Object.entries(policy.temporaryAliases)) {
        const canonical = canonicalCampaigns.get(String(canonicalId))
        if (!canonical) continue
        const row = requiredGachaCampaignRow(decoded, temporaryId)
        if (row[STRING_ID_INDEX] !== `${canonical.row[STRING_ID_INDEX]}_temporary_${canonicalId}`
            || row[CAMPAIGN_START_TIME_INDEX] !== TEMPORARY_CAMPAIGN_START_TIME
            || row[CAMPAIGN_END_TIME_INDEX] !== TEMPORARY_CAMPAIGN_END_TIME
            || row[CAMPAIGN_GACHA_IDS_INDEX] !== temporaryId) {
            throw new Error(`temporary gacha campaign cannot be decoded: ${temporaryId}`)
        }
        temporaryCampaignAliasCount += 1
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, encoded)
    return {
        retainedOriginalCampaignCount: Object.keys(master).length,
        temporaryCampaignAliasCount,
        bytes: encoded.length,
    }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try {
        const inputPath = readRequiredOption(process.argv.slice(2), "--input")
        const outputPath = readRequiredOption(process.argv.slice(2), "--output")
        const args = process.argv.slice(2)
        const policyIndex = args.indexOf("--policy")
        const policyPath = policyIndex < 0 ? undefined : readRequiredOption(args, "--policy")
        const campaignMapIndex = args.indexOf("--campaign-map")
        const campaignMapPath = campaignMapIndex < 0
            ? DEFAULT_CAMPAIGN_MAP_PATH
            : readRequiredOption(args, "--campaign-map")
        const campaignInputIndex = args.indexOf("--campaign-input")
        const campaignOutputIndex = args.indexOf("--campaign-output")
        if ((campaignInputIndex < 0) !== (campaignOutputIndex < 0)) {
            throw new Error("campaign input and output must be provided together")
        }
        const result = { gacha: patchGachaMasterFile(inputPath, outputPath, policyPath) }
        if (campaignInputIndex >= 0) {
            result.gachaCampaign = patchGachaCampaignMasterFile(
                readRequiredOption(args, "--campaign-input"),
                readRequiredOption(args, "--campaign-output"),
                policyPath,
                campaignMapPath,
            )
        }
        const featureBannerInputIndex = args.indexOf("--feature-banner-input")
        const featureBannerOutputIndex = args.indexOf("--feature-banner-output")
        if ((featureBannerInputIndex < 0) !== (featureBannerOutputIndex < 0)) {
            throw new Error("feature banner input and output must be provided together")
        }
        if (featureBannerInputIndex >= 0) {
            result.featureBanner = patchFeatureBannerMasterFile(
                readRequiredOption(args, "--feature-banner-input"),
                readRequiredOption(args, "--feature-banner-output"),
                policyPath,
                "feature_banner",
            )
        }
        process.stdout.write(`${JSON.stringify(result)}\n`)
    } catch (error) {
        console.error(error.message)
        console.error("usage: node patch-cn-gacha-master.mjs --input <gacha.orderedmap> --output <patched.orderedmap> [--campaign-input <gacha_campaign.orderedmap> --campaign-output <patched.orderedmap>] [--feature-banner-input <feature_banner.orderedmap> --feature-banner-output <patched.orderedmap>] [--campaign-map <gacha_campaign.json>] [--policy <policy.json>]")
        process.exitCode = 1
    }
}
// //// /读取参数并输出补丁后的 orderedmap ////
