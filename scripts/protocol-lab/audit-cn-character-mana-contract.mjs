// audience: internal
// # audit-cn-character-mana-contract
//
// 该脚本核对 CN 角色能力槽与 Mana node 支援技能引用, 并比较 iOS 目标角色资产与原始 character master.

import fs from "node:fs"
import path from "node:path"
import process from "node:process"
import { fileURLToPath } from "node:url"

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIRECTORY, "..", "..")

// //// 读取并验证审计输入 [@x380kkm 2026-08-28] ////
function readOption(args, name, fallback) {
    const index = args.indexOf(name)
    if (index < 0) return fallback
    const value = args[index + 1]
    if (!value || value.startsWith("--")) throw new Error(`missing value for ${name}`)
    return path.resolve(value)
}

function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, "utf8"))
}

function requiredObject(value, name) {
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
        throw new Error(`${name} must be an object`)
    }
    return value
}

function positiveInteger(value) {
    return Number.isSafeInteger(value) && value > 0 ? value : null
}

// //// /读取并验证审计输入 ////

// //// 解析角色能力槽和 Mana board 布局 [@x380kkm 2026-08-28] ////
function rawSkillSlotCount(row) {
    return row.slice(19, 25).filter((value) => value !== "" && value !== "(None)").length
}

function derivedSkillCount(row) {
    return String(row[36] ?? "")
        .split(",")
        .filter((value) => value === "6")
        .length
}

function boardNodeRows(board, characterId, boardIndex) {
    const boardData = board[characterId]?.[boardIndex]
    if (boardData === undefined) return null
    const rows = []
    for (const [layoutId, entries] of Object.entries(boardData)) {
        if (!Array.isArray(entries)) continue
        for (const entry of entries) {
            if (!Array.isArray(entry) || entry.length < 6) {
                rows.push({ layoutId, entry })
                continue
            }
            rows.push({ layoutId, entry })
        }
    }
    return rows
}

function collectCharacterRewardReferences(assetRoot, characterIds) {
    const references = []
    const addReference = (source, pointer, value) => {
        const characterId = String(value)
        if (characterIds.has(characterId)) references.push({ source, pointer, character_id: characterId })
    }
    const visitRewards = (value, source, characterRewardType, pointer = "") => {
        if (Array.isArray(value)) {
            value.forEach((entry, index) => visitRewards(entry, source, characterRewardType, `${pointer}/${index}`))
            return
        }
        if (value === null || typeof value !== "object") return
        if (Number(value.type) === characterRewardType) addReference(source, pointer, value.id)
        for (const [key, entry] of Object.entries(value)) {
            visitRewards(entry, source, characterRewardType, `${pointer}/${key}`)
        }
    }

    const gachaPath = path.join(assetRoot, "gacha.json")
    if (fs.existsSync(gachaPath)) {
        const gachas = requiredObject(readJson(gachaPath), "gacha asset")
        for (const [gachaId, gacha] of Object.entries(gachas)) {
            if (Number(gacha.type) !== 0) continue
            for (const [rank, entries] of Object.entries(gacha.pool ?? {})) {
                if (!Array.isArray(entries)) continue
                entries.forEach((entry, index) => {
                    addReference("gacha.json", `/${gachaId}/pool/${rank}/${index}`, entry?.id)
                })
            }
        }
    }

    for (const [fileName, characterRewardType] of [
        ["box_reward.json", 5],
        ["clear_reward.json", 2],
        ["rare_score_reward.json", 2],
        ["rush_event_ranking_reward.json", 2],
    ]) {
        const filePath = path.join(assetRoot, fileName)
        if (fs.existsSync(filePath)) {
            visitRewards(readJson(filePath), fileName, characterRewardType)
        }
    }
    for (const fileName of [
        "treasure_shop.json",
        "general_shop.json",
        "star_grain_shop.json",
        "equipment_enhancement_shop.json",
        "event_item_shop.json",
        "boss_coin_shop.json",
    ]) {
        const filePath = path.join(assetRoot, fileName)
        if (fs.existsSync(filePath)) visitRewards(readJson(filePath), fileName, 3)
    }
    return references
}
// //// /解析角色能力槽和 Mana board 布局 ////

