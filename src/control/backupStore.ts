// audience: internal | external
// # backup-store
// 此模块创建 SQLite 一致性备份, 校验恢复暂存文件, 并在数据库加载前原子替换文件.
// schemaVersion 1 备份可以出现在列表中, 但自动恢复只接受 schemaVersion 2.

import { createHash, randomBytes } from "crypto"
import { promises as fs } from "fs"
import path from "path"
import { writeJsonAtomic } from "../storage/atomicFile"
import { checkpointSqliteDatabase, createSqliteBackup, validateSqliteDatabase } from "../storage/sqliteFile"

export interface BackupFile {
    name: string
    bytes: number
    sha256: string
}

export interface BackupManifest {
    schemaVersion: 1 | 2
    id: string
    createdAt: string
    databasePath: string
    files: BackupFile[]
}

export interface PendingRestore {
    schemaVersion: 1
    backupId: string
    sha256: string
    stagedAt: string
    databaseExisted: boolean
}

export interface AppliedRestore {
    backupId: string
    appliedAt: string
    rollbackRetained: boolean
}

export interface BackupStoreOptions {
    databasePath: string
    backupDir: string
    pendingRestorePath: string
}

// //// 校验备份和恢复元数据 [@x380kkm 2026-07-22] ////
function isRecord(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === "object" && !Array.isArray(value)
}

function assertBackupId(id: string): void {
    if (!/^[A-Za-z0-9_-]+$/.test(id)) throw new Error("Invalid backup id.")
}

function parseBackupManifest(value: unknown): BackupManifest {
    if (!isRecord(value) || (value.schemaVersion !== 1 && value.schemaVersion !== 2)) {
        throw new Error("Backup manifest schema is unsupported.")
    }
    if (typeof value.id !== "string" || typeof value.createdAt !== "string" || typeof value.databasePath !== "string" || !Array.isArray(value.files)) {
        throw new Error("Backup manifest is invalid.")
    }
    const files = value.files.map((entry) => {
        if (!isRecord(entry) || typeof entry.name !== "string" || !/^[A-Za-z0-9._-]+$/.test(entry.name)) {
            throw new Error("Backup file name is invalid.")
        }
        if (!Number.isInteger(entry.bytes) || (entry.bytes as number) < 0 || typeof entry.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(entry.sha256)) {
            throw new Error("Backup file metadata is invalid.")
        }
        return { name: entry.name, bytes: entry.bytes as number, sha256: entry.sha256 }
    })
    return {
        schemaVersion: value.schemaVersion,
        id: value.id,
        createdAt: value.createdAt,
        databasePath: value.databasePath,
        files,
    }
}

function parsePendingRestore(value: unknown): PendingRestore {
    if (!isRecord(value) || value.schemaVersion !== 1 || typeof value.backupId !== "string" || typeof value.sha256 !== "string" || typeof value.stagedAt !== "string" || typeof value.databaseExisted !== "boolean") {
        throw new Error("Pending restore state is invalid.")
    }
    assertBackupId(value.backupId)
    if (!/^[a-f0-9]{64}$/.test(value.sha256)) throw new Error("Pending restore checksum is invalid.")
    return { schemaVersion: 1, backupId: value.backupId, sha256: value.sha256, stagedAt: value.stagedAt, databaseExisted: value.databaseExisted }
}
// //// /校验备份和恢复元数据 ////

// //// 计算文件摘要并检查文件存在性 [@x380kkm 2026-07-22] ////
async function sha256File(filePath: string): Promise<string> {
    const contents = await fs.readFile(filePath)
    return createHash("sha256").update(contents).digest("hex")
}

async function pathExists(filePath: string): Promise<boolean> {
    try {
        await fs.stat(filePath)
        return true
    } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") return false
        throw error
    }
}
// //// /计算文件摘要并检查文件存在性 ////

export class BackupStore {
    readonly databasePath: string
    readonly backupDir: string
    readonly pendingRestorePath: string
    readonly stagedDatabasePath: string
    readonly rollbackDatabasePath: string

    constructor(options: BackupStoreOptions) {
        this.databasePath = path.resolve(options.databasePath)
        this.backupDir = path.resolve(options.backupDir)
        this.pendingRestorePath = path.resolve(options.pendingRestorePath)
        this.stagedDatabasePath = `${this.databasePath}.restore`
        this.rollbackDatabasePath = `${this.databasePath}.rollback`
    }

