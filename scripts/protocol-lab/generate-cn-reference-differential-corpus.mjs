// audience: internal
// # cn-reference-differential-corpus
//
// 生成 CN 参考服务与本地服务共享的动态差分请求语料. 运行时需要关联参考仓库,
// 抓包仓库和反编译仓库位于当前工作区的同级目录.

import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { spawnSync } from "node:child_process"

// //// 定义语料生成配置 [@x380kkm 2026-08-23] ////
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIR, "../..")
const WORKSPACE_ROOT = path.dirname(REPOSITORY_ROOT)
const OUTPUT_PATH = path.join(SCRIPT_DIR, "cn-reference-differential-corpus.json")
const AUDITOR_PATH = path.join(SCRIPT_DIR, "audit-cn-reference-route-coverage.mjs")
const VIEWER_REF = Object.freeze({ $ref: "tool-signup.data_headers.viewer_id" })
const PLAYER_DEPENDENCY = Object.freeze(["tool-signup"])
const ALLOWED_ENCODINGS = new Set([
    "none", "json", "form", "messagepack", "base64-messagepack", "raw-messagepack", "text", "base64",
])
const CONFIRMED_RESPONSE_CONTRACTS = Object.freeze({
    "POST /api/index.php/asset/version_info": {
        targetEvidence: ["starpoint-capture-history/docs/routes/asset_version_info.md:36"],
        responsePaths: [],
    },
    "POST /api/index.php/character/add_character_from_town": {
        targetEvidence: ["starpoint-capture-history/docs/routes/character_add_character_from_town.md:86"],
        responsePaths: ["$.data.encyclopedia_info"],
    },
    "POST /api/index.php/character/receive_bond_token": {
        targetEvidence: [
            "starpoint-capture-history/docs/routes/character_receive_bond_token.md:86",
            "starpoint-capture-history/docs/routes/character_receive_bond_token.md:93",
        ],
        responsePaths: ["$.data.mission_info", "$.data.active_mission_list"],
    },
    "POST /api/index.php/shop/buy": {
        targetEvidence: ["starpoint-capture-history/docs/routes/shop_buy.md:71"],
        responsePaths: ["$.data.active_mission_list"],
    },
    "POST /api/index.php/mission/get_mission_progress": {
        targetEvidence: ["starpoint-capture-history/docs/routes/mission_get_mission_progress.md:693"],
        responsePaths: ["$.data.mail_arrived"],
    },
    "POST /api/index.php/mission/update_mission_progress": {
        targetEvidence: ["starpoint-capture-history/docs/routes/mission_update_mission_progress.md:81"],
        responsePaths: ["$.data.mail_arrived"],
    },
})

const STATEFUL_TERMINALS = new Set([
    "abort", "action", "add_character_from_town", "awake_mana_node", "bulk_buy", "bulk_over_limit",
    "bulk_sell_stack", "bulk_stack_to_exp", "bulk_upgrade", "buy", "close", "draw", "edit", "endless_battle",
    "exchange_character", "exchange_equipment", "exec", "finish", "finish_trigger", "finish_with_skip", "first_draw",
    "inject_exp", "learn_mana_node", "open_mana_board", "over_limit", "play_continue", "publish", "receive",
    "receive_all", "receive_bond_token", "receive_reward", "recover_stamina", "rename", "report_purchase_result",
    "reset", "reward", "select", "select_folder", "sell", "sell_equipment", "sell_stack", "set_campaign_lineup_id",
    "set_illustration_settings", "set_protection", "start", "update", "update_comment", "update_degree",
    "stack_to_exp", "unlock", "update_in_battle", "update_mission_progress", "update_profile_settings", "update_step",
    "upgrade", "use_item",
])
const EQUIPMENT_SALE_PATHS = new Set([
    "/api/index.php/equipment/sell_equipment",
    "/api/index.php/equipment/sell_stack",
])
const REFERENCE_UNCHANGED_STATE_ROUTES = new Set([
    "POST /api/index.php/active_mission/receive",
    "POST /api/index.php/attention/action",
    "POST /api/index.php/character/bulk_over_limit",
    "POST /api/index.php/contents_guide/start",
    "POST /api/index.php/equipment/bulk_sell_stack",
    "POST /api/index.php/equipment/bulk_upgrade",
    "POST /api/index.php/equipment/set_protection",
    "POST /api/index.php/event/raid/reset",
    "POST /api/index.php/event/raid/battle/start",
    "POST /api/index.php/event/raid/select_folder",
    "POST /api/index.php/event/rush/endless_battle",
    "POST /api/index.php/event/rush/battle/start",
    "POST /api/index.php/event/rush/reset",
    "POST /api/index.php/event/rush/reward",
    "POST /api/index.php/event/rush/select_folder",
    "POST /api/index.php/expod/bulk_stack_to_exp",
    "POST /api/index.php/history/receive",
    "POST /api/index.php/mail/receive_all",
    "POST /api/index.php/mission/update_mission_progress",
    "POST /api/index.php/multi_battle_quest/abort",
    "POST /api/index.php/multi_battle_quest/start",
    "POST /api/index.php/party_group/edit",
    "POST /api/index.php/payment/finish",
    "POST /api/index.php/payment/report_purchase_result",
    "POST /api/index.php/payment/start",
    "POST /api/index.php/profile/update_degree",
    "POST /api/index.php/profile/update_profile_settings",
    "POST /api/index.php/ranking_event/receive_reward",
    "POST /api/index.php/shop/bulk_buy",
    "POST /api/index.php/shop/set_campaign_lineup_id",
    "POST /api/index.php/single_battle_quest/abort",
    "POST /api/index.php/single_battle_quest/start",
])
const STAMINA_STATE_ROUTES = new Set([
    "POST /api/index.php/shop/recover_stamina",
    "POST /api/index.php/single_battle_quest/start",
])

const FIELD_VALUES = Object.freeze({
    viewer_id: VIEWER_REF,
    keychain: VIEWER_REF,
    api_count: 1,
    retry_count: 0,
    character_id: 1,
    equipment_id: 1,
    item_id: 13,
    quest_id: 1001002,
    party_id: 1,
    main_party_id: 1,
    gacha_id: 1,
    box_gacha_id: 1001,
    box_id: 1,
    event_id: 1,
    ranking_event_id: 1,
    category: 1,
    current_page: 1,
    page_index: 0,
    page: 0,
    number: 1,
    number_of_exec: 1,
    payment_type: 1,
    type: 1,
    step: 0,
    skip: true,
    play_id: "${play_id}",
    device_id: 18420260823,
    userId: "10000001",
})

const TUTORIAL_SKIP_SETUP = Object.freeze({
    method: "POST",
    path: "/api/index.php/tutorial/update_step",
    encoding: "base64-messagepack",
    body: { retry_count: 1, api_count: 1, viewer_id: VIEWER_REF, skip: true, step: 0 },
})

const RAID_BATTLE_ABORT_AFTER = Object.freeze({
    method: "POST",
    path: "/api/index.php/single_battle_quest/abort",
    encoding: "base64-messagepack",
    body: {
        viewer_id: VIEWER_REF, finish_kind: 1, quest_id: 1001, play_id: "${play_id}-raid", category: 23,
        statistics: { clear_phase: 0, party: {} },
    },
})

const RUSH_BATTLE_ABORT_AFTER = Object.freeze({
    method: "POST",
    path: "/api/index.php/single_battle_quest/abort",
    encoding: "base64-messagepack",
    body: {
        viewer_id: VIEWER_REF, finish_kind: 1, quest_id: 700007001, play_id: "${play_id}-rush", category: 24,
        statistics: { clear_phase: 0, party: {} },
    },
})

const MULTI_CATEGORY = 2
const MULTI_QUEST_ID = 1001001
const MULTI_PARTY_ID = 1

// //// 构造两侧邮件资源准备请求 [@x380kkm 2026-08-23] ////
function createMailSideRequest(id, referenceReward, rustRewards) {
    return {
        id,
        reference: {
            method: "POST",
            path: "/api/mail/send",
            encoding: "form",
            body: {
                type: referenceReward.type,
                ...(referenceReward.typeId === undefined ? {} : { type_id: referenceReward.typeId }),
                number: referenceReward.number,
                subject: "Differential setup",
                description: "Differential setup",
            },
            expectedStatus: 302,
            expectedLocationIncludes: "/mail?ok=",
        },
        rust: {
            method: "POST",
            path: "/v1/mails",
            encoding: "json",
            body: {
                viewer_id: VIEWER_REF,
                title: "Differential setup",
                body: "Differential setup",
                sender: "Differential",
                rewards: rustRewards,
                expires_at: null,
            },
            expectedStatus: 201,
        },
    }
}
// //// /构造两侧邮件资源准备请求 ////

// //// 构造装备堆叠邮件准备请求 [@x380kkm 2026-08-25] ////
function createEquipmentMailSideRequests(id, equipmentId, count) {
    return Array.from({ length: count }, (_, index) => createMailSideRequest(
        `${id}-${index + 1}`,
        { type: 6, typeId: equipmentId, number: 1 },
        { equipmentList: { [equipmentId]: 1 } },
    ))
}
// //// /构造装备堆叠邮件准备请求 ////

// //// 构造两侧邮件领取请求 [@x380kkm 2026-08-23] ////
function createMailClaimSideRequests(id) {
    return [
        {
            id: `${id}-mail-index`,
            reference: {
                method: "POST",
                path: "/api/index.php/mail/index",
                encoding: "base64-messagepack",
                body: { viewer_id: VIEWER_REF, current_page: 1 },
                expectedStatus: 200,
            },
        },
        {
            id,
            reference: {
                method: "POST",
                path: "/api/index.php/mail/receive_all",
                encoding: "base64-messagepack",
                body: {
                    viewer_id: VIEWER_REF,
                    mail_ids: { $collect: { from: `${id}-mail-index.data.mail`, field: "id" } },
                },
                expectedStatus: 200,
            },
            rust: {
                method: "POST",
                path: "/api/index.php/mail/receive_all",
                encoding: "base64-messagepack",
                body: { viewer_id: VIEWER_REF },
                expectedStatus: 200,
            },
        },
    ]
}
// //// /构造两侧邮件领取请求 ////

// //// 构造两侧客户端状态准备请求 [@x380kkm 2026-08-23] ////
function createGameSideRequest(id, requestPath, body, options = {}) {
    const { referenceBody = body, rustBody = body, ...requestOptions } = options
    const request = {
        method: "POST",
        path: requestPath,
        encoding: "base64-messagepack",
        expectedStatus: 200,
        ...requestOptions,
    }
    return {
        id,
        reference: { ...request, body: referenceBody },
        rust: { ...structuredClone(request), body: rustBody },
    }
}
// //// /构造两侧客户端状态准备请求 ////

// //// 构造同账号联机房间准备请求 [@x380kkm 2026-08-23] ////
function createMultiRoomSideRequest(id) {
    return createGameSideRequest(id, "/api/index.php/multi_battle_quest/create_room", {
        viewer_id: VIEWER_REF,
        category: MULTI_CATEGORY,
        quest_id: MULTI_QUEST_ID,
        party_id: MULTI_PARTY_ID,
    })
}
// //// /构造同账号联机房间准备请求 ////

