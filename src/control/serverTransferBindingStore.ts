// audience: internal
// # server-transfer-binding-store
//
// 此模块在 WDFP SQLite 中保存完整服务器的传输绑定和同步调度状态.
// 所有状态提交使用 binding revision 防止异步远端请求覆盖新配置.

import { randomBytes } from "crypto"
import {
    CreateServerTransferBindingInput,
    ServerTransferBinding,
    UpdateServerTransferBindingInput,
} from "./serverTransferBindingTypes"
import {
    ServerTransferBindingStoreError,
    getNextServerTransferRunAt,
    requireServerTransferBindingRevision,
    serverTransferDatabase as db,
} from "./serverTransferStoreSupport"

interface ServerTransferBindingRow {
    id: string
    source_player_id: number
    target_base_url: string
    target_instance_id: string
    target_shell_id: string
    target_player_id: number
    target_token: string
    upload_mode: "manual" | "interval"
    pull_mode: "manual" | "interval"
    conflict_policy: "local_wins" | "remote_wins" | "ask"
    interval_seconds: number
    enabled: number
    last_common_etag: string | null
    last_source_etag: string | null
    last_target_etag: string | null
    pending_direction: "none" | "upload" | "pull" | "conflict"
    next_run_at: string
    last_synced_at: string | null
    last_error: string | null
    revision: number
    created_at: string
    updated_at: string
}

function mapBinding(row: ServerTransferBindingRow): ServerTransferBinding {
    return {
        id: row.id,
        sourcePlayerId: row.source_player_id,
        targetBaseUrl: row.target_base_url,
        targetInstanceId: row.target_instance_id,
        targetShellId: row.target_shell_id,
        targetPlayerId: row.target_player_id,
        targetToken: row.target_token,
        uploadMode: row.upload_mode,
        pullMode: row.pull_mode,
        conflictPolicy: row.conflict_policy,
        intervalSeconds: row.interval_seconds,
        enabled: row.enabled === 1,
        lastCommonEtag: row.last_common_etag,
        lastSourceEtag: row.last_source_etag,
        lastTargetEtag: row.last_target_etag,
        pendingDirection: row.pending_direction,
        nextRunAt: row.next_run_at,
        lastSyncedAt: row.last_synced_at,
        lastError: row.last_error,
        revision: row.revision,
        createdAt: row.created_at,
        updatedAt: row.updated_at,
    }
}

function getConstraintCode(error: unknown): string | null {
    if (typeof error !== "object" || error === null || !("code" in error)) return null
    return typeof error.code === "string" ? error.code : null
}

// //// 创建和读取传输绑定 [@x380kkm 2026-08-04] ////
export function createServerTransferBindingSync(
    input: CreateServerTransferBindingInput,
): ServerTransferBinding {
    const sourceExists = db.prepare("SELECT 1 FROM players WHERE id = ?").get(input.sourcePlayerId)
    if (sourceExists === undefined) {
        throw new ServerTransferBindingStoreError("source_player_not_found")
    }
    const id = randomBytes(16).toString("hex")
    const now = new Date().toISOString()
    try {
        db.prepare(`
        INSERT INTO server_transfer_bindings (
            id, source_player_id, target_base_url, target_instance_id,
            target_shell_id, target_player_id, target_token,
            upload_mode, pull_mode, conflict_policy, interval_seconds, enabled,
            last_common_etag, last_source_etag, last_target_etag,
            pending_direction, next_run_at, last_synced_at, last_error,
            revision, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            NULL, ?, ?, 'none', ?, NULL, NULL, 0, ?, ?
        )
        `).run(
            id,
            input.sourcePlayerId,
            input.targetBaseUrl,
            input.targetInstanceId,
            input.targetShellId,
            input.targetPlayerId,
            input.targetToken,
            input.uploadMode,
            input.pullMode,
            input.conflictPolicy,
            input.intervalSeconds,
            input.enabled ? 1 : 0,
            input.observedSourceEtag,
            input.observedTargetEtag,
            getNextServerTransferRunAt(input.intervalSeconds),
            now,
            now,
        )
    } catch (error) {
        if (getConstraintCode(error) === "SQLITE_CONSTRAINT_UNIQUE") {
            throw new ServerTransferBindingStoreError("duplicate_binding")
        }
        throw error
    }
    return getServerTransferBindingSync(id) as ServerTransferBinding
}

export function getServerTransferBindingSync(bindingId: string): ServerTransferBinding | null {
    const row = db.prepare(`
    SELECT * FROM server_transfer_bindings WHERE id = ?
    `).get(bindingId) as ServerTransferBindingRow | undefined
    return row === undefined ? null : mapBinding(row)
}

