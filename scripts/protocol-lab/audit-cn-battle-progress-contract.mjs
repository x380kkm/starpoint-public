// audience: internal
// # audit-cn-battle-progress-contract
//
// 该脚本将 1.8.4 参考服务的战斗追踪器和 CN master 参数与个人服务生成资产逐项对照.

import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const DEFAULT_PROJECT_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")

// //// 读取并规范战斗进度契约 [@x380kkm 2026-08-25] ////
function argument(name) {
    const index = process.argv.indexOf(name)
    return index === -1 ? undefined : process.argv[index + 1]
}

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, "utf8"))
}

function requireCondition(condition, message) {
    if (!condition) throw new Error(message)
}

function optionalPositiveInteger(value) {
    const integer = Number.parseInt(value, 10)
    return Number.isSafeInteger(integer) && integer > 0 ? integer : null
}

function integerList(value) {
    return String(value ?? "")
        .split(",")
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0)
        .map((entry) => Number.parseInt(entry, 10))
}

function textList(value) {
    return String(value ?? "")
        .split(",")
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0)
}

function stable(value) {
    if (Array.isArray(value)) return value.map(stable)
    if (value !== null && typeof value === "object") {
        return Object.fromEntries(
            Object.keys(value)
                .sort((left, right) => left.localeCompare(right, "en"))
                .map((key) => [key, stable(value[key])]),
        )
    }
    return value
}

function equal(left, right) {
    return JSON.stringify(stable(left)) === JSON.stringify(stable(right))
}
// //// /读取并规范战斗进度契约 ////

// //// 对照任务参数, 角色种族和 score attack 门槛 [@x380kkm 2026-08-25] ////
function auditMissionContracts(referenceAssets, projectRoot) {
    const referenceMissions = readJson(path.join(referenceAssets, "mission_char_awake.json"))
    const localMissionMaster = readJson(
        path.join(projectRoot, "core", "personal-service", "assets", "cn-mission-master.json"),
    )
    const localMissions = new Map(
        localMissionMaster.categories["9"].map((mission) => [String(mission.id), mission]),
    )
    const counters = {
        leader: 0,
        character_sets: 0,
        race_sets: 0,
        statistics: 0,
    }
    for (const [missionId, rows] of Object.entries(referenceMissions)) {
        const row = rows[0]
        const local = localMissions.get(missionId)
        requireCondition(local !== undefined, "Missing local awake mission " + missionId)
        const expected = {
            battle_kind: optionalPositiveInteger(row[7]),
            statistics_kind: optionalPositiveInteger(row[5]),
            leader_character_id: optionalPositiveInteger(row[23]),
            required_character_ids: integerList(row[24]),
            required_races: textList(row[25]),
        }
        for (const [field, value] of Object.entries(expected)) {
            requireCondition(
                equal(local[field], value),
                "Awake mission " + missionId + " differs at " + field,
            )
        }
        counters.leader += expected.leader_character_id === null ? 0 : 1
        counters.character_sets += expected.required_character_ids.length === 0 ? 0 : 1
        counters.race_sets += expected.required_races.length === 0 ? 0 : 1
        counters.statistics += expected.statistics_kind === null ? 0 : 1
    }
    const raceMission = localMissions.get("2310012")
    const raceKey = [...new Set(raceMission.required_races)].sort().join("+")
    requireCondition(raceKey === "Devil+Dragon+Human", "Mission 2310012 race key is not authoritative")
    return { ...counters, race_key: raceKey }
}

function auditCharacterRaces(referenceAssets, projectRoot) {
    const referenceCharacters = readJson(path.join(referenceAssets, "cdndata", "character.json"))
    const localBattle = readJson(
        path.join(projectRoot, "core", "personal-service", "assets", "cn-single-battle.json"),
    )
    requireCondition(
        Object.keys(referenceCharacters).length === Object.keys(localBattle.characters).length,
        "Character race catalogs contain different character counts",
    )
    for (const [characterId, rows] of Object.entries(referenceCharacters)) {
        const expected = textList(rows[0][4])
        requireCondition(
            equal(localBattle.characters[characterId]?.races, expected),
            "Character " + characterId + " has different races",
        )
    }
    return Object.keys(referenceCharacters).length
}

function auditScoreAttack(referenceAssets, projectRoot) {
    const referenceBorders = readJson(path.join(referenceAssets, "score_attack_border_reward.json"))
    const localBattle = readJson(
        path.join(projectRoot, "core", "personal-service", "assets", "cn-single-battle.json"),
    )
    let matchedQuests = 0
    for (const quest of Object.values(localBattle.quests).filter((quest) => quest.category === 27)) {
        const key = String(quest.event_id) + "_" + String(quest.folder_id)
        const source = referenceBorders[key]
        if (source === undefined) {
            requireCondition(
                quest.score_attack_border_rewards.length === 0,
                "Score attack quest " + quest.quest_id + " has an unknown border source",
            )
            continue
        }
        const expected = source.map((border) => ({
            score: border.score,
            coinItemId: border.coinItemId,
            coinCount: border.coinCount,
        }))
        requireCondition(
            equal(quest.score_attack_border_rewards, expected),
            "Score attack quest " + quest.quest_id + " has different borders",
        )
        matchedQuests += 1
    }
    return matchedQuests
}
// //// /对照任务参数, 角色种族和 score attack 门槛 ////

// //// 确认参考服务在单机和联机结算调用全部追踪器 [@x380kkm 2026-08-25] ////
function auditReferenceTrackers(referenceRoot) {
    const trackerNames = [
        "trackCharacterClears",
        "trackLeaderPowerflip",
        "trackPartyCoClears",
        "trackPowerflip",
    ]
    const routes = [
        path.join(referenceRoot, "resources", "server", "out", "routes", "api", "singleBattleQuest.js"),
        path.join(referenceRoot, "resources", "server", "out", "multi", "http", "battle.js"),
    ]
    for (const route of routes) {
        const source = fs.readFileSync(route, "utf8")
        for (const trackerName of trackerNames) {
            requireCondition(source.includes(trackerName), route + " does not call " + trackerName)
        }
    }
    return { routes: routes.length, trackers: trackerNames.length }
}
// //// /确认参考服务在单机和联机结算调用全部追踪器 ////

// //// 执行战斗进度契约差分 [@x380kkm 2026-08-25] ////
function main() {
    const projectRoot = path.resolve(argument("--project-root") ?? DEFAULT_PROJECT_ROOT)
    const referenceRoot = path.resolve(
        argument("--reference-root") ?? path.join(projectRoot, "..", "startpoint-cn-launcher"),
    )
    const referenceAssets = path.join(referenceRoot, "resources", "server", "assets")
    const result = {
        mission: auditMissionContracts(referenceAssets, projectRoot),
        character_race_count: auditCharacterRaces(referenceAssets, projectRoot),
        score_attack_quest_count: auditScoreAttack(referenceAssets, projectRoot),
        reference: auditReferenceTrackers(referenceRoot),
    }
    console.log(JSON.stringify(result))
}

main()
// //// /执行战斗进度契约差分 ////