// //// 构造同账号联机战斗准备请求 [@x380kkm 2026-08-23] ////
function createMultiBattleSideRequests(id) {
    const roomRequestId = `${id}-room`
    const playId = `\${play_id}-${id}`
    return {
        playId,
        roomNumber: { $ref: `${roomRequestId}.data.room_number` },
        sideRequests: [
            createMultiRoomSideRequest(roomRequestId),
            createGameSideRequest(`${id}-start`, "/api/index.php/multi_battle_quest/start", {
                viewer_id: VIEWER_REF,
                category: MULTI_CATEGORY,
                quest_id: MULTI_QUEST_ID,
                party_id: MULTI_PARTY_ID,
                play_id: playId,
                room_number: { $ref: `${roomRequestId}.data.room_number` },
                use_boost_point: false,
                use_boss_boost_point: false,
                is_auto_start_mode: false,
                mate_player_ids: [],
            }),
        ],
    }
}
// //// /构造同账号联机战斗准备请求 ////

const MANA_NODE_IDS = Object.freeze(Array.from({ length: 23 }, (_, index) => 2201 + index))
const CHARACTER_MANA_MAILS = Object.freeze([
    createMailSideRequest("grant-mana", { type: 8, number: 113240 }, { freeMana: 113240 }),
    createMailSideRequest("grant-mana-item-1", { type: 1, typeId: 1, number: 212 }, { itemList: { 1: 212 } }),
    createMailSideRequest("grant-mana-item-2", { type: 1, typeId: 2, number: 179 }, { itemList: { 2: 179 } }),
    createMailSideRequest("grant-mana-item-3", { type: 1, typeId: 3, number: 90 }, { itemList: { 3: 90 } }),
    createMailSideRequest("grant-mana-item-4", { type: 1, typeId: 4, number: 29 }, { itemList: { 4: 29 } }),
    createMailSideRequest("grant-mana-item-99", { type: 1, typeId: 99, number: 100 }, { itemList: { 99: 100 } }),
    createMailSideRequest("grant-mana-awake-item", { type: 1, typeId: 70047, number: 100 }, { itemList: { 70047: 100 } }),
    ...createMailClaimSideRequests("claim-mana-resources"),
])
const MULTI_ABORT_SETUP = Object.freeze(createMultiBattleSideRequests("multi-abort"))
const MULTI_CONTINUE_SETUP = Object.freeze(createMultiBattleSideRequests("multi-continue"))
const MULTI_FINISH_SETUP = Object.freeze(createMultiBattleSideRequests("multi-finish"))

