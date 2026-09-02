// audience: internal
// # test-cn-activity-catalog
// 此测试验证 CN 活动 master 反射枚举、JST 时间和多来源图片元数据.

import assert from "node:assert/strict"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import {
    assetEntryPaths,
    buildActivityCatalogArtifacts,
    buildActivityCatalogSource,
    computeCnEntityDigest,
    hashCnAssetPath,
    matchesEntityRecord,
    parseCnMasterTimestamp,
    readMasterBuffers,
} from "./generate-cn-activity-catalog.mjs"
import { ACTIVITY_SCHEMA_BY_NAME, FEATURE_BANNER_RELATION_SCHEMAS } from "./cn-activity-master-schema.mjs"

// //// 验证活动目录源数据生成 [@x380kkm 2026-08-19] ////
const raidBannerPath = "quest/event/banner/raid_event/raid_event_banner_01_001.png"
const raidSecondaryPath = "quest/event/banner/raid_event/raid_event_banner_01_003.png"
const raidHomeBannerPath = "banner/home/raid_event_01.png"
const raidEntryPath = "quest/event/entry/raid_event_01.png"
const raidFeatureHomePath = "banner/home/raid_feature_01.png"
const raidFeatureNoticePath = "banner/notice/raid_feature_01.png"
const rankingBannerPath = "quest/event/banner/time_attack_event/time_attack_event_water_001.png"
const unrelatedHomeBannerPath = "banner/home/unrelated_ranking.png"
const ambiguousRankingHomePathA = "banner/home/ambiguous_ranking_a.png"
const ambiguousRankingHomePathB = "banner/home/ambiguous_ranking_b.png"
assert.equal(hashCnAssetPath(raidBannerPath), "fb3424f666f94ef9cf5822a109d3da8560ae6624")
assert.equal(parseCnMasterTimestamp("2023-09-07 12:00:00"), Date.parse("2023-09-07T03:00:00.000Z"))
assert.equal(parseCnMasterTimestamp("(None)"), null)
const entityBuffer = Buffer.from("current entity")
const entityDigest = computeCnEntityDigest(entityBuffer)
assert.equal(matchesEntityRecord(entityBuffer, { byteLength: entityBuffer.length, digest: entityDigest }), true)
assert.equal(matchesEntityRecord(Buffer.from("stale entity"), { byteLength: entityBuffer.length, digest: entityDigest }), false)
assert.equal(
    computeCnEntityDigest(Buffer.from([0x00, 0x02])),
    "-PCmxwDdE_J0tvuo3uqN2bJuTu3eNJVxfKyECMnFF38",
)
const expectedFeatureSchemaFields = new Map([
    ["feature_banner", [8, 12, 13, 14, 15, 21, 28, 35]],
    ["feature_banner_secondary", [6, 10, 11, 12, 13, 19, 26, 33]],
    ["feature_banner_misc", [7, 11, 12, 13, 14, 20, 27, 34]],
])
assert.deepEqual(ACTIVITY_SCHEMA_BY_NAME.get("gacha").nameIndexes, [1])
for (const schema of FEATURE_BANNER_RELATION_SCHEMAS) {
    const [discriminator, collect, gacha, eventKind, event, activeMission, home, notice]
        = expectedFeatureSchemaFields.get(schema.name)
    assert.equal(schema.discriminatorIndex, discriminator)
    assert.equal(schema.targetsByDiscriminator.get("3").idIndex, collect)
    assert.equal(schema.targetsByDiscriminator.get("4").idIndex, gacha)
    assert.equal(schema.targetsByDiscriminator.get("10").eventKindIndex, eventKind)
    assert.equal(schema.targetsByDiscriminator.get("10").idIndex, event)
    assert.equal(schema.targetsByDiscriminator.get("28").idIndex, activeMission)
    assert.equal(schema.pathFields.get(home), "home_banner")
    assert.equal(schema.pathFields.get(notice), "notice_banner")
}

