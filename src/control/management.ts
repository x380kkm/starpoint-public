// audience: internal | external
// # management-store
// 此模块把虚拟时间, 实例设置和 COM 模板保存在游戏数据库之外.
// 备份和恢复操作委托给独立 BackupStore, 且不在此模块加载游戏数据库.
// CN 新实例使用内容基线时间, 虚拟时间保存真实时间锚点并在重启后继续按配置倍率推进.

import { promises as fs } from "fs"
import path from "path"
import { CN_CONTENT_BASELINE_ISO } from "../lib/cnAssets"
export { CN_CONTENT_BASELINE_ISO } from "../lib/cnAssets"
import { getServerDate, setServerTime, setServerTimeRate } from "../utils"
import { writeJsonAtomic } from "../storage/atomicFile"
import { BackupStore } from "./backupStore"
import type { AppliedRestore, BackupManifest, PendingRestore } from "./backupStore"

export type { AppliedRestore, BackupFile, BackupManifest, PendingRestore } from "./backupStore"

export type ControlMode = "global" | "cn"
export type InstanceStatus = "stopped" | "starting" | "running" | "stopping" | "failed"

export interface NpcMateConfig {
    id: string
    displayName: string
    enabled: boolean
    pairingKey: string
    sourcePlayerId: number | null
    partySlot: number | null
    rank: number
    degreeId: number | null
}

export interface NpcFillConfig {
    enabled: boolean
    delaySeconds: number
}

export interface NpcConfigurationUpdate extends NpcFillConfig {
    mates: NpcMateConfig[]
}

export interface ManagementConfig {
    schemaVersion: 1
    instance: {
        mode: ControlMode
        status: InstanceStatus
        httpPort: number
        sessionPort: number
    }
    virtualTime: {
        enabled: boolean
        iso: string | null
        rate: number
        realTimeAnchor: string | null
    }
    npcFill: NpcFillConfig
    npcMates: NpcMateConfig[]
    updatedAt: string
}

export interface ManagementStoreOptions {
    rootDir?: string
    statePath?: string
    databasePath?: string
    backupDir?: string
}

const CONTROL_DIRECTORY = ".management"
const DATABASE_FILE = "wdfp_data.db"

function createDefaultConfig(): ManagementConfig {
    return {
        schemaVersion: 1,
        instance: {
            mode: "cn",
            status: "stopped",
            httpPort: 8001,
            sessionPort: 8003,
        },
        virtualTime: {
            enabled: true,
            iso: CN_CONTENT_BASELINE_ISO,
            rate: 1,
            realTimeAnchor: new Date().toISOString(),
        },
        npcFill: {
            enabled: false,
            delaySeconds: 30,
        },
        npcMates: [],
        updatedAt: new Date(0).toISOString(),
    }
}

function assertPort(port: number, field: string): void {
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
        throw new Error(`${field} must be an integer between 1 and 65535.`)
    }
}

// //// 兼容旧状态并规范 COM 回填配置 [@x380kkm 2026-07-22] ////
function normalizeNpcFill(value: unknown): NpcFillConfig {
    if (value === undefined) return { enabled: false, delaySeconds: 30 }
    if (value === null || typeof value !== "object") throw new Error("NPC fill configuration is invalid.")
    const fill = value as Partial<NpcFillConfig>
    if (typeof fill.enabled !== "boolean" || !Number.isInteger(fill.delaySeconds) || (fill.delaySeconds as number) < 0 || (fill.delaySeconds as number) > 3600) {
        throw new Error("NPC fill configuration is invalid.")
    }
    return { enabled: fill.enabled, delaySeconds: fill.delaySeconds as number }
}

function normalizeNpcMate(value: unknown): NpcMateConfig {
    if (value === null || typeof value !== "object") throw new Error("NPC configuration is invalid.")
    const mate = value as Partial<NpcMateConfig>
    if (typeof mate.id !== "string" || typeof mate.displayName !== "string" || typeof mate.pairingKey !== "string" || typeof mate.enabled !== "boolean") {
        throw new Error("NPC configuration is invalid.")
    }
    const sourcePlayerId = mate.sourcePlayerId ?? null
    const partySlot = mate.partySlot ?? null
    const rank = mate.rank ?? 1
    const degreeId = mate.degreeId ?? null
    if (sourcePlayerId !== null && (!Number.isInteger(sourcePlayerId) || sourcePlayerId <= 0)) throw new Error("NPC sourcePlayerId is invalid.")
    if (partySlot !== null && (!Number.isInteger(partySlot) || partySlot <= 0)) throw new Error("NPC partySlot is invalid.")
    if (!Number.isInteger(rank) || rank < 1 || rank > 999) throw new Error("NPC rank is invalid.")
    if (degreeId !== null && (!Number.isInteger(degreeId) || degreeId < 0)) throw new Error("NPC degreeId is invalid.")
    return { id: mate.id, displayName: mate.displayName, pairingKey: mate.pairingKey, enabled: mate.enabled, sourcePlayerId, partySlot, rank, degreeId }
}
// //// /兼容旧状态并规范 COM 回填配置 ////

