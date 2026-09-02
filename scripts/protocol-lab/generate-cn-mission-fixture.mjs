// audience: internal
// # generate-cn-mission-fixture
//
// 该脚本从 CN master 资产生成个人服务使用的任务定义, 阶段和奖励数据.
// MissionPatternKind 和 QuestRangeReferenceIdKind 编码表对应客户端生成类读取的整数代码.
// --asset-root 指向包含 mission_*.json 和 cdndata/player_rank_full.json 的资产目录.
// --check 以只读方式比较目标文件.

import fs from "node:fs"
import path from "node:path"

const CATEGORY_SOURCES = [
    { category: 1, definition: "mission_regular", stages: "mission_regular_reward", patternIndex: 2, targetIndex: 1, questKindIndex: 7, missions: 120, stageCount: 568, rewards: 0 },
    { category: 2, definition: "mission_daily", stages: "mission_daily_reward", patternIndex: 2, targetIndex: 1, questKindIndex: 7, missions: 656, stageCount: 656, rewards: 0 },
    { category: 3, definition: "mission_event", stages: "mission_event_reward", patternIndex: 2, targetIndex: 1, questKindIndex: 7, missions: 2512, stageCount: 2559, rewards: 0 },
    { category: 4, definition: "mission_collect_item", stages: "mission_collect_item_reward", patternIndex: 4, targetIndex: 2, questKindIndex: 9, missions: 997, stageCount: 997, rewards: 0 },
    { category: 5, definition: "mission_degree", stages: "mission_degree_reward", patternIndex: 3, targetIndex: 1, questKindIndex: 8, missions: 1288, stageCount: 1288, rewards: 33 },
    { category: 9, definition: "mission_char_awake", stages: "mission_char_awake_reward", patternIndex: 4, targetIndex: 5, questKindIndex: 9, missions: 144, stageCount: 144, rewards: 144 },
    { category: 10, definition: "mission_weekly_def", stages: "mission_weekly_reward", patternIndex: 2, targetIndex: 1, questKindIndex: 7, missions: 2, stageCount: 2, rewards: 0 },
]

const MISSION_PATTERN_KINDS = [
    "total_login_days",
    "player_rank_achievement",
    "mana_addition_count",
    "treasure_shop_used_mana_count",
    "characters_count",
    "character_level_achievement",
    "level_80_character_count",
    "total_released_mana_node_count",
    "total_obtained_bond_token_count",
    "over_limit_total_count",
    "over_limit_100_level_count",
    "got_equip_kind_count",
    "got_new_equip_count",
    "target_mission_clear",
    "single_battle_clear_count",
    "single_battle_clear_time",
    "multi_battle_clear_count",
    "multi_battle_host_count",
    "multi_battle_guest_count",
    "multi_battle_mvp_count",
    "attention_quest_clear_count",
    "episode_clear_count",
    "chapter_clear",
    "battle_clear_count",
    "battle_clear_time",
    "high_score_achievement",
    "ss_rank_count",
    "max_power_achievement",
    "battle_zone_statistics_count",
    "battle_zone_statistics_max_value",
    "max_combo_achievement",
    "max_skill_chain_achievement",
    "total_attained_drop_mana_count",
    "skill_last_attack_achievement",
    "upgrade_equipment_count",
    "set_soul_sphere_count",
    "level_max_equipment_count",
    "get_item_count",
    "get_item_list_count",
    "used_stamina_count",
    "character_detail_zoom_illust_for_1min_count",
    "character_detail_play_dot_sp_motion_count",
    "home_tap_town_character_count",
    "home_change_voice_count",
    "character_level_100_and_got_bond_token",
    "treasure_shop_bought_item_count",
    "total_used_mana_count",
    "mana_board_2nd_open_count",
    "mana_board_2nd_complete_count",
    "time_attack_clear_phase_1",
    "time_attack_clear_phase_2",
    "time_attack_clear_phase_3",
    "time_attack_clear_phase_4",
    "encyclopedia_unlock_count",
    "time_attack_quest_clear",
    "daily_event_quest_lv50_clear",
    "daily_event_quest_lv60_clear",
    "quest_clear",
    "equipped_first_time",
    "set_unison_first_time",
    "set_party_character",
    "evolved_character_count",
    "total_released_ability_node_count",
    "injected_exp_first_time",
    "traded_count_to_equipment_by_boss_coin",
    "quest_challenge",
    "chapter_complete",
    "twitter_check",
    "character_election_vote_count",
    "multi_special_exchange",
    "battle_clear_with_specific_party",
    "battle_clear_with_character_learned_all_abilities_in_mana_board_2nd",
    "battle_clear_with_level80_character",
    "battle_clear_with_level100_character",
    "contents_guide_start",
    "max_pattern",
    "min_pattern",
    "increment_pattern",
    "total_gacha_character_count",
    "raid_event_top_check",
    "raid_event_set_edit_main",
    "raid_event_set_edit_sub1",
    "raid_event_set_edit_sub2",
    "gacha_campaign",
    "traded_count_by_boss_coin_shop",
    "send_emotion_count",
    "ss_rank_clear_with_not_received_resistance_debuff",
    "battle_client_check",
    "player_history_check",
    "battle_clear_with_specific_character",
    "battle_clear_with_specific_skill",
    "battle_clear_with_skill_gauge_over",
    "multi_battle_newbie_count",
    "battle_clear_with_specific_characters",
    "battle_clear_with_specific_races",
    "battle_zone_statistics_no_more_than_zero",
    "character_quest_finish",
]

