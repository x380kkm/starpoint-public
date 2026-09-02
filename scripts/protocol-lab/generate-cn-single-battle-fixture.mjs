// audience: internal
// # generate-cn-single-battle-fixture
//
// 该脚本从 startpoint-cn 的 CN 任务和奖励资产生成个人服务使用的单机战斗数据.

import { createHash } from "node:crypto"
import fs from "node:fs"
import path from "node:path"

const QUEST_SOURCES = [
    { name: "main_quest", category: 1, entryCategory: 1, expectedCount: 419 },
    { name: "boss_battle_quest", category: 2, entryCategory: 2, expectedCount: 232 },
    { name: "character_quest", category: 3, entryCategory: 3, expectedCount: 1318 },
    { name: "ex_quest", category: 4, entryCategory: 4, expectedCount: 221 },
    { name: "daily_week_event_quest", category: 6, entryCategory: 6, expectedCount: 114 },
    { name: "advent_event_quest", category: 7, entryCategory: 7, expectedCount: 459 },
    { name: "story_event_single_quest", category: 10, entryCategory: 11, expectedCount: 348 },
    { name: "ranking_event_single_quest", category: 11, entryCategory: 10, expectedCount: 7 },
    { name: "challenge_dungeon_event_quest", category: 13, entryCategory: 13, expectedCount: 46 },
    { name: "daily_exp_mana_event_quest", category: 14, entryCategory: 14, expectedCount: 6 },
    { name: "practice_quest", category: 15, entryCategory: 15, expectedCount: 98 },
    { name: "world_story_event_quest", category: 18, entryCategory: 18, expectedCount: 913 },
    { name: "world_story_event_boss_battle_quest", category: 19, entryCategory: 19, expectedCount: 96 },
    { name: "tower_dungeon_event_quest", category: 20, entryCategory: 20, expectedCount: 480 },
    { name: "expert_single_event_quest", category: 21, entryCategory: 21, expectedCount: 28 },
    { name: "carnival_event_quest", category: 22, entryCategory: 22, expectedCount: 171 },
    { name: "raid_event_quest", category: 23, entryCategory: 8, expectedCount: 50 },
    { name: "rush_event_quest", category: 24, entryCategory: 24, expectedCount: 110 },
    { name: "solo_time_attack_event_quest", category: 25, entryCategory: 25, expectedCount: 6 },
    { name: "hard_multi_event_quest", category: 26, entryCategory: 26, expectedCount: 12 },
    { name: "score_attack_event_quest", category: 27, entryCategory: 9, expectedCount: 123 },
]

const EXPECTED_SOURCE_TOTAL = 5257
const EXPECTED_CHARACTER_COUNT = 505
const EXPECTED_ENTRY_COST_COUNT = 3021
const EXPECTED_CLEAR_REWARD_COUNT = 238
const EXPECTED_SCORE_REWARD_COUNT = 1532
const EXPECTED_RARE_REWARD_COUNT = 2768
const ENEMY_ELEMENT_BY_QUEST_ELEMENT = { 0: 3, 1: 0, 2: 1, 3: 2, 4: 5, 5: 4 }

// //// 读取并校验输入资产 [@x380kkm 2026-08-22] ////
function requiredArgument(name) {
    const index = process.argv.indexOf(name)
    const value = index === -1 ? undefined : process.argv[index + 1]
    if (value === undefined || value.startsWith("--")) throw new Error(`${name} is required`)
    return path.resolve(value)
}

function readJson(assetRoot, name) {
    const filePath = path.join(assetRoot, `${name}.json`)
    if (!fs.existsSync(filePath)) throw new Error(`Missing source asset ${filePath}`)
    const parsed = JSON.parse(fs.readFileSync(filePath, "utf8"))
    if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
        throw new Error(`${name}.json must contain an object`)
    }
    return parsed
}

function requireSourceCount(name, values, expectedCount) {
    const actualCount = Object.keys(values).length
    if (actualCount !== expectedCount) {
        throw new Error(`${name}.json contains ${actualCount} records; expected ${expectedCount}`)
    }
}

function requireInteger(value, label) {
    if (!Number.isSafeInteger(value)) throw new Error(`${label} must be a safe integer`)
    return value
}

function optionalInteger(value, label) {
    return value === undefined || value === null ? undefined : requireInteger(value, label)
}