    // //// 创建可独立恢复的 SQLite 在线备份 [@x380kkm 2026-07-22] ////
    async createBackup(): Promise<BackupManifest> {
        const id = `${new Date().toISOString().replace(/[-:.TZ]/g, "")}-${randomBytes(4).toString("hex")}`
        const directory = path.join(this.backupDir, id)
        const databaseFileName = path.basename(this.databasePath)
        const targetPath = path.join(directory, databaseFileName)
        const temporaryPath = `${targetPath}.${randomBytes(6).toString("hex")}.tmp`
        await fs.mkdir(directory, { recursive: true })

        try {
            await createSqliteBackup(this.databasePath, temporaryPath)
            validateSqliteDatabase(temporaryPath)
            await fs.rename(temporaryPath, targetPath)
            const stat = await fs.stat(targetPath)
            const manifest: BackupManifest = {
                schemaVersion: 2,
                id,
                createdAt: new Date().toISOString(),
                databasePath: this.databasePath,
                files: [{ name: databaseFileName, bytes: stat.size, sha256: await sha256File(targetPath) }],
            }
            await writeJsonAtomic(path.join(directory, "manifest.json"), manifest)
            return manifest
        } catch (error) {
            try {
                await fs.rm(directory, { recursive: true, force: true })
            } catch { }
            throw error
        }
    }
    // //// /创建可独立恢复的 SQLite 在线备份 ////

    // //// 列出格式有效的备份清单 [@x380kkm 2026-07-22] ////
    async listBackups(): Promise<BackupManifest[]> {
        let entries: import("fs").Dirent[]
        try {
            entries = await fs.readdir(this.backupDir, { withFileTypes: true })
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code === "ENOENT") return []
            throw error
        }

