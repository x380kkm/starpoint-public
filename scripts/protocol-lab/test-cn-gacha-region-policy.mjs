// audience: internal
// # test-cn-gacha-region-policy
// 此脚本核对 CN 地区别名证据, 客户端临时入口和三端共用窗口策略.

import assert from "node:assert/strict"
import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"
import { FEATURE_BANNER_RELATION_SCHEMAS } from "./cn-activity-master-schema.mjs"
import {
    buildActivityCatalogArtifacts,
    readEntityRecords,
    readMasterBuffers,
} from "./generate-cn-activity-catalog.mjs"
import {
    buildCnGachaRegionPolicy,
    gachaBehaviorFingerprint,
} from "./generate-cn-gacha-region-policy.mjs"
import { decodeOrderedMap } from "./decode-cn-orderedmap.mjs"
import { encodeOrderedMap } from "./encode-cn-orderedmap.mjs"
import { parseCnMasterTimestamp } from "./cn-master-time.mjs"
import {
    applyCnFeatureBannerRegionPolicy,
    applyCnGachaCampaignPolicy,
    applyCnGachaRegionPolicy,
} from "./patch-cn-gacha-master.mjs"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")

// //// 读取策略输入并构造客户端 fixture [@x380kkm 2026-08-24] ////
function readJson(relativePath) {
    return JSON.parse(fs.readFileSync(path.join(REPOSITORY_ROOT, relativePath), "utf8"))
}

function clientMasterFixture(document) {
    return Object.fromEntries(Object.entries(document).map(([id, pool]) => {
        const row = Array(47).fill("")
        row[0] = pool.stringId
        row[1] = pool.title
        row[2] = String(pool.listOrder)
        row[3] = pool.bannerImage
        row[4] = String(pool.pageKind)
        row[5] = String(pool.singleCost)
        row[6] = String(pool.multiCost)
        row[7] = String(pool.discountCost)
        row[8] = pool.tenTimesPerAccountCost === null ? "" : String(pool.tenTimesPerAccountCost)
        row[13] = String(pool.type)
        row[27] = pool.onceTicketItemId === null ? "" : String(pool.onceTicketItemId)
        row[28] = pool.tenTimesTicketItemId === null ? "" : String(pool.tenTimesTicketItemId)
        row[29] = pool.startDate
        row[30] = pool.endDate
        row[31] = pool.endDate
        row[43] = String(pool.isComeback)
        row[44] = String(pool.freemiumGuaranteeAvailable)
        row[45] = pool.crazyTenTimesTicketItemId === null ? "" : String(pool.crazyTenTimesTicketItemId)
        row[46] = String(pool.isStarsGacha)
        return [id, row]
    }))
}

function clientCampaignMasterFixture(campaignByGachaId) {
    const gachaIdsByCampaign = new Map()
    for (const [gachaId, campaignId] of Object.entries(campaignByGachaId)) {
        gachaIdsByCampaign.set(String(campaignId), [
            ...(gachaIdsByCampaign.get(String(campaignId)) ?? []),
            gachaId,
        ])
    }
    return Object.fromEntries([...gachaIdsByCampaign].map(([campaignId, gachaIds]) => [
        campaignId,
        [
            `campaign_${campaignId}`,
            `Campaign ${campaignId}`,
            "2",
            "2020-01-01 00:00:00",
            "2020-01-02 00:00:00",
            gachaIds.sort((left, right) => Number(left) - Number(right)).join(","),
            "(None)",
            "",
        ],
    ]))
}

function clientVisible(row, gachaInfo, nowMs) {
    if (row[43] === "true") {
        const period = gachaInfo?.comeback_campaign
        return period !== undefined
            && period.period_start_time * 1000 <= nowMs
            && nowMs <= period.period_end_time * 1000
    }
    const startAtMs = parseCnMasterTimestamp(row[29])
    const endAtMs = parseCnMasterTimestamp(row[30])
    return startAtMs <= nowMs && nowMs <= endAtMs
}
// //// /读取策略输入并构造客户端 fixture ////