function integerOrZero(value, label) {
    return value === undefined || value === null ? 0 : requireInteger(value, label)
}

function optionalNumber(value, label) {
    if (value === undefined || value === null) return undefined
    if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`${label} must be finite`)
    return value
}
// //// /读取并校验输入资产 ////

// //// 规范奖励和任务字段 [@x380kkm 2026-08-22] ////
function normalizeReward(reward, label) {
    if (reward === undefined) return undefined
    return {
        type: requireInteger(reward.type, `${label}.type`),
        id: optionalInteger(reward.id, `${label}.id`),
        count: optionalInteger(reward.count, `${label}.count`),
        rarity: optionalNumber(reward.rarity, `${label}.rarity`),
    }
}

function resolveElementRewardId(rewardElementMap, rewardType, rarity, questElement, label) {
    const mapType = rewardType === 6 ? 1 : 2
    const enemyElement = ENEMY_ELEMENT_BY_QUEST_ELEMENT[questElement ?? 0] ?? 3
    const value = rewardElementMap[String(mapType)]?.[String(rarity)]?.[String(enemyElement)]?.[0]?.[0]
    const itemId = Number(value)
    if (!Number.isSafeInteger(itemId)) throw new Error(`${label} has no element reward mapping`)
    return itemId
}

function normalizeScoreReward(reward, rewardElementMap, questElement, label) {
    const rewardType = optionalInteger(reward.reward_type, `${label}.reward_type`)
    const sourceId = optionalInteger(reward.id, `${label}.id`)
    const resolvesElementItem = rewardType === 6 || rewardType === 7
    if (resolvesElementItem && sourceId === undefined) throw new Error(`${label} has no element rarity`)
    return {
        type: requireInteger(reward.type, `${label}.type`),
        reward_type: rewardType,
        id: resolvesElementItem
            ? resolveElementRewardId(rewardElementMap, rewardType, sourceId, questElement, label)
            : sourceId,
        element_rarity: resolvesElementItem ? sourceId : undefined,
        count: optionalInteger(reward.count, `${label}.count`),
        rarity: optionalNumber(reward.rarity, `${label}.rarity`),
        position: optionalInteger(reward.position, `${label}.position`),
        field5: optionalInteger(reward.field5, `${label}.field5`),
    }
}

function requireReference(values, id, label) {
    const value = values[String(id)]
    if (value === undefined) throw new Error(`${label} ${id} does not exist`)
    return value
}

function resolveClearReward(clearRewards, id, label, includedClearRewardIds) {
    if (id === undefined) return undefined
    const reward = requireReference(clearRewards, id, `${label} clear reward`)
    includedClearRewardIds.add(String(id))
    return normalizeReward(reward, `${label}.clear_reward`)
}

function resolveScoreRewards(context, groupId, questElement, label) {
    if (groupId === undefined) return []
    const rewards = requireReference(context.scoreRewardGroups, groupId, `${label} score reward group`)
    if (!Array.isArray(rewards)) throw new Error(`${label} score reward group ${groupId} must be an array`)
    context.includedScoreRewardIds.add(String(groupId))
    return rewards.map((reward, index) => {
        const normalized = normalizeScoreReward(
            reward,
            context.rewardElementMap,
            questElement,
            `${label}.score_rewards[${index}]`,
        )
        if (normalized.type === 1) {
            if (normalized.id === undefined) throw new Error(`${label} rare reward group has no id`)
            requireReference(context.rareRewardGroups, normalized.id, `${label} rare reward group`)
            context.includedRareRewardIds.add(String(normalized.id))
        }
        return normalized
    })
}

function normalizeScoreAttackBorderRewards(rewards, label) {
    if (!Array.isArray(rewards) || rewards.length === 0) {
        throw new Error(`${label} must contain at least one border reward`)
    }
    return rewards.map((reward, index) => ({
        score: requireInteger(reward.score, `${label}[${index}].score`),
        coinItemId: requireInteger(reward.coinItemId, `${label}[${index}].coinItemId`),
        coinCount: requireInteger(reward.coinCount, `${label}[${index}].coinCount`),
    }))
}

function normalizeEntryCost(entryCost, label) {
    if (entryCost === undefined) {
        return { entry_item_id: undefined, entry_item_count: 0, stamina_cost: 0 }
    }
    const itemId = integerOrZero(entryCost.itemId, `${label}.itemId`)
    return {
        entry_item_id: itemId === 0 ? undefined : itemId,
        entry_item_count: integerOrZero(entryCost.itemCount, `${label}.itemCount`),
        stamina_cost: integerOrZero(entryCost.stamina, `${label}.stamina`),
    }
}

