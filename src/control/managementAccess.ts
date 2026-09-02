// audience: internal | external
// # management-access
// 此模块在独立 SQLite 数据库中保存管理用户, 登录会话和玩家绑定.
// 会话数据库只保存 token 的 SHA-256 摘要, 密码只保存带随机盐的 scrypt 结果.

import sqlite3, { Database as BetterSqlite3Database } from "better-sqlite3"
import { createHash, randomBytes, scrypt, timingSafeEqual } from "crypto"
import { mkdirSync } from "fs"
import path from "path"

export type ManagementRole = "admin" | "player"

export interface ManagementUser {
    id: number
    username: string
    role: ManagementRole
    disabled: boolean
    createdAt: string
}

export interface ManagementUserOverview extends ManagementUser {
    playerIds: number[]
}

export interface ManagementPrincipal extends ManagementUser {
    authentication: "session" | "bearer"
}

export interface ManagementSession {
    token: string
    expiresAt: string
}

export type TransferPermission = "upload" | "download" | "both"

export interface TransferTokenMetadata {
    id: string
    accountId: number
    playerId: number | null
    permission: TransferPermission | null
    deviceName: string | null
    createdAt: string
    expiresAt: string | null
    revokedAt: string | null
}

export interface IssuedTransferToken {
    token: string
    instanceId: string
    metadata: TransferTokenMetadata
}

export interface CreateTransferTokenInput {
    expiresAt?: string | null
    deviceName?: string | null
}

export interface CreateSlotTransferTokenInput extends CreateTransferTokenInput {
    permission: TransferPermission
}

interface ManagementUserRow {
    id: number
    username: string
    password_hash: string
    role: ManagementRole
    disabled: number
    created_at: number
}

interface ScryptParameters {
    cost: number
    blockSize: number
    parallelization: number
}

const PASSWORD_PARAMETERS: ScryptParameters = { cost: 16384, blockSize: 8, parallelization: 1 }
const PASSWORD_KEY_BYTES = 64
const PASSWORD_SALT_BYTES = 16
const SESSION_TOKEN_BYTES = 32
const TRANSFER_TOKEN_BYTES = 32
const DEFAULT_SESSION_SECONDS = 12 * 60 * 60
const MAX_TRANSFER_TOKEN_SECONDS = 366 * 24 * 60 * 60

// //// 散列和验证管理密码 [@x380kkm 2026-07-22] ////
function derivePassword(password: string, salt: Buffer, parameters: ScryptParameters): Promise<Buffer> {
    return new Promise((resolve, reject) => {
        scrypt(password, salt, PASSWORD_KEY_BYTES, {
            N: parameters.cost,
            r: parameters.blockSize,
            p: parameters.parallelization,
            maxmem: 64 * 1024 * 1024,
        }, (error, key) => {
            if (error !== null) reject(error)
            else resolve(key)
        })
    })
}

function assertPassword(password: string): void {
    if (password.length < 10 || password.length > 256) throw new Error("Password must contain between 10 and 256 characters.")
}

async function createPasswordHash(password: string): Promise<string> {
    assertPassword(password)
    const salt = randomBytes(PASSWORD_SALT_BYTES)
    const key = await derivePassword(password, salt, PASSWORD_PARAMETERS)
    return [
        "scrypt",
        PASSWORD_PARAMETERS.cost,
        PASSWORD_PARAMETERS.blockSize,
        PASSWORD_PARAMETERS.parallelization,
        salt.toString("base64url"),
        key.toString("base64url"),
    ].join("$")
}

async function verifyPassword(password: string, encodedHash: string): Promise<boolean> {
    const parts = encodedHash.split("$")
    if (parts.length !== 6 || parts[0] !== "scrypt") return false
    const parameters = {
        cost: Number(parts[1]),
        blockSize: Number(parts[2]),
        parallelization: Number(parts[3]),
    }
    if (!Number.isInteger(parameters.cost) || !Number.isInteger(parameters.blockSize) || !Number.isInteger(parameters.parallelization)) return false
    try {
        const expected = Buffer.from(parts[5], "base64url")
        const actual = await derivePassword(password, Buffer.from(parts[4], "base64url"), parameters)
        return actual.length === expected.length && timingSafeEqual(actual, expected)
    } catch {
        return false
    }
}
// //// /散列和验证管理密码 ////

function normalizeUsername(username: string): string {
    const normalized = username.trim()
    if (!/^[A-Za-z0-9_.-]{3,64}$/.test(normalized)) {
        throw new Error("Username must contain 3 to 64 ASCII letters, digits, dots, underscores, or hyphens.")
    }
    return normalized
}

