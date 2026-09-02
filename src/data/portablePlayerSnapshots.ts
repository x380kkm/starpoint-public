// audience: internal
// # portable-player-snapshots
//
// 该模块保存导入 Node 实例的来源快照和导入时规范基线.
// 两个 JSON 文档与玩家存档使用同一个 SQLite 事务边界.

import getDatabase, { Database } from "."

const database = getDatabase(Database.WDFP_DATA)

export interface PortablePlayerSnapshot {
    source: Record<string, unknown>
    baseline: Record<string, unknown>
}

// //// 读取和替换玩家的可移植来源快照 [@x380kkm 2026-08-03] ////
export function getPortablePlayerSnapshotSync(playerId: number): PortablePlayerSnapshot | null {
    const row = database.prepare(`
        SELECT source_json, baseline_json
        FROM player_portable_snapshots
        WHERE player_id = ?
    `).get(playerId) as { source_json: string, baseline_json: string } | undefined
    if (row === undefined) return null
    return {
        source: parseSnapshot(row.source_json),
        baseline: parseSnapshot(row.baseline_json),
    }
}

export function setPortablePlayerSnapshotSync(
    playerId: number,
    snapshot: PortablePlayerSnapshot,
): void {
    database.prepare(`
        INSERT INTO player_portable_snapshots (player_id, source_json, baseline_json)
        VALUES (?, ?, ?)
        ON CONFLICT(player_id) DO UPDATE SET
            source_json = excluded.source_json,
            baseline_json = excluded.baseline_json
    `).run(
        playerId,
        JSON.stringify(snapshot.source),
        JSON.stringify(snapshot.baseline),
    )
}

function parseSnapshot(serialized: string): Record<string, unknown> {
    const value: unknown = JSON.parse(serialized)
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
        throw new Error("Portable player snapshot is invalid.")
    }
    return value as Record<string, unknown>
}
// //// /读取和替换玩家的可移植来源快照 ////