function normalizeQuest(context) {
    const { descriptor, questId, rawQuest, entryCost, carnivalScore } = context
    const label = `${descriptor.name}:${questId}`
    const isHardMulti = descriptor.name === "hard_multi_event_quest"
    const isScoreAttack = descriptor.name === "score_attack_event_quest"
    const clearRewardId = isHardMulti ? undefined : optionalInteger(rawQuest.clearRewardId, `${label}.clearRewardId`)
    const sPlusRewardId = optionalInteger(rawQuest.sPlusRewardId, `${label}.sPlusRewardId`)
    const sourceScoreRewardGroupId =
        optionalInteger(rawQuest.scoreRewardGroupId, `${label}.scoreRewardGroupId`)
    const scoreRewardGroupId = isScoreAttack
        ? undefined
        : sourceScoreRewardGroupId
    const fixedPartyId = optionalInteger(rawQuest.fixedParty, `${label}.fixedParty`)
    const eventId = optionalInteger(rawQuest.eventId, `${label}.eventId`)
    const folderId = optionalInteger(rawQuest.folderId, `${label}.folderId`)
    const element = optionalInteger(rawQuest.element, `${label}.element`)
    const entry = normalizeEntryCost(entryCost, `${label}.entry`)
    const scoreAttackBorderKey = isScoreAttack ? `${eventId}_${folderId}` : undefined
    const scoreAttackBorderSource = scoreAttackBorderKey === undefined
        ? undefined
        : context.scoreAttackBorderRewards[scoreAttackBorderKey]
    const scoreAttackBorderRewards = scoreAttackBorderSource === undefined
        ? []
        : normalizeScoreAttackBorderRewards(
            scoreAttackBorderSource,
            `${label}.score_attack_border_rewards`,
        )

    return {
        category: descriptor.category,
        quest_id: requireInteger(Number(questId), `${label}.quest_id`),
        name: typeof rawQuest.name === "string" ? rawQuest.name : "",
        clear_reward_id: clearRewardId,
        clear_reward: resolveClearReward(context.clearRewards, clearRewardId, label, context.includedClearRewardIds),
        s_plus_reward_id: sPlusRewardId,
        s_plus_reward: resolveClearReward(context.clearRewards, sPlusRewardId, label, context.includedClearRewardIds),
        score_reward_group_id: scoreRewardGroupId,
        score_attack_reward_group_id: isScoreAttack
            ? sourceScoreRewardGroupId
            : undefined,
        score_rewards: resolveScoreRewards(context, scoreRewardGroupId, element, label),
        score_attack_border_rewards: scoreAttackBorderRewards,
        b_rank_time: integerOrZero(rawQuest.bRankTime, `${label}.bRankTime`),
        a_rank_time: integerOrZero(rawQuest.aRankTime, `${label}.aRankTime`),
        s_rank_time: integerOrZero(rawQuest.sRankTime, `${label}.sRankTime`),
        s_plus_rank_time: integerOrZero(rawQuest.sPlusRankTime, `${label}.sPlusRankTime`),
        rank_point_reward: integerOrZero(rawQuest.rankPointReward, `${label}.rankPointReward`),
        character_exp_reward: integerOrZero(rawQuest.characterExpReward, `${label}.characterExpReward`),
        mana_reward: integerOrZero(rawQuest.manaReward, `${label}.manaReward`),
        pool_exp_reward: integerOrZero(rawQuest.poolExpReward, `${label}.poolExpReward`),
        element,
        event_id: eventId,
        folder_id: folderId,
        fixed_party_id: fixedPartyId,
        has_fixed_party: fixedPartyId !== undefined,
        linked_quest_id: isHardMulti
            ? optionalInteger(rawQuest.clearRewardId, `${label}.clearRewardId`)
            : undefined,
        rush_event_id: optionalInteger(rawQuest.rushEventId, `${label}.rushEventId`),
        rush_event_folder_id: optionalInteger(rawQuest.rushEventFolderId, `${label}.rushEventFolderId`),
        rush_event_round: optionalInteger(rawQuest.rushEventRound, `${label}.rushEventRound`),
        raid_event_id: descriptor.name === "raid_event_quest" ? Math.trunc(Number(questId) / 1000) : undefined,
        carnival_event_id: carnivalScore?.event_id,
        carnival_folder_id: carnivalScore?.folder_id,
        carnival_difficulty_score: carnivalScore?.difficulty_score,
        carnival_time_limit_ms: carnivalScore?.time_limit_ms,
        ...entry,
    }
}
// //// /规范奖励和任务字段 ////