const EXPECTED_MISSION_COUNT = 5719
const EXPECTED_STAGE_COUNT = 6214
const EXPECTED_REWARD_COUNT = 177

// //// 映射任务关卡范围 [@x380kkm 2026-08-29] ////
const QUEST_KIND_CATEGORIES = Object.freeze({
    0: [1],
    1: [4],
    2: [2],
    3: [6],
    4: [14],
    5: [7],
    6: [10],
    7: [13],
    8: [11],
    9: [18],
    10: [19],
    11: [15],
    12: [6, 14, 13, 20],
    13: [20],
    14: [21],
    15: [22],
    16: [23],
    17: [24],
    18: [25],
    19: [26],
    20: [27],
})
// //// /映射任务关卡范围 ////

// //// 解析命令行和输入资产 [@x380kkm 2026-08-23] ////
function requiredArgument(name) {
    const index = process.argv.indexOf(name)
    const value = index === -1 ? undefined : process.argv[index + 1]
    if (value === undefined || value.startsWith("--")) throw new Error(`${name} is required`)
    return path.resolve(value)
}

function hasFlag(name) {
    return process.argv.includes(name)
}

function readJsonObject(assetRoot, relativePath) {
    const filePath = path.join(assetRoot, relativePath)
    if (!fs.existsSync(filePath)) throw new Error(`Missing source asset ${filePath}`)
    const value = JSON.parse(fs.readFileSync(filePath, "utf8"))
    if (value === null || Array.isArray(value) || typeof value !== "object") {
        throw new Error(`${relativePath} must contain an object`)
    }
    return value
}

function requireSafeInteger(value, label) {
    const integer = Number.parseInt(value, 10)
    if (!Number.isSafeInteger(integer)) throw new Error(`${label} must be a safe integer`)
    return integer
}

function optionalPositiveInteger(value) {
    const integer = Number.parseInt(value, 10)
    return Number.isSafeInteger(integer) && integer > 0 ? integer : null
}
// //// /解析命令行和输入资产 ////

// //// 解码任务模式, 阶段和奖励 [@x380kkm 2026-08-23] ////
function missionPattern(row, patternIndex, label) {
    const code = requireSafeInteger(row[patternIndex], `${label}.mission_pattern`)
    const pattern = MISSION_PATTERN_KINDS[code]
    if (pattern === undefined) throw new Error(`${label} references unknown MissionPatternKind ${code}`)
    return pattern
}

function questKindContract(row, questKindIndex, label) {
    const raw = row[questKindIndex]
    if (raw === undefined || raw === null || raw === "" || raw === "(None)") {
        return { quest_kind: null, quest_categories: [] }
    }
    const code = requireSafeInteger(raw, `${label}.quest_kind`)
    const categories = QUEST_KIND_CATEGORIES[code]
    if (categories === undefined) throw new Error(`${label} references unknown QuestRangeReferenceIdKind ${code}`)
    return { quest_kind: code, quest_categories: [...categories] }
}

function stageTarget(row, targetIndex, label) {
    return requireSafeInteger(row[targetIndex], `${label}.target`)
}

