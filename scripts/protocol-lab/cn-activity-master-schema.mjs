// audience: internal
// # cn-activity-master-schema
// 此模块声明 CN EventList 的 15 种活动、独立活动和图片关系表的已证实字段.

const EVENT_MASTER_ROOT = "master/quest/event"

// //// 构造已证实的活动 master schema [@x380kkm 2026-08-19] ////
function imageFields(indexes, bannerIndex) {
    return new Map(indexes.map((index) => [index, index === bannerIndex ? "activity_banner" : "auto"]))
}

function eventSchema(kindCode, name, options) {
    return Object.freeze({
        kindCode: String(kindCode),
        name,
        identityPrefix: options.identityPrefix ?? name.replace(/_event$/, "").replaceAll("_", "-"),
        kind: options.kind,
        label: options.label,
        logicalPath: `${EVENT_MASTER_ROOT}/${name}.orderedmap`,
        nameIndexes: [options.nameIndex],
        startIndex: options.startIndex,
        endIndex: options.endIndex,
        imageFields: imageFields(options.imageIndexes, options.bannerIndex),
    })
}

function independentSchema(name, logicalPath, options) {
    return Object.freeze({
        name,
        identityPrefix: options.identityPrefix ?? name.replace(/_event$/, "").replaceAll("_", "-"),
        kind: options.kind,
        label: options.label,
        logicalPath,
        nameIndexes: Number.isInteger(options.nameIndex) ? [options.nameIndex] : [],
        startIndex: options.startIndex,
        endIndex: options.endIndex,
        imageFields: imageFields(options.imageIndexes ?? [], options.bannerIndex),
    })
}

function featureBannerSchema(name, logicalPath, fields) {
    const pathFields = [[fields.homeBannerIndex, "home_banner"]]
    if (Number.isInteger(fields.noticeBannerIndex)) pathFields.push([fields.noticeBannerIndex, "notice_banner"])
    return Object.freeze({
        name,
        logicalPath,
        discriminatorIndex: fields.linkSceneIndex,
        pathFields: new Map(pathFields),
        targetsByDiscriminator: new Map([
            ["3", Object.freeze({ activitySchemaName: "collect_item_event", idIndex: fields.collectItemEventIdIndex })],
            ["4", Object.freeze({ activitySchemaName: "gacha", idIndex: fields.gachaIdIndex })],
            ["10", Object.freeze({ idIndex: fields.eventIdIndex, eventKindIndex: fields.eventKindIndex })],
            ["28", Object.freeze({ activitySchemaName: "active_mission_event", idIndex: fields.activeMissionEventIdIndex })],
        ]),
    })
}
// //// /构造已证实的活动 master schema ////