// //// 核对地区证据和运行时资产 [@x380kkm 2026-08-24] ////
const gachaBytes = fs.readFileSync(path.join(REPOSITORY_ROOT, "assets", "gacha.json"))
const document = JSON.parse(gachaBytes.toString("utf8"))
const rawOddsEvidence = readJson("assets/gacha-raw-odds-signatures.json")
const bannerResolution = readJson("assets/gacha-banner-resolution.json")
const featureLinkEvidence = readJson("assets/gacha-feature-link-evidence.json")
const campaignByGachaId = readJson("assets/gacha_campaign.json")
const policy = readJson("assets/gacha-region-policy.json")
const rebuilt = buildCnGachaRegionPolicy(
    document,
    gachaBytes,
    bannerResolution,
    rawOddsEvidence,
    featureLinkEvidence,
)
assert.deepEqual(rebuilt, policy)
assert.deepEqual(policy.featureLinkSources, Object.fromEntries(featureLinkEvidence.tables
    .map((table) => [table.name, table.sourceSha256])))
assert.deepEqual(policy.evidence, {
    aliasPairCount: 100,
    behaviorGroupCount: 88,
    characterAliasCount: 70,
    equipmentAliasCount: 30,
    normalizedCoverageAliasCount: 1,
    projectedFeatureLinkCount: 11,
    retainedPoolCount: 483,
    temporaryAliasCount: 473,
    bannerPathOverrideCount: 5,
})
assert.equal(Object.keys(policy.excludedRegionalAliases)
    .filter((id) => document[id].pageKind !== 0).length, 0)
for (const [aliasId, canonicalId] of Object.entries(policy.excludedRegionalAliases)) {
    assert.equal(document[canonicalId].stringId, `${document[aliasId].stringId}_1`)
    assert.equal(document[canonicalId].bannerImage, document[aliasId].bannerImage)
    assert.equal(gachaBehaviorFingerprint(document[canonicalId]), gachaBehaviorFingerprint(document[aliasId]))
    assert.equal(rawOddsEvidence.signatures[canonicalId], rawOddsEvidence.signatures[aliasId])
}
assert.deepEqual(policy.normalizedCoverageAliases, { "61": 1 })
assert.deepEqual(policy.featureLinkProjections, {
    feature_banner: {
        "94": 900002,
        "134": 1623,
        "136": 1625,
        "138": 1627,
        "139": 1628,
        "142": 1631,
        "144": 1633,
        "146": 1634,
        "5000": 5009,
        "5006": 5015,
        "5031": 25027,
    },
    feature_banner_secondary: {},
    feature_banner_misc: {},
})
assert.equal(rawOddsEvidence.signatures["61"], rawOddsEvidence.signatures["1"])
assert.equal(document["61"].bannerImage, document["1"].bannerImage)
assert.equal(document["61"].guaranteeNumber, document["1"].guaranteeNumber)
assert.deepEqual(document["61"].rankRates, document["1"].rankRates)

const retainedOrdinary = Object.keys(document).filter((id) => document[id].pageKind === 0
    && !Object.hasOwn(policy.excludedRegionalAliases, id))
const retainedByFingerprint = new Map()
for (const id of retainedOrdinary) {
    const fingerprint = gachaBehaviorFingerprint(document[id])
    retainedByFingerprint.set(fingerprint, [...(retainedByFingerprint.get(fingerprint) ?? []), id])
}
const sameRegionReplay = [...retainedByFingerprint.values()].find((ids) => ids.length > 1
    && ids.every((id) => !document[id].stringId.endsWith("_1")))
assert.ok(sameRegionReplay, "same-region replay pools must remain independent")
assert.ok(sameRegionReplay.every((id) => !Object.hasOwn(policy.excludedRegionalAliases, id)))
// //// /核对地区证据和运行时资产 ////

// //// 核对客户端 master 补丁和默认时间 [@x380kkm 2026-08-24] ////
const originalMaster = clientMasterFixture(document)
const patchedMaster = applyCnGachaRegionPolicy(originalMaster, policy)
const decodedMaster = decodeOrderedMap(encodeOrderedMap(patchedMaster))
assert.equal(Object.keys(originalMaster).length, 584)
assert.equal(Object.keys(decodedMaster).length, 1057)
assert.ok(Object.keys(originalMaster).every((id) => Object.hasOwn(decodedMaster, id)))