// //// 核对角色派生资产和 Mana node 引用 [@x380kkm 2026-08-28] ////
function audit({ assetRoot, targetCharacterPath, rawCharacterPath }) {
    const characters = requiredObject(readJson(path.join(assetRoot, "character.json")), "character asset")
    const manaNodes = requiredObject(readJson(path.join(assetRoot, "mana_node.json")), "mana node asset")
    const manaBoard = requiredObject(readJson(path.join(assetRoot, "mana_board.json")), "mana board asset")
    const targetCharacters = targetCharacterPath === null
        ? null
        : requiredObject(readJson(targetCharacterPath), "target character asset")
    const rawCharacters = rawCharacterPath === null
        ? null
        : requiredObject(readJson(rawCharacterPath), "raw character master")

    const skillSlotMismatches = []
    const targetSkillCountMismatches = []
    const rawSkillCountDifferences = []
    const missingBoardEntries = []
    const missingParentEntries = []
    const missingCharacterEntries = []
    const missingManaNodeCharacterEntries = Object.keys(characters)
        .filter((characterId) => manaNodes[characterId] === undefined)
    const missingTargetCharacterEntries = []
    let nodeCount = 0
    let supportNodeCount = 0

    for (const [characterId, boards] of Object.entries(manaNodes)) {
        const character = characters[characterId]
        if (character === undefined) {
            missingCharacterEntries.push(characterId)
            continue
        }
        const skillCount = positiveInteger(character.skill_count) ?? 0
        const targetCharacter = targetCharacters?.[characterId]
        if (targetCharacters !== null && targetCharacter === undefined) {
            missingTargetCharacterEntries.push(characterId)
        } else if (targetCharacter !== undefined) {
            const expectedSkillCount = positiveInteger(targetCharacter.skill_count) ?? 0
            if (skillCount !== expectedSkillCount) {
                targetSkillCountMismatches.push({
                    character_id: characterId,
                    actual: skillCount,
                    expected: expectedSkillCount,
                })
            }
        }
        const rawRow = rawCharacters?.[characterId]?.[0]
        if (rawRow !== undefined) {
            const expectedSkillCount = derivedSkillCount(rawRow)
            if (skillCount !== expectedSkillCount) {
                rawSkillCountDifferences.push({
                    character_id: characterId,
                    actual: skillCount,
                    expected: expectedSkillCount,
                })
            }
        }

        for (const [boardIndex, nodes] of Object.entries(boards)) {
            const nodeRows = boardNodeRows(manaBoard, characterId, boardIndex)
            const layoutNodeIds = new Set(
                (nodeRows ?? [])
                    .map(({ entry }) => entry)
                    .filter((entry) => Array.isArray(entry) && entry.length >= 6)
                    .map((entry) => String(entry[0])),
            )
            const nodeIds = new Set(Object.keys(nodes))
            if (nodeRows === null) {
                missingBoardEntries.push({ character_id: characterId, board_index: boardIndex })
            }
            for (const [nodeId, node] of Object.entries(nodes)) {
                nodeCount += 1
                if (nodeRows !== null && !layoutNodeIds.has(nodeId)) {
                    missingBoardEntries.push({ character_id: characterId, board_index: boardIndex, node_id: nodeId })
                }
                if (String(node.field5) !== "0") continue
                supportNodeCount += 1
                const slot = Number(node.field6)
                const rawSlotCount = rawRow === undefined ? null : rawSkillSlotCount(rawRow)
                const targetSlotCount = positiveInteger(targetCharacter?.skill_count)
                const allowedSlotCount = rawSlotCount ?? targetSlotCount ?? skillCount
                if (!Number.isSafeInteger(slot) || slot < 1 || slot > allowedSlotCount) {
                    skillSlotMismatches.push({
                        character_id: characterId,
                        board_index: boardIndex,
                        node_id: nodeId,
                        skill_slot_index: slot,
                        allowed_slot_count: allowedSlotCount,
                    })
                }
                const parent = nodeRows?.find(({ entry }) => Array.isArray(entry) && String(entry[0]) === nodeId)?.entry?.[5]
                if (parent !== undefined && parent !== "(None)" && !nodeIds.has(String(parent))) {
                    missingParentEntries.push({
                        character_id: characterId,
                        board_index: boardIndex,
                        node_id: nodeId,
                        parent_node_id: String(parent),
                    })
                }
            }
        }
    }

    const missingBoardCharacterIds = new Set(
        missingBoardEntries.map((entry) => entry.character_id),
    )
    const missingBoardRewardReferences = collectCharacterRewardReferences(
        assetRoot,
        missingBoardCharacterIds,
    )
    const missingManaCharacterIds = new Set([
        ...missingManaNodeCharacterEntries,
        ...missingBoardCharacterIds,
    ])
    const missingManaRewardReferences = collectCharacterRewardReferences(
        assetRoot,
        missingManaCharacterIds,
    )

    return {
        character_count: Object.keys(characters).length,
        mana_node_character_count: Object.keys(manaNodes).length,
        mana_board_character_count: Object.keys(manaBoard).length,
        node_count: nodeCount,
        support_node_count: supportNodeCount,
        skill_slot_mismatch_count: skillSlotMismatches.length,
        target_skill_count_mismatch_count: targetSkillCountMismatches.length,
        raw_skill_count_difference_count: rawSkillCountDifferences.length,
        missing_board_entry_count: missingBoardEntries.length,
        missing_board_character_count: missingBoardCharacterIds.size,
        missing_board_reward_reference_count: missingBoardRewardReferences.length,
        missing_mana_node_character_count: missingManaNodeCharacterEntries.length,
        missing_mana_character_count: missingManaCharacterIds.size,
        missing_mana_reward_reference_count: missingManaRewardReferences.length,
        missing_parent_count: missingParentEntries.length,
        missing_character_count: missingCharacterEntries.length,
        missing_target_character_count: missingTargetCharacterEntries.length,
        skill_slot_mismatches: skillSlotMismatches,
        target_skill_count_mismatches: targetSkillCountMismatches,
        raw_skill_count_differences: rawSkillCountDifferences,
        missing_board_entries: missingBoardEntries,
        missing_board_reward_references: missingBoardRewardReferences,
        missing_mana_node_character_entries: missingManaNodeCharacterEntries,
        missing_mana_reward_references: missingManaRewardReferences,
        missing_parent_entries: missingParentEntries,
        missing_character_entries: missingCharacterEntries,
        missing_target_character_entries: missingTargetCharacterEntries,
    }
}
// //// /核对角色派生资产和 Mana node 引用 ////