const raidRow = Array(25).fill("")
Object.assign(raidRow, {
    0: "raid_event_01",
    1: "测试 Raid",
    3: "quest/event/banner/raid_event/raid_event_banner_01_001,quest/event/banner/raid_event/raid_event_banner_01_003",
    22: "2023-09-07 12:00:00",
    23: "2023-09-28 11:59:59",
})
const rankingRow = Array(21).fill("")
Object.assign(rankingRow, {
    0: "time_attack_event_water_001",
    2: "测试 Ranking",
    4: "quest/event/banner/time_attack_event/time_attack_event_water_001,(None)",
    18: "2020-08-21 12:00:00",
    19: "2020-08-31 11:59:59",
})
const bannerRecord = { assetKind: "common", byteLength: 123, digest: "A".repeat(43), version: "1.4.54" }
const entityRecords = new Map([
    [assetEntryPaths(hashCnAssetPath(raidBannerPath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(raidSecondaryPath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(raidHomeBannerPath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(raidEntryPath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(raidFeatureHomePath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(raidFeatureNoticePath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(rankingBannerPath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(unrelatedHomeBannerPath))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(ambiguousRankingHomePathA))[0], bannerRecord],
    [assetEntryPaths(hashCnAssetPath(ambiguousRankingHomePathB))[0], bannerRecord],
])
const raidBannerImageRow = Array(7).fill("")
Object.assign(raidBannerImageRow, {
    0: "raid_banner_image_01",
    1: "event",
    4: raidHomeBannerPath.replace(/\.png$/, ""),
    5: raidBannerPath.replace(/\.png$/, ""),
    6: raidEntryPath.replace(/\.png$/, ""),
})
const unrelatedBannerImageRow = Array(7).fill("")
Object.assign(unrelatedBannerImageRow, {
    0: "unrelated_ranking_banner",
    1: "event",
    4: unrelatedHomeBannerPath.replace(/\.png$/, ""),
    5: `other/${path.basename(rankingBannerPath, ".png")}`,
})
const ambiguousRankingRows = [ambiguousRankingHomePathA, ambiguousRankingHomePathB].map((homePath, index) => {
    const row = Array(7).fill("")
    Object.assign(row, {
        0: `ambiguous_ranking_${index + 1}`,
        4: homePath.replace(/\.png$/, ""),
        5: rankingBannerPath.replace(/\.png$/, ""),
    })
    return row
})
const rankingWithoutImage = Array(21).fill("")
Object.assign(rankingWithoutImage, { 0: "ranking_without_image", 2: "没有图片的 Ranking" })
const raidFeatureBannerRow = Array(37).fill("")
Object.assign(raidFeatureBannerRow, {
    8: "10",
    14: "10",
    15: "1",
    28: raidFeatureHomePath.replace(/\.png$/, ""),
    35: raidFeatureNoticePath.replace(/\.png$/, ""),
})
const { catalog: source, coverage: sourceCoverage } = buildActivityCatalogArtifacts({
    assetVersion: "1.4.54",
    clientVersion: "1.8.1",
    entityRecords,
    masterMaps: new Map([["event_list", {
        1: ["10", "1", "1"],
        2: ["1", "1", "2"],
        3: ["1", "1000", "3"],
        4: ["1", "2000", "4"],
        5: ["10", "1", "5"],
    }], ["banner_image", {
        raid: raidBannerImageRow,
        unrelated: unrelatedBannerImageRow,
        ambiguous: Object.fromEntries(ambiguousRankingRows.map((row, index) => [index + 1, row])),
    }], ["feature_banner", { 1: raidFeatureBannerRow }]]),
    raidMap: { 1: raidRow },
    rankingMap: { 1: rankingRow, 1000: rankingRow, 2000: rankingWithoutImage },
})

assert.equal(source.activities.length, 4)
assert.deepEqual(
    source.activities.map((activity) => activity.activity_id),
    ["raid:1", "ranking:1", "ranking:1000", "ranking:2000"],
)
assert.equal(source.activities[0].banner_candidate, hashCnAssetPath(raidBannerPath))
assert.equal(source.activities[0].banner_source_entry, assetEntryPaths(hashCnAssetPath(raidBannerPath))[0])
assert.equal(source.activities[0].banner_source_version, "1.4.54")
assert.equal(source.activities[0].banner_source_byte_length, 123)
assert.equal(source.activities[0].banner_source_digest, "A".repeat(43))
assert.equal(source.activities[0].image_candidates.length, 6)
assert.deepEqual(
    source.activities[0].image_candidates.slice(0, 2).map((candidate) => candidate.logical_path),
    [raidBannerPath, raidSecondaryPath],
)
assert.equal(source.activities[0].image_candidates[0].source_type, "activity_banner")
assert.equal(source.activities[0].image_candidates[0].association_confidence, "direct-field")
assert.equal(source.activities[0].image_candidates[0].evidence, "master:raid_event:field:3")
assert.equal(source.activities[0].image_candidates[0].width, null)
assert.equal(source.activities[0].image_candidates[0].height, null)
const homeBannerCandidate = source.activities[0].image_candidates
    .find((candidate) => candidate.logical_path === raidHomeBannerPath)
assert.equal(homeBannerCandidate.source_type, "home_banner")
assert.equal(homeBannerCandidate.association_confidence, "exact-logical-path")
assert.equal(homeBannerCandidate.evidence, "master:banner_image:string_id:raid_banner_image_01:field:4")
const entryCandidate = source.activities[0].image_candidates
    .find((candidate) => candidate.logical_path === raidEntryPath)
assert.equal(entryCandidate.source_type, "activity_entry")
assert.equal(entryCandidate.evidence, "master:banner_image:string_id:raid_banner_image_01:field:6")
assert.equal(source.activities[0].default_start_at_ms, Date.parse("2023-09-07T03:00:00.000Z"))
assert.equal(source.activities[0].default_end_at_ms, Date.parse("2023-09-28T02:59:59.000Z"))
assert.equal(source.activities[1].banner_logical_path, rankingBannerPath)
assert.equal(source.activities[2].banner_logical_path, rankingBannerPath)
assert.deepEqual(source.activities[3].image_candidates, [])
assert.equal(Object.hasOwn(source.activities[3], "banner_candidate"), false)
assert.equal(Object.hasOwn(source, "coverage"), false)
assert.equal(sourceCoverage.pending_relation_tables.length, 1)
assert.equal(sourceCoverage.duplicate_event_reference_count, 1)
assert.equal(
    sourceCoverage.pending_relation_tables.some((relation) => relation.source_table.includes("banner_image")),
    false,
)
const bannerImageCoverage = sourceCoverage.relation_tables
    .find((relation) => relation.name === "banner_image")
assert.equal(bannerImageCoverage.status, "included")
assert.equal(bannerImageCoverage.matched_activity_count, 1)
assert.equal(bannerImageCoverage.candidate_count, 2)
assert.equal(bannerImageCoverage.ambiguous_path_count, 1)
assert.equal(bannerImageCoverage.unresolved_count, 3)
const featureBannerCoverage = sourceCoverage.relation_tables
    .find((relation) => relation.name === "feature_banner")
assert.equal(featureBannerCoverage.status, "included")
assert.equal(featureBannerCoverage.relation_count, 1)
assert.equal(featureBannerCoverage.matched_activity_count, 1)
assert.equal(featureBannerCoverage.candidate_count, 2)
assert.equal(featureBannerCoverage.ambiguous_target_count, 0)
assert.equal(featureBannerCoverage.unresolved_count, 0)
const featureHomeCandidate = source.activities[0].image_candidates
    .find((candidate) => candidate.logical_path === raidFeatureHomePath)
assert.equal(featureHomeCandidate.source_type, "home_banner")
assert.equal(featureHomeCandidate.association_confidence, "exact-activity-id")
assert.equal(featureHomeCandidate.evidence, "master:feature_banner:key:1:target:raid:1:field:28")
const featureNoticeCandidate = source.activities[0].image_candidates
    .find((candidate) => candidate.logical_path === raidFeatureNoticePath)
assert.equal(featureNoticeCandidate.source_type, "notice_banner")
assert.equal(featureNoticeCandidate.evidence, "master:feature_banner:key:1:target:raid:1:field:35")
assert.equal(
    source.activities.flatMap((activity) => activity.image_candidates)
        .some((candidate) => candidate.logical_path === unrelatedHomeBannerPath),
    false,
)
assert.equal(
    source.activities.flatMap((activity) => activity.image_candidates)
        .some((candidate) => [ambiguousRankingHomePathA, ambiguousRankingHomePathB].includes(candidate.logical_path)),
    false,
)
assert.equal(sourceCoverage.master_tables.find((table) => table.name === "event_list").status, "included")
assert.ok(sourceCoverage.missing_master_tables.includes("master/gacha/gacha.orderedmap"))
const sourceWithoutEntityImage = buildActivityCatalogSource({
    assetVersion: "1.4.54",
    clientVersion: "1.8.1",
    entityRecords: new Map(),
    raidMap: { 1: raidRow },
    rankingMap: {},
})
assert.equal(sourceWithoutEntityImage.activities.length, 1)
assert.deepEqual(sourceWithoutEntityImage.activities[0].image_candidates, [])

const dailyWeekRow = Array(18).fill("")
dailyWeekRow[1] = "每周活动"
const dailyExpManaRow = Array(11).fill("")
dailyExpManaRow[1] = "经验和 Mana"
const sharedCategorySource = buildActivityCatalogSource({
    assetVersion: "1.4.54",
    clientVersion: "1.8.1",
    entityRecords: new Map(),
    masterMaps: new Map([
        ["event_list", {
            1: ["3", "1", "1"],
            2: ["5", "1", "2"],
        }],
        ["daily_week_event", { 1: dailyWeekRow }],
        ["daily_exp_mana_event", { 1: dailyExpManaRow }],
    ]),
})
assert.deepEqual(
    sharedCategorySource.activities.map((activity) => activity.activity_id),
    ["daily-exp-mana:1", "daily-week:1"],
)

const unlistedRaidRow = [...raidRow]
unlistedRaidRow[1] = "未进入 EventList 的 Raid"
const { catalog: completeEventSource, coverage: completeEventCoverage } = buildActivityCatalogArtifacts({
    assetVersion: "1.4.54",
    clientVersion: "1.8.4",
    entityRecords: new Map(),
    masterMaps: new Map([
        ["event_list", { 1: ["10", "1", "1"] }],
        ["raid_event", { 1: raidRow, 2: unlistedRaidRow }],
    ]),
})
assert.deepEqual(
    completeEventSource.activities.map((activity) => activity.activity_id),
    ["raid:1", "raid:2"],
)
assert.equal(completeEventCoverage.additional_event_master_count, 1)

const featureRelationPaths = {
    collectHome: "banner/home/collect_feature.png",
    collectSecondHome: "banner/home/collect_feature_second.png",
    gachaHome: "banner/home/gacha_feature.png",
    raidSecondaryHome: "banner/home/raid_secondary_feature.png",
    raidSecondaryNotice: "banner/notice/raid_secondary_feature.png",
    activeSecondaryHome: "banner/home/active_secondary_feature.png",
    activeMiscHome: "banner/home/active_misc_feature.png",
    activeMiscNotice: "banner/notice/active_misc_feature.png",
    invalidTargetHome: "banner/home/invalid_target_feature.png",
    missingAssetHome: "banner/home/missing_asset_feature.png",
    missingTargetHome: "banner/home/missing_target_feature.png",
    unsupportedHome: "banner/home/unsupported_feature.png",
}
const featureEntityRecords = new Map(Object.values(featureRelationPaths)
    .filter((logicalPath) => logicalPath !== featureRelationPaths.missingAssetHome)
    .map((logicalPath) => [assetEntryPaths(hashCnAssetPath(logicalPath))[0], bannerRecord]))
const collectItemRow = Array(22).fill("")
collectItemRow[0] = "collect_item_1"
const gachaRow = Array(31).fill("")
Object.assign(gachaRow, {
    1: "测试 Gacha",
    3: "dynamic/gacha_list_banner/test",
    4: "0",
})
const activeMissionRow = Array(16).fill("")
activeMissionRow[0] = "active_mission_1"
const primaryCollectRow = Array(37).fill("")
Object.assign(primaryCollectRow, {
    8: "3",
    12: "1",
    28: featureRelationPaths.collectHome.replace(/\.png$/, ""),
})
const primaryGachaRow = Array(37).fill("")
Object.assign(primaryGachaRow, {
    8: "4",
    13: "1",
    28: featureRelationPaths.gachaHome.replace(/\.png$/, ""),
})
const primaryMissingTargetRow = Array(37).fill("")
Object.assign(primaryMissingTargetRow, {
    8: "28",
    21: "999",
    28: featureRelationPaths.missingTargetHome.replace(/\.png$/, ""),
})
const primaryCollectSecondRow = Array(37).fill("")
Object.assign(primaryCollectSecondRow, {
    8: "3",
    12: "1",
    28: featureRelationPaths.collectSecondHome.replace(/\.png$/, ""),
})
const primaryGachaDuplicateRow = Array(37).fill("")
Object.assign(primaryGachaDuplicateRow, {
    8: "4",
    13: "1",
    28: featureRelationPaths.gachaHome.replace(/\.png$/, ""),
})
const primaryUnsupportedRow = Array(37).fill("")
Object.assign(primaryUnsupportedRow, {
    8: "99",
    28: featureRelationPaths.unsupportedHome.replace(/\.png$/, ""),
})
const primaryInvalidTargetRow = Array(37).fill("")
Object.assign(primaryInvalidTargetRow, {
    8: "10",
    14: "99",
    15: "1",
    28: featureRelationPaths.invalidTargetHome.replace(/\.png$/, ""),
})
const primaryMissingAssetRow = Array(37).fill("")
Object.assign(primaryMissingAssetRow, {
    8: "3",
    12: "1",
    28: featureRelationPaths.missingAssetHome.replace(/\.png$/, ""),
})
const secondaryRaidRow = Array(35).fill("")
Object.assign(secondaryRaidRow, {
    6: "10",
    12: "10",
    13: "1",
    26: featureRelationPaths.raidSecondaryHome.replace(/\.png$/, ""),
    33: featureRelationPaths.raidSecondaryNotice.replace(/\.png$/, ""),
})
const secondaryActiveRow = Array(35).fill("")
Object.assign(secondaryActiveRow, {
    6: "28",
    19: "1",
    26: featureRelationPaths.activeSecondaryHome.replace(/\.png$/, ""),
})
const miscActiveRow = Array(36).fill("")
Object.assign(miscActiveRow, {
    7: "28",
    20: "1",
    27: featureRelationPaths.activeMiscHome.replace(/\.png$/, ""),
    34: featureRelationPaths.activeMiscNotice.replace(/\.png$/, ""),
})
const { catalog: featureRelationSource, coverage: featureRelationCoverage } = buildActivityCatalogArtifacts({
    assetVersion: "1.4.54",
    clientVersion: "1.8.1",
    entityRecords: featureEntityRecords,
    masterMaps: new Map([
        ["event_list", { 1: ["10", "1", "1"] }],
        ["raid_event", { 1: raidRow }],
        ["collect_item_event", { 1: collectItemRow }],
        ["gacha", { 1: gachaRow }],
        ["active_mission_event", { 1: activeMissionRow }],
        ["feature_banner", {
            collect: primaryCollectRow,
            collect_second: primaryCollectSecondRow,
            gacha: primaryGachaRow,
            gacha_duplicate: primaryGachaDuplicateRow,
            invalid: primaryInvalidTargetRow,
            missing_asset: primaryMissingAssetRow,
            missing: primaryMissingTargetRow,
            unsupported: primaryUnsupportedRow,
        }],
        ["feature_banner_secondary", { active: secondaryActiveRow, raid: secondaryRaidRow }],
        ["feature_banner_misc", { active: miscActiveRow }],
    ]),
})
assert.equal(featureRelationSource.activities.length, 4)
assert.equal(featureRelationSource.activities.find((activity) => activity.activity_id === "gacha:1").name, "测试 Gacha")
assert.equal(
    featureRelationSource.activities.find((activity) => activity.activity_id === "collect-item:1").name,
    "未命名活动 1",
)
assert.equal(featureRelationSource.activities
    .filter((activity) => activity.kind !== "gacha")
    .every((activity) => activity.description === ""), true)
assert.match(
    featureRelationSource.activities.find((activity) => activity.activity_id === "gacha:1").description,
    /卡池类型: 普通卡池\. Banner:/,
)
const relatedGacha = featureRelationSource.activities
    .find((activity) => activity.activity_id === "gacha:1")
assert.equal(relatedGacha.tags.includes("banner:unresolved"), false)
assert.doesNotMatch(relatedGacha.description, /未解析到对应纹理/)
assert.equal(relatedGacha.banner_logical_path, featureRelationPaths.gachaHome)
for (const [activityId, logicalPath] of [
    ["collect-item:1", featureRelationPaths.collectHome],
    ["gacha:1", featureRelationPaths.gachaHome],
    ["raid:1", featureRelationPaths.raidSecondaryHome],
    ["active-mission:1", featureRelationPaths.activeMiscHome],
]) {
    assert.ok(featureRelationSource.activities.find((activity) => activity.activity_id === activityId)
        .image_candidates.some((candidate) => candidate.logical_path === logicalPath))
}
const collectFeatureCandidates = featureRelationSource.activities
    .find((activity) => activity.activity_id === "collect-item:1")
    .image_candidates.filter((candidate) => candidate.association_confidence === "exact-activity-id")
assert.deepEqual(
    Object.fromEntries(collectFeatureCandidates.map((candidate) => [candidate.logical_path, candidate.evidence])),
    {
        [featureRelationPaths.collectHome]: "master:feature_banner:key:collect:target:collect-item:1:field:28",
        [featureRelationPaths.collectSecondHome]:
            "master:feature_banner:key:collect_second:target:collect-item:1:field:28",
    },
)
const featureCoverageByName = new Map(featureRelationCoverage.relation_tables
    .map((relation) => [relation.name, relation]))
function relationCoverageCounts(name) {
    const coverage = featureCoverageByName.get(name)
    return {
        source: coverage.source_row_count,
        unsupported: coverage.unsupported_link_kind_count,
        invalid: coverage.invalid_target_count,
        unresolvedTarget: coverage.unresolved_target_count,
        matchedRelation: coverage.matched_relation_count,
        matched: coverage.matched_activity_count,
        candidate: coverage.candidate_count,
        ambiguous: coverage.ambiguous_target_count,
        missingAsset: coverage.missing_asset_count,
        unresolved: coverage.unresolved_count,
    }
}
assert.deepEqual(relationCoverageCounts("feature_banner"),
    {
        source: 8,
        unsupported: 1,
        invalid: 1,
        unresolvedTarget: 1,
        matchedRelation: 5,
        matched: 2,
        candidate: 3,
        ambiguous: 0,
        missingAsset: 1,
        unresolved: 1,
    })
assert.deepEqual(relationCoverageCounts("feature_banner_secondary"),
    {
        source: 2,
        unsupported: 0,
        invalid: 0,
        unresolvedTarget: 0,
        matchedRelation: 2,
        matched: 2,
        candidate: 3,
        ambiguous: 0,
        missingAsset: 0,
        unresolved: 0,
    })
assert.deepEqual(relationCoverageCounts("feature_banner_misc"),
    {
        source: 1,
        unsupported: 0,
        invalid: 0,
        unresolvedTarget: 0,
        matchedRelation: 1,
        matched: 1,
        candidate: 2,
        ambiguous: 0,
        missingAsset: 0,
        unresolved: 0,
    })
// //// /验证活动目录源数据生成 ////

// //// 验证 full 和 diff 资源按当前 EntityLists 记录选择 [@x380kkm 2026-08-19] ////
function makeStoredZip(entryPath, bytes) {
    const name = Buffer.from(entryPath, "utf8")
    const localHeader = Buffer.alloc(30 + name.length)
    localHeader.writeUInt32LE(0x04034b50, 0)
    localHeader.writeUInt16LE(20, 4)
    localHeader.writeUInt16LE(0, 6)
    localHeader.writeUInt16LE(0, 8)
    localHeader.writeUInt16LE(0, 10)
    localHeader.writeUInt16LE(0, 12)
    localHeader.writeUInt32LE(0, 14)
    localHeader.writeUInt32LE(bytes.length, 18)
    localHeader.writeUInt32LE(bytes.length, 22)
    localHeader.writeUInt16LE(name.length, 26)
    localHeader.writeUInt16LE(0, 28)
    name.copy(localHeader, 30)

    const centralHeader = Buffer.alloc(46 + name.length)
    centralHeader.writeUInt32LE(0x02014b50, 0)
    centralHeader.writeUInt16LE(20, 4)
    centralHeader.writeUInt16LE(20, 6)
    centralHeader.writeUInt16LE(0, 8)
    centralHeader.writeUInt16LE(0, 10)
    centralHeader.writeUInt16LE(0, 12)
    centralHeader.writeUInt16LE(0, 14)
    centralHeader.writeUInt32LE(0, 16)
    centralHeader.writeUInt32LE(bytes.length, 20)
    centralHeader.writeUInt32LE(bytes.length, 24)
    centralHeader.writeUInt16LE(name.length, 28)
    centralHeader.writeUInt16LE(0, 30)
    centralHeader.writeUInt16LE(0, 32)
    centralHeader.writeUInt16LE(0, 34)
    centralHeader.writeUInt16LE(0, 36)
    centralHeader.writeUInt32LE(0, 38)
    centralHeader.writeUInt32LE(0, 42)
    name.copy(centralHeader, 46)

    const centralOffset = localHeader.length + bytes.length
    const endRecord = Buffer.alloc(22)
    endRecord.writeUInt32LE(0x06054b50, 0)
    endRecord.writeUInt16LE(0, 4)
    endRecord.writeUInt16LE(0, 6)
    endRecord.writeUInt16LE(1, 8)
    endRecord.writeUInt16LE(1, 10)
    endRecord.writeUInt32LE(centralHeader.length, 12)
    endRecord.writeUInt32LE(centralOffset, 16)
    endRecord.writeUInt16LE(0, 20)
    return Buffer.concat([localHeader, bytes, centralHeader, endRecord])
}

const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "starpoint-cn-master-"))
try {
    const fullDirectory = path.join(fixtureRoot, "archive-common-full")
    const diffDirectory = path.join(fixtureRoot, "archive-common-diff")
    fs.mkdirSync(fullDirectory, { recursive: true })
    fs.mkdirSync(diffDirectory, { recursive: true })
    const masters = [
        { name: "raid_event", path: "master/quest/event/raid_event.orderedmap" },
        { name: "ranking_event", path: "master/quest/event/ranking_event.orderedmap" },
        { name: "banner_image", path: "master/banner/banner_image.orderedmap" },
        { name: "feature_banner", path: "master/feature_banner/feature_banner.orderedmap" },
        { name: "feature_banner_secondary", path: "master/feature_banner/feature_banner_secondary.orderedmap" },
        { name: "feature_banner_misc", path: "master/feature_banner/feature_banner_misc.orderedmap" },
    ]
    const entityRecordsForFixture = new Map()
    const expectedBuffers = new Map()
    for (const [index, master] of masters.entries()) {
        const entryPath = assetEntryPaths(hashCnAssetPath(master.path))[0]
        const stale = Buffer.from(`stale-${index}`)
        const current = Buffer.from(`current-${index}-from-diff`)
        entityRecordsForFixture.set(entryPath, {
            assetKind: "common",
            byteLength: current.length,
            digest: computeCnEntityDigest(current),
            version: "1.4.99",
        })
        expectedBuffers.set(master.name, current)
        fs.writeFileSync(
            path.join(fullDirectory, `000${index}.zip`),
            makeStoredZip(entryPath, stale),
        )
        fs.writeFileSync(
            path.join(diffDirectory, `000${index}.zip`),
            makeStoredZip(entryPath, current),
        )
    }
    const selectedBuffers = await readMasterBuffers(fixtureRoot, entityRecordsForFixture)
    for (const master of masters) {
        assert.deepEqual(selectedBuffers.get(master.name), expectedBuffers.get(master.name))
    }
} finally {
    fs.rmSync(fixtureRoot, { recursive: true, force: true })
}
// //// /验证 full 和 diff 资源按当前 EntityLists 记录选择 ////

console.log("CN activity catalog tests passed.")