function assertRole(role: string): asserts role is ManagementRole {
    if (role !== "admin" && role !== "player") throw new Error("Role must be admin or player.")
}

function hashSessionToken(token: string): string {
    return createHash("sha256").update(token).digest("hex")
}

function hashTransferToken(kind: "shell" | "slot", token: string): string {
    return createHash("sha256").update(`starpoint-transfer-${kind}:${token}`).digest("hex")
}

function normalizeTransferPermission(value: string): TransferPermission {
    if (value === "upload" || value === "download" || value === "both") return value
    throw new Error("Transfer token permission is invalid.")
}

function normalizeTransferDeviceName(value: string | null | undefined): string | null {
    if (value === undefined || value === null) return null
    const normalized = value.trim()
    if (normalized.length === 0 || normalized.length > 64 || /[\u0000-\u001f\u007f]/.test(normalized)) {
        throw new Error("Transfer token device name is invalid.")
    }
    return normalized
}

function normalizeTransferExpiration(value: string | null | undefined, now: number): number | null {
    if (value === undefined || value === null) return null
    const parsed = Date.parse(value)
    if (!Number.isFinite(parsed)) throw new Error("Transfer token expiration is invalid.")
    const expiration = Math.floor(parsed / 1000)
    if (expiration < now + 60 || expiration > now + MAX_TRANSFER_TOKEN_SECONDS) {
        throw new Error("Transfer token expiration is outside the allowed range.")
    }
    return expiration
}

function assertTransferAccountId(accountId: number): void {
    if (!Number.isInteger(accountId) || accountId <= 0) throw new Error("Transfer token account id is invalid.")
}

function assertTransferPlayerId(playerId: number): void {
    if (!Number.isInteger(playerId) || playerId <= 0) throw new Error("Transfer token player id is invalid.")
}

function toTransferTimestamp(value: number | null): string | null {
    return value === null ? null : new Date(value * 1000).toISOString()
}

interface TransferTokenRow {
    id: string
    account_id: number
    player_id: number | null
    permission: TransferPermission | null
    device_name: string | null
    created_at: number
    expires_at: number | null
    revoked_at: number | null
}

function mapTransferToken(row: TransferTokenRow): TransferTokenMetadata {
    return {
        id: row.id,
        accountId: row.account_id,
        playerId: row.player_id,
        permission: row.permission,
        deviceName: row.device_name,
        createdAt: new Date(row.created_at * 1000).toISOString(),
        expiresAt: toTransferTimestamp(row.expires_at),
        revokedAt: toTransferTimestamp(row.revoked_at),
    }
}

function mapUser(row: ManagementUserRow): ManagementUser {
    return {
        id: row.id,
        username: row.username,
        role: row.role,
        disabled: row.disabled !== 0,
        createdAt: new Date(row.created_at * 1000).toISOString(),
    }
}

export class ManagementAccessStore {
    readonly databasePath: string
    private readonly database: BetterSqlite3Database

    constructor(databasePath: string) {
        this.databasePath = path.resolve(databasePath)
        mkdirSync(path.dirname(this.databasePath), { recursive: true })
        this.database = new sqlite3(this.databasePath)
        this.initializeSchema()
    }

    // //// 创建管理数据库结构 [@x380kkm 2026-07-22] ////
    private initializeSchema(): void {
        this.database.pragma("foreign_keys = ON")
        this.database.pragma("journal_mode = WAL")
        this.database.pragma("busy_timeout = 5000")
        this.database.exec(`
            CREATE TABLE IF NOT EXISTS management_users (
                id INTEGER PRIMARY KEY,
                username TEXT NOT NULL COLLATE NOCASE UNIQUE,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL CHECK (role IN ('admin', 'player')),
                disabled INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS management_sessions (
                token_hash TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES management_users(id) ON DELETE CASCADE,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS management_sessions_expires_at ON management_sessions(expires_at);
            CREATE TABLE IF NOT EXISTS management_player_bindings (
                user_id INTEGER NOT NULL REFERENCES management_users(id) ON DELETE CASCADE,
                player_id INTEGER NOT NULL UNIQUE,
                PRIMARY KEY (user_id, player_id)
            );
            CREATE TABLE IF NOT EXISTS transfer_token_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                instance_id TEXT NOT NULL
            );
            INSERT OR IGNORE INTO transfer_token_state (id, instance_id)
            VALUES (1, lower(hex(randomblob(16))));
            CREATE TABLE IF NOT EXISTS transfer_shell_tokens (
                id TEXT PRIMARY KEY NOT NULL,
                token_hash TEXT NOT NULL UNIQUE,
                account_id INTEGER NOT NULL,
                device_name TEXT,
                created_at INTEGER NOT NULL,
                expires_at INTEGER,
                revoked_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS transfer_shell_tokens_account
            ON transfer_shell_tokens (account_id, created_at DESC);
            CREATE TABLE IF NOT EXISTS transfer_slot_tokens (
                id TEXT PRIMARY KEY NOT NULL,
                token_hash TEXT NOT NULL UNIQUE,
                account_id INTEGER NOT NULL,
                player_id INTEGER NOT NULL,
                permission TEXT NOT NULL CHECK (permission IN ('upload', 'download', 'both')),
                device_name TEXT,
                created_at INTEGER NOT NULL,
                expires_at INTEGER,
                revoked_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS transfer_slot_tokens_player
            ON transfer_slot_tokens (account_id, player_id, created_at DESC);
        `)
    }
    // //// /创建管理数据库结构 ////