function normalizeConfig(value: unknown): ManagementConfig {
    const config = value as Partial<ManagementConfig> | null
    if (config === null || typeof config !== "object" || config.schemaVersion !== 1) {
        throw new Error("Unsupported management state schema.")
    }
    const instance = config.instance
    const virtualTime = config.virtualTime
    if (instance === undefined || virtualTime === undefined || !Array.isArray(config.npcMates)) {
        throw new Error("Management state is missing required fields.")
    }
    const mode = instance.mode === "global" || instance.mode === "cn" ? instance.mode : null
    const status = ["stopped", "starting", "running", "stopping", "failed"].includes(instance.status as string)
        ? instance.status as InstanceStatus
        : null
    if (mode === null || status === null) throw new Error("Management instance state is invalid.")
    assertPort(instance.httpPort, "httpPort")
    assertPort(instance.sessionPort, "sessionPort")
    if (typeof virtualTime.enabled !== "boolean" || typeof virtualTime.rate !== "number") {
        throw new Error("Management virtual time state is invalid.")
    }
    if (virtualTime.iso !== null && (typeof virtualTime.iso !== "string" || Number.isNaN(Date.parse(virtualTime.iso)))) {
        throw new Error("Management virtual time date is invalid.")
    }
    if (!Number.isFinite(virtualTime.rate) || virtualTime.rate <= 0 || virtualTime.rate > 1000) {
        throw new Error("Management virtual time rate is invalid.")
    }
    if (virtualTime.enabled && virtualTime.iso === null) {
        throw new Error("Enabled management virtual time requires a date.")
    }
    const legacyAnchor = typeof config.updatedAt === "string" && !Number.isNaN(Date.parse(config.updatedAt))
        ? config.updatedAt
        : new Date().toISOString()
    const realTimeAnchor = virtualTime.realTimeAnchor ?? (virtualTime.enabled ? legacyAnchor : null)
    if (realTimeAnchor !== null && (typeof realTimeAnchor !== "string" || Number.isNaN(Date.parse(realTimeAnchor)))) {
        throw new Error("Management virtual time anchor is invalid.")
    }
    const npcFill = normalizeNpcFill(config.npcFill)
    const npcMates = config.npcMates.map(normalizeNpcMate)
    return {
        schemaVersion: 1,
        instance: { mode, status, httpPort: instance.httpPort, sessionPort: instance.sessionPort },
        virtualTime: { enabled: virtualTime.enabled, iso: virtualTime.iso ?? null, rate: virtualTime.rate, realTimeAnchor },
        npcFill,
        npcMates,
        updatedAt: typeof config.updatedAt === "string" ? config.updatedAt : new Date(0).toISOString(),
    }
}

// //// 按真实时间锚点计算重启后的虚拟日期 [@x380kkm 2026-07-22] ////
function calculateVirtualDate(config: ManagementConfig["virtualTime"], realDate: Date): Date | null {
    if (!config.enabled || config.iso === null || config.realTimeAnchor === null) return null
    const elapsedMilliseconds = Math.max(0, realDate.getTime() - Date.parse(config.realTimeAnchor))
    return new Date(Date.parse(config.iso) + elapsedMilliseconds * config.rate)
}
// //// /按真实时间锚点计算重启后的虚拟日期 ////

export class ManagementStore {
    readonly rootDir: string
    readonly statePath: string
    readonly databasePath: string
    readonly backupDir: string
    private readonly backupStore: BackupStore

    constructor(options: ManagementStoreOptions = {}) {
        this.rootDir = path.resolve(options.rootDir ?? process.cwd())
        this.statePath = path.resolve(options.statePath ?? process.env.MANAGEMENT_STATE_FILE ?? path.join(this.rootDir, CONTROL_DIRECTORY, "state.json"))
        this.databasePath = path.resolve(options.databasePath ?? process.env.DATABASE_PATH ?? path.join(this.rootDir, ".database", DATABASE_FILE))
        this.backupDir = path.resolve(options.backupDir ?? process.env.MANAGEMENT_BACKUP_DIR ?? path.join(this.rootDir, CONTROL_DIRECTORY, "backups"))
        this.backupStore = new BackupStore({
            databasePath: this.databasePath,
            backupDir: this.backupDir,
            pendingRestorePath: path.join(path.dirname(this.statePath), "pending-restore.json"),
        })
    }