const CASE_OVERRIDES = Object.freeze({
    "GET /shijtswy/version/client_release_android.dis": {
        comparison: {
            text: { stripFirstLines: 1, parseJson: true },
            ignorePaths: ["$.default"],
        },
    },
    "GET /shijtswy/version/client_release_ios.dis": {
        comparison: { text: { stripFirstLines: 1, parseJson: true } },
    },
    "POST /api/index.php/tool/signup": {
        id: "tool-signup",
        body: {
            device_id: 18420260823, channelNo: "leiting", media: "none", androidId: "", oaid: "", mac: "",
            terminInfo: "", osVer: "", storage_directory_path: "/data/user/0/com.leiting.wf",
        },
        capture: { viewer_id: "$.data_headers.viewer_id" },
        evidence: ["scripts/protocol-lab/test-cn-server.js:1959", "scripts/protocol-lab/ios_cn_game_scenario_stages.py:282"],
        branch: "success",
    },
    "POST /api/index.php/load": {
        id: "load",
        body: {
            device_id: 18420260823, device_token: "", keychain: VIEWER_REF,
            graphics_device_name: "Differential Runner", platform_os_version: "iOS 18",
            storage_directory_path: "/var/mobile/Containers/Data/Application", viewer_id: VIEWER_REF,
        },
        comparison: {
            ignorePaths: [
                "$.data.gacha_campaign_list",
                "$.data.gacha_info_list",
                "$.data.tutorial_gacha",
                "$.data.user_character_list.243001",
                "$.data.user_character_list.251001",
                "$.data.user_info.free_vmoney",
                "$.data.user_info.stamina",
                "$.data.user_triggered_tutorial",
                "$.data.user_tutorial",
            ],
        },
        evidence: ["scripts/protocol-lab/test-cn-server.js:2004", "scripts/protocol-lab/ios_cn_game_scenario_stages.py:305"],
        branch: "success",
    },
    "POST /api/index.php/channels/channel_leiting/leiting_antiaddiction_login": {
        body: {}, evidence: ["scripts/protocol-lab/test-cn-server.js:278"], branch: "success",
    },
    "POST /api/index.php/channels/channel_leiting/leiting_login": {
        body: { userId: "10000001" }, evidence: ["scripts/protocol-lab/ios_cn_game_scenario_stages.py:253"], branch: "success",
    },
    "POST /api/index.php/asset/version_info": {
        encoding: "json", body: {},
        comparison: { ignorePaths: ["$.data.total_size"] },
        evidence: ["scripts/protocol-lab/test-cn-server.js:2190"], branch: "success",
        targetOverride: {
            kind: "reference-defect",
            reason: "目标响应使用 MessagePack envelope; launcher 仍返回 JSON envelope.",
            evidence: ["starpoint-capture-history/docs/routes/asset_version_info.md:36"],
        },
    },
    "POST /api/index.php/asset/get_path": {
        encoding: "json",
        headers: { device_lang: "en", requestedby: "ios", res_ver: "1.4.0" },
        body: {},
        comparison: {
            ignorePaths: ["$.data.full.archive", "$.data.diff.*.archive"],
        },
        targetOverride: {
            kind: "reference-defect",
            reason: "目标响应返回服务端实际资源版本; launcher 把客户端 res_ver 回填为目标版本.",
            evidence: ["starpoint-capture-history/docs/routes/asset_get_path.md:64"],
        },
        evidence: ["scripts/protocol-lab/test-cn-server.js:2200"], branch: "success",
    },
    "POST /api/index.php/assetintitle/version_info_in_title": {
        comparison: { ignorePaths: ["$.data.total_size"] },
    },
    "POST /api/index.php/attention/action": {
        body: { viewer_id: VIEWER_REF, priority_factors: [] }, branch: "success",
    },
    "POST /api/index.php/attention/logger": {
        body: { viewer_id: VIEWER_REF, client_logs: [] }, branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/get_rooms": {
        body: { viewer_id: VIEWER_REF, category_id: MULTI_CATEGORY },
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/create_room": {
        body: {
            viewer_id: VIEWER_REF,
            category: MULTI_CATEGORY,
            quest_id: MULTI_QUEST_ID,
            party_id: MULTI_PARTY_ID,
        },
        comparison: {
            ignorePaths: ["$.data.access_token", "$.data.room_number"],
        },
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/search_room": {
        body: {
            viewer_id: VIEWER_REF,
            room_number: { $ref: "multi-search-room.data.room_number" },
        },
        sideRequests: [createMultiRoomSideRequest("multi-search-room")],
        comparison: { ignorePaths: ["$.data.establisher_viewer_id", "$.data.room_number"] },
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/select_room": {
        body: {
            viewer_id: VIEWER_REF,
            category: MULTI_CATEGORY,
            quest_id: MULTI_QUEST_ID,
            party_id: MULTI_PARTY_ID,
            room_number: { $ref: "multi-select-room.data.room_number" },
        },
        sideRequests: [createMultiRoomSideRequest("multi-select-room")],
        comparison: { ignorePaths: ["$.data.port", "$.data.room_number"] },
        stateExpectation: "unchanged",
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/prepare": {
        body: {
            viewer_id: VIEWER_REF,
            category: MULTI_CATEGORY,
            quest_id: MULTI_QUEST_ID,
            room_number: { $ref: "multi-prepare-room.data.room_number" },
        },
        sideRequests: [createMultiRoomSideRequest("multi-prepare-room")],
        comparison: { ignorePaths: ["$.data.port", "$.data.room_number"] },
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/summon": {
        body: {
            viewer_id: VIEWER_REF,
            category_id: MULTI_CATEGORY,
            quest_id: MULTI_QUEST_ID,
            room_number: { $ref: "multi-summon-room.data.room_number" },
        },
        sideRequests: [createMultiRoomSideRequest("multi-summon-room")],
        comparison: {
            ignorePaths: [
                "$.data.mate1.com_id",
                "$.data.mate1.degree_id",
                "$.data.mate1.rank",
                "$.data.mate1.party.characters",
                "$.data.mate1.party.unison_characters",
                "$.data.mate1.party.equipments",
                "$.data.mate1.party.ability_soul_ids",
                "$.data.mate2.com_id",
                "$.data.mate2.degree_id",
                "$.data.mate2.rank",
                "$.data.mate2.party.characters",
                "$.data.mate2.party.unison_characters",
                "$.data.mate2.party.equipments",
                "$.data.mate2.party.ability_soul_ids",
            ],
        },
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/restore_room": {
        body: {
            viewer_id: VIEWER_REF,
            room_number: { $ref: "multi-restore-room.data.room_number" },
            room_sequence: { $ref: "multi-restore-prepare.data.room_sequence" },
        },
        sideRequests: [
            createMultiRoomSideRequest("multi-restore-room"),
            createGameSideRequest("multi-restore-prepare", "/api/index.php/multi_battle_quest/prepare", {
                viewer_id: VIEWER_REF,
                category: MULTI_CATEGORY,
                quest_id: MULTI_QUEST_ID,
                room_number: { $ref: "multi-restore-room.data.room_number" },
            }),
        ],
        comparison: { ignorePaths: ["$.data.port", "$.data.room_number"] },
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/share_room": {
        body: {
            viewer_id: VIEWER_REF,
            room_number: { $ref: "multi-share-room.data.room_number" },
        },
        sideRequests: [createMultiRoomSideRequest("multi-share-room")],
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/disband_room": {
        body: {
            viewer_id: VIEWER_REF,
            room_number: { $ref: "multi-disband-room.data.room_number" },
        },
        sideRequests: [createMultiRoomSideRequest("multi-disband-room")],
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/start": {
        body: {
            viewer_id: VIEWER_REF,
            category: MULTI_CATEGORY,
            quest_id: MULTI_QUEST_ID,
            party_id: MULTI_PARTY_ID,
            play_id: "${play_id}-multi-start",
            room_number: { $ref: "multi-start-room.data.room_number" },
            use_boost_point: false,
            use_boss_boost_point: false,
            is_auto_start_mode: false,
            mate_player_ids: [],
        },
        sideRequests: [createMultiRoomSideRequest("multi-start-room")],
        comparison: {
            ignorePaths: [
                "$.data.category_id",
                "$.data.client_checks",
                "$.data.follow_bonus_info",
                "$.data.quest_name",
                "$.data.start_time",
                "$.data.user_info",
            ],
        },
        stateIgnorePaths: [
            "$.data.unfinished_multi_quest_list",
            "$.data.user_info.total_stamina_used",
        ],
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/finish": {
        body: {
            viewer_id: VIEWER_REF,
            category: MULTI_CATEGORY,
            quest_id: MULTI_QUEST_ID,
            play_id: MULTI_FINISH_SETUP.playId,
            elapsed_time_ms: 1000,
            score: 0,
            add_mana: 0,
            is_accomplished: true,
            statistics: {
                party: {
                    characters: [{ id: 1 }, null, null],
                    unison_characters: [null, null, null],
                },
            },
        },
        sideRequests: MULTI_FINISH_SETUP.sideRequests,
        comparison: {
            ignorePaths: [
                "$.data.drop_rare_reward_ids",
                "$.data.equipment_list",
                "$.data.user_info.free_mana",
                "$.data.user_info.stamina",
            ],
        },
        stateIgnorePaths: [
            "$.data.unfinished_multi_quest_list",
            "$.data.user_equipment_list",
            "$.data.user_info.free_mana",
        ],
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/abort": {
        body: {
            viewer_id: VIEWER_REF,
            category: MULTI_CATEGORY,
            quest_id: MULTI_QUEST_ID,
            play_id: MULTI_ABORT_SETUP.playId,
        },
        sideRequests: MULTI_ABORT_SETUP.sideRequests,
        stateIgnorePaths: ["$.data.unfinished_multi_quest_list"],
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/play_continue": {
        body: { viewer_id: VIEWER_REF },
        sideRequests: MULTI_CONTINUE_SETUP.sideRequests,
        branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/verify_access_token": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/micro_community": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/multi_battle_quest/publish_room": {
        body: {}, branch: "success",
    },
    "POST /api/index.php/carnival_event/index": {
        body: { viewer_id: VIEWER_REF, event_id: 1 },
        branch: "success",
    },
    "POST /api/index.php/carnival_event/get_party": {
        body: { viewer_id: VIEWER_REF },
        branch: "success",
    },
    "POST /api/index.php/character/awake_mana_node": {
        body: { viewer_id: VIEWER_REF, character_id: 1, mana_node_multiplied_id_list: [2201], awake_level: 1 },
        sideRequests: [
            ...CHARACTER_MANA_MAILS,
            createGameSideRequest("learn-mana-for-awake", "/api/index.php/character/learn_mana_node", {
                viewer_id: VIEWER_REF, character_id: 1, mana_node_multiplied_id_list: MANA_NODE_IDS,
            }),
        ],
        branch: "success",
    },
    "POST /api/index.php/character/learn_mana_node": {
        body: { viewer_id: VIEWER_REF, character_id: 1, mana_node_multiplied_id_list: MANA_NODE_IDS },
        sideRequests: CHARACTER_MANA_MAILS,
        branch: "success",
    },
    "POST /api/index.php/character/open_mana_board": {
        body: { viewer_id: VIEWER_REF, character_id: 1, mana_board_index: 2 },
        sideRequests: [
            createMailSideRequest("grant-board-exp", { type: 9, number: 76262 }, { expPool: 76262 }),
            createMailSideRequest("grant-board-limit-items", { type: 1, typeId: 10001, number: 4 }, { itemList: { 10001: 4 } }),
            ...CHARACTER_MANA_MAILS.slice(0, -2),
            ...createMailClaimSideRequests("claim-board-resources"),
            createGameSideRequest("inject-exp-for-board", "/api/index.php/expod/inject_exp", {
                viewer_id: VIEWER_REF, character_id: 1, exp: 76262,
            }),
            createGameSideRequest("over-limit-for-board", "/api/index.php/character/over_limit", {
                viewer_id: VIEWER_REF, character_id: 1, over_limit_count: 4, use_stack: false, item_id: 10001,
            }),
            createGameSideRequest("learn-mana-for-board", "/api/index.php/character/learn_mana_node", {
                viewer_id: VIEWER_REF, character_id: 1, mana_node_multiplied_id_list: MANA_NODE_IDS,
            }),
            createGameSideRequest("awake-mana-for-board", "/api/index.php/character/awake_mana_node", {
                viewer_id: VIEWER_REF, character_id: 1, mana_node_multiplied_id_list: [2201], awake_level: 1,
            }),
            createGameSideRequest("receive-token-for-board", "/api/index.php/character/receive_bond_token", {
                viewer_id: VIEWER_REF, character_id: 1, mana_board_index: 1,
            }),
        ],
        branch: "success",
    },
    "POST /api/index.php/character/over_limit": {
        body: {
            viewer_id: VIEWER_REF, character_id: 1, over_limit_count: 4, use_stack: false, item_id: 10001,
        },
        sideRequests: [
            createMailSideRequest("grant-limit-items", { type: 1, typeId: 10001, number: 4 }, { itemList: { 10001: 4 } }),
            ...createMailClaimSideRequests("claim-limit-items"),
        ],
        branch: "success",
    },
    "POST /api/index.php/character/receive_bond_token": {
        body: { viewer_id: VIEWER_REF, character_id: 1, mana_board_index: 1 },
        sideRequests: [
            ...CHARACTER_MANA_MAILS,
            createGameSideRequest("learn-mana-for-token", "/api/index.php/character/learn_mana_node", {
                viewer_id: VIEWER_REF, character_id: 1, mana_node_multiplied_id_list: MANA_NODE_IDS,
            }),
            createGameSideRequest("awake-mana-for-token", "/api/index.php/character/awake_mana_node", {
                viewer_id: VIEWER_REF, character_id: 1, mana_node_multiplied_id_list: [2201], awake_level: 1,
            }),
        ],
        branch: "success",
        targetOverride: {
            kind: "reference-defect",
            reason: "目标响应包含普通任务完成信息与主动任务进度增量; launcher 缺少 mission_info 和 active_mission_list.",
            evidence: [
                "starpoint-capture-history/docs/routes/character_receive_bond_token.md:86",
                "starpoint-capture-history/docs/routes/character_receive_bond_token.md:93",
            ],
        },
    },
    "POST /api/index.php/character/add_character_from_town": {
        branch: "success",
        targetOverride: {
            kind: "reference-defect",
            reason: "目标响应包含新角色图鉴增量; launcher 缺少 encyclopedia_info.",
            evidence: ["starpoint-capture-history/docs/routes/character_add_character_from_town.md:86"],
        },
    },
    "POST /api/index.php/comic/get_list": {
        body: { viewer_id: VIEWER_REF, kind: 0, page_index: 0 }, branch: "success",
    },
    "GET /api/index.php/comic/image": {
        query: { kind: 0, episode: 1 },
        status: 404,
        branch: "error",
        localExtension: {
            reason: "缺失漫画图片返回透明 PNG, 避免客户端弹出资源错误.",
            reference: { status: 404, contentType: "application/json" },
            rust: { status: 200, contentType: "image/png" },
        },
    },
    "POST /api/index.php/contents_guide/start": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/equipment/bulk_sell_stack": {
        body: { viewer_id: VIEWER_REF, equipment_ids: [999999] }, branch: "success",
    },
    "POST /api/index.php/equipment/sell_equipment": {
        body: {
            viewer_id: VIEWER_REF,
            equipment_list: [{ equipment_id: 5_030_037 }],
        },
        targetOverride: {
            kind: "reference-defect",
            reason: "目标 CN 响应按完整装备堆叠发放能力魂并移除装备; launcher 编译产物固定发放 1 个并保留空堆叠.",
            evidence: [
                "starpoint-capture-history/docs/routes/equipment_sell_equipment.md:32",
                "starpoint-capture-history/docs/routes/equipment_sell_equipment.md:67",
            ],
        },
        sideRequests: [
            ...createEquipmentMailSideRequests("grant-sell-equipment-stack", 5_030_037, 4),
            ...createMailClaimSideRequests("claim-sell-equipment-stack"),
        ],
        comparison: {
            valuePaths: [
                "$.data.equipment_list",
                "$.data.item_list.100000",
                "$.data.item_list.5030037",
                "$.data.mail_arrived",
            ],
        },
        stateProjection: {
            valuePaths: [
                "$.data.user_equipment_list.5030037",
                "$.data.item_list.100000",
                "$.data.item_list.5030037",
            ],
        },
        stateExpectation: "changed",
        branch: "success",
    },
    "POST /api/index.php/equipment/sell_stack": {
        body: {
            viewer_id: VIEWER_REF,
            equipment_list: [{ equipment_id: 5_040_028, number: 2 }],
        },
        sideRequests: [
            ...createEquipmentMailSideRequests("grant-sell-stack", 5_040_028, 4),
            ...createMailClaimSideRequests("claim-sell-stack"),
        ],
        comparison: {
            arrayFilters: [{
                path: "$.data.equipment_list",
                where: { field: "equipment_id", equals: 5_040_028 },
            }],
            valuePaths: [
                "$.data.equipment_list[0].equipment_id",
                "$.data.equipment_list[0].stack",
                "$.data.item_list.100000",
                "$.data.item_list.5040028",
                "$.data.mail_arrived",
            ],
        },
        stateProjection: {
            valuePaths: [
                "$.data.user_equipment_list.5040028.stack",
                "$.data.item_list.100000",
                "$.data.item_list.5040028",
            ],
        },
        stateExpectation: "changed",
        branch: "success",
    },
    "POST /api/index.php/equipment/upgrade": {
        body: { viewer_id: VIEWER_REF, equipment_id: 5030037, upgrade_count: 1, use_stack: true },
        sideRequests: [
            createMailSideRequest("grant-upgrade-equipment-1", { type: 6, typeId: 5030037, number: 1 }, { equipmentList: { 5030037: 1 } }),
            createMailSideRequest("grant-upgrade-equipment-2", { type: 6, typeId: 5030037, number: 1 }, { equipmentList: { 5030037: 1 } }),
            createMailSideRequest("grant-upgrade-items", { type: 1, typeId: 100000, number: 25 }, { itemList: { 100000: 25 } }),
            ...createMailClaimSideRequests("claim-upgrade-resources"),
        ],
        branch: "success",
    },
    "POST /api/index.php/event/raid/summary": {
        body: { viewer_id: VIEWER_REF, event_id: 1 }, branch: "success",
    },
    "POST /api/index.php/event/raid/get_boss": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/event/raid/ranking_reward": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/event/raid/party": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/event/raid/ranking": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/event/raid/ranking/party": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/event/raid/battle/start": {
        body: {
            viewer_id: VIEWER_REF, quest_id: 1001, event_id: 1, party_group_id: 1,
            is_auto_start_mode: false, play_id: "${play_id}-raid",
        },
        probes: { after: [RAID_BATTLE_ABORT_AFTER] },
        branch: "success",
    },
    "POST /api/index.php/event/raid/select_folder": {
        body: { viewer_id: VIEWER_REF, event_id: 1, folder_id: 1 }, branch: "success",
    },
    "POST /api/index.php/event/raid/reset": {
        body: {
            viewer_id: VIEWER_REF, event_id: 1, quest_type: 0,
            reset_target_id: 1, is_reset_after_target_round: false,
        },
        branch: "success",
    },
    "POST /api/index.php/event/rush/summary": {
        body: { viewer_id: VIEWER_REF, event_id: 700007 }, branch: "success",
    },
    "POST /api/index.php/event/rush/select_folder": {
        body: { viewer_id: VIEWER_REF, event_id: 700007, folder_id: 1 },
        dependsOn: ["event-rush-summary"], branch: "success",
    },
    "POST /api/index.php/event/rush/ranking": {
        body: { viewer_id: VIEWER_REF, event_id: 700007, page: 0 }, branch: "success",
        targetOverride: {
            kind: "reference-defect",
            reason: "CN 1.8.x 客户端读取 data.ranking_list; launcher 编译产物使用 ranking_data.",
            evidence: [
                "wf-2.1.125-cn-decompiled/scripts/scripts/pinball/remote/event/rush/ranking/EventRushRankingRealRemote.as:348",
            ],
        },
    },
    "POST /api/index.php/event/rush/ranking/played_party": {
        body: { viewer_id: VIEWER_REF, event_id: 700007, rank_number: 1 }, branch: "success",
    },
    "POST /api/index.php/event/rush/aggregated_time": {
        body: { viewer_id: VIEWER_REF, event_id: 700007 }, branch: "success",
    },
    "POST /api/index.php/event/rush/party": {
        body: { viewer_id: VIEWER_REF },
        normalize: {
            ignorePaths: ["$.data.user_party_group_list.*.party_list.*.party_id"],
        },
        branch: "success",
    },
    "POST /api/index.php/event/rush/battle/start": {
        body: {
            viewer_id: VIEWER_REF, quest_id: 700007001, party_id: 1,
            is_auto_start_mode: false, play_id: "${play_id}-rush",
        },
        probes: { after: [RUSH_BATTLE_ABORT_AFTER] },
        branch: "success",
    },
    "POST /api/index.php/event/rush/reset": {
        body: {
            viewer_id: VIEWER_REF, event_id: 700007, quest_type: 0,
            reset_target_id: 1, is_reset_after_target_round: false,
        },
        branch: "success",
    },
    "POST /api/index.php/event/rush/reward": {
        body: { viewer_id: VIEWER_REF, event_id: 700007 }, branch: "success",
    },
    "POST /api/index.php/event/rush/endless_battle": {
        body: { viewer_id: VIEWER_REF, event_id: 700007 }, branch: "success",
    },
    "POST /api/index.php/history/practice_battle": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/history/score_attack_event_battle": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/ex_boost/draw": {
        body: { viewer_id: VIEWER_REF, character_id: 311001, cost_item_id: 14002 },
        sideRequests: [
            createMailSideRequest("grant-ex-character", { type: 5, typeId: 311001, number: 1 }, { characterList: [311001] }),
            createMailSideRequest("grant-ex-limit-items", { type: 1, typeId: 10001, number: 8 }, { itemList: { 10001: 8 } }),
            createMailSideRequest("grant-ex-draw-item", { type: 1, typeId: 14002, number: 1 }, { itemList: { 14002: 1 } }),
            ...createMailClaimSideRequests("claim-ex-draw-resources"),
            createGameSideRequest("over-limit-for-ex-draw", "/api/index.php/character/over_limit", {
                viewer_id: VIEWER_REF, character_id: 311001, over_limit_count: 8, use_stack: false, item_id: 10001,
            }),
        ],
        normalize: {
            paths: [
                "$.data.draw_result.status_id",
                "$.data.draw_result.ability_id_list.*",
            ],
        },
        branch: "success",
    },
    "POST /api/index.php/ex_boost/first_draw": {
        body: { viewer_id: VIEWER_REF, character_id: 311001, cost_item_id: 14002 },
        sideRequests: [
            createMailSideRequest("grant-first-ex-character", { type: 5, typeId: 311001, number: 1 }, { characterList: [311001] }),
            createMailSideRequest("grant-first-ex-limit-items", { type: 1, typeId: 10001, number: 8 }, { itemList: { 10001: 8 } }),
            createMailSideRequest("grant-first-ex-draw-item", { type: 1, typeId: 14002, number: 1 }, { itemList: { 14002: 1 } }),
            ...createMailClaimSideRequests("claim-first-ex-resources"),
            createGameSideRequest("over-limit-for-first-ex", "/api/index.php/character/over_limit", {
                viewer_id: VIEWER_REF, character_id: 311001, over_limit_count: 8, use_stack: false, item_id: 10001,
            }),
        ],
        normalize: {
            paths: [
                "$.data.character_list.*.ex_boost.status_id",
                "$.data.character_list.*.ex_boost.ability_id_list.*",
                "$.data.user_character_list.*.ex_boost.status_id",
                "$.data.user_character_list.*.ex_boost.ability_id_list.*",
            ],
        },
        branch: "success",
    },
    "POST /api/index.php/ex_boost/select": {
        body: { viewer_id: VIEWER_REF, is_confirm: true },
        sideRequests: [
            createMailSideRequest("grant-select-ex-character", { type: 5, typeId: 311001, number: 1 }, { characterList: [311001] }),
            createMailSideRequest("grant-select-ex-limit-items", { type: 1, typeId: 10001, number: 8 }, { itemList: { 10001: 8 } }),
            createMailSideRequest("grant-select-ex-draw-item", { type: 1, typeId: 14002, number: 1 }, { itemList: { 14002: 1 } }),
            ...createMailClaimSideRequests("claim-select-ex-resources"),
            createGameSideRequest("over-limit-for-ex-select", "/api/index.php/character/over_limit", {
                viewer_id: VIEWER_REF, character_id: 311001, over_limit_count: 8, use_stack: false, item_id: 10001,
            }),
            createGameSideRequest("draw-for-ex-select", "/api/index.php/ex_boost/draw", {
                viewer_id: VIEWER_REF, character_id: 311001, cost_item_id: 14002,
            }),
        ],
        normalize: {
            paths: [
                "$.data.character_list.*.ex_boost.status_id",
                "$.data.character_list.*.ex_boost.ability_id_list.*",
                "$.data.user_character_list.*.ex_boost.status_id",
                "$.data.user_character_list.*.ex_boost.ability_id_list.*",
            ],
        },
        branch: "success",
    },
    "POST /api/index.php/expod/inject_exp": {
        body: { viewer_id: VIEWER_REF, character_id: 1, exp: 76262 },
        sideRequests: [
            createMailSideRequest("grant-inject-exp", { type: 9, number: 76262 }, { expPool: 76262 }),
            ...createMailClaimSideRequests("claim-inject-exp"),
        ],
        branch: "success",
    },
    "POST /api/index.php/expod/stack_to_exp": {
        body: { viewer_id: VIEWER_REF, character_id: 141165, number: 1 },
        stateIgnorePaths: ["$.data.user_character_list.141165.stack"],
        sideRequests: [
            createGameSideRequest("grant-stack-character-1", "/api/index.php/character/add_character_from_town", {
                viewer_id: VIEWER_REF, character_id: 141165,
            }),
            createGameSideRequest("grant-stack-character-2", "/api/index.php/character/add_character_from_town", {
                viewer_id: VIEWER_REF, character_id: 141165,
            }),
        ],
        branch: "success",
    },
    "POST /api/index.php/news/index": {
        body: { viewer_id: VIEWER_REF, page_index: 1 }, branch: "success",
    },
    "POST /api/index.php/news/get_info": {
        body: { viewer_id: VIEWER_REF, news_id: 1 }, branch: "success",
    },
    "POST /api/index.php/news/system_index": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/news/get_system_info": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/news/latest_forced": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/news/latest_forced_system": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/party/check_word": {
        body: { viewer_id: VIEWER_REF, word: "Differential" }, branch: "success",
    },
    "POST /api/index.php/party/publish": {
        body: { viewer_id: VIEWER_REF },
        comparison: { ignorePaths: ["$.data.party_code"] },
        stateExpectation: "unchanged",
        branch: "success",
    },
    "POST /api/index.php/profile/get_my_profile": {
        comparison: {
            ignorePaths: [
                "$.data.profile_info.max_owned_character_count",
                "$.data.profile_info.owned_character_count",
            ],
        },
    },
    "POST /api/index.php/profile/get_last_login_region": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/profile/get_degree_list": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/profile/update_degree": {
        body: { viewer_id: VIEWER_REF, degree_id: 1 }, branch: "success",
    },
    "POST /api/index.php/profile/update_profile_settings": {
        body: { viewer_id: VIEWER_REF, profile_settings: {} }, branch: "success",
    },
    "POST /api/index.php/profile/update_comment": {
        body: { viewer_id: VIEWER_REF, comment: "Differential corpus" }, branch: "success",
    },
    "POST /api/index.php/profile/rename": {
        body: { viewer_id: VIEWER_REF, name: "Differential" }, branch: "success",
    },
    "POST /api/index.php/quest/unlock": {
        body: { viewer_id: VIEWER_REF, category: 1, quest_id: 9001001 }, branch: "success",
    },
    "POST /api/index.php/ranking_event/get_summary": {
        body: { viewer_id: VIEWER_REF, ranking_event_id: 1 }, branch: "success",
    },
    "POST /api/index.php/ranking_event/receive_reward": {
        body: { viewer_id: VIEWER_REF, ranking_event_id: 1 }, branch: "success",
    },
    "POST /api/index.php/shop/get_campaign_lineup_id": {
        body: { viewer_id: VIEWER_REF }, branch: "success",
    },
    "POST /api/index.php/shop/set_campaign_lineup_id": {
        body: { viewer_id: VIEWER_REF, lineup_id: 1 }, branch: "success",
    },
    "POST /api/index.php/box_gacha/close": {
        body: { viewer_id: VIEWER_REF, box_gacha_id: 1001, box_id: 1 },
        branch: "success",
    },
    "POST /api/index.php/box_gacha/exec": {
        body: {
            viewer_id: VIEWER_REF,
            box_gacha_id: 1001,
            box_id: 1,
            number: 1,
            stop_on_featured_rewards: false,
        },
        sideRequests: [
            createMailSideRequest("grant-box-tokens", { type: 1, typeId: 70017, number: 10 }, { itemList: { 70017: 10 } }),
            ...createMailClaimSideRequests("claim-box-tokens"),
        ],
        comparison: {
            valuePaths: [
                "$.data.all_box_info",
                "$.data.drawn_reward_list",
                "$.data.item_list.70017",
                "$.data.mail_arrived",
            ],
        },
        normalize: {
            paths: [
                "$.data.all_box_info.*.all_drawn_reward_list.*.reward_id",
                "$.data.drawn_reward_list.*.reward_id",
            ],
        },
        stateProjection: {
            valuePaths: [
                "$.data.box_gacha_list.1001[0].remaining_number",
                "$.data.item_list.70017",
            ],
        },
        branch: "success",
    },
    "POST /api/index.php/gacha/exchange_character": {
        body: { viewer_id: VIEWER_REF, gacha_id: 157, character_id: 151153 },
        rust: { body: { viewer_id: VIEWER_REF, gacha_id: 1635, character_id: 121141 } },
        sideRequests: [
            createMailSideRequest("grant-character-exchange-tickets", { type: 1, typeId: 999001, number: 25 }, { itemList: { 999001: 25 } }),
            ...createMailClaimSideRequests("claim-character-exchange-tickets"),
            createGameSideRequest("draw-for-character-exchange", "/api/index.php/gacha/exec", {
                viewer_id: VIEWER_REF, gacha_id: 157, payment_type: 3, type: 9, number_of_exec: 25,
            }, {
                rustBody: {
                    viewer_id: VIEWER_REF, gacha_id: 1635, payment_type: 3, type: 9, number_of_exec: 25,
                },
                timeoutMs: 120_000,
            }),
            createMailSideRequest(
                "grant-character-for-exchange",
                { type: 5, typeId: 151153, number: 1 },
                { characterList: [121141] },
            ),
            ...createMailClaimSideRequests("claim-character-for-exchange"),
        ],
        comparison: {
            valuePaths: [
                "$.data.character_list[0].character_id",
                "$.data.character_list[0].entry_count",
                "$.data.character_list[0].evolution_level",
                "$.data.gacha_info_list[0].gacha_exchange_point",
                "$.data.mail_arrived",
            ],
        },
        normalize: {
            paths: ["$.data.character_list[0].character_id", "$.data.gacha_info_list[0].gacha_id"],
            zeroBaselinePaths: ["$.data.item_list.14018"],
        },
        stateComparison: "change-presence",
        branch: "success",
    },
    "POST /api/index.php/gacha/exchange_equipment": {
        body: { viewer_id: VIEWER_REF, gacha_id: 157, equipment_id: 5030037 },
        rust: { body: { viewer_id: VIEWER_REF, gacha_id: 25030, equipment_id: 5070036 } },
        sideRequests: [
            createMailSideRequest("grant-equipment-exchange-tickets", { type: 1, typeId: 999001, number: 25 }, { itemList: { 999004: 25 } }),
            ...createMailClaimSideRequests("claim-equipment-exchange-tickets"),
            createGameSideRequest("draw-for-equipment-exchange", "/api/index.php/gacha/exec", {
                viewer_id: VIEWER_REF, gacha_id: 157, payment_type: 3, type: 9, number_of_exec: 25,
            }, {
                rustBody: {
                    viewer_id: VIEWER_REF, gacha_id: 25030, payment_type: 3, type: 13, number_of_exec: 25,
                },
                timeoutMs: 120_000,
            }),
        ],
        normalize: {
            paths: ["$.data.equipment_list.*.equipment_id", "$.data.gacha_info_list[0].gacha_id"],
        },
        comparison: {
            ignorePaths: [
                "$.data.equipment_list.*.stack",
                "$.data.gacha_info_list.*.crazy_draw_count",
                "$.data.gacha_info_list.*.daily_one_count",
                "$.data.gacha_info_list.*.daily_ten_count",
                "$.data.gacha_info_list.*.is_daily_first",
            ],
        },
        stateComparison: "change-presence",
        branch: "success",
    },
    "POST /api/index.php/exchange/star_crumb": {
        body: { viewer_id: VIEWER_REF, exchange_id: 9000001 },
        sideRequests: [
            createMailSideRequest("grant-star-crumb", { type: 7, number: 300 }, { starCrumb: 300 }),
            ...createMailClaimSideRequests("claim-star-crumb"),
        ],
        branch: "success",
    },
    "POST /api/index.php/item/sell": {
        body: { viewer_id: VIEWER_REF, item_id: 1, sell_number: 1 },
        sideRequests: [
            createMailSideRequest("grant-sell-item", { type: 1, typeId: 1, number: 1 }, { itemList: { 1: 1 } }),
            ...createMailClaimSideRequests("claim-sell-item"),
        ],
        branch: "success",
    },
    "POST /api/index.php/item/use_item": {
        body: { viewer_id: VIEWER_REF, items: [{ id: 100, number: 1 }] },
        sideRequests: [
            createMailSideRequest("grant-use-item", { type: 1, typeId: 100, number: 1 }, { itemList: { 100: 1 } }),
            ...createMailClaimSideRequests("claim-use-item"),
        ],
        comparison: { ignorePaths: ["$.data.user_info.stamina"] },
        branch: "success",
    },
    "POST /api/index.php/mail/receive": {
        body: { viewer_id: VIEWER_REF, mail_id: { $ref: "mail-index.data.mail.0.id" } },
        sideRequests: [
            createMailSideRequest("grant-single-mail", { type: 1, typeId: 1, number: 1 }, { itemList: { 1: 1 } }),
            {
                id: "mail-index",
                requireDecodedResponse: true,
                reference: {
                    method: "POST", path: "/api/index.php/mail/index", encoding: "base64-messagepack",
                    body: { viewer_id: VIEWER_REF, current_page: 1 }, expectedStatus: 200,
                },
                rust: {
                    method: "POST", path: "/api/index.php/mail/index", encoding: "base64-messagepack",
                    body: { viewer_id: VIEWER_REF, current_page: 1 }, expectedStatus: 200,
                },
            },
        ],
        branch: "success",
    },
    "POST /api/index.php/tutorial/finish_trigger": {
        body: { viewer_id: VIEWER_REF, tutorial_ids: [12] },
        dependsOn: ["tutorial-update-step-skip-finish"],
        stateProjection: { valuePaths: ["$.data.user_tutorial"] },
        branch: "success",
    },
    "POST /api/index.php/tutorial/update_step": {
        evidence: ["scripts/protocol-lab/test-cn-server.js:2063", "scripts/protocol-lab/ios_cn_game_scenario_stages.py:319"],
        branch: "success",
        variants: [
            {
                id: "tutorial-update-step-skip-start",
                body: { retry_count: 1, api_count: 1, viewer_id: VIEWER_REF, skip: true, step: 0 },
                branchText: "else {",
            },
            {
                id: "tutorial-update-step-skip-gacha",
                body: {
                    retry_count: 1, api_count: 2, viewer_id: VIEWER_REF, skip: true, step: 3, gacha_id: 1,
                },
                dependsOn: ["tutorial-update-step-skip-start"],
                branchText: "if (nextStep === 15",
                normalize: {
                    paths: [
                        "$.data.character_list.*.character_id",
                        "$.data.gacha.draw.*.character_id",
                    ],
                },
                stateProjection: {
                    valuePaths: [
                        "$.data.user_info.free_vmoney",
                        "$.data.user_tutorial.tutorial_step",
                    ],
                },
            },
            {
                id: "tutorial-update-step-skip-finish",
                body: { retry_count: 1, api_count: 3, viewer_id: VIEWER_REF, skip: true, step: 4 },
                dependsOn: ["tutorial-update-step-skip-gacha"],
                branchText: "else if (nextStep === 16)",
                stateProjection: {
                    valuePaths: [
                        "$.data.mail_arrived",
                        "$.data.user_character_list.243001",
                        "$.data.user_info.free_vmoney",
                        "$.data.user_tutorial",
                    ],
                },
            },
        ],
    },
    "POST /api/index.php/single_battle_quest/start": {
        body: {
            viewer_id: VIEWER_REF, api_count: 1, quest_id: 1001002, use_boss_boost_point: false,
            use_boost_point: false, category: 1, play_id: "${play_id}", is_auto_start_mode: false, party_id: 1,
        },
        stateIgnorePaths: [
            "$.data.unfinished_quest_list",
            "$.data.user_info.stamina",
            "$.data.user_info.total_stamina_used",
        ],
        comparison: { ignorePaths: ["$.data.user_info.stamina"] },
        evidence: ["scripts/protocol-lab/test-cn-server.js:526", "scripts/protocol-lab/ios_cn_gameplay_scenario_stages.py:52"],
        branch: "success",
    },
    "POST /api/index.php/single_battle_quest/play_continue": {
        body: { viewer_id: VIEWER_REF, api_count: 1, payment_type: 0, quest_id: 1001002, paly_id: "${play_id}", category: 1 },
        dependsOn: ["single-battle-quest-start"], evidence: ["scripts/protocol-lab/test-cn-server.js:559"], branch: "success",
        stateIgnorePaths: ["$.data.unfinished_quest_list"],
    },
    "POST /api/index.php/single_battle_quest/finish": {
        body: {
            viewer_id: VIEWER_REF, api_count: 1, is_restored: false, continue_count: 0, elapsed_time_ms: 100000,
            quest_id: 1001002, play_id: "${play_id}", category: 1, score: 1000, add_mana: 7,
            is_accomplished: true,
            statistics: {
                clear_phase: 1,
                party: {
                    characters: [{ id: 1 }, null, null], unison_characters: [null, null, null],
                    equipments: [null, null, null], ability_soul_ids: [null, null, null],
                },
            },
        },
        dependsOn: ["single-battle-quest-play-continue"],
        comparison: {
            ignorePaths: [
                "$.data.drop_rare_reward_ids",
                "$.data.equipment_list",
                "$.data.user_info.stamina",
            ],
        },
        stateIgnorePaths: [
            "$.data.unfinished_quest_list",
            "$.data.user_equipment_list",
        ],
        evidence: ["scripts/protocol-lab/test-cn-server.js:535", "scripts/protocol-lab/ios_cn_gameplay_scenario_stages.py:80"],
        branch: "success",
    },
    "POST /api/index.php/single_battle_quest/abort": {
        body: {
            viewer_id: VIEWER_REF, api_count: 1, finish_kind: 1, quest_id: 1001002, play_id: "${play_id}", category: 1,
            statistics: {
                clear_phase: 0,
                party: {
                    characters: [{ id: 1 }, null, null], unison_characters: [null, null, null],
                    equipments: [null, null, null], ability_soul_ids: [null, null, null],
                },
            },
        },
        evidence: ["scripts/protocol-lab/test-cn-server.js:599"], branch: "success",
    },
    "POST /api/index.php/gacha/exec": {
        prerequisites: [TUTORIAL_SKIP_SETUP],
        normalize: {
            paths: [
                "$.data.gacha_campaign_list.*.campaign_id",
                "$.data.gacha_campaign_list.*.gacha_id",
                "$.data.gacha_info_list.*.gacha_id",
            ],
            ignorePaths: [
                "$.data.character_list.*.character_id",
                "$.data.draw.*.character_id",
                "$.data.draw.*.movie_id",
                "$.data.draw.*.seed",
                "$.data.draw_equipment.*.equipment_id",
                "$.data.equipment_list.*.equipment_id",
            ],
        },
        comparison: {
            ignorePaths: [
                "$.data.gacha_info_list.*.crazy_draw_count",
                "$.data.gacha_info_list.*.daily_one_count",
                "$.data.gacha_info_list.*.daily_ten_count",
                "$.data.gacha_info_list.*.is_daily_first",
            ],
        },
        stateComparison: "change-presence",
        stateProjection: {
            valuePaths: [
                "$.data.gacha_info_list",
                "$.data.user_info.free_vmoney",
                "$.data.user_info.vmoney",
            ],
        },
        evidence: ["scripts/protocol-lab/test-cn-server.js:1317", "scripts/protocol-lab/ios_cn_gameplay_scenario_stages.py:178"],
        branch: "success",
        variants: [
            {
                id: "gacha-exec-free-vmoney-single",
                body: {
                    api_count: 1, payment_type: 1, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1, type: 1,
                },
                rust: { body: {
                    api_count: 1, payment_type: 1, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1635, type: 1,
                } },
                branchText: "case GachaPaymentType.FREE_VMONEY",
            },
            {
                id: "gacha-exec-free-vmoney-multi",
                body: {
                    api_count: 2, payment_type: 1, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 2, type: 2,
                },
                rust: { body: {
                    api_count: 2, payment_type: 1, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1635, type: 2,
                } },
                sideRequests: [
                    createMailSideRequest(
                        "grant-gacha-free-vmoney-multi",
                        { type: 4, number: 1_500 },
                        { freeVmoney: 1_500 },
                    ),
                    ...createMailClaimSideRequests("claim-gacha-free-vmoney-multi"),
                ],
                comparison: {
                    ignorePaths: [
                        "$.data.character_list",
                        "$.data.draw.*.ex_boost_item",
                        "$.data.item_list",
                    ],
                },
                branchText: "case GachaPaymentType.FREE_VMONEY",
            },
            {
                id: "gacha-exec-paid-vmoney-daily",
                body: {
                    api_count: 3, payment_type: 2, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 4, type: 5,
                },
                rust: { body: {
                    api_count: 3, payment_type: 2, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1635, type: 5,
                } },
                sideRequests: [
                    createMailSideRequest(
                        "grant-gacha-paid-vmoney-daily",
                        { type: 3, number: 100 },
                        { vmoney: 100 },
                    ),
                    ...createMailClaimSideRequests("claim-gacha-paid-vmoney-daily"),
                ],
                branchText: "case GachaPaymentType.VMONEY",
            },
            {
                id: "gacha-exec-ticket-character-multi",
                body: {
                    api_count: 4, payment_type: 3, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 2, type: 9,
                },
                rust: { body: {
                    api_count: 4, payment_type: 3, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1635, type: 9,
                } },
                sideRequests: [
                    createMailSideRequest(
                        "grant-gacha-character-multi-ticket",
                        { type: 1, typeId: 999001, number: 1 },
                        { itemList: { 999001: 1 } },
                    ),
                    ...createMailClaimSideRequests("claim-gacha-character-multi-ticket"),
                ],
                comparison: {
                    ignorePaths: [
                        "$.data.character_list",
                        "$.data.draw.*.ex_boost_item",
                        ...Array.from(
                            { length: 18 },
                            (_, index) => `$.data.item_list.${14_001 + index}`,
                        ),
                    ],
                },
                branchText: "case GachaPaymentType.TICKET",
            },
            {
                id: "gacha-exec-ticket-character-single",
                body: {
                    api_count: 5, payment_type: 3, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 2, type: 10,
                },
                rust: { body: {
                    api_count: 5, payment_type: 3, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1635, type: 10,
                } },
                sideRequests: [
                    createMailSideRequest(
                        "grant-gacha-character-single-ticket",
                        { type: 1, typeId: 999003, number: 1 },
                        { itemList: { 999003: 1 } },
                    ),
                    ...createMailClaimSideRequests("claim-gacha-character-single-ticket"),
                ],
                branchText: "case GachaPaymentType.TICKET",
            },
            {
                id: "gacha-exec-ticket-equipment-multi",
                body: {
                    api_count: 6, payment_type: 3, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 3, type: 13,
                },
                rust: { body: {
                    api_count: 6, payment_type: 3, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 25030, type: 13,
                } },
                sideRequests: [
                    createMailSideRequest(
                        "grant-gacha-equipment-multi-ticket",
                        { type: 1, typeId: 999004, number: 1 },
                        { itemList: { 999004: 1 } },
                    ),
                    ...createMailClaimSideRequests("claim-gacha-equipment-multi-ticket"),
                ],
                comparison: { ignorePaths: ["$.data.equipment_list"] },
                branchText: "case GachaPaymentType.TICKET",
            },
            {
                id: "gacha-exec-campaign-single",
                body: {
                    api_count: 7, payment_type: 4, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 28, type: 7,
                },
                rust: { body: {
                    api_count: 7, payment_type: 4, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1635, type: 11,
                } },
                branchText: "case GachaPaymentType.CAMPAIGN",
            },
            {
                id: "gacha-exec-campaign-multi",
                body: {
                    api_count: 8, payment_type: 4, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 29, type: 8,
                },
                rust: { body: {
                    api_count: 8, payment_type: 4, number_of_exec: 1,
                    viewer_id: VIEWER_REF, gacha_id: 1635, type: 8,
                } },
                comparison: {
                    ignorePaths: [
                        "$.data.character_list",
                        "$.data.draw.*.ex_boost_item",
                        "$.data.item_list",
                    ],
                },
                branchText: "case GachaPaymentType.CAMPAIGN",
            },
        ],
    },
    "POST /api/index.php/story_quest/finish": {
        body: { party_id: 1, quest_id: 1003001, viewer_id: VIEWER_REF, category: 1 },
        evidence: ["scripts/protocol-lab/test-cn-server.js:1539"], branch: "success",
    },
    "POST /api/index.php/story_quest/finish_with_skip": {
        body: { party_id: 1, quest_id: 1003001, viewer_id: VIEWER_REF, category: 1 },
        evidence: ["scripts/protocol-lab/test-cn-server.js:1539"], branch: "success",
    },
    "POST /api/index.php/mission/get_mission_progress": {
        body: { api_count: 1, viewer_id: VIEWER_REF, category_list: [{ category: 5 }] },
        targetOverride: {
            kind: "reference-defect",
            reason: "目标 CN 响应包含数据库邮件到达状态; launcher 编译产物省略该字段.",
            evidence: ["starpoint-capture-history/docs/routes/mission_get_mission_progress.md:693"],
        },
        evidence: [
            "scripts/protocol-lab/test-cn-server.js:1746",
            "starpoint-capture-history/docs/routes/mission_get_mission_progress.md:693",
        ],
        branch: "success",
    },
    "POST /api/index.php/mission/update_mission_progress": {
        body: {
            api_count: 2, viewer_id: VIEWER_REF,
            mission_param_list: [{ progress_value: 1, mission_pattern: "home_tap_town_character_count" }],
        },
        targetOverride: {
            kind: "reference-defect",
            reason: "目标 CN 响应包含数据库邮件到达状态; launcher 编译产物省略该字段.",
            evidence: ["starpoint-capture-history/docs/routes/mission_update_mission_progress.md:81"],
        },
        evidence: [
            "scripts/protocol-lab/test-cn-server.js:1759",
            "starpoint-capture-history/docs/routes/mission_update_mission_progress.md:81",
        ],
        branch: "success",
    },
    "POST /api/index.php/option/update": {
        body: { api_count: 1, viewer_id: VIEWER_REF, option_params: { stamina: true } },
        evidence: ["scripts/protocol-lab/test-cn-server.js:1819"], branch: "success",
    },
    "POST /api/index.php/payment/item_list": {
        body: { api_count: 1, viewer_id: VIEWER_REF }, evidence: ["scripts/protocol-lab/test-cn-server.js:1826"], branch: "success",
    },
    "POST /api/index.php/shop/recover_stamina": {
        body: { api_count: 1, viewer_id: VIEWER_REF },
        comparison: { ignorePaths: ["$.data.user_info.stamina"] },
        stateIgnorePaths: ["$.data.user_info.stamina"],
        evidence: ["scripts/protocol-lab/test-cn-server.js:1429"], branch: "success",
    },
    "POST /api/index.php/channels/channel_leiting_pay/query_unfinish_order": {
        body: { viewer_id: VIEWER_REF }, evidence: ["scripts/protocol-lab/test-cn-server.js:2180"], branch: "success",
    },
    "POST /api/index.php/mail/index": {
        body: { viewer_id: VIEWER_REF, current_page: 1 }, evidence: ["scripts/protocol-lab/test-cn-server.js:1659"], branch: "success",
    },
    "POST /api/index.php/mail/receive_all": {
        body: { viewer_id: VIEWER_REF, mail_ids: [] }, evidence: ["scripts/protocol-lab/test-cn-server.js:1669"], branch: "success",
    },
    "POST /api/index.php/party/edit": {
        body: {
            api_count: 1, viewer_id: VIEWER_REF, ignore_ngword: true, main_party_id: 1, use_party_group_edit: false,
            party_info_list: [{
                party_id: 1, party_category: 1, party_edited: true, party_name: "Party A",
                character_ids: [1, null, null], unison_character_ids: [null, null, null],
                equipment_ids: [null, null, null], ability_soul_ids: [null, null, null],
                options: { allow_other_players_to_heal_me: true },
            }],
        },
        evidence: ["starpoint-capture-history/docs/routes/party_edit.md:35"], branch: "success",
    },
    "POST /api/index.php/party_group/edit": {
        body: { viewer_id: VIEWER_REF, party_group_edit_params_list: [] },
        evidence: ["starpoint-capture-history/docs/routes/party_group_edit.md:35"], branch: "success",
    },
    "POST /api/index.php/active_mission/receive": {
        body: { viewer_id: VIEWER_REF, active_mission_list: [] },
        evidence: ["starpoint-capture-history/docs/routes/active_mission_receive.md:35"], branch: "success",
    },
    "POST /api/index.php/character/bulk_over_limit": {
        body: { viewer_id: VIEWER_REF, character_list: [] }, branch: "success",
    },
    "POST /api/index.php/equipment/bulk_upgrade": {
        body: { viewer_id: VIEWER_REF, equipment_ids: [999999] }, branch: "success",
    },
    "POST /api/index.php/expod/bulk_stack_to_exp": {
        body: { viewer_id: VIEWER_REF, character_ids: [] }, branch: "success",
    },
    "POST /api/index.php/shop/buy": {
        comparison: {
            ignorePaths: [
                "$.data.joined_character_id_list",
                "$.data.user_info.exp_pooled_time",
            ],
        },
        targetOverride: {
            kind: "reference-defect",
            reason: "目标响应包含购物主动任务进度增量; launcher 缺少 active_mission_list.",
            evidence: ["starpoint-capture-history/docs/routes/shop_buy.md:71"],
        },
    },
    "POST /api/index.php/shop/bulk_buy": {
        body: { viewer_id: VIEWER_REF, shop_item_list: [] }, branch: "success",
    },
})
// //// /定义语料生成配置 ////