    // //// 创建和认证管理用户 [@x380kkm 2026-07-22] ////
    hasUsers(): boolean {
        const row = this.database.prepare("SELECT COUNT(*) AS count FROM management_users").get() as { count: number }
        return row.count > 0
    }

    async createUser(username: string, password: string, role: ManagementRole): Promise<ManagementUser> {
        const normalizedUsername = normalizeUsername(username)
        assertRole(role)
        const passwordHash = await createPasswordHash(password)
        const createdAt = Math.floor(Date.now() / 1000)
        const result = this.database.prepare(`
            INSERT INTO management_users (username, password_hash, role, created_at)
            VALUES (?, ?, ?, ?)
        `).run(normalizedUsername, passwordHash, role, createdAt)
        return this.getUser(Number(result.lastInsertRowid)) as ManagementUser
    }

    async createBootstrapAdmin(username: string | undefined, password: string | undefined): Promise<ManagementUser | null> {
        if (this.hasUsers() || password === undefined || password.length === 0) return null
        return this.createUser(username ?? "admin", password, "admin")
    }

    async authenticate(username: string, password: string): Promise<ManagementUser | null> {
        if (password.length > 256) return null
        let normalizedUsername: string
        try {
            normalizedUsername = normalizeUsername(username)
        } catch {
            return null
        }
        const row = this.database.prepare(`
            SELECT id, username, password_hash, role, disabled, created_at
            FROM management_users
            WHERE username = ?
        `).get(normalizedUsername) as ManagementUserRow | undefined
        if (row === undefined || row.disabled !== 0) return null
        return await verifyPassword(password, row.password_hash) ? mapUser(row) : null
    }

    getUser(userId: number): ManagementUser | null {
        const row = this.database.prepare(`
            SELECT id, username, password_hash, role, disabled, created_at
            FROM management_users
            WHERE id = ?
        `).get(userId) as ManagementUserRow | undefined
        return row === undefined ? null : mapUser(row)
    }

    listUsers(): ManagementUserOverview[] {
        const rows = this.database.prepare(`
            SELECT id, username, password_hash, role, disabled, created_at
            FROM management_users
            ORDER BY id
        `).all() as ManagementUserRow[]
        const bindings = this.database.prepare(`
            SELECT user_id, player_id
            FROM management_player_bindings
            ORDER BY player_id
        `).all() as { user_id: number, player_id: number }[]
        return rows.map((row) => ({
            ...mapUser(row),
            playerIds: bindings.filter((binding) => binding.user_id === row.id).map((binding) => binding.player_id),
        }))
    }
    // //// /创建和认证管理用户 ////

    // //// 创建和解析持久化登录会话 [@x380kkm 2026-07-22] ////
    createSession(userId: number, lifetimeSeconds: number = DEFAULT_SESSION_SECONDS): ManagementSession {
        if (!Number.isInteger(lifetimeSeconds) || lifetimeSeconds < 60 || lifetimeSeconds > 30 * 24 * 60 * 60) {
            throw new Error("Session lifetime is invalid.")
        }
        const createdAt = Math.floor(Date.now() / 1000)
        const expiresAt = createdAt + lifetimeSeconds
        const token = randomBytes(SESSION_TOKEN_BYTES).toString("base64url")
        this.database.prepare(`
            INSERT INTO management_sessions (token_hash, user_id, created_at, expires_at)
            VALUES (?, ?, ?, ?)
        `).run(hashSessionToken(token), userId, createdAt, expiresAt)
        return { token, expiresAt: new Date(expiresAt * 1000).toISOString() }
    }