const defaultTimeMs = parseCnMasterTimestamp("2019-12-02 13:01:00")
for (const aliasId of Object.keys(policy.excludedRegionalAliases)) {
    assert.equal(decodedMaster[aliasId][43], "true")
    assert.equal(clientVisible(decodedMaster[aliasId], undefined, defaultTimeMs), false)
}
for (const [aliasId, canonicalId] of Object.entries(policy.normalizedCoverageAliases)) {
    assert.equal(decodedMaster[aliasId][43], "true")
    assert.equal(decodedMaster[aliasId][0], `${decodedMaster[canonicalId][0]}_coverage_${aliasId}`)
    assert.equal(decodedMaster[aliasId][29], originalMaster[aliasId][29])
    assert.equal(decodedMaster[aliasId][30], originalMaster[aliasId][30])
    assert.equal(clientVisible(decodedMaster[aliasId], undefined, defaultTimeMs), false)
}
const temporaryStringIds = new Set()
for (const [temporaryId, canonicalId] of Object.entries(policy.temporaryAliases)) {
    assert.equal(decodedMaster[temporaryId][43], "true")
    assert.equal(clientVisible(decodedMaster[temporaryId], undefined, defaultTimeMs), false)
    assert.equal(decodedMaster[temporaryId][0], `${decodedMaster[canonicalId][0]}_temporary_${canonicalId}`)
    assert.equal(decodedMaster[temporaryId][3], decodedMaster[canonicalId][3])
    assert.equal(decodedMaster[temporaryId][4], decodedMaster[canonicalId][4])
    assert.equal(decodedMaster[temporaryId][46], decodedMaster[canonicalId][46])
    temporaryStringIds.add(decodedMaster[temporaryId][0])
}
assert.equal(temporaryStringIds.size, 473)
for (const [id, pool] of Object.entries(document)) {
    if (!pool.isComeback && !pool.isStarsGacha) continue
    assert.ok(!Object.values(policy.temporaryAliases).includes(Number(id)))
    assert.ok(!Object.hasOwn(decodedMaster, String(1_000_000 + Number(id))))
}

const mixedTicketPoolId = "57"
const mixedTicketAliasId = "1000057"
assert.equal(policy.temporaryAliases[mixedTicketAliasId], Number(mixedTicketPoolId))
assert.equal(decodedMaster[mixedTicketAliasId][3], decodedMaster[mixedTicketPoolId][3])
assert.equal(decodedMaster[mixedTicketAliasId][4], "2")
assert.equal(decodedMaster[mixedTicketAliasId][27], "20065")
assert.equal(decodedMaster[mixedTicketAliasId][28], "20064")
assert.equal(decodedMaster[mixedTicketAliasId][43], "true")
assert.equal(decodedMaster[mixedTicketAliasId][46], "false")

const accountFirstPoolId = "800000"
const accountFirstAliasId = "1800000"
assert.equal(policy.temporaryAliases[accountFirstAliasId], Number(accountFirstPoolId))
assert.equal(decodedMaster[accountFirstAliasId][3], decodedMaster[accountFirstPoolId][3])
assert.equal(decodedMaster[accountFirstAliasId][4], "1")
assert.equal(decodedMaster[accountFirstAliasId][8], "1500")
assert.equal(decodedMaster[accountFirstAliasId][43], "true")
assert.equal(decodedMaster[accountFirstAliasId][46], "false")

const originalFeatureBannerMaster = Object.fromEntries(Object.entries(policy.featureLinkProjections.feature_banner)
    .map(([aliasId], index) => {
        const row = Array(37).fill("")
        row[4] = `feature_banner_projection_${aliasId}`
        row[8] = "4"
        row[13] = aliasId
        row[28] = `dynamic/home_banner/gacha/projection_${index}`
        return [String(index + 1), row]
    }))