// //// 生成完整任务和引用闭包 [@x380kkm 2026-08-22] ////
function loadSourceAssets(assetRoot) {
    const sources = Object.fromEntries(QUEST_SOURCES.map((source) => [source.name, readJson(assetRoot, source.name)]))
    for (const descriptor of QUEST_SOURCES) {
        requireSourceCount(descriptor.name, sources[descriptor.name], descriptor.expectedCount)
    }

    const assets = {
        sources,
        characters: readJson(assetRoot, "character"),
        rawCharacters: readJson(assetRoot, "cdndata/character"),
        clearRewards: readJson(assetRoot, "clear_reward"),
        scoreRewardGroups: readJson(assetRoot, "score_reward"),
        rareRewardGroups: readJson(assetRoot, "rare_score_reward"),
        rewardElementMap: readJson(assetRoot, "reward_element_map"),
        entryCosts: readJson(assetRoot, "quest_entry_costs"),
        carnivalScores: readJson(assetRoot, "carnival_event_quest_scores"),
        scoreAttackBorderRewards: readJson(assetRoot, "score_attack_border_reward"),
    }
    requireSourceCount("character", assets.characters, EXPECTED_CHARACTER_COUNT)
    requireSourceCount("cdndata/character", assets.rawCharacters, EXPECTED_CHARACTER_COUNT)
    requireSourceCount("clear_reward", assets.clearRewards, EXPECTED_CLEAR_REWARD_COUNT)
    requireSourceCount("score_reward", assets.scoreRewardGroups, EXPECTED_SCORE_REWARD_COUNT)
    requireSourceCount("rare_score_reward", assets.rareRewardGroups, EXPECTED_RARE_REWARD_COUNT)
    requireSourceCount("quest_entry_costs", assets.entryCosts, EXPECTED_ENTRY_COST_COUNT)
    requireSourceCount("carnival_event_quest_scores", assets.carnivalScores, 171)
    requireSourceCount("score_attack_border_reward", assets.scoreAttackBorderRewards, 123)
    return assets
}

function normalizeCharacters(characters, rawCharacters) {
    return Object.fromEntries(Object.entries(characters).map(([characterId, character]) => [
        characterId,
        {
            rarity: requireInteger(character.rarity, `character:${characterId}.rarity`),
            element: requireInteger(character.element, `character:${characterId}.element`),
            races: normalizeCharacterRaces(
                requireReference(rawCharacters, characterId, "raw character")[0],
                characterId,
            ),
        },
    ]))
}

function normalizeCharacterRaces(row, characterId) {
    if (!Array.isArray(row)) throw new Error(`raw character ${characterId} has no primary row`)
    return String(row[4] ?? "")
        .split(",")
        .map((race) => race.trim())
        .filter((race) => race.length > 0)
}