    async load(): Promise<ManagementConfig> {
        try {
            const contents = await fs.readFile(this.statePath, "utf8")
            return normalizeConfig(JSON.parse(contents))
        } catch (error) {
            const code = (error as NodeJS.ErrnoException).code
            if (code === "ENOENT") return this.save(createDefaultConfig())
            throw error
        }
    }

    async save(config: ManagementConfig): Promise<ManagementConfig> {
        const normalized = normalizeConfig(config)
        normalized.updatedAt = new Date().toISOString()
        await writeJsonAtomic(this.statePath, normalized)
        return normalized
    }

    async applyVirtualTime(): Promise<ManagementConfig> {
        const config = await this.load()
        const virtualDate = calculateVirtualDate(config.virtualTime, new Date())
        if (virtualDate === null) {
            setServerTime(null)
            return config
        }
        setServerTimeRate(config.virtualTime.rate)
        setServerTime(virtualDate)
        return config
    }

    async setVirtualTime(enabled: boolean, iso: string | null, rate: number): Promise<ManagementConfig> {
        if (!Number.isFinite(rate) || rate <= 0 || rate > 1000) throw new Error("rate must be greater than 0 and no greater than 1000.")
        if (enabled && (iso === null || Number.isNaN(Date.parse(iso)))) throw new Error("enabled virtual time requires a valid ISO date.")
        const config = await this.load()
        const normalizedIso = enabled && iso !== null ? new Date(iso).toISOString() : null
        config.virtualTime = {
            enabled,
            iso: normalizedIso,
            rate: enabled ? rate : 1,
            realTimeAnchor: enabled ? new Date().toISOString() : null,
        }
        if (enabled && normalizedIso !== null) {
            setServerTimeRate(rate)
            setServerTime(new Date(normalizedIso))
        } else {
            setServerTime(null)
        }
        return this.save(config)
    }

    async setInstance(instance: Partial<ManagementConfig["instance"]>): Promise<ManagementConfig> {
        const config = await this.load()
        const next = { ...config.instance, ...instance }
        if (next.mode !== "global" && next.mode !== "cn") throw new Error("mode must be global or cn.")
        if (!["stopped", "starting", "running", "stopping", "failed"].includes(next.status)) throw new Error("status is invalid.")
        assertPort(next.httpPort, "httpPort")
        assertPort(next.sessionPort, "sessionPort")
        config.instance = next as ManagementConfig["instance"]
        return this.save(config)
    }

    // //// 保存可按任务筛选的 COM 回填配置 [@x380kkm 2026-07-22] ////
    async setNpcConfiguration(update: NpcConfigurationUpdate): Promise<ManagementConfig> {
        if (update.mates.length > 2) throw new Error("At most two NPC mates can be configured.")
        const fill = normalizeNpcFill(update)
        const mates = update.mates.map(normalizeNpcMate)
        if (new Set(mates.map((mate) => mate.id)).size !== mates.length) throw new Error("NPC ids must be unique.")
        for (const mate of mates) {
            if (!/^[A-Za-z0-9_-]{1,64}$/.test(mate.id)) throw new Error("NPC id is invalid.")
            if (mate.displayName.trim().length === 0 || mate.displayName.length > 80) throw new Error("NPC displayName is invalid.")
            if (mate.pairingKey !== "*" && !/^\d+:(?:\d+|\*)$/.test(mate.pairingKey)) throw new Error("NPC pairingKey is invalid.")
            if (mate.enabled && (mate.sourcePlayerId === null || mate.partySlot === null)) {
                throw new Error("Enabled NPC mates require sourcePlayerId and partySlot.")
            }
        }
        const config = await this.load()
        config.npcFill = fill
        config.npcMates = mates
        return this.save(config)
    }
    // //// /保存可按任务筛选的 COM 回填配置 ////

    // //// 委托一致性备份和启动前恢复操作 [@x380kkm 2026-07-22] ////
    async createBackup(): Promise<BackupManifest> {
        return this.backupStore.createBackup()
    }

    async listBackups(): Promise<BackupManifest[]> {
        return this.backupStore.listBackups()
    }

    async stageRestore(id: string): Promise<PendingRestore> {
        return this.backupStore.stageRestore(id)
    }

    async getPendingRestore(): Promise<PendingRestore | null> {
        return this.backupStore.getPendingRestore()
    }

    async applyPendingRestore(): Promise<AppliedRestore | null> {
        return this.backupStore.applyPendingRestore()
    }
    // //// /委托一致性备份和启动前恢复操作 ////

    async getStatus(): Promise<Record<string, unknown>> {
        const config = await this.load()
        return {
            config,
            serverDate: getServerDate().toISOString(),
            databasePath: this.databasePath,
            pendingRestore: await this.getPendingRestore(),
        }
    }
}

export const managementStore = new ManagementStore()