function activeMissionRewards(activeRewards, missionId, stage) {
    const row = activeRewards[String(missionId)]?.[String(stage)]?.[0]
    if (!Array.isArray(row)) return []
    const rewards = []
    for (let slot = 0; slot < 4; slot += 1) {
        const base = 7 + slot * 6
        const kind = Number.parseInt(row[base], 10) || 0
        const amount = Number.parseInt(row[base + 1], 10) || 0
        if (kind === 0 || amount === 0) continue
        rewards.push({
            kind,
            amount,
            item_id: optionalPositiveInteger(row[base + 2]),
            character_id: optionalPositiveInteger(row[base + 3]),
            equipment_id: optionalPositiveInteger(row[base + 4]),
        })
    }
    return rewards
}

function characterAwakeRewards(characterAwakeRewardTable, missionId, stage) {
    const row = characterAwakeRewardTable[String(missionId)]?.[String(stage)]?.[0]
    if (!Array.isArray(row)) return []
    const kind = Number.parseInt(row[9], 10) || 0
    const amount = Number.parseInt(row[10], 10) || 0
    if (kind === 0 || amount === 0) return []
    return [{
        kind,
        amount,
        item_id: optionalPositiveInteger(row[11]),
        character_id: null,
        equipment_id: null,
    }]
}

function degreeTarget(category, definitionRow) {
    if (category !== 5) return null
    const match = /玩家(?:达到|级别达到)\s*(\d+)/.exec(String(definitionRow[2] ?? ""))
    return match === null ? null : requireSafeInteger(match[1], "degree_target")
}

function positiveIntegerList(value, label) {
    return String(value ?? "")
        .split(",")
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0)
        .map((entry, index) => {
            const integer = requireSafeInteger(entry, `${label}[${index}]`)
            if (integer <= 0) throw new Error(`${label}[${index}] must be positive`)
            return integer
        })
}

function textList(value) {
    return String(value ?? "")
        .split(",")
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0)
}

function awakeBattleContract(category, definitionRow, label) {
    if (category !== 9) return {}
    return {
        battle_kind: optionalPositiveInteger(definitionRow[7]),
        statistics_kind: optionalPositiveInteger(definitionRow[5]),
        leader_character_id: optionalPositiveInteger(definitionRow[23]),
        required_character_ids: positiveIntegerList(definitionRow[24], `${label}.required_character_ids`),
        required_races: textList(definitionRow[25]),
    }
}
// //// /解码任务模式, 阶段和奖励 ////

// //// 生成任务目录和派生索引 [@x380kkm 2026-08-23] ////
function buildCategory(assetRoot, descriptor, activeRewards) {
    const definitions = readJsonObject(assetRoot, `${descriptor.definition}.json`)
    const stages = readJsonObject(assetRoot, `${descriptor.stages}.json`)
    const definitionIds = Object.keys(definitions)
    const missionIds = Object.keys(stages)
    if (definitionIds.length !== descriptor.missions || missionIds.length !== descriptor.missions) {
        throw new Error(`Category ${descriptor.category} contains an unexpected mission count`)
    }
    if (definitionIds.some((missionId) => stages[missionId] === undefined)) {
        throw new Error(`Category ${descriptor.category} definitions and stages do not match`)
    }

    let stageCount = 0
    let rewardCount = 0
    const missions = missionIds.map((missionId) => {
        const definitionRow = definitions[missionId]?.[0]
        if (!Array.isArray(definitionRow)) throw new Error(`Missing definition for category ${descriptor.category}:${missionId}`)
        const missionStages = Object.entries(stages[missionId]).map(([stageText, rows]) => {
            const row = rows?.[0]
            if (!Array.isArray(row)) throw new Error(`Missing stage ${stageText} for category ${descriptor.category}:${missionId}`)
            const stage = requireSafeInteger(stageText, `category ${descriptor.category}:${missionId}.stage`)
            const rewards = descriptor.category === 9
                ? characterAwakeRewards(stages, missionId, stage)
                : activeMissionRewards(activeRewards, missionId, stage)
            stageCount += 1
            rewardCount += rewards.length
            return {
                stage,
                reward_id: requireSafeInteger(row[0], `category ${descriptor.category}:${missionId}.${stage}.reward_id`),
                target: stageTarget(row, descriptor.targetIndex, `category ${descriptor.category}:${missionId}.${stage}`),
                rewards,
            }
        })
        const label = `category ${descriptor.category}:${missionId}`
        return {
            id: requireSafeInteger(missionId, `category ${descriptor.category}.mission_id`),
            pattern: missionPattern(definitionRow, descriptor.patternIndex, label),
            degree_target: degreeTarget(descriptor.category, definitionRow),
            ...questKindContract(definitionRow, descriptor.questKindIndex, label),
            stages: missionStages,
            ...awakeBattleContract(descriptor.category, definitionRow, label),
        }
    })
    if (stageCount !== descriptor.stageCount || rewardCount !== descriptor.rewards) {
        throw new Error(`Category ${descriptor.category} contains unexpected stage or reward counts`)
    }
    return missions
}