    getSessionPrincipal(token: string): ManagementPrincipal | null {
        const currentTime = Math.floor(Date.now() / 1000)
        this.database.prepare("DELETE FROM management_sessions WHERE expires_at <= ?").run(currentTime)
        const row = this.database.prepare(`
            SELECT users.id, users.username, users.password_hash, users.role, users.disabled, users.created_at
            FROM management_sessions AS sessions
            JOIN management_users AS users ON users.id = sessions.user_id
            WHERE sessions.token_hash = ? AND sessions.expires_at > ? AND users.disabled = 0
        `).get(hashSessionToken(token), currentTime) as ManagementUserRow | undefined
        return row === undefined ? null : { ...mapUser(row), authentication: "session" }
    }

    revokeSession(token: string): boolean {
        return this.database.prepare("DELETE FROM management_sessions WHERE token_hash = ?").run(hashSessionToken(token)).changes > 0
    }
    // //// /创建和解析持久化登录会话 ////

    // //// 保存用户和玩家的一对一归属 [@x380kkm 2026-07-22] ////
    bindPlayer(userId: number, playerId: number): void {
        if (this.getUser(userId) === null) throw new Error("Management user does not exist.")
        if (!Number.isInteger(playerId) || playerId <= 0) throw new Error("Player id is invalid.")
        this.database.prepare(`
            INSERT INTO management_player_bindings (user_id, player_id)
            VALUES (?, ?)
        `).run(userId, playerId)
    }

    unbindPlayer(userId: number, playerId: number): boolean {
        return this.database.prepare(`
            DELETE FROM management_player_bindings
            WHERE user_id = ? AND player_id = ?
        `).run(userId, playerId).changes > 0
    }

    getBoundPlayerIds(userId: number): number[] {
        const rows = this.database.prepare(`
            SELECT player_id
            FROM management_player_bindings
            WHERE user_id = ?
            ORDER BY player_id
        `).all(userId) as { player_id: number }[]
        return rows.map((row) => row.player_id)
    }

    canAccessPlayer(principal: ManagementPrincipal, playerId: number): boolean {
        if (principal.role === "admin") return true
        const row = this.database.prepare(`
            SELECT 1
            FROM management_player_bindings
            WHERE user_id = ? AND player_id = ?
        `).get(principal.id, playerId)
        return row !== undefined
    }
    // //// /保存用户和玩家的一对一归属 ////

    // //// 签发和列出壳 transfer token [@x380kkm 2026-07-27] ////
    getTransferInstanceId(): string {
        const row = this.database.prepare(`
            SELECT instance_id
            FROM transfer_token_state
            WHERE id = 1
        `).get() as { instance_id: string } | undefined
        if (row === undefined) throw new Error("Transfer token instance id is missing.")
        return row.instance_id
    }

    issueShellTransferToken(accountId: number, input: CreateTransferTokenInput = {}): IssuedTransferToken {
        assertTransferAccountId(accountId)
        const now = Math.floor(Date.now() / 1000)
        const expiresAt = normalizeTransferExpiration(input.expiresAt, now)
        const deviceName = normalizeTransferDeviceName(input.deviceName)
        const id = randomBytes(16).toString("hex")
        const token = `spt_shell_${randomBytes(TRANSFER_TOKEN_BYTES).toString("base64url")}`
        this.database.prepare(`
            INSERT INTO transfer_shell_tokens (
                id, token_hash, account_id, device_name, created_at, expires_at, revoked_at
            ) VALUES (?, ?, ?, ?, ?, ?, NULL)
        `).run(id, hashTransferToken("shell", token), accountId, deviceName, now, expiresAt)
        return {
            token,
            instanceId: this.getTransferInstanceId(),
            metadata: {
                id,
                accountId,
                playerId: null,
                permission: null,
                deviceName,
                createdAt: new Date(now * 1000).toISOString(),
                expiresAt: toTransferTimestamp(expiresAt),
                revokedAt: null,
            },
        }
    }

    listShellTransferTokens(accountId: number): TransferTokenMetadata[] {
        assertTransferAccountId(accountId)
        const rows = this.database.prepare(`
            SELECT id, account_id, NULL AS player_id, NULL AS permission,
                   device_name, created_at, expires_at, revoked_at
            FROM transfer_shell_tokens
            WHERE account_id = ?
            ORDER BY created_at DESC, id DESC
        `).all(accountId) as TransferTokenRow[]
        return rows.map(mapTransferToken)
    }

    revokeShellTransferToken(accountId: number, tokenId: string): boolean {
        assertTransferAccountId(accountId)
        const now = Math.floor(Date.now() / 1000)
        return this.database.prepare(`
            UPDATE transfer_shell_tokens
            SET revoked_at = ?
            WHERE id = ? AND account_id = ? AND revoked_at IS NULL
        `).run(now, tokenId, accountId).changes > 0
    }
    // //// /签发和列出壳 transfer token ////