// //// 声明 EventList 构造码和根 master 字段 [@x380kkm 2026-08-19] ////
export const EVENT_MASTER_SCHEMAS = Object.freeze([
    eventSchema(0, "advent_event", {
        kind: "advent", label: "Advent", nameIndex: 2, imageIndexes: [4, 5, 7, 8, 9, 10], bannerIndex: 4,
        startIndex: 24, endIndex: 25,
    }),
    eventSchema(1, "ranking_event", {
        kind: "ranking", label: "Ranking", nameIndex: 2, imageIndexes: [4, 5, 6, 7, 8, 9, 14], bannerIndex: 4,
        startIndex: 18, endIndex: 19,
    }),
    eventSchema(2, "story_event", {
        kind: "story", label: "Story event", nameIndex: 2, imageIndexes: [4, 6], bannerIndex: 4,
        startIndex: 16, endIndex: 17,
    }),
    eventSchema(3, "daily_week_event", {
        kind: "daily", label: "Daily rotation", nameIndex: 1, imageIndexes: [3, 5], bannerIndex: 3,
        startIndex: 16, endIndex: 17,
    }),
    eventSchema(4, "challenge_dungeon_event", {
        kind: "challenge", label: "Challenge dungeon", nameIndex: 1, imageIndexes: [3, 5], bannerIndex: 3,
        startIndex: 13, endIndex: 14,
    }),
    eventSchema(5, "daily_exp_mana_event", {
        kind: "daily", label: "Daily EXP and mana", nameIndex: 1, imageIndexes: [3, 5], bannerIndex: 3,
        startIndex: 9, endIndex: 10,
    }),
    eventSchema(6, "world_story_event", {
        kind: "world-story", label: "World story", nameIndex: 2, imageIndexes: [4, 5, 6, 7, 8, 9], bannerIndex: 4,
        startIndex: 22, endIndex: 23,
    }),
    eventSchema(7, "tower_dungeon_event", {
        kind: "tower", label: "Tower", nameIndex: 1, imageIndexes: [3, 5, 7, 8, 9], bannerIndex: 3,
        startIndex: 11, endIndex: 12,
    }),
    eventSchema(8, "expert_single_event", {
        kind: "expert", label: "Expert event", nameIndex: 1, imageIndexes: [3, 5, 6, 7, 8], bannerIndex: 3,
        startIndex: 13, endIndex: 14,
    }),
    eventSchema(9, "carnival_event", {
        kind: "carnival", label: "Carnival", nameIndex: 1, imageIndexes: [3, 5, 6, 7, 8], bannerIndex: 3,
        startIndex: 20, endIndex: 21,
    }),
    eventSchema(10, "raid_event", {
        kind: "raid", label: "Raid", nameIndex: 1, imageIndexes: [3, 4, 5, 10], bannerIndex: 3,
        startIndex: 22, endIndex: 23,
    }),
    eventSchema(11, "rush_event", {
        kind: "rush", label: "Rush event", nameIndex: 1, imageIndexes: [3, 4, 5, 6, 7, 8], bannerIndex: 3,
        startIndex: 15, endIndex: 16,
    }),
    eventSchema(12, "solo_time_attack_event", {
        kind: "time-attack", label: "Solo time attack", nameIndex: 1, imageIndexes: [3, 5, 6, 7, 10], bannerIndex: 3,
        startIndex: 12, endIndex: 13,
    }),
    eventSchema(13, "hard_multi_event", {
        kind: "multi", label: "Hard multiplayer", nameIndex: 2, imageIndexes: [4, 5, 6, 7, 9], bannerIndex: 4,
        startIndex: 23, endIndex: 24,
    }),
    eventSchema(14, "score_attack_event", {
        kind: "score-attack", label: "Score attack", nameIndex: 1, imageIndexes: [3, 5, 6, 7, 8, 10], bannerIndex: 3,
        startIndex: 17, endIndex: 18,
    }),
])

export const EVENT_SCHEMA_BY_KIND = new Map(EVENT_MASTER_SCHEMAS.map((schema) => [schema.kindCode, schema]))
export const EVENT_LIST_SCHEMA = Object.freeze({
    name: "event_list",
    logicalPath: `${EVENT_MASTER_ROOT}/event_list.orderedmap`,
})

export const INDEPENDENT_ACTIVITY_SCHEMAS = Object.freeze([
    independentSchema("collect_item_event", "master/reward/event/collect_item_event.orderedmap", {
        kind: "collect-item", label: "Collect item", imageIndexes: [7, 9, 12, 13, 14, 15, 16], bannerIndex: 7,
        startIndex: 20, endIndex: 21,
    }),
    independentSchema("gacha", "master/gacha/gacha.orderedmap", {
        kind: "gacha", label: "Gacha", nameIndex: 1, imageIndexes: [3], bannerIndex: 3,
        startIndex: 29, endIndex: 30,
    }),
    independentSchema("gacha_campaign", "master/gacha/gacha_campaign.orderedmap", {
        kind: "gacha-campaign", label: "Gacha campaign", nameIndex: 1, imageIndexes: [6], bannerIndex: 6,
        startIndex: 3, endIndex: 4,
    }),
    independentSchema("box_gacha", "master/box_gacha/box_gacha.orderedmap", {
        kind: "box-gacha", label: "Box gacha", nameIndex: 0, imageIndexes: [4], bannerIndex: 4,
        startIndex: 6, endIndex: 7,
    }),
    independentSchema("active_mission_event", "master/active_mission/active_mission_event.orderedmap", {
        kind: "active-mission", label: "Active mission", imageIndexes: [7, 8], bannerIndex: 7,
        startIndex: 14, endIndex: 15,
    }),
    independentSchema("event_shop_select_item_campaign", `${EVENT_MASTER_ROOT}/event_shop_select_item_campaign.orderedmap`, {
        kind: "event-shop", label: "Event shop campaign", imageIndexes: [3], bannerIndex: 3,
        startIndex: 6, endIndex: 7,
    }),
    independentSchema("pass_card_event", "master/pass_card/pass_card_event.orderedmap", {
        kind: "pass-card", label: "Pass card", startIndex: 8, endIndex: 9,
    }),
])

