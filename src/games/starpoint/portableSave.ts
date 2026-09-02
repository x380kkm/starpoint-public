// audience: internal
// # starpoint-portable-save
//
// 该模块定义 CN 本地实例和远端实例共同使用的身份无关存档包.
// 数据摘要使用类型标记, UTF-8 键排序和 IEEE 754 数字编码, 所有实例据此拒绝损坏或被替换的载荷.
// 创建存档包时递归删除实例身份, 关系和权限字段.
// 解析旧版存档包时先验证原始摘要, 再删除实例数据并生成规范摘要.

import { createHash } from "crypto"

export const STARPOINT_SAVE_FORMAT = "starpoint-save-package"
export const STARPOINT_SAVE_VERSION = 1

const INSTANCE_IDENTITY_FIELDS = new Set([
    "account_id",
    "associate_token",
    "data_headers",
    "device_id",
    "keychain",
    "management_token",
    "player_id",
    "session",
    "session_id",
    "shell_credential",
    "shell_id",
    "token",
    "transfer_token",
    "viewer_id",
])

const INSTANCE_RELATIONSHIP_FIELDS = new Set([
    "block_list",
    "follow_info",
    "follow_list",
    "followed_count",
    "follower_list",
    "friend_list",
    "friends",
])

const INSTANCE_PERMISSION_FIELDS = new Set([
    "management_role",
    "permissions",
])

type InstanceKind = "local" | "remote"
type ClientPlatform = "android" | "ios" | "unknown"

export interface PortableSaveSource {
    instanceKind: InstanceKind
    slotId: string | null
    slotName: string | null
    revisionId: string | null
}

export interface PortableSaveClient {
    platform: ClientPlatform
    version: string | null
}

export interface StarpointSavePackage {
    format: typeof STARPOINT_SAVE_FORMAT
    version: typeof STARPOINT_SAVE_VERSION
    game: "starpoint"
    region: "cn"
    createdAt: string
    source: PortableSaveSource
    sourceClient: PortableSaveClient
    payloadSha256: string
    data: Record<string, unknown>
}

interface CreatePortableSaveInput {
    data: unknown
    createdAt: string
    source: PortableSaveSource
    sourceClient?: PortableSaveClient
}

interface SanitizedPortableValue {
    value: unknown
    removedInstanceData: boolean
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value)
}

function isNullableString(value: unknown): value is string | null {
    return value === null || typeof value === "string"
}

function hasExactKeys(value: Record<string, unknown>, expectedKeys: string[]): boolean {
    const actualKeys = Object.keys(value).sort()
    return actualKeys.length === expectedKeys.length
        && actualKeys.every((key, index) => key === expectedKeys[index])
}

// //// 在运行时验证来源和客户端元数据的精确形状 [@x380kkm 2026-08-13] ////
function isPortableSaveSource(value: unknown): value is PortableSaveSource {
    return isRecord(value)
        && hasExactKeys(value, ["instanceKind", "revisionId", "slotId", "slotName"])
        && (value.instanceKind === "local" || value.instanceKind === "remote")
        && isNullableString(value.slotId)
        && isNullableString(value.slotName)
        && isNullableString(value.revisionId)
}

function isPortableSaveClient(value: unknown): value is PortableSaveClient {
    return isRecord(value)
        && hasExactKeys(value, ["platform", "version"])
        && (value.platform === "android" || value.platform === "ios" || value.platform === "unknown")
        && isNullableString(value.version)
}
// //// /在运行时验证来源和客户端元数据的精确形状 ////

function isPortableTimestamp(value: unknown): value is string {
    if (typeof value !== "string") return false
    const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})\.(\d{1,9})Z$/.exec(value)
    if (match === null) return false
    const [year, month, day, hour, minute, second] = match.slice(1, 7).map(Number)
    const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0)
    const daysInMonth = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    return month >= 1
        && month <= 12
        && day >= 1
        && day <= daysInMonth[month - 1]
        && hour <= 23
        && minute <= 59
        && second <= 59
}

// //// 递归删除实例拥有的身份, 关系和权限字段 [@x380kkm 2026-08-03] ////
function isInstanceOwnedField(key: string): boolean {
    return INSTANCE_IDENTITY_FIELDS.has(key)
        || INSTANCE_RELATIONSHIP_FIELDS.has(key)
        || INSTANCE_PERMISSION_FIELDS.has(key)
}

function sanitizePortableValue(value: unknown): SanitizedPortableValue {
    if (Array.isArray(value)) {
        const sanitized = value.map(sanitizePortableValue)
        return {
            value: sanitized.map((entry) => entry.value),
            removedInstanceData: sanitized.some((entry) => entry.removedInstanceData),
        }
    }
    if (!isRecord(value)) return { value, removedInstanceData: false }

    const sanitizedEntries: Array<[string, unknown]> = []
    let removedInstanceData = false
    for (const [key, entry] of Object.entries(value)) {
        if (isInstanceOwnedField(key)) {
            removedInstanceData = true
            continue
        }
        const sanitizedEntry = sanitizePortableValue(entry)
        sanitizedEntries.push([key, sanitizedEntry.value])
        removedInstanceData ||= sanitizedEntry.removedInstanceData
    }
    return { value: Object.fromEntries(sanitizedEntries), removedInstanceData }
}
// //// /递归删除实例拥有的身份, 关系和权限字段 ////

