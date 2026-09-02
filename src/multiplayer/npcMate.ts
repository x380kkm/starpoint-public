// audience: internal | external
// # npc-mate
// 此模块从服务器内已有玩家队伍生成 summon 接口和 CN 大厅使用的 COM 队友.
// 配置只保存玩家和队伍引用, 战斗属性每次从当前数据库读取.

import { ManagementConfig, NpcMateConfig } from "../control/management"
import {
    getPlayerCharactersManaNodesSync,
    getPlayerCharactersSync,
    getPlayerEquipmentListSync,
    getPlayerPartyGroupListSync,
    getPlayerSync,
} from "../data/wdfpData"
import { PartyCategory, PlayerCharacter, PlayerEquipment, PlayerParty } from "../data/types"

export interface ClientNpcCharacter {
    id: number
    mana_node_ids: number[]
    evolution_level: number
    exp: number
    over_limit_step: number
    ex_boost?: {
        status_id: number
        ability_id_list: number[]
    }
}

export interface ClientNpcEquipment {
    equipment_id: number
    level: number
    enhancement_level: number
}

export interface ClientNpcMate {
    com_id: number
    rank: number
    party: {
        characters: (ClientNpcCharacter | null)[]
        unison_characters: (ClientNpcCharacter | null)[]
        equipments: (ClientNpcEquipment | null)[]
        ability_soul_ids: (number | null)[]
    }
    degree_id: number
}

export interface SelectedNpcFillMate {
    config: NpcMateConfig
    clientMate: ClientNpcMate
}

export interface NpcFillRequest {
    categoryId: number
    questId: number
    roomCreatedAt: number
    currentTime?: number
}

// //// 按 category 和 quest 匹配 COM 模板 [@x380kkm 2026-07-22] ////
function doesPairingKeyMatch(pairingKey: string, categoryId: number, questId: number): boolean {
    return pairingKey === "*" || pairingKey === `${categoryId}:*` || pairingKey === `${categoryId}:${questId}`
}
// //// /按 category 和 quest 匹配 COM 模板 ////

// //// 从玩家队伍读取角色和装备战斗属性 [@x380kkm 2026-07-22] ////
function findParty(playerId: number, partySlot: number): PlayerParty | null {
    const groups = getPlayerPartyGroupListSync(playerId, PartyCategory.NORMAL)
    for (const group of Object.values(groups)) {
        const party = group.list[partySlot.toString()]
        if (party !== undefined) return party
    }
    return null
}

function createClientCharacter(characterId: number | null, characters: Record<string, PlayerCharacter>, manaNodes: Record<string, number[]>): ClientNpcCharacter | null {
    if (characterId === null) return null
    const character = characters[characterId.toString()]
    if (character === undefined) throw new Error(`NPC source player does not own character ${characterId}.`)
    const result: ClientNpcCharacter = {
        id: characterId,
        mana_node_ids: manaNodes[characterId.toString()] ?? [],
        evolution_level: character.evolutionLevel,
        exp: character.exp,
        over_limit_step: character.overLimitStep,
    }
    if (character.exBoost !== undefined) {
        result.ex_boost = {
            status_id: character.exBoost.statusId,
            ability_id_list: character.exBoost.abilityIdList,
        }
    }
    return result
}

function createClientEquipment(equipmentId: number | null, equipment: Record<string, PlayerEquipment>): ClientNpcEquipment | null {
    if (equipmentId === null) return null
    const item = equipment[equipmentId.toString()]
    if (item === undefined) throw new Error(`NPC source player does not own equipment ${equipmentId}.`)
    return { equipment_id: equipmentId, level: item.level, enhancement_level: item.enhancementLevel }
}
// //// /从玩家队伍读取角色和装备战斗属性 ////

// //// 生成 HTTP 和大厅共用的 COM 队友数据 [@x380kkm 2026-07-23] ////
function createNpcFillSelection(config: NpcMateConfig): SelectedNpcFillMate {
    if (config.sourcePlayerId === null || config.partySlot === null) {
        throw new Error(`NPC ${config.id} does not reference a source player and party.`)
    }
    const player = getPlayerSync(config.sourcePlayerId)
    if (player === null) throw new Error(`NPC source player does not exist: ${config.sourcePlayerId}`)
    const party = findParty(config.sourcePlayerId, config.partySlot)
    if (party === null) throw new Error(`NPC source party does not exist: ${config.partySlot}`)

    const characters = getPlayerCharactersSync(config.sourcePlayerId)
    const manaNodes = getPlayerCharactersManaNodesSync(config.sourcePlayerId)
    const equipment = getPlayerEquipmentListSync(config.sourcePlayerId)
    return {
        config,
        clientMate: {
            com_id: config.sourcePlayerId,
            rank: config.rank,
            party: {
                characters: party.characterIds.map((id) => createClientCharacter(id, characters, manaNodes)),
                unison_characters: party.unisonCharacterIds.map((id) => createClientCharacter(id, characters, manaNodes)),
                equipments: party.equipmentIds.map((id) => createClientEquipment(id, equipment)),
                ability_soul_ids: party.abilitySoulIds,
            },
            degree_id: config.degreeId ?? player.degreeId,
        },
    }
}
// //// /生成 HTTP 和大厅共用的 COM 队友数据 ////

export function createClientNpcMate(config: NpcMateConfig): ClientNpcMate {
    return createNpcFillSelection(config).clientMate
}

// //// 在配置延时结束后选择最多 2 个 COM 队友 [@x380kkm 2026-07-22] ////
export function selectNpcFillSelections(config: ManagementConfig, request: NpcFillRequest): SelectedNpcFillMate[] {
    if (!config.npcFill.enabled) return []
    const currentTime = request.currentTime ?? Date.now()
    if (currentTime - request.roomCreatedAt < config.npcFill.delaySeconds * 1000) return []
    return config.npcMates
        .filter((mate) => mate.enabled && doesPairingKeyMatch(mate.pairingKey, request.categoryId, request.questId))
        .slice(0, 2)
        .map(createNpcFillSelection)
}

export function selectNpcFillMates(config: ManagementConfig, request: NpcFillRequest): ClientNpcMate[] {
    return selectNpcFillSelections(config, request).map((selection) => selection.clientMate)
}
// //// /在配置延时结束后选择最多 2 个 COM 队友 ////
