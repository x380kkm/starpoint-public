// audience: internal
// # cn-rush-character-references-test
// 狂热激战排名, 历史队伍与载入响应按真实角色主表处理空槽和失效引用.

import assert from "node:assert/strict"
import { mkdtempSync, rmSync } from "node:fs"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"

const require = createRequire(import.meta.url)
const root = mkdtempSync(join(tmpdir(), "starpoint-rush-reference-"))
const previous = process.env.DATABASE_PATH
process.env.DATABASE_PATH = join(root, "players.sqlite3")
let database
try {
    require("ts-node/register")
    database = require("../../src/data/index.ts").default(0)
    const { getDefaultPlayerData, serializeRushEvent } = require("../../src/data/utils.ts")
    const { serializePlayerRushEventPlayedParty } = require("../../src/data/wdfpData.ts")
    const { getPlayerRushEventEndlessBattleRankingSync } = require("../../src/lib/rush.ts")
    const { characterReference } = require("../../src/lib/characterReference.ts")
    const event = {
        eventId: 700006, activeRushBattleFolderId: null, endlessBattleNextRound: 13,
        endlessBattleMaxRound: 12, endlessBattleMaxRoundTime: 12345,
        endlessBattleMaxRoundCharacterIds: [0, 99999999, 111001],
        endlessBattleMaxRoundCharacterEvolutionImgLvls: [0, 0, 2],
    }
    const before = JSON.stringify(event)
    const ranking = getPlayerRushEventEndlessBattleRankingSync(1, 700006, {
        playerData: getDefaultPlayerData(), rushEventData: event, rankNumber: 1,
    })
    assert.equal(ranking.best_round, 12)
    assert.equal(ranking.elapsed_time_ms, 12345)
    assert.deepEqual(ranking.party_member_list, [{ character_id: 111001, evolution_img_level: 2 }])
    const summary = serializeRushEvent(event)
    assert.equal(summary.endless_battle_max_round_character_id_1, null)
    assert.equal(summary.endless_battle_max_round_character_id_2, null)
    assert.equal(summary.endless_battle_max_round_character_id_3, 111001)
    const detail = serializePlayerRushEventPlayedParty({
        characterIds: event.endlessBattleMaxRoundCharacterIds, unisonCharacterIds: [null, 111002, 0],
        equipmentIds: [5100001, null, null], abilitySoulIds: [null, null, null],
        evolutionImgLevels: [0, 0, 2], unisonEvolutionImgLevels: [null, 1, 0],
    })
    assert.equal(detail.character_id_1, null)
    assert.equal(detail.character_id_2, null)
    assert.equal(detail.character_id_3, 111001)
    assert.equal(detail.unison_character_id_3, null)
    assert.equal(detail.equipment_id_1, 5100001)
    assert.equal(characterReference(0), null)
    assert.equal(characterReference(99999999), null)
    assert.equal(characterReference(111001), 111001)
    assert.equal(JSON.stringify(event), before)
    console.log("狂热激战的排名, 载入和历史队伍保留成绩, 空槽和失效角色引用使用 null.")
} finally {
    database?.close()
    if (previous === undefined) delete process.env.DATABASE_PATH
    else process.env.DATABASE_PATH = previous
    rmSync(root, { recursive: true, force: true })
}