export function listServerTransferBindingsSync(sourcePlayerId: number): ServerTransferBinding[] {
    const rows = db.prepare(`
    SELECT * FROM server_transfer_bindings
    WHERE source_player_id = ?
    ORDER BY created_at, id
    `).all(sourcePlayerId) as ServerTransferBindingRow[]
    return rows.map(mapBinding)
}

export function listDueServerTransferBindingIdsSync(now: Date = new Date()): string[] {
    const rows = db.prepare(`
    SELECT id FROM server_transfer_bindings
    WHERE enabled = 1
      AND next_run_at <= ?
      AND (upload_mode = 'interval' OR pull_mode = 'interval')
      AND NOT EXISTS (
          SELECT 1 FROM server_transfer_conflicts
          WHERE binding_id = server_transfer_bindings.id AND status = 'open'
      )
    ORDER BY next_run_at, id
    `).all(now.toISOString()) as { id: string }[]
    return rows.map((row) => row.id)
}
// //// /创建和读取传输绑定 ////

// //// 更新和删除传输绑定 [@x380kkm 2026-08-04] ////
export function updateServerTransferBindingSync(
    binding: ServerTransferBinding,
    input: UpdateServerTransferBindingInput,
): ServerTransferBinding {
    const token = input.targetToken ?? binding.targetToken
    const now = new Date().toISOString()
    const result = db.prepare(`
    UPDATE server_transfer_bindings
    SET target_token = ?,
        upload_mode = ?,
        pull_mode = ?,
        conflict_policy = ?,
        interval_seconds = ?,
        enabled = ?,
        next_run_at = ?,
        last_error = NULL,
        revision = revision + 1,
        updated_at = ?
    WHERE id = ? AND source_player_id = ? AND revision = ?
    `).run(
        token,
        input.uploadMode,
        input.pullMode,
        input.conflictPolicy,
        input.intervalSeconds,
        input.enabled ? 1 : 0,
        getNextServerTransferRunAt(input.intervalSeconds),
        now,
        binding.id,
        binding.sourcePlayerId,
        binding.revision,
    )
    requireServerTransferBindingRevision(result.changes)
    return getServerTransferBindingSync(binding.id) as ServerTransferBinding
}

export function deleteServerTransferBindingSync(
    sourcePlayerId: number,
    bindingId: string,
): boolean {
    return db.prepare(`
    DELETE FROM server_transfer_bindings
    WHERE id = ? AND source_player_id = ?
    `).run(bindingId, sourcePlayerId).changes === 1
}
// //// /更新和删除传输绑定 ////

// //// 提交同步结果和错误 [@x380kkm 2026-08-04] ////
export function recordServerTransferBindingSuccessSync(
    binding: ServerTransferBinding,
    commonEtag: string,
    sourceEtag: string,
    targetEtag: string,
): ServerTransferBinding {
    const now = new Date()
    const result = db.prepare(`
    UPDATE server_transfer_bindings
    SET last_common_etag = ?,
        last_source_etag = ?,
        last_target_etag = ?,
        pending_direction = 'none',
        next_run_at = ?,
        last_synced_at = ?,
        last_error = NULL,
        revision = revision + 1,
        updated_at = ?
    WHERE id = ? AND revision = ?
    `).run(
        commonEtag,
        sourceEtag,
        targetEtag,
        getNextServerTransferRunAt(binding.intervalSeconds, now),
        now.toISOString(),
        now.toISOString(),
        binding.id,
        binding.revision,
    )
    requireServerTransferBindingRevision(result.changes)
    return getServerTransferBindingSync(binding.id) as ServerTransferBinding
}

export function recordServerTransferBindingFailureSync(
    binding: ServerTransferBinding,
    sourceEtag: string,
    targetEtag: string | null,
    errorCode: string,
    retrySeconds: number = binding.intervalSeconds,
): void {
    const now = new Date()
    const result = db.prepare(`
    UPDATE server_transfer_bindings
    SET last_source_etag = ?,
        last_target_etag = ?,
        next_run_at = ?,
        last_error = ?,
        revision = revision + 1,
        updated_at = ?
    WHERE id = ? AND revision = ?
    `).run(
        sourceEtag,
        targetEtag,
        getNextServerTransferRunAt(retrySeconds, now),
        errorCode,
        now.toISOString(),
        binding.id,
        binding.revision,
    )
    requireServerTransferBindingRevision(result.changes)
}
// //// /提交同步结果和错误 ////