export const ACTIVITY_SCHEMA_BY_NAME = new Map([
    ...EVENT_MASTER_SCHEMAS,
    ...INDEPENDENT_ACTIVITY_SCHEMAS,
].map((schema) => [schema.name, schema]))

export const BANNER_IMAGE_RELATION_SCHEMA = Object.freeze({
    name: "banner_image",
    logicalPath: "master/banner/banner_image.orderedmap",
    pathFields: new Map([
        [4, "home_banner"],
        [5, "activity_banner"],
        [6, "activity_entry"],
    ]),
})

export const FEATURE_BANNER_RELATION_SCHEMAS = Object.freeze([
    featureBannerSchema("feature_banner", "master/feature_banner/feature_banner.orderedmap", {
        linkSceneIndex: 8,
        homeBannerIndex: 28,
        noticeBannerIndex: 35,
        eventKindIndex: 14,
        eventIdIndex: 15,
        collectItemEventIdIndex: 12,
        gachaIdIndex: 13,
        activeMissionEventIdIndex: 21,
    }),
    featureBannerSchema("feature_banner_secondary", "master/feature_banner/feature_banner_secondary.orderedmap", {
        linkSceneIndex: 6,
        homeBannerIndex: 26,
        noticeBannerIndex: 33,
        eventKindIndex: 12,
        eventIdIndex: 13,
        collectItemEventIdIndex: 10,
        gachaIdIndex: 11,
        activeMissionEventIdIndex: 19,
    }),
    featureBannerSchema("feature_banner_misc", "master/feature_banner/feature_banner_misc.orderedmap", {
        linkSceneIndex: 7,
        homeBannerIndex: 27,
        noticeBannerIndex: 34,
        eventKindIndex: 13,
        eventIdIndex: 14,
        collectItemEventIdIndex: 11,
        gachaIdIndex: 12,
        activeMissionEventIdIndex: 20,
    }),
])

export const ACTIVITY_MASTER_SCHEMAS = Object.freeze([
    EVENT_LIST_SCHEMA,
    ...EVENT_MASTER_SCHEMAS,
    ...INDEPENDENT_ACTIVITY_SCHEMAS,
])

export const CATALOG_MASTER_SCHEMAS = Object.freeze([
    ...ACTIVITY_MASTER_SCHEMAS,
    BANNER_IMAGE_RELATION_SCHEMA,
    ...FEATURE_BANNER_RELATION_SCHEMAS,
])

export const PENDING_ACTIVITY_RELATIONS = Object.freeze([
    Object.freeze({
        relation: "unproven-activity-foreign-key",
        source_table: "master/feature_banner/feature_announcement.orderedmap",
    }),
])
// //// /声明 EventList 构造码和根 master 字段 ////

// //// 枚举嵌套 master 行和 EventList 精确引用 [@x380kkm 2026-08-19] ////
function collectRowEntries(value, keys, entries) {
    if (Array.isArray(value)) {
        entries.push({ key: keys.length > 0 ? keys.join("/") : "(root)", row: value })
        return
    }
    if (!value || typeof value !== "object") return
    for (const [key, child] of Object.entries(value)) collectRowEntries(child, [...keys, key], entries)
}

export function enumerateMasterRows(value) {
    const entries = []
    collectRowEntries(value, [], entries)
    return entries
}

export function enumerateEventReferences(eventListMap) {
    return enumerateMasterRows(eventListMap).flatMap(({ row }) => {
        const schema = EVENT_SCHEMA_BY_KIND.get(String(row[0]))
        const eventId = String(row[1] ?? "")
        if (!schema || !/^\d+$/.test(eventId) || Number(eventId) < 1) return []
        return [{ displayOrder: Number(row[2]), eventId, schema }]
    })
}

export function collectMasterRows(value) {
    return enumerateMasterRows(value).map(({ row }) => row)
}
// //// /枚举嵌套 master 行和 EventList 精确引用 ////
