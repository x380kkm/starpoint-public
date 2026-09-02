// audience: internal
// # server-transfer-conflict-store
//
// 此模块在 WDFP SQLite 中保存完整服务器的传输冲突.
// 冲突变化与对应 binding revision 更新在同一个事务中提交.

import { randomBytes } from "crypto"
import { getServerTransferBindingSync } from "./serverTransferBindingStore"
import {
    ServerTransferBinding,
    ServerTransferConflict,
    ServerTransferConflictStatus,
} from "./serverTransferBindingTypes"
import {
    ServerTransferBindingStoreError,
    getNextServerTransferRunAt,
    requireServerTransferBindingRevision,
    serverTransferDatabase as db,
} from "./serverTransferStoreSupport"

interface ServerTransferConflictRow {
    id: string
    binding_id: string
    source_revision_id: string
    source_etag: string
    target_revision_id: string | null
    target_etag: string
    detected_at: string
    status: ServerTransferConflictStatus
    resolved_at: string | null
}

function mapConflict(row: ServerTransferConflictRow): ServerTransferConflict {
    return {
        id: row.id,
        bindingId: row.binding_id,
        sourceRevisionId: row.source_revision_id,
        sourceEtag: row.source_etag,
        targetRevisionId: row.target_revision_id,
        targetEtag: row.target_etag,
        detectedAt: row.detected_at,
        status: row.status,
        resolvedAt: row.resolved_at,
    }
}

// //// 保存和解决传输冲突 [@x380kkm 2026-08-04] ////
const recordConflictTransaction = db.transaction((
    binding: ServerTransferBinding,
    sourceRevisionId: string,
    sourceEtag: string,
    targetRevisionId: string | null,
    targetEtag: string,
): ServerTransferConflict => {
    const existing = getOpenServerTransferConflictSync(binding.id)
    if (existing !== null) return existing
    const id = randomBytes(16).toString("hex")
    const now = new Date()
    db.prepare(`
    INSERT INTO server_transfer_conflicts (
        id, binding_id, source_revision_id, source_etag,
        target_revision_id, target_etag, detected_at, status, resolved_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, 'open', NULL)
    `).run(
        id,
        binding.id,
        sourceRevisionId,
        sourceEtag,
        targetRevisionId,
        targetEtag,
        now.toISOString(),
    )
    const updated = db.prepare(`
    UPDATE server_transfer_bindings
    SET last_source_etag = ?,
        last_target_etag = ?,
        pending_direction = 'conflict',
        next_run_at = ?,
        last_error = 'transfer_conflict',
        revision = revision + 1,
        updated_at = ?
    WHERE id = ? AND revision = ?
    `).run(
        sourceEtag,
        targetEtag,
        getNextServerTransferRunAt(binding.intervalSeconds, now),
        now.toISOString(),
        binding.id,
        binding.revision,
    )
    requireServerTransferBindingRevision(updated.changes)
    return getServerTransferConflictSync(id) as ServerTransferConflict
})

export function recordServerTransferConflictSync(
    binding: ServerTransferBinding,
    sourceRevisionId: string,
    sourceEtag: string,
    targetRevisionId: string | null,
    targetEtag: string,
): ServerTransferConflict {
    return recordConflictTransaction(
        binding,
        sourceRevisionId,
        sourceEtag,
        targetRevisionId,
        targetEtag,
    )
}

const refreshConflictTransaction = db.transaction((
    binding: ServerTransferBinding,
    conflict: ServerTransferConflict,
    sourceRevisionId: string,
    sourceEtag: string,
    targetRevisionId: string | null,
    targetEtag: string,
): ServerTransferConflict => {
    const now = new Date()
    const conflictUpdate = db.prepare(`
    UPDATE server_transfer_conflicts
    SET source_revision_id = ?,
        source_etag = ?,
        target_revision_id = ?,
        target_etag = ?,
        detected_at = ?
    WHERE id = ? AND binding_id = ? AND status = 'open'
    `).run(
        sourceRevisionId,
        sourceEtag,
        targetRevisionId,
        targetEtag,
        now.toISOString(),
        conflict.id,
        binding.id,
    )
    if (conflictUpdate.changes !== 1) {
        throw new ServerTransferBindingStoreError("conflict_changed")
    }
    const bindingUpdate = db.prepare(`
    UPDATE server_transfer_bindings
    SET last_source_etag = ?,
        last_target_etag = ?,
        pending_direction = 'conflict',
        next_run_at = ?,
        last_error = 'transfer_conflict_changed',
        revision = revision + 1,
        updated_at = ?
    WHERE id = ? AND revision = ?
    `).run(
        sourceEtag,
        targetEtag,
        getNextServerTransferRunAt(binding.intervalSeconds, now),
        now.toISOString(),
        binding.id,
        binding.revision,
    )
    requireServerTransferBindingRevision(bindingUpdate.changes)
    return getServerTransferConflictSync(conflict.id) as ServerTransferConflict
})