// //// 读取命令行选项 [@x380kkm 2026-08-23] ////
function readOptions(args) {
    const option = (name, fallback) => {
        const index = args.indexOf(name)
        if (index < 0) return fallback
        if (!args[index + 1] || args[index + 1].startsWith("--")) throw new Error(`${name} requires a value`)
        return path.resolve(args[index + 1])
    }
    return {
        referenceRoot: option("--reference-root", path.join(WORKSPACE_ROOT, "startpoint-cn-launcher/resources/server")),
        captureRoot: option("--capture-root", path.join(WORKSPACE_ROOT, "starpoint-capture-history/docs/routes")),
        decompiledRoot: option("--decompiled-root", path.join(WORKSPACE_ROOT, "wf-2.1.125-cn-decompiled/scripts/scripts")),
        outputPath: option("--output", OUTPUT_PATH),
        check: args.includes("--check"),
    }
}

// //// 枚举目录文件 [@x380kkm 2026-08-23] ////
function collectFiles(root, extension) {
    const files = []
    if (!existsSync(root)) return files
    for (const entry of readdirSync(root, { withFileTypes: true })) {
        const entryPath = path.join(root, entry.name)
        if (entry.isDirectory()) files.push(...collectFiles(entryPath, extension))
        else if (entry.isFile() && entry.name.endsWith(extension)) files.push(entryPath)
    }
    return files.sort()
}

