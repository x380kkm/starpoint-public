// audience: internal
// # save-revisions
// 此模块保存远端实例玩家槽的不可变完整快照.
// 当前指针提供乐观并发控制.
// revision 内容创建后不更新.
// 覆盖和恢复会追加 revision.
// 覆盖和恢复会移动当前指针.

import { randomUUID } from "crypto"
import getDatabase, { Database } from "."
import { calculatePortableGameDataSha256 } from "../games/starpoint/portableSave"

const db = getDatabase(Database.WDFP_DATA)

interface RawSaveRevision {
    id: string
    player_id: number
    parent_revision_id: string | null
    payload_sha256: string
    data_json: string
    label: string
    created_at: string
    pinned: number
}

export interface SaveRevision {
    id: string
    playerId: number
    parentRevisionId: string | null
    etag: string
    data: Record<string, unknown>
    label: string
    createdAt: string
    pinned: boolean
}

interface CreateSaveRevisionInput {
    playerId: number
    data: unknown
    label: string
    createdAt?: string
}

function buildSaveRevision(raw: RawSaveRevision): SaveRevision {
    return {
        id: raw.id,
        playerId: raw.player_id,
        parentRevisionId: raw.parent_revision_id,
        etag: raw.payload_sha256,
        data: JSON.parse(raw.data_json) as Record<string, unknown>,
        label: raw.label,
        createdAt: raw.created_at,
        pinned: raw.pinned === 1,
    }
}

function normalizeSaveData(data: unknown): Record<string, unknown> {
    if (typeof data !== "object" || data === null || Array.isArray(data)) {
        throw new Error("Save revision data must be an object.")
    }
    return data as Record<string, unknown>
}

// //// 读取当前 revision [@x380kkm 2026-07-27] ////
export function getCurrentSaveRevisionSync(playerId: number): SaveRevision | null {
    const raw = db.prepare(`
    SELECT revisions.*
    FROM player_save_heads AS heads
    JOIN player_save_revisions AS revisions ON revisions.id = heads.revision_id
    WHERE heads.player_id = ?
    `).get(playerId) as RawSaveRevision | undefined
    return raw === undefined ? null : buildSaveRevision(raw)
}
// //// /读取当前 revision ////

// //// 按 ID 读取玩家 revision [@x380kkm 2026-07-27] ////
export function getSaveRevisionSync(playerId: number, revisionId: string): SaveRevision | null {
    const raw = db.prepare(`
    SELECT *
    FROM player_save_revisions
    WHERE player_id = ? AND id = ?
    `).get(playerId, revisionId) as RawSaveRevision | undefined
    return raw === undefined ? null : buildSaveRevision(raw)
}
// //// /按 ID 读取玩家 revision ////

// //// 列出玩家历史 revision [@x380kkm 2026-07-27] ////
export function listSaveRevisionsSync(playerId: number): SaveRevision[] {
    const rows = db.prepare(`
    SELECT *
    FROM player_save_revisions
    WHERE player_id = ?
    ORDER BY created_at DESC, id DESC
    `).all(playerId) as RawSaveRevision[]
    return rows.map(buildSaveRevision)
}
// //// /列出玩家历史 revision ////

// //// 追加 revision 并原子移动当前指针 [@x380kkm 2026-07-27] ////
const createSaveRevisionTransaction = db.transaction((input: CreateSaveRevisionInput): SaveRevision => {
    const data = normalizeSaveData(input.data)
    const etag = calculatePortableGameDataSha256(data)
    const current = getCurrentSaveRevisionSync(input.playerId)
    if (current?.etag === etag) return current

    const revision: SaveRevision = {
        id: randomUUID(),
        playerId: input.playerId,
        parentRevisionId: current?.id ?? null,
        etag,
        data,
        label: input.label,
        createdAt: input.createdAt ?? new Date().toISOString(),
        pinned: false,
    }
    db.prepare(`
    INSERT INTO player_save_revisions (
        id, player_id, parent_revision_id, payload_sha256, data_json, label, created_at, pinned
    ) VALUES (?, ?, ?, ?, ?, ?, ?, 0)
    `).run(
        revision.id,
        revision.playerId,
        revision.parentRevisionId,
        revision.etag,
        JSON.stringify(revision.data),
        revision.label,
        revision.createdAt,
    )
    db.prepare(`
    INSERT INTO player_save_heads (player_id, revision_id, updated_at)
    VALUES (?, ?, ?)
    ON CONFLICT(player_id) DO UPDATE SET
        revision_id = excluded.revision_id,
        updated_at = excluded.updated_at
    `).run(revision.playerId, revision.id, revision.createdAt)
    return revision
})

export function createSaveRevisionSync(input: CreateSaveRevisionInput): SaveRevision {
    return createSaveRevisionTransaction(input)
}
// //// /追加 revision 并原子移动当前指针 ////

// //// 原子提交玩家数据和 revision 指针变更 [@x380kkm 2026-07-27] ////
export function commitSaveRevisionChangeSync<T>(change: () => T): T {
    return db.transaction(change)()
}
// //// /原子提交玩家数据和 revision 指针变更 ////

// //// 规范化 revision ETag [@x380kkm 2026-07-27] ////
export function normalizeRevisionEtag(value: string | string[] | undefined): string | null {
    if (value === undefined || Array.isArray(value)) return null
    const weakNormalized = value.trim().replace(/^W\//, "")
    const normalized = weakNormalized.startsWith('"') && weakNormalized.endsWith('"')
        ? weakNormalized.slice(1, -1)
        : weakNormalized
    if (normalized.includes('"')) return null
    return /^[a-f0-9]{64}$/.test(normalized) ? normalized : null
}
// //// /规范化 revision ETag ////

// //// 比较客户端父版本和服务器当前版本 [@x380kkm 2026-07-27] ////
export function hasSaveRevisionConflict(playerId: number, expectedEtag: string | null): boolean {
    if (expectedEtag === null) return false
    return getCurrentSaveRevisionSync(playerId)?.etag !== expectedEtag
}
// //// /比较客户端父版本和服务器当前版本 ////
