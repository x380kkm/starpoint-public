// audience: internal | external
// # encrypted-save-store
// 该模块按管理用户保存客户端生成的加密存档封装. 服务端不接收密钥且不解密内容.

import sqlite3, { Database as BetterSqlite3Database } from "better-sqlite3"
import { createHash } from "crypto"

export const ENCRYPTED_SAVE_BODY_LIMIT_BYTES = 8 * 1024 * 1024
const MAX_ENCRYPTED_SAVES_PER_USER = 32

export interface EncryptedSaveMetadata {
    objectId: string
    bytes: number
    sha256: string
    createdAt: string
    updatedAt: string
}

export interface StoredEncryptedSave extends EncryptedSaveMetadata {
    envelopeJson: string
}

export type EncryptedSaveWriteCondition =
    | { type: "create" }
    | { type: "replace", sha256: string }

export interface EncryptedSaveWriteResult {
    created: boolean
    metadata: EncryptedSaveMetadata
}

interface EncryptedSaveRow {
    object_id: string
    envelope_json: string
    bytes: number
    sha256: string
    created_at: number
    updated_at: number
}

export class EncryptedSaveCapacityError extends Error {}
export class EncryptedSaveConflictError extends Error {}

function mapMetadata(row: EncryptedSaveRow): EncryptedSaveMetadata {
    return {
        objectId: row.object_id,
        bytes: row.bytes,
        sha256: row.sha256,
        createdAt: new Date(row.created_at * 1000).toISOString(),
        updatedAt: new Date(row.updated_at * 1000).toISOString(),
    }
}

export function parseEncryptedSaveObjectId(value: string): string | null {
    return /^[A-Za-z0-9_-]{1,64}$/.test(value) ? value : null
}

function requireObjectId(objectId: string): void {
    if (parseEncryptedSaveObjectId(objectId) === null) throw new Error("Encrypted save object id is invalid.")
}

export class EncryptedSaveStore {
    private readonly database: BetterSqlite3Database

    constructor(databasePath: string) {
        this.database = new sqlite3(databasePath)
        this.initializeSchema()
    }

    // //// 创建加密存档表 [@x380kkm 2026-07-23] ////
    private initializeSchema(): void {
        this.database.pragma("foreign_keys = ON")
        this.database.pragma("journal_mode = WAL")
        this.database.pragma("synchronous = FULL")
        this.database.pragma("busy_timeout = 5000")
        this.database.exec(`
            CREATE TABLE IF NOT EXISTS management_encrypted_saves (
                user_id INTEGER NOT NULL REFERENCES management_users(id) ON DELETE CASCADE,
                object_id TEXT NOT NULL,
                envelope_json TEXT NOT NULL,
                bytes INTEGER NOT NULL CHECK (bytes > 0 AND bytes <= ${ENCRYPTED_SAVE_BODY_LIMIT_BYTES}),
                sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, object_id)
            );
            CREATE INDEX IF NOT EXISTS management_encrypted_saves_updated_at
                ON management_encrypted_saves(user_id, updated_at DESC);
        `)
    }
    // //// /创建加密存档表 ////

    // //// 保存并列出用户的加密存档 [@x380kkm 2026-07-23] ////
    put(
        userId: number,
        objectId: string,
        envelopeJson: string,
        condition: EncryptedSaveWriteCondition,
    ): EncryptedSaveWriteResult {
        requireObjectId(objectId)
        const bytes = Buffer.byteLength(envelopeJson)
        if (bytes === 0 || bytes > ENCRYPTED_SAVE_BODY_LIMIT_BYTES) {
            throw new Error("Encrypted save envelope exceeds the storage limit.")
        }
        const sha256 = createHash("sha256").update(envelopeJson).digest("hex")
        const now = Math.floor(Date.now() / 1000)
        let created = false
        const persistEncryptedSave = this.database.transaction(() => {
            const current = this.database.prepare(`
                SELECT sha256 FROM management_encrypted_saves
                WHERE user_id = ? AND object_id = ?
            `).get(userId, objectId) as { sha256: string } | undefined
            if (condition.type === "create" ? current !== undefined : current?.sha256 !== condition.sha256) {
                throw new EncryptedSaveConflictError("Encrypted save write condition does not match.")
            }
            created = current === undefined
            if (created) {
                const count = this.database.prepare(`
                    SELECT COUNT(*) AS count FROM management_encrypted_saves WHERE user_id = ?
                `).get(userId) as { count: number }
                if (count.count >= MAX_ENCRYPTED_SAVES_PER_USER) {
                    throw new EncryptedSaveCapacityError("Encrypted save capacity is exhausted.")
                }
            }
            this.database.prepare(`
                INSERT INTO management_encrypted_saves (
                    user_id, object_id, envelope_json, bytes, sha256, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id, object_id) DO UPDATE SET
                    envelope_json = excluded.envelope_json,
                    bytes = excluded.bytes,
                    sha256 = excluded.sha256,
                    updated_at = excluded.updated_at
            `).run(userId, objectId, envelopeJson, bytes, sha256, now, now)
        })
        persistEncryptedSave.immediate()
        const saved = this.get(userId, objectId)
        if (saved === null) throw new Error("Encrypted save was not persisted.")
        return {
            created,
            metadata: {
                objectId: saved.objectId,
                bytes: saved.bytes,
                sha256: saved.sha256,
                createdAt: saved.createdAt,
                updatedAt: saved.updatedAt,
            },
        }
    }

    list(userId: number): EncryptedSaveMetadata[] {
        const rows = this.database.prepare(`
            SELECT object_id, envelope_json, bytes, sha256, created_at, updated_at
            FROM management_encrypted_saves
            WHERE user_id = ?
            ORDER BY updated_at DESC, object_id
        `).all(userId) as EncryptedSaveRow[]
        return rows.map(mapMetadata)
    }
    // //// /保存并列出用户的加密存档 ////

    // //// 读取和删除用户的加密存档 [@x380kkm 2026-07-23] ////
    get(userId: number, objectId: string): StoredEncryptedSave | null {
        requireObjectId(objectId)
        const row = this.database.prepare(`
            SELECT object_id, envelope_json, bytes, sha256, created_at, updated_at
            FROM management_encrypted_saves
            WHERE user_id = ? AND object_id = ?
        `).get(userId, objectId) as EncryptedSaveRow | undefined
        return row === undefined ? null : { ...mapMetadata(row), envelopeJson: row.envelope_json }
    }

    delete(userId: number, objectId: string): boolean {
        requireObjectId(objectId)
        return this.database.prepare(`
            DELETE FROM management_encrypted_saves WHERE user_id = ? AND object_id = ?
        `).run(userId, objectId).changes > 0
    }
    // //// /读取和删除用户的加密存档 ////

    close(): void {
        if (!this.database.open) return
        this.database.pragma("wal_checkpoint(TRUNCATE)")
        this.database.close()
    }
}