const patchedFeatureBannerMaster = decodeOrderedMap(encodeOrderedMap(
    applyCnFeatureBannerRegionPolicy(originalFeatureBannerMaster, "feature_banner", policy),
))
for (const [aliasId, canonicalId] of Object.entries(policy.featureLinkProjections.feature_banner)) {
    const featureRow = Object.values(patchedFeatureBannerMaster)
        .find((row) => row[4] === `feature_banner_projection_${aliasId}`)
    assert.equal(featureRow[13], String(canonicalId))
    assert.equal(decodedMaster[aliasId][43], "true")
    assert.equal(decodedMaster[canonicalId][43], "false")
    assert.equal(decodedMaster[canonicalId][46], "false")
}
for (const [aliasId, canonicalId] of [["5000", "5009"], ["5006", "5015"], ["5031", "25027"]]) {
    assert.equal(policy.featureLinkProjections.feature_banner[aliasId], Number(canonicalId))
    assert.equal(document[aliasId].bannerImage, document[canonicalId].bannerImage)
}

for (const canonicalId of ["6", "8", "63", "66", "71", "1526"]) {
    const temporaryId = Object.entries(policy.temporaryAliases)
        .find(([, candidateId]) => String(candidateId) === canonicalId)?.[0]
    assert.ok(temporaryId)
    assert.equal(decodedMaster[temporaryId][4], decodedMaster[canonicalId][4])
    assert.equal(decodedMaster[temporaryId][46], "false")
}

const originalCampaignMaster = clientCampaignMasterFixture(campaignByGachaId)
const patchedCampaignMaster = decodeOrderedMap(encodeOrderedMap(
    applyCnGachaCampaignPolicy(originalCampaignMaster, policy, campaignByGachaId),
))
const temporaryCampaignAliases = Object.entries(policy.temporaryAliases)
    .filter(([, canonicalId]) => Object.hasOwn(campaignByGachaId, String(canonicalId)))
assert.equal(
    Object.keys(patchedCampaignMaster).length,
    Object.keys(originalCampaignMaster).length + temporaryCampaignAliases.length,
)
for (const [temporaryId, canonicalId] of temporaryCampaignAliases) {
    const canonicalCampaignId = String(campaignByGachaId[canonicalId])
    assert.equal(patchedCampaignMaster[temporaryId][0],
        `${originalCampaignMaster[canonicalCampaignId][0]}_temporary_${canonicalId}`)
    assert.equal(patchedCampaignMaster[temporaryId][3], "1970-01-01 00:00:00")
    assert.equal(patchedCampaignMaster[temporaryId][4], "2200-12-31 23:59:59")
    assert.equal(patchedCampaignMaster[temporaryId][5], temporaryId)
    assert.deepEqual(patchedCampaignMaster[canonicalCampaignId], originalCampaignMaster[canonicalCampaignId])
}

const leasedPoolId = "1"
const leasedAliasId = Object.entries(policy.temporaryAliases)
    .find(([, canonicalId]) => String(canonicalId) === leasedPoolId)[0]
assert.equal(clientVisible(decodedMaster[leasedAliasId], {
    comeback_campaign: {
        period_start_time: defaultTimeMs / 1000 - 1,
        period_end_time: defaultTimeMs / 1000 + 86_399,
    },
}, defaultTimeMs), true)
const excludedIds = new Set([
    ...Object.keys(policy.excludedRegionalAliases),
    ...Object.keys(policy.normalizedCoverageAliases),
])
const openOrdinaryPools = Object.entries(document).filter(([id, pool]) => {
    if (excludedIds.has(id) || pool.pageKind !== 0) return false
    return pool.startAtMs <= defaultTimeMs && defaultTimeMs < pool.endAtMs
})
assert.equal(openOrdinaryPools.filter(([, pool]) => pool.type === 0).length, 1)
assert.equal(openOrdinaryPools.filter(([, pool]) => pool.type === 1).length, 1)

function coverageAliasesAt(timestamp) {
    const nowMs = parseCnMasterTimestamp(timestamp)
    const hiddenAliases = {
        ...policy.excludedRegionalAliases,
        ...policy.normalizedCoverageAliases,
    }
    const selected = []
    for (const type of [0, 1]) {
        const retainedOpen = Object.entries(document).some(([id, pool]) => !Object.hasOwn(hiddenAliases, id)
            && pool.pageKind === 0 && pool.type === type
            && pool.startAtMs <= nowMs && nowMs < pool.endAtMs)
        if (retainedOpen) continue
        const candidates = Object.keys(hiddenAliases)
            .filter((id) => document[id].pageKind === 0 && document[id].type === type
                && document[id].startAtMs <= nowMs && nowMs < document[id].endAtMs)
            .sort((left, right) => document[left].startAtMs - document[right].startAtMs
                || document[left].listOrder - document[right].listOrder
                || Number(left) - Number(right))
        if (candidates.length > 0) selected.push(candidates.at(-1))
    }
    return selected
}