export function refreshServerTransferConflictSync(
    binding: ServerTransferBinding,
    conflict: ServerTransferConflict,
    sourceRevisionId: string,
    sourceEtag: string,
    targetRevisionId: string | null,
    targetEtag: string,
): ServerTransferConflict {
    return refreshConflictTransaction(
        binding,
        conflict,
        sourceRevisionId,
        sourceEtag,
        targetRevisionId,
        targetEtag,
    )
}

export function getServerTransferConflictSync(conflictId: string): ServerTransferConflict | null {
    const row = db.prepare(`
    SELECT * FROM server_transfer_conflicts WHERE id = ?
    `).get(conflictId) as ServerTransferConflictRow | undefined
    return row === undefined ? null : mapConflict(row)
}

export function getOpenServerTransferConflictSync(bindingId: string): ServerTransferConflict | null {
    const row = db.prepare(`
    SELECT * FROM server_transfer_conflicts
    WHERE binding_id = ? AND status = 'open'
    `).get(bindingId) as ServerTransferConflictRow | undefined
    return row === undefined ? null : mapConflict(row)
}

export function listServerTransferConflictsSync(bindingId: string): ServerTransferConflict[] {
    const rows = db.prepare(`
    SELECT * FROM server_transfer_conflicts
    WHERE binding_id = ?
    ORDER BY detected_at DESC, id DESC
    `).all(bindingId) as ServerTransferConflictRow[]
    return rows.map(mapConflict)
}

const resolveConflictTransaction = db.transaction((
    binding: ServerTransferBinding,
    conflict: ServerTransferConflict,
    status: Exclude<ServerTransferConflictStatus, "open">,
    commonEtag: string | null,
    sourceEtag: string | null,
    targetEtag: string | null,
    disableBinding: boolean,
): ServerTransferBinding => {
    const now = new Date()
    const conflictUpdate = db.prepare(`
    UPDATE server_transfer_conflicts
    SET status = ?, resolved_at = ?
    WHERE id = ? AND binding_id = ? AND status = 'open'
    `).run(status, now.toISOString(), conflict.id, binding.id)
    if (conflictUpdate.changes !== 1) {
        throw new ServerTransferBindingStoreError("conflict_changed")
    }
    const bindingUpdate = db.prepare(`
    UPDATE server_transfer_bindings
    SET enabled = CASE WHEN ? THEN 0 ELSE enabled END,
        last_common_etag = COALESCE(?, last_common_etag),
        last_source_etag = COALESCE(?, last_source_etag),
        last_target_etag = COALESCE(?, last_target_etag),
        pending_direction = 'none',
        next_run_at = ?,
        last_synced_at = CASE WHEN ? IS NULL THEN last_synced_at ELSE ? END,
        last_error = NULL,
        revision = revision + 1,
        updated_at = ?
    WHERE id = ? AND revision = ?
    `).run(
        disableBinding ? 1 : 0,
        commonEtag,
        sourceEtag,
        targetEtag,
        getNextServerTransferRunAt(binding.intervalSeconds, now),
        commonEtag,
        now.toISOString(),
        now.toISOString(),
        binding.id,
        binding.revision,
    )
    requireServerTransferBindingRevision(bindingUpdate.changes)
    return getServerTransferBindingSync(binding.id) as ServerTransferBinding
})

export function resolveServerTransferConflictSync(
    binding: ServerTransferBinding,
    conflict: ServerTransferConflict,
    status: Exclude<ServerTransferConflictStatus, "open">,
    etags: {
        common: string | null
        source: string | null
        target: string | null
    },
    disableBinding: boolean = false,
): ServerTransferBinding {
    return resolveConflictTransaction(
        binding,
        conflict,
        status,
        etags.common,
        etags.source,
        etags.target,
        disableBinding,
    )
}
// //// /保存和解决传输冲突 ////