// //// 生成跨运行时稳定的类型化数据摘要 [@x380kkm 2026-07-27] ////
function encodeCanonicalString(value: string): string {
    return `s${Buffer.byteLength(value, "utf8")}:${value}`
}

function encodeCanonicalNumber(value: number): string {
    if (!Number.isFinite(value)) throw new Error("Portable save data contains a non-finite number.")
    if (Number.isInteger(value) && !Number.isSafeInteger(value)) {
        throw new Error("Portable save data contains an unsafe integer.")
    }
    const normalized = Object.is(value, -0) ? 0 : value
    const bytes = Buffer.allocUnsafe(8)
    bytes.writeDoubleBE(normalized)
    return `n${bytes.toString("hex")}`
}

function encodeCanonicalJson(value: unknown): string {
    if (value === null) return "z"
    if (typeof value === "boolean") return value ? "b1" : "b0"
    if (typeof value === "string") return encodeCanonicalString(value)
    if (typeof value === "number") {
        return encodeCanonicalNumber(value)
    }
    if (Array.isArray(value)) return `a${value.length}[${value.map(encodeCanonicalJson).join("")}]`
    if (!isRecord(value)) throw new Error("Portable save data contains a non-JSON value.")
    const keys = Object.keys(value).sort((left, right) =>
        Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8")),
    )
    return `o${keys.length}{${keys
        .map((key) => `${encodeCanonicalString(key)}${encodeCanonicalJson(value[key])}`)
        .join("")}}`
}

export function calculatePortableGameDataSha256(data: Record<string, unknown>): string {
    return createHash("sha256").update(encodeCanonicalJson(data), "utf8").digest("hex")
}
// //// /生成跨运行时稳定的类型化数据摘要 ////

// //// 创建和验证身份无关的 CN 存档包 [@x380kkm 2026-07-27] ////
export function isPortableGameData(data: unknown): data is Record<string, unknown> {
    return hasPortableGameDataShape(data) && !sanitizePortableValue(data).removedInstanceData
}

export function sanitizePortableGameData(data: unknown): Record<string, unknown> {
    const sanitized = sanitizePortableValue(data).value
    if (!hasPortableGameDataShape(sanitized)) throw new Error("Portable save data is invalid.")
    return sanitized
}

function hasPortableGameDataShape(data: unknown): data is Record<string, unknown> {
    return isRecord(data) && isRecord(data.user_info) && isRecord(data.user_character_list)
}

export function createStarpointSavePackage(input: CreatePortableSaveInput): StarpointSavePackage {
    if (!isRecord(input) || !isPortableSaveSource(input.source)) {
        throw new Error("Portable save source is invalid.")
    }
    const sourceClient = input.sourceClient ?? { platform: "unknown", version: null }
    if (!isPortableSaveClient(sourceClient)) {
        throw new Error("Portable save client metadata is invalid.")
    }
    const sanitized = sanitizePortableGameData(input.data)
    if (!isPortableTimestamp(input.createdAt)) throw new Error("Portable save creation time is invalid.")
    return {
        format: STARPOINT_SAVE_FORMAT,
        version: STARPOINT_SAVE_VERSION,
        game: "starpoint",
        region: "cn",
        createdAt: input.createdAt,
        source: input.source,
        sourceClient,
        payloadSha256: calculatePortableGameDataSha256(sanitized),
        data: sanitized,
    }
}

export function parseStarpointSavePackage(value: unknown): StarpointSavePackage | null {
    if (!isRecord(value)) return null
    if (!hasExactKeys(value, [
        "createdAt",
        "data",
        "format",
        "game",
        "payloadSha256",
        "region",
        "source",
        "sourceClient",
        "version",
    ])) return null
    if (value.format !== STARPOINT_SAVE_FORMAT || value.version !== STARPOINT_SAVE_VERSION) return null
    if (value.game !== "starpoint" || value.region !== "cn") return null
    if (!isPortableTimestamp(value.createdAt)) return null
    if (!isPortableSaveSource(value.source) || !isPortableSaveClient(value.sourceClient) || !isRecord(value.data)) return null
    if (typeof value.payloadSha256 !== "string" || !/^[a-f0-9]{64}$/.test(value.payloadSha256)) return null
    try {
        if (calculatePortableGameDataSha256(value.data) !== value.payloadSha256) return null
        const data = sanitizePortableGameData(value.data)
        return {
            ...value,
            payloadSha256: calculatePortableGameDataSha256(data),
            data,
        } as StarpointSavePackage
    } catch {
        return null
    }
}
// //// /创建和验证身份无关的 CN 存档包 ////