        const manifests: BackupManifest[] = []
        for (const entry of entries) {
            if (!entry.isDirectory() || !/^[A-Za-z0-9_-]+$/.test(entry.name)) continue
            try {
                const value = JSON.parse(await fs.readFile(path.join(this.backupDir, entry.name, "manifest.json"), "utf8"))
                const manifest = parseBackupManifest(value)
                if (manifest.id === entry.name) manifests.push(manifest)
            } catch { }
        }
        return manifests.sort((left, right) => right.createdAt.localeCompare(left.createdAt))
    }
    // //// /列出格式有效的备份清单 ////

    // //// 校验并暂存一个可恢复数据库文件 [@x380kkm 2026-07-22] ////
    async stageRestore(id: string): Promise<PendingRestore> {
        assertBackupId(id)
        const manifestPath = path.join(this.backupDir, id, "manifest.json")
        const manifest = parseBackupManifest(JSON.parse(await fs.readFile(manifestPath, "utf8")))
        if (manifest.schemaVersion !== 2 || manifest.id !== id || manifest.files.length !== 1) {
            throw new Error("Only schemaVersion 2 backups can be restored automatically.")
        }

        const databaseFile = manifest.files[0]
        if (databaseFile.name !== path.basename(this.databasePath)) {
            throw new Error("Backup database file name does not match the configured database.")
        }
        const backupPath = path.join(this.backupDir, id, databaseFile.name)
        if (await sha256File(backupPath) !== databaseFile.sha256) {
            throw new Error(`Backup checksum does not match: ${databaseFile.name}`)
        }
        validateSqliteDatabase(backupPath)

        const temporaryPath = `${this.stagedDatabasePath}.${randomBytes(6).toString("hex")}.tmp`
        await fs.mkdir(path.dirname(this.databasePath), { recursive: true })
        try {
            await fs.copyFile(backupPath, temporaryPath)
            validateSqliteDatabase(temporaryPath)
            await fs.rm(this.stagedDatabasePath, { force: true })
            await fs.rename(temporaryPath, this.stagedDatabasePath)
        } catch (error) {
            await fs.rm(temporaryPath, { force: true })
            throw error
        }

        const pending: PendingRestore = {
            schemaVersion: 1,
            backupId: id,
            sha256: databaseFile.sha256,
            stagedAt: new Date().toISOString(),
            databaseExisted: await pathExists(this.databasePath),
        }
        await writeJsonAtomic(this.pendingRestorePath, pending)
        return pending
    }
    // //// /校验并暂存一个可恢复数据库文件 ////

    // //// 读取待恢复状态 [@x380kkm 2026-07-22] ////
    async getPendingRestore(): Promise<PendingRestore | null> {
        try {
            return parsePendingRestore(JSON.parse(await fs.readFile(this.pendingRestorePath, "utf8")))
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code === "ENOENT") return null
            throw error
        }
    }
    // //// /读取待恢复状态 ////

    // //// 完成已经替换但尚未清理状态的恢复 [@x380kkm 2026-07-22] ////
    private async finishInterruptedRestore(pending: PendingRestore): Promise<AppliedRestore | null> {
        if (await pathExists(this.stagedDatabasePath)) return null
        if (pending.databaseExisted && !await pathExists(this.rollbackDatabasePath)) {
            throw new Error("Pending restore file is missing without an interrupted replacement marker.")
        }
        if (!await pathExists(this.databasePath) || await sha256File(this.databasePath) !== pending.sha256) {
            throw new Error("Pending restore file is missing and the active database does not match it.")
        }

        await fs.rm(`${this.databasePath}-wal`, { force: true })
        await fs.rm(`${this.databasePath}-shm`, { force: true })
        validateSqliteDatabase(this.databasePath)
        await fs.rm(this.pendingRestorePath, { force: true })
        let rollbackRetained = false
        try {
            await fs.rm(this.rollbackDatabasePath, { force: true })
        } catch {
            rollbackRetained = true
        }
        return { backupId: pending.backupId, appliedAt: new Date().toISOString(), rollbackRetained }
    }
    // //// /完成已经替换但尚未清理状态的恢复 ////

    // //// 在数据库加载前原子应用待恢复文件 [@x380kkm 2026-07-22] ////
    async applyPendingRestore(): Promise<AppliedRestore | null> {
        const pending = await this.getPendingRestore()
        if (pending === null) return null

        const interrupted = await this.finishInterruptedRestore(pending)
        if (interrupted !== null) return interrupted
        if (await sha256File(this.stagedDatabasePath) !== pending.sha256) {
            throw new Error("Pending restore checksum does not match.")
        }
        validateSqliteDatabase(this.stagedDatabasePath)

        const databaseExists = await pathExists(this.databasePath)
        const rollbackExists = await pathExists(this.rollbackDatabasePath)
        if (pending.databaseExisted && !databaseExists && !rollbackExists) {
            throw new Error("The original database and its rollback file are both missing.")
        }
        if (databaseExists) {
            checkpointSqliteDatabase(this.databasePath)
            validateSqliteDatabase(this.databasePath)
            if (rollbackExists) await fs.rm(this.rollbackDatabasePath, { force: true })
            await fs.rename(this.databasePath, this.rollbackDatabasePath)
        } else if (rollbackExists) {
            validateSqliteDatabase(this.rollbackDatabasePath)
        }

        let committed = false
        try {
            await fs.rename(this.stagedDatabasePath, this.databasePath)
            await fs.rm(`${this.databasePath}-wal`, { force: true })
            await fs.rm(`${this.databasePath}-shm`, { force: true })
            validateSqliteDatabase(this.databasePath)
            if (await sha256File(this.databasePath) !== pending.sha256) {
                throw new Error("Restored database checksum does not match.")
            }
            await fs.rm(this.pendingRestorePath, { force: true })
            committed = true
        } catch (error) {
            if (!committed) {
                if (await pathExists(this.databasePath) && !await pathExists(this.stagedDatabasePath)) {
                    await fs.rename(this.databasePath, this.stagedDatabasePath)
                }
                if (await pathExists(this.rollbackDatabasePath)) {
                    await fs.rename(this.rollbackDatabasePath, this.databasePath)
                }
            }
            throw error
        }

        let rollbackRetained = false
        try {
            await fs.rm(this.rollbackDatabasePath, { force: true })
        } catch {
            rollbackRetained = true
        }
        return { backupId: pending.backupId, appliedAt: new Date().toISOString(), rollbackRetained }
    }
    // //// /在数据库加载前原子应用待恢复文件 ////
}