assert.deepEqual(coverageAliasesAt("2019-12-02 13:01:00"), [])
assert.deepEqual(coverageAliasesAt("2020-02-10 12:00:00"), ["5001"])
assert.deepEqual(coverageAliasesAt("2020-06-25 12:00:00"), ["5006"])
assert.deepEqual(coverageAliasesAt("2021-10-01 12:00:00"), ["94"])
assert.deepEqual(coverageAliasesAt("2021-10-30 12:00:00"), ["61"])
// //// /核对客户端 master 补丁和默认时间 ////

// //// 核对活动目录不暴露两类内部别名 [@x380kkm 2026-08-24] ////
const { catalog, coverage: catalogCoverage } = buildActivityCatalogArtifacts({
    assetVersion: "fixture",
    clientVersion: "1.8.4",
    entityRecords: new Map(),
    masterMaps: new Map([["gacha", decodedMaster]]),
    regionPolicy: policy,
})
const catalogGachaIds = new Set(catalog.activities
    .filter((activity) => activity.kind === "gacha")
    .map((activity) => activity.activity_id.slice("gacha:".length)))
assert.equal(catalogGachaIds.size, 483)
assert.equal(Object.hasOwn(catalog, "coverage"), false)
assert.ok(Object.keys(policy.excludedRegionalAliases).every((id) => !catalogGachaIds.has(id)))
assert.ok(Object.keys(policy.normalizedCoverageAliases).every((id) => !catalogGachaIds.has(id)))
assert.ok(Object.keys(policy.temporaryAliases).every((id) => !catalogGachaIds.has(id)))
assert.deepEqual(catalogCoverage.gacha_region_policy, {
    excluded_regional_alias_count: 100,
    excluded_normalized_coverage_alias_count: 1,
    excluded_temporary_alias_count: 473,
    retained_gacha_activity_count: 483,
})
assert.ok(!catalogGachaIds.has("61"))

// //// 核对当前 CN FeatureBanner master 的实际投影 [@x380kkm 2026-08-25] ////
const cdnRootOptionIndex = process.argv.indexOf("--cdn-root")
if (cdnRootOptionIndex >= 0) {
    const cdnRoot = process.argv[cdnRootOptionIndex + 1]
    if (!cdnRoot || cdnRoot.startsWith("--")) throw new Error("missing value for --cdn-root")
    const featureSchema = FEATURE_BANNER_RELATION_SCHEMAS.find((schema) => schema.name === "feature_banner")
    const featureBuffers = await readMasterBuffers(
        path.resolve(cdnRoot),
        readEntityRecords(path.resolve(cdnRoot)),
        [featureSchema],
    )
    const sourceFeatureMaster = decodeOrderedMap(featureBuffers.get(featureSchema.name))
    const projectedFeatureMaster = decodeOrderedMap(encodeOrderedMap(
        applyCnFeatureBannerRegionPolicy(sourceFeatureMaster, featureSchema.name, policy),
    ))
    const target = featureSchema.targetsByDiscriminator.get("4")
    const remainingAliases = Object.values(projectedFeatureMaster).filter((row) => row[featureSchema.discriminatorIndex] === "4"
        && Object.hasOwn(policy.featureLinkProjections.feature_banner, row[target.idIndex]))
    assert.equal(remainingAliases.length, 0)
}
// //// /核对当前 CN FeatureBanner master 的实际投影 ////

process.stdout.write(`${JSON.stringify({
    aliasPairs: policy.evidence.aliasPairCount,
    retainedPools: catalogGachaIds.size,
    temporaryAliases: Object.keys(policy.temporaryAliases).length,
    bannerOverrides: Object.keys(policy.bannerPathOverrides).length,
})}\n`)
// //// /核对活动目录不暴露两类内部别名 ////
