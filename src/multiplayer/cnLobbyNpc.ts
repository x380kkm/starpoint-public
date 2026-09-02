// audience: external
// # cn-lobby-npc
// 此模块把 summon HTTP 数据转换为 CN v1.8.1 大厅 Mates 帧使用的 COM 玩家对象.
// 客户端在 lobby Summon 命令中提供随机名称; 其余字段必须与同一 HTTP 选择一致.
// Haxe Option 使用 [0, value] 表示 Some, 使用 [1] 表示 None.

import { isDeepStrictEqual } from "util"
import type {
    ClientNpcCharacter,
    ClientNpcEquipment,
    SelectedNpcFillMate,
} from "./npcMate"

type HaxeOption<T> = [0, T] | [1]

export interface CnLobbyNpcPlayer {
    viewerId: number
    comId: number
    name: string
    rank: number
    degreeId: number
    playerRoleKind: number
    party: Record<string, unknown>
    connectionId: string
    autoplayMode: boolean
    autoskillMode: number
    autoSpeedLevel: number
    autoStart: boolean
    skillAbilityBehaviorMode: number
    dashBehaviorMode: number
    allowHealFromOtherPlayers: boolean
    state: number[]
    entryTime: number
    isNewbie: boolean
    isHost: boolean
}

function encodeOption<T>(value: T | null | undefined): HaxeOption<T> {
    return value === null || value === undefined ? [1] : [0, value]
}

// //// 按客户端 SummonMateTools 生成大厅队伍 [@x380kkm 2026-07-23] ////
function createManaNodeMap(nodeIds: number[]): Record<string, number> {
    return Object.fromEntries(nodeIds.map((nodeId) => [nodeId.toString(), 0]))
}

function createLobbyCharacter(character: ClientNpcCharacter): Record<string, unknown> {
    return {
        id: character.id,
        evolution_level: character.evolution_level,
        exp: character.exp,
        over_limit_step: character.over_limit_step,
        mana_node_ids: createManaNodeMap(character.mana_node_ids),
        illustration_settings: [1],
        ex_boost: encodeOption(character.ex_boost),
    }
}

function createLobbyEquipment(equipment: ClientNpcEquipment): Record<string, number> {
    return {
        equipmentId: equipment.equipment_id,
        level: equipment.level,
        enhancementLevel: equipment.enhancement_level,
    }
}

function createLobbyParty(selection: SelectedNpcFillMate): Record<string, unknown> {
    const party = selection.clientMate.party
    return {
        characters: party.characters.map((character) => encodeOption(character === null ? null : createLobbyCharacter(character))),
        unison_characters: party.unison_characters.map((character) => encodeOption(character === null ? null : createLobbyCharacter(character))),
        equipments: party.equipments.map((equipment) => encodeOption(equipment === null ? null : createLobbyEquipment(equipment))),
        abilitySoulIds: party.ability_soul_ids.map((abilitySoulId) => encodeOption(abilitySoulId)),
        options: null,
    }
}
// //// /按客户端 SummonMateTools 生成大厅队伍 ////

// //// 验证客户端回传的 summon COM 数据 [@x380kkm 2026-07-23] ////
function createCnLobbyNpcRequest(selection: SelectedNpcFillMate, name: string): Record<string, unknown> {
    const mate = selection.clientMate
    return {
        degreeId: mate.degree_id,
        rank: mate.rank,
        name,
        comId: mate.com_id,
        party: createLobbyParty(selection),
    }
}

export function validateCnLobbyNpcRequestsAndReadNames(
    requests: unknown,
    selections: SelectedNpcFillMate[],
): string[] | null {
    if (!Array.isArray(requests) || requests.length !== selections.length) return null
    const names: string[] = []
    for (let index = 0; index < requests.length; index++) {
        const request = requests[index]
        if (typeof request !== "object" || request === null || Array.isArray(request)) return null
        const name = (request as Record<string, unknown>).name
        if (typeof name !== "string" || name.length === 0) return null
        if (!isDeepStrictEqual(request, createCnLobbyNpcRequest(selections[index], name))) return null
        names.push(name)
    }
    return names
}
// //// /验证客户端回传的 summon COM 数据 ////

// //// 生成抓包确认的 CN 大厅 COM 玩家 [@x380kkm 2026-07-23] ////
export function createCnLobbyNpcPlayer(
    selection: SelectedNpcFillMate,
    name: string,
    roomNumber: string,
    position: number,
    entryTime: number,
): CnLobbyNpcPlayer {
    const mate = selection.clientMate
    return {
        viewerId: 900000000 + position,
        comId: mate.com_id,
        name,
        rank: mate.rank,
        degreeId: mate.degree_id,
        playerRoleKind: 99,
        party: createLobbyParty(selection),
        connectionId: `${roomNumber}-npc-${position}`,
        autoplayMode: false,
        autoskillMode: 1,
        autoSpeedLevel: 1,
        autoStart: false,
        skillAbilityBehaviorMode: 1,
        dashBehaviorMode: 1,
        allowHealFromOtherPlayers: true,
        state: [0],
        entryTime,
        isNewbie: false,
        isHost: false,
    }
}
// //// /生成抓包确认的 CN 大厅 COM 玩家 ////