function buildFixture(assets) {
    const quests = {}
    const consumedEntryCosts = new Set()
    const consumedCarnivalScores = new Set()
    const includedClearRewardIds = new Set()
    const includedScoreRewardIds = new Set()
    const includedRareRewardIds = new Set()
    const sourceCounts = {}
    const categoryCounts = {}

    for (const descriptor of QUEST_SOURCES) {
        const sourceQuests = assets.sources[descriptor.name]
        sourceCounts[descriptor.name] = Object.keys(sourceQuests).length
        categoryCounts[String(descriptor.category)] = Object.keys(sourceQuests).length
        for (const [questId, rawQuest] of Object.entries(sourceQuests)) {
            const questKey = `${descriptor.category}:${questId}`
            if (quests[questKey] !== undefined) throw new Error(`Duplicate output quest ${questKey}`)
            const entryCostKey = `${descriptor.entryCategory}_${questId}`
            const entryCost = assets.entryCosts[entryCostKey]
            if (entryCost !== undefined) consumedEntryCosts.add(entryCostKey)
            const carnivalScore = descriptor.name === "carnival_event_quest"
                ? requireReference(assets.carnivalScores, questId, "carnival quest score")
                : undefined
            if (carnivalScore !== undefined) consumedCarnivalScores.add(String(questId))
            quests[questKey] = normalizeQuest({
                descriptor,
                questId,
                rawQuest,
                entryCost,
                carnivalScore,
                scoreAttackBorderRewards: assets.scoreAttackBorderRewards,
                clearRewards: assets.clearRewards,
                scoreRewardGroups: assets.scoreRewardGroups,
                rareRewardGroups: assets.rareRewardGroups,
                rewardElementMap: assets.rewardElementMap,
                includedClearRewardIds,
                includedScoreRewardIds,
                includedRareRewardIds,
            })
        }
    }

    const sourceTotal = Object.values(sourceCounts).reduce((total, count) => total + count, 0)
    if (sourceTotal !== EXPECTED_SOURCE_TOTAL || Object.keys(quests).length !== EXPECTED_SOURCE_TOTAL) {
        throw new Error(`Generated ${Object.keys(quests).length} quests from ${sourceTotal} source records; expected ${EXPECTED_SOURCE_TOTAL}`)
    }
    if (sourceCounts.main_quest !== 419) throw new Error("main_quest must contain 419 queryable records")

    const unusedEntryCosts = Object.keys(assets.entryCosts).filter((key) => !consumedEntryCosts.has(key))
    if (unusedEntryCosts.length > 0) throw new Error(`Entry costs reference unknown quests: ${unusedEntryCosts.join(", ")}`)
    const unusedCarnivalScores = Object.keys(assets.carnivalScores).filter((key) => !consumedCarnivalScores.has(key))
    if (unusedCarnivalScores.length > 0) throw new Error(`Carnival scores reference unknown quests: ${unusedCarnivalScores.join(", ")}`)

    const rareRewardGroups = Object.fromEntries([...includedRareRewardIds].map((groupId) => {
        const rewards = requireReference(assets.rareRewardGroups, groupId, "rare reward group")
        if (!Array.isArray(rewards)) throw new Error(`Rare reward group ${groupId} must be an array`)
        return [groupId, rewards.map((reward, index) => normalizeReward(reward, `rare_reward:${groupId}[${index}]`))]
    }))

    return {
        source: {
            source_total: sourceTotal,
            quest_total: Object.keys(quests).length,
            source_counts: sourceCounts,
            category_counts: categoryCounts,
            entry_cost_count: consumedEntryCosts.size,
            character_count: Object.keys(assets.characters).length,
            clear_reward_source_count: Object.keys(assets.clearRewards).length,
            included_clear_reward_count: includedClearRewardIds.size,
            score_reward_source_count: Object.keys(assets.scoreRewardGroups).length,
            included_score_reward_group_count: includedScoreRewardIds.size,
            rare_reward_source_count: Object.keys(assets.rareRewardGroups).length,
            included_rare_reward_group_count: includedRareRewardIds.size,
            included_score_attack_border_quest_count: Object.values(quests)
                .filter((quest) => quest.score_attack_border_rewards.length > 0)
                .length,
        },
        characters: normalizeCharacters(assets.characters, assets.rawCharacters),
        quests,
        rare_reward_groups: rareRewardGroups,
    }
}
// //// /生成完整任务和引用闭包 ////

// //// 写出确定的单机战斗数据 [@x380kkm 2026-08-22] ////
function sortObjectKeys(value) {
    if (Array.isArray(value)) return value.map(sortObjectKeys)
    if (value !== null && typeof value === "object") {
        return Object.fromEntries(
            Object.keys(value)
                .sort((left, right) => left.localeCompare(right, "en"))
                .map((key) => [key, sortObjectKeys(value[key])]),
        )
    }
    return value
}

function main() {
    const assetRoot = requiredArgument("--asset-root")
    const outputPath = requiredArgument("--output")
    const fixture = buildFixture(loadSourceAssets(assetRoot))
    const serialized = `${JSON.stringify(sortObjectKeys(fixture))}\n`
    fs.mkdirSync(path.dirname(outputPath), { recursive: true })
    fs.writeFileSync(outputPath, serialized, "utf8")
    console.log(JSON.stringify({
        output: outputPath,
        bytes: Buffer.byteLength(serialized),
        sha256: createHash("sha256").update(serialized).digest("hex"),
        source: fixture.source,
    }))
}
// //// /写出确定的单机战斗数据 ////

main()