    // //// 签发和列出槽 transfer token [@x380kkm 2026-07-27] ////
    issueSlotTransferToken(
        accountId: number,
        playerId: number,
        input: CreateSlotTransferTokenInput,
    ): IssuedTransferToken {
        assertTransferAccountId(accountId)
        assertTransferPlayerId(playerId)
        const now = Math.floor(Date.now() / 1000)
        const permission = normalizeTransferPermission(input.permission)
        const expiresAt = normalizeTransferExpiration(input.expiresAt, now)
        const deviceName = normalizeTransferDeviceName(input.deviceName)
        const id = randomBytes(16).toString("hex")
        const token = `spt_slot_${randomBytes(TRANSFER_TOKEN_BYTES).toString("base64url")}`
        this.database.prepare(`
            INSERT INTO transfer_slot_tokens (
                id, token_hash, account_id, player_id, permission,
                device_name, created_at, expires_at, revoked_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)
        `).run(
            id,
            hashTransferToken("slot", token),
            accountId,
            playerId,
            permission,
            deviceName,
            now,
            expiresAt,
        )
        return {
            token,
            instanceId: this.getTransferInstanceId(),
            metadata: {
                id,
                accountId,
                playerId,
                permission,
                deviceName,
                createdAt: new Date(now * 1000).toISOString(),
                expiresAt: toTransferTimestamp(expiresAt),
                revokedAt: null,
            },
        }
    }

    listSlotTransferTokens(accountId: number, playerId: number): TransferTokenMetadata[] {
        assertTransferAccountId(accountId)
        assertTransferPlayerId(playerId)
        const rows = this.database.prepare(`
            SELECT id, account_id, player_id, permission,
                   device_name, created_at, expires_at, revoked_at
            FROM transfer_slot_tokens
            WHERE account_id = ? AND player_id = ?
            ORDER BY created_at DESC, id DESC
        `).all(accountId, playerId) as TransferTokenRow[]
        return rows.map(mapTransferToken)
    }

    revokeSlotTransferToken(accountId: number, playerId: number, tokenId: string): boolean {
        assertTransferAccountId(accountId)
        assertTransferPlayerId(playerId)
        const now = Math.floor(Date.now() / 1000)
        return this.database.prepare(`
            UPDATE transfer_slot_tokens
            SET revoked_at = ?
            WHERE id = ? AND account_id = ? AND player_id = ? AND revoked_at IS NULL
        `).run(now, tokenId, accountId, playerId).changes > 0
    }
    // //// /签发和列出槽 transfer token ////

    // //// 验证壳和槽 transfer token 作用域 [@x380kkm 2026-07-27] ////
    resolveShellTransferToken(token: string): TransferTokenMetadata | null {
        const now = Math.floor(Date.now() / 1000)
        const row = this.database.prepare(`
            SELECT id, account_id, NULL AS player_id, NULL AS permission,
                   device_name, created_at, expires_at, revoked_at
            FROM transfer_shell_tokens
            WHERE token_hash = ?
              AND revoked_at IS NULL
              AND (expires_at IS NULL OR expires_at > ?)
        `).get(hashTransferToken("shell", token), now) as TransferTokenRow | undefined
        return row === undefined ? null : mapTransferToken(row)
    }

    resolveSlotTransferToken(
        token: string,
        playerId: number,
        permission: "upload" | "download",
    ): TransferTokenMetadata | null {
        assertTransferPlayerId(playerId)
        const now = Math.floor(Date.now() / 1000)
        const row = this.database.prepare(`
            SELECT id, account_id, player_id, permission,
                   device_name, created_at, expires_at, revoked_at
            FROM transfer_slot_tokens
            WHERE token_hash = ?
              AND player_id = ?
              AND permission IN (?, 'both')
              AND revoked_at IS NULL
              AND (expires_at IS NULL OR expires_at > ?)
        `).get(hashTransferToken("slot", token), playerId, permission, now) as TransferTokenRow | undefined
        return row === undefined ? null : mapTransferToken(row)
    }
    // //// /验证壳和槽 transfer token 作用域 ////

    close(): void {
        if (this.database.open) this.database.close()
    }
}

const managementAccessDatabasePath = path.resolve(
    process.env.MANAGEMENT_ACCESS_DATABASE_PATH ?? path.join(process.cwd(), ".management", "control.db"),
)

export const managementAccessStore = new ManagementAccessStore(managementAccessDatabasePath)