// //// 读取参考路由清单 [@x380kkm 2026-08-23] ////
function readReferenceRoutes(referenceRoot) {
    const result = spawnSync(
        process.execPath,
        [AUDITOR_PATH, "--reference-root", referenceRoot, "--routes-only", "--report-only"],
        {
            cwd: REPOSITORY_ROOT,
            encoding: "utf8",
        },
    )
    if (result.status !== 0) throw new Error(result.stderr || result.stdout || "reference route audit failed")
    const report = JSON.parse(result.stdout)
    return [...report.covered.routes, ...report.missing.routes]
        .map(({ method, path: routePath, source }) => ({ method, path: routePath, source }))
        .sort((left, right) => left.method.localeCompare(right.method) || left.path.localeCompare(right.path))
}

// //// 解析真实抓包请求 [@x380kkm 2026-08-23] ////
function readCaptureCases(captureRoot) {
    const cases = new Map()
    for (const file of collectFiles(captureRoot, ".md")) {
        const source = readFileSync(file, "utf8")
        const heading = source.match(/^#\s+(\S+)/m)
        const bodyMatch = source.match(/## Request[\s\S]*?### Body\s*```(?:json)?\s*([\s\S]*?)```/i)
        if (!heading || !bodyMatch) continue
        const routePath = heading[1].replace(/^\/latest(?=\/)/, "")
        if (!routePath.startsWith("/api/index.php/")) continue
        try {
            const body = normalizeDynamicValues(JSON.parse(bodyMatch[1]))
            const line = source.slice(0, bodyMatch.index + bodyMatch[0].indexOf(bodyMatch[1])).split(/\r?\n/).length
            const relative = path.relative(WORKSPACE_ROOT, file).replaceAll(path.sep, "/")
            cases.set(`POST ${routePath}`, { body, evidence: `${relative}:${line}` })
        } catch {
            continue
        }
    }
    return cases
}

// //// 解析客户端 RealRemote 锚点 [@x380kkm 2026-08-23] ////
function readRealRemoteEvidence(decompiledRoot) {
    const evidence = new Map()
    for (const file of collectFiles(decompiledRoot, "RealRemote.as")) {
        const source = readFileSync(file, "utf8")
        for (const match of source.matchAll(/startUserRequest\(\s*["']([^"']+)["']/g)) {
            const routePath = `/api/index.php/${match[1]}`
            const line = source.slice(0, match.index).split(/\r?\n/).length
            const relative = path.relative(WORKSPACE_ROOT, file).replaceAll(path.sep, "/")
            const key = `POST ${routePath}`
            if (!evidence.has(key)) evidence.set(key, `${relative}:${line}`)
        }
    }
    return evidence
}

// //// 标准化动态请求值 [@x380kkm 2026-08-23] ////
function normalizeDynamicValues(value, key = "") {
    if (key === "viewer_id" || key === "keychain") return { ...VIEWER_REF }
    if (key === "play_id") {
        return typeof value === "string" && value.includes("${play_id}")
            ? value
            : "${play_id}"
    }
    if (Array.isArray(value)) return value.map((entry) => normalizeDynamicValues(entry))
    if (value && typeof value === "object") {
        return Object.fromEntries(Object.entries(value).map(([childKey, child]) => [childKey, normalizeDynamicValues(child, childKey)]))
    }
    if (typeof value === "string" && value === "<redacted>") return "differential-redacted-value"
    return value
}

// //// 定位参考 handler [@x380kkm 2026-08-23] ////
function locateHandlers(referenceRoot, routes) {
    const handlers = new Map()
    const grouped = new Map()
    for (const route of routes) {
        const sourceRoutes = grouped.get(route.source) ?? []
        sourceRoutes.push(route)
        grouped.set(route.source, sourceRoutes)
    }
    for (const [sourcePath, sourceRoutes] of grouped) {
        const absolutePath = path.join(referenceRoot, sourcePath)
        const source = readFileSync(absolutePath, "utf8")
        const registrations = [...source.matchAll(/fastify\.(get|post)\(\s*([`"'])(.*?)\2/g)].map((match, index, all) => ({
            method: match[1].toUpperCase(),
            literal: match[3].replaceAll("${apiPrefix}", "/api/index.php"),
            index: match.index,
            end: all[index + 1]?.index ?? source.length,
            line: source.slice(0, match.index).split(/\r?\n/).length,
        }))
        const used = new Set()
        for (const route of [...sourceRoutes].sort((left, right) => right.path.length - left.path.length)) {
            const candidates = registrations
                .map((registration, index) => ({ registration, index }))
                .filter(({ registration, index }) => !used.has(index) && registration.method === route.method &&
                    (route.path === registration.literal || route.path.endsWith(registration.literal)))
                .sort((left, right) => right.registration.literal.length - left.registration.literal.length)
            if (candidates.length === 0) throw new Error(`cannot locate reference handler: ${route.method} ${route.path}`)
            used.add(candidates[0].index)
            const registration = candidates[0].registration
            handlers.set(`${route.method} ${route.path}`, {
                sourcePath,
                source: source.slice(registration.index, registration.end),
                line: registration.line,
            })
        }
    }
    return handlers
}

// //// 推导 handler 输入字段 [@x380kkm 2026-08-23] ////
function inferBody(handlerSource) {
    const fields = new Set([...handlerSource.matchAll(/\bbody\.([A-Za-z_$][\w$]*)/g)].map((match) => match[1]))
    const body = {}
    for (const field of [...fields].sort()) body[field] = inferFieldValue(field)
    return body
}

// //// 推导字段的可序列化值 [@x380kkm 2026-08-23] ////
function inferFieldValue(field) {
    if (Object.hasOwn(FIELD_VALUES, field)) return structuredClone(FIELD_VALUES[field])
    if (field.endsWith("_list") || field.endsWith("_ids")) return []
    if (field.startsWith("is_") || field.startsWith("use_") || field.startsWith("enable_") || field === "protection") return false
    if (field.endsWith("_id") || field.endsWith("_count") || field.endsWith("_index") || field.endsWith("_level")) return 1
    if (field.includes("name")) return "Differential"
    if (field.includes("comment")) return "Differential corpus"
    return 0
}

// //// 生成稳定 case 标识 [@x380kkm 2026-08-23] ////
function caseId(route, override) {
    if (override?.id) return override.id
    const withoutApiPrefix = route.path.replace(/^\/api\/index\.php\//, "")
    return `${route.method === "GET" ? "get-" : ""}${withoutApiPrefix}`
        .replace(/[^A-Za-z0-9]+/g, "-")
        .replace(/^-|-$/g, "")
        .toLowerCase()
}

// //// 选择预期分支源码锚点 [@x380kkm 2026-08-23] ////
function branchSource(handler, status, referenceRoot, branchText) {
    const lines = handler.source.split(/\r?\n/)
    let offset = branchText === undefined ? -1 : lines.findIndex((line) => line.includes(branchText))
    if (branchText !== undefined && offset < 0) {
        throw new Error(`cannot locate reference branch text: ${branchText}`)
    }
    const statusPattern = new RegExp(`reply\\.(?:status|code)\\(\\s*${status}\\s*\\)`)
    if (offset < 0) offset = lines.findIndex((line) => statusPattern.test(line))
    if (offset < 0 && status === 200) {
        offset = lines.findIndex((line) => /stubMsgpackReply|sendCharacterResponse|reply\.send\(/.test(line))
    }
    const relative = path.relative(WORKSPACE_ROOT, path.join(referenceRoot, handler.sourcePath)).replaceAll(path.sep, "/")
    return `${relative}:${handler.line + Math.max(0, offset)}`
}

// //// 识别可执行错误状态 [@x380kkm 2026-08-23] ////
function firstErrorStatus(handlerSource) {
    const match = handlerSource.match(/reply\.(?:status|code)\(\s*(4\d\d)\s*\)/)
    return match ? Number(match[1]) : null
}

// //// 识别状态变更路由 [@x380kkm 2026-08-23] ////
function isStateful(routePath) {
    const parts = routePath.split("/").filter(Boolean)
    return STATEFUL_TERMINALS.has(parts.at(-1))
}

// //// 构造状态探针 [@x380kkm 2026-08-23] ////
function stateProbe() {
    return {
        method: "POST",
        path: "/api/index.php/load",
        encoding: "base64-messagepack",
        body: { keychain: VIEWER_REF, viewer_id: VIEWER_REF },
    }
}

// //// 构造单条差分 case [@x380kkm 2026-08-23] ////
function buildCase(route, handler, capture, realRemote, referenceRoot, override) {
    const key = `${route.method} ${route.path}`
    const errorStatus = firstErrorStatus(handler.source)
    const hasSuccessEvidence = override.branch === "success" || capture !== undefined || errorStatus === null
    const branchKind = override.branch ?? (hasSuccessEvidence ? "success" : "error")
    const status = override.status ?? (branchKind === "success" ? 200 : errorStatus ?? 400)
    const inferredBody = route.method === "GET" ? null : inferBody(handler.source)
    const selectedBody = override.body ?? capture?.body ?? inferredBody
    const body = route.method === "GET"
        ? null
        : branchKind === "error" ? {} : normalizeDynamicValues(structuredClone(selectedBody))
    const source = branchSource(handler, status, referenceRoot, override.branchText)
    const evidence = [...new Set([
        ...(override.evidence ?? []),
        capture?.evidence,
        realRemote,
        source,
    ].filter(Boolean))]
    const dependencies = new Set(override.dependsOn ?? [])
    if (key !== "POST /api/index.php/tool/signup" && JSON.stringify(body).includes("tool-signup")) dependencies.add("tool-signup")
    const result = {
        id: caseId(route, override),
        method: route.method,
        path: route.path,
        encoding: override.encoding ?? (route.method === "GET" ? "none" : "base64-messagepack"),
        body,
        headers: override.headers ?? {},
        prerequisites: override.prerequisites ?? [],
        dependsOn: [...dependencies],
        branch: {
            kind: branchKind,
            status,
            source,
            ...(branchKind === "error" ? {
                executable: true,
                reason: "优先证据源没有给出可在全新玩家状态复现的成功前置条件; 此 case 执行参考 handler 的输入校验分支.",
            } : {}),
        },
        evidence,
    }
    if (override.capture) result.capture = override.capture
    if (override.reference) result.reference = structuredClone(override.reference)
    if (override.rust) result.rust = structuredClone(override.rust)
    if (override.query) result.query = override.query
    if (override.probes) result.probes = structuredClone(override.probes)
    if (override.comparison) result.comparison = structuredClone(override.comparison)
    if (override.normalize) result.normalize = structuredClone(override.normalize)
    if (override.sideRequests) result.sideRequests = structuredClone(override.sideRequests)
    if (override.stateIgnorePaths) result.stateIgnorePaths = [...override.stateIgnorePaths]
    if (override.stateComparison) result.stateComparison = override.stateComparison
    if (override.stateProjection) result.stateProjection = structuredClone(override.stateProjection)
    if (override.localExtension) result.localExtension = structuredClone(override.localExtension)
    if (override.targetOverride) result.targetOverride = structuredClone(override.targetOverride)
    if (isStateful(route.path) && dependencies.has("tool-signup") && key !== "POST /api/index.php/load") {
        result.probes = { ...(result.probes ?? {}), state: [stateProbe()] }
        if (!STAMINA_STATE_ROUTES.has(key)) {
            result.stateIgnorePaths = [
                ...new Set([...(result.stateIgnorePaths ?? []), "$.data.user_info.stamina"]),
            ]
        }
        result.stateExpectation = override.stateExpectation
            ?? (REFERENCE_UNCHANGED_STATE_ROUTES.has(key) ? "unchanged" : "changed")
    }
    return result
}

function buildCases(route, handler, capture, realRemote, referenceRoot) {
    const configured = CASE_OVERRIDES[`${route.method} ${route.path}`] ?? {}
    if (!Array.isArray(configured.variants)) {
        return [buildCase(route, handler, capture, realRemote, referenceRoot, configured)]
    }
    const { variants, ...shared } = configured
    return variants.map((variant) => {
        const merged = { ...shared, ...variant }
        if (shared.comparison || variant.comparison) {
            merged.comparison = {
                ...(shared.comparison ?? {}),
                ...(variant.comparison ?? {}),
                ignorePaths: [
                    ...(shared.comparison?.ignorePaths ?? []),
                    ...(variant.comparison?.ignorePaths ?? []),
                ],
            }
        }
        return buildCase(route, handler, capture, realRemote, referenceRoot, merged)
    })
}

// //// 选择可复现漫画查询 [@x380kkm 2026-08-23] ////
function addComicQuery(cases, referenceRoot) {
    const comicCase = cases.find((entry) => entry.method === "GET" && entry.path === "/api/index.php/comic/image")
    if (!comicCase) return
    const comicRoot = path.join(referenceRoot, "web/public/comic")
    for (const kind of ["0", "1"]) {
        const directory = path.join(comicRoot, kind)
        if (!existsSync(directory)) continue
        for (const filename of readdirSync(directory).sort()) {
            const match = filename.match(kind === "1" ? /第(\d+)课/ : /^第(\d+)话/)
            if (!match) continue
            comicCase.query = { kind: Number(kind), episode: Number(match[1]) }
            comicCase.branch.kind = "success"
            comicCase.branch.status = 200
            delete comicCase.branch.reason
            return
        }
    }
}

// //// 验证语料结构与路由集合 [@x380kkm 2026-08-23] ////
function validateCorpus(corpus, routes) {
    const expected = new Set(routes.map((route) => `${route.method} ${route.path}`))
    const actual = new Set()
    const ids = new Set()
    for (const entry of corpus.cases) {
        const key = `${entry.method} ${entry.path}`
        if (ids.has(entry.id)) throw new Error(`duplicate corpus id: ${entry.id}`)
        if (!ALLOWED_ENCODINGS.has(entry.encoding)) throw new Error(`unsupported encoding for ${key}: ${entry.encoding}`)
        if (!/^.+:\d+$/.test(entry.branch.source)) throw new Error(`missing branch source for ${key}`)
        if (entry.branch.kind === "error" && !entry.branch.reason) throw new Error(`missing error reason for ${key}`)
        if (entry.targetOverride !== undefined) {
            if (entry.targetOverride.kind !== "reference-defect"
                || typeof entry.targetOverride.reason !== "string"
                || entry.targetOverride.reason.length === 0
                || !Array.isArray(entry.targetOverride.evidence)
                || entry.targetOverride.evidence.length === 0
                || entry.targetOverride.evidence.some((source) => !/^.+:\d+$/.test(source))) {
                throw new Error(`invalid target override evidence for ${key}`)
            }
        }
        if (EQUIPMENT_SALE_PATHS.has(entry.path)) {
            const equipmentList = entry.body?.equipment_list
            if (!Array.isArray(equipmentList) || equipmentList.length === 0) {
                throw new Error(`equipment sale case requires a non-empty equipment_list: ${key}`)
            }
            const equipmentId = equipmentList[0]?.equipment_id
            if (!Number.isSafeInteger(equipmentId) || equipmentId <= 0) {
                throw new Error(`equipment sale case requires a valid equipment_id: ${key}`)
            }
            if (entry.path.endsWith("/sell_stack")
                && equipmentList.some((item) => !Number.isSafeInteger(item.number) || item.number <= 0)) {
                throw new Error(`equipment stack sale case requires a positive number: ${key}`)
            }
            const referenceBody = entry.reference?.body ?? entry.body
            const rustBody = entry.rust?.body ?? entry.body
            if (JSON.stringify(referenceBody) !== JSON.stringify(rustBody)) {
                throw new Error(`equipment sale case must use the same request body on both sides: ${key}`)
            }
            const responsePaths = new Set(entry.comparison?.valuePaths ?? [])
            const statePaths = new Set(entry.stateProjection?.valuePaths ?? [])
            for (const rewardPath of ["$.data.item_list.100000", `$.data.item_list.${equipmentId}`]) {
                if (!responsePaths.has(rewardPath) || !statePaths.has(rewardPath)) {
                    throw new Error(`equipment sale case must compare reward path ${rewardPath}: ${key}`)
                }
            }
            if (![...responsePaths].some((fieldPath) => fieldPath.startsWith("$.data.equipment_list"))
                || ![...statePaths].some((fieldPath) => fieldPath.startsWith(`$.data.user_equipment_list.${equipmentId}`))) {
                throw new Error(`equipment sale case must compare equipment inventory changes: ${key}`)
            }
            if (!Array.isArray(entry.sideRequests) || entry.sideRequests.length === 0
                || entry.stateExpectation !== "changed") {
                throw new Error(`equipment sale case requires inventory setup and a changed state: ${key}`)
            }
        }
        actual.add(key)
        ids.add(entry.id)
    }
    const missing = [...expected].filter((key) => !actual.has(key))
    const extra = [...actual].filter((key) => !expected.has(key))
    if (missing.length > 0 || extra.length > 0) {
        throw new Error(`corpus route mismatch: missing=${JSON.stringify(missing)} extra=${JSON.stringify(extra)}`)
    }
    for (const entry of corpus.cases) {
        for (const dependency of entry.dependsOn) {
            if (!ids.has(dependency)) throw new Error(`unknown dependency for ${entry.id}: ${dependency}`)
        }
    }
    validateConfirmedResponseContracts(corpus.cases)
}

// //// 保持目标抓包确认的响应字段处于强比较范围 [@x380kkm 2026-08-25] ////
function validateConfirmedResponseContracts(cases) {
    for (const [key, contract] of Object.entries(CONFIRMED_RESPONSE_CONTRACTS)) {
        const [method, routePath] = key.split(" ", 2)
        const entry = cases.find((candidate) => candidate.method === method
            && candidate.path === routePath
            && candidate.branch.kind === "success")
        if (entry === undefined) throw new Error(`confirmed response contract has no success case: ${key}`)
        const hiddenPaths = [
            ...(entry.comparison?.ignorePaths ?? []),
            ...(entry.normalize?.ignorePaths ?? []),
        ]
        const projectedPaths = entry.comparison?.valuePaths ?? []
        for (const responsePath of contract.responsePaths) {
            if (hiddenPaths.some((hiddenPath) => hiddenPath === responsePath
                || responsePath.startsWith(`${hiddenPath}.`)
                || hiddenPath.endsWith(".*") && responsePath.startsWith(hiddenPath.slice(0, -1)))) {
                throw new Error(`confirmed response path is hidden for ${key}: ${responsePath}`)
            }
            if (projectedPaths.length > 0 && !projectedPaths.includes(responsePath)) {
                throw new Error(`confirmed response path is outside the comparison projection for ${key}: ${responsePath}`)
            }
        }
        if (contract.targetEvidence.length > 0) {
            const overrideEvidence = entry.targetOverride?.evidence ?? []
            if (entry.targetOverride?.kind !== "reference-defect"
                || contract.targetEvidence.some((evidence) => !overrideEvidence.includes(evidence))) {
                throw new Error(`confirmed target override is missing capture evidence for ${key}`)
            }
        }
    }
}
// //// /保持目标抓包确认的响应字段处于强比较范围 ////

// //// 按依赖关系排列 case [@x380kkm 2026-08-23] ////
function sortCasesByDependencies(cases) {
    const compare = (left, right) => {
        const rank = (entry) => entry.id === "tool-signup" ? 0 : entry.id === "load" ? 1 : 2
        return rank(left) - rank(right) || left.method.localeCompare(right.method) || left.path.localeCompare(right.path)
    }
    const pending = [...cases].sort(compare)
    const emitted = new Set()
    const result = []
    while (pending.length > 0) {
        const index = pending.findIndex((entry) => entry.dependsOn.every((dependency) => emitted.has(dependency)))
        if (index < 0) throw new Error(`cyclic corpus dependencies: ${pending.map((entry) => entry.id).join(", ")}`)
        const [entry] = pending.splice(index, 1)
        result.push(entry)
        emitted.add(entry.id)
    }
    return result
}

// //// 生成并写入规范语料 [@x380kkm 2026-08-23] ////
function main() {
    const options = readOptions(process.argv.slice(2))
    const routes = readReferenceRoutes(options.referenceRoot)
    const captures = readCaptureCases(options.captureRoot)
    const realRemoteEvidence = readRealRemoteEvidence(options.decompiledRoot)
    const handlers = locateHandlers(options.referenceRoot, routes)
    let cases = routes.flatMap((route) => {
        const key = `${route.method} ${route.path}`
        return buildCases(route, handlers.get(key), captures.get(key), realRemoteEvidence.get(key), options.referenceRoot)
    })
    addComicQuery(cases, options.referenceRoot)
    cases = sortCasesByDependencies(cases)
    const corpus = {
        version: 1,
        variables: { device_id: 18420260823, play_id: "cn-reference-differential-play" },
        cases,
    }
    validateCorpus(corpus, routes)
    const content = `${JSON.stringify(corpus, null, 2)}\n`
    if (options.check) {
        if (!existsSync(options.outputPath) || readFileSync(options.outputPath, "utf8") !== content) {
            throw new Error(`corpus is stale: ${options.outputPath}`)
        }
    } else {
        writeFileSync(options.outputPath, content, "utf8")
    }
    const successes = cases.filter((entry) => entry.branch.kind === "success").length
    process.stdout.write(`${JSON.stringify({
        routes: routes.length,
        cases: cases.length,
        successes,
        errors: cases.length - successes,
        output: options.outputPath,
    })}\n`)
}

main()