function buildCharacterStories(categories, characterQuestLookup) {
    const characterIds = [...new Set(categories["9"].map((mission) => String(mission.id).slice(0, -1)))]
        .sort((left, right) => Number(left) - Number(right))
    return Object.fromEntries(characterIds.map((characterId) => {
        const prefix = characterId === "1" ? "10" : characterId
        const questIds = Object.keys(characterQuestLookup)
            .filter((questId) => questId.startsWith(prefix))
            .map((questId) => requireSafeInteger(questId, `character ${characterId}.quest_id`))
        return [characterId, questIds]
    }))
}

function buildRankThresholds(playerRanks) {
    return Object.entries(playerRanks).map(([degree, rows]) => {
        const row = rows?.[0]
        if (!Array.isArray(row)) throw new Error(`Missing player rank ${degree}`)
        return {
            degree: requireSafeInteger(degree, "rank.degree"),
            threshold: requireSafeInteger(row[1], `rank ${degree}.threshold`),
        }
    })
}

function buildFixture(assetRoot) {
    const activeRewards = readJsonObject(assetRoot, "mission_active_reward.json")
    const categories = Object.fromEntries(CATEGORY_SOURCES.map((descriptor) => [
        String(descriptor.category),
        buildCategory(assetRoot, descriptor, activeRewards),
    ]))
    const characterQuestLookup = readJsonObject(assetRoot, "character_quest_lookup.json")
    const playerRanks = readJsonObject(assetRoot, "cdndata/player_rank_full.json")
    return {
        categories,
        character_stories: buildCharacterStories(categories, characterQuestLookup),
        rank_thresholds: buildRankThresholds(playerRanks),
    }
}
// //// /生成任务目录和派生索引 ////

// //// 校验并输出任务 fixture [@x380kkm 2026-08-23] ////
function fixtureMetrics(fixture) {
    let missionCount = 0
    let stageCount = 0
    let rewardCount = 0
    for (const missions of Object.values(fixture.categories)) {
        missionCount += missions.length
        for (const mission of missions) {
            stageCount += mission.stages.length
            rewardCount += mission.stages.reduce((total, stage) => total + stage.rewards.length, 0)
        }
    }
    if (missionCount !== EXPECTED_MISSION_COUNT || stageCount !== EXPECTED_STAGE_COUNT || rewardCount !== EXPECTED_REWARD_COUNT) {
        throw new Error(`Generated ${missionCount} missions, ${stageCount} stages and ${rewardCount} rewards`)
    }
    return { mission_count: missionCount, stage_count: stageCount, reward_count: rewardCount }
}

function main() {
    const assetRoot = requiredArgument("--asset-root")
    const outputPath = requiredArgument("--output")
    const check = hasFlag("--check")
    const fixture = buildFixture(assetRoot)
    const metrics = fixtureMetrics(fixture)
    const serialized = `${JSON.stringify(fixture)}\n`
    if (check) {
        const current = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, "utf8") : ""
        const mismatchCount = current === serialized ? 0 : 1
        console.log(JSON.stringify({ mode: "check", output: outputPath, ...metrics, mismatch_count: mismatchCount }))
        if (mismatchCount !== 0) throw new Error(`Mission fixture differs from ${outputPath}`)
        return
    }
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, serialized, "utf8")
    console.log(JSON.stringify({ mode: "generate", output: outputPath, bytes: Buffer.byteLength(serialized), ...metrics, mismatch_count: 0 }))
}

main()
// //// /校验并输出任务 fixture ////