// //// 执行 CN 角色 Mana 契约审计 [@x380kkm 2026-08-28] ////
function main() {
    const args = process.argv.slice(2)
    const assetRoot = readOption(args, "--asset-root", path.join(REPOSITORY_ROOT, "assets"))
    const referenceAssetRoot = path.join(
        REPOSITORY_ROOT,
        "..",
        "startpoint-cn-launcher",
        "resources",
        "server",
        "assets",
    )
    const defaultTargetCharacterPath = path.join(referenceAssetRoot, "character.json")
    const defaultRawCharacterPath = path.join(
        referenceAssetRoot,
        "cdndata",
        "character.json",
    )
    const targetCharacterPath = readOption(
        args,
        "--target-character",
        fs.existsSync(defaultTargetCharacterPath) ? defaultTargetCharacterPath : null,
    )
    const rawCharacterPath = readOption(
        args,
        "--raw-character",
        fs.existsSync(defaultRawCharacterPath) ? defaultRawCharacterPath : null,
    )
    const report = audit({ assetRoot, targetCharacterPath, rawCharacterPath })
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`)
    if (report.skill_slot_mismatch_count > 0 ||
        report.target_skill_count_mismatch_count > 0 ||
        report.missing_target_character_count > 0 ||
        report.missing_board_reward_reference_count > 0 ||
        report.missing_mana_reward_reference_count > 0) {
        process.exitCode = 1
    }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
// //// /执行 CN 角色 Mana 契约审计 ////
