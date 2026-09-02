// audience: internal
// # player-mails
// 此模块保存玩家邮件, 管理邮件奖励并在领取时原子写入玩家数据.

import getDatabase, { Database } from "."
import { Player } from "./types"
import { getPlayerSync, givePlayerItemSync, updatePlayerSync } from "./wdfpData"
import { givePlayerCharacterSync } from "../lib/character"
import { givePlayerEquipmentSync } from "../lib/equipment"
import { getServerTime } from "../utils"

const db = getDatabase(Database.WDFP_DATA)
const MAX_MAIL_PAGE_SIZE = 100
const MAX_MAIL_PAGE = 1000000
const MAX_REWARD_ENTRIES = 100

export interface PlayerMailReward {
    itemList: Record<string, number>
    equipmentList: Record<string, number>
    characterList: number[]
    freeMana: number
    paidMana: number
    freeVmoney: number
    vmoney: number
    expPool: number
}

export interface CreatePlayerMailInput {
    playerId: number
    title: string
    body: string
    sender: string
    rewards: Partial<PlayerMailReward>
    expiresAt?: number | null
}

export interface PlayerMail {
    id: number
    playerId: number
    title: string
    body: string
    sender: string
    rewards: PlayerMailReward
    createdAt: number
    expiresAt: number | null
    receivedAt: number | null
}

export interface PlayerMailPage {
    mails: PlayerMail[]
    total: number
}

export interface PlayerMailClaimResult {
    mailIds: number[]
    itemList: Record<string, number>
    equipmentList: object[]
    characterList: object[]
    expiredMailCount: number
    remainingCount: number
}

interface RawPlayerMail {
    id: number
    player_id: number
    title: string
    body: string
    sender: string
    rewards_json: string
    created_at: number
    expires_at: number | null
    received_at: number | null
}

function assertCount(value: unknown, field: string): number {
    if (!Number.isSafeInteger(value) || (value as number) < 0) throw new Error(`${field} must be a non-negative integer.`)
    return value as number
}

function normalizeCountMap(value: unknown, field: string): Record<string, number> {
    if (value === undefined || value === null) return {}
    if (typeof value !== "object" || Array.isArray(value)) throw new Error(`${field} must be an object.`)
    const entries = Object.entries(value as Record<string, unknown>)
    if (entries.length > MAX_REWARD_ENTRIES) throw new Error(`${field} has too many entries.`)
    const result: Record<string, number> = {}
    for (const [key, amount] of entries) {
        if (!/^\d+$/.test(key) || Number(key) <= 0) throw new Error(`${field} contains an invalid id.`)
        const count = assertCount(amount, `${field}.${key}`)
        if (count > 0) result[key] = count
    }
    return result
}

function normalizeIdList(value: unknown, field: string): number[] {
    if (value === undefined || value === null) return []
    if (!Array.isArray(value) || value.length > MAX_REWARD_ENTRIES) throw new Error(`${field} must be an array.`)
    return value.map((id) => {
        if (!Number.isSafeInteger(id) || (id as number) <= 0) throw new Error(`${field} contains an invalid id.`)
        return id as number
    })
}

function normalizeRewards(value: unknown): PlayerMailReward {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("rewards must be an object.")
    const rewards = value as Partial<PlayerMailReward>
    const normalized: PlayerMailReward = {
        itemList: normalizeCountMap(rewards.itemList, "itemList"),
        equipmentList: normalizeCountMap(rewards.equipmentList, "equipmentList"),
        characterList: normalizeIdList(rewards.characterList, "characterList"),
        freeMana: assertCount(rewards.freeMana ?? 0, "freeMana"),
        paidMana: assertCount(rewards.paidMana ?? 0, "paidMana"),
        freeVmoney: assertCount(rewards.freeVmoney ?? 0, "freeVmoney"),
        vmoney: assertCount(rewards.vmoney ?? 0, "vmoney"),
        expPool: assertCount(rewards.expPool ?? 0, "expPool"),
    }
    if (
        Object.keys(normalized.itemList).length === 0 &&
        Object.keys(normalized.equipmentList).length === 0 &&
        normalized.characterList.length === 0 &&
        normalized.freeMana === 0 && normalized.paidMana === 0 &&
        normalized.freeVmoney === 0 && normalized.vmoney === 0 && normalized.expPool === 0
    ) throw new Error("rewards must contain at least one value.")
    return normalized
}

function buildPlayerMail(raw: RawPlayerMail): PlayerMail {
    return {
        id: raw.id,
        playerId: raw.player_id,
        title: raw.title,
        body: raw.body,
        sender: raw.sender,
        rewards: normalizeRewards(JSON.parse(raw.rewards_json)),
        createdAt: raw.created_at,
        expiresAt: raw.expires_at,
        receivedAt: raw.received_at,
    }
}

function selectMail(mailId: number, playerId?: number): RawPlayerMail | undefined {
    const where = playerId === undefined ? "id = ?" : "id = ? AND player_id = ?"
    const values = playerId === undefined ? [mailId] : [mailId, playerId]
    return db.prepare(`SELECT id, player_id, title, body, sender, rewards_json, created_at, expires_at, received_at FROM player_mails WHERE ${where}`).get(...values) as RawPlayerMail | undefined
}

function getRemainingMailCount(playerId: number, now: number): number {
    const result = db.prepare("SELECT COUNT(*) AS count FROM player_mails WHERE player_id = ? AND received_at IS NULL AND (expires_at IS NULL OR expires_at > ?)").get(playerId, now) as { count: number }
    return result.count
}

// //// 创建管理员发放的玩家邮件 [@x380kkm 2026-07-24] ////
export function createPlayerMailSync(input: CreatePlayerMailInput): PlayerMail {
    if (!Number.isSafeInteger(input.playerId) || input.playerId <= 0) throw new Error("playerId must be a positive integer.")
    if (getPlayerSync(input.playerId) === null) throw new Error("player not found.")
    if (typeof input.title !== "string" || input.title.length < 1 || input.title.length > 200) throw new Error("title must contain between 1 and 200 characters.")
    if (typeof input.body !== "string" || input.body.length < 1 || input.body.length > 5000) throw new Error("body must contain between 1 and 5000 characters.")
    if (typeof input.sender !== "string" || input.sender.length < 1 || input.sender.length > 100) throw new Error("sender must contain between 1 and 100 characters.")
    const rewards = normalizeRewards(input.rewards)
    const expiresAt = input.expiresAt ?? null
    if (expiresAt !== null && (!Number.isSafeInteger(expiresAt) || expiresAt <= getServerTime())) throw new Error("expiresAt must be a future Unix timestamp.")
    const createdAt = getServerTime()
    const result = db.prepare("INSERT INTO player_mails (player_id, title, body, sender, rewards_json, created_at, expires_at, received_at) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)").run(input.playerId, input.title, input.body, input.sender, JSON.stringify(rewards), createdAt, expiresAt)
    return buildPlayerMail(selectMail(Number(result.lastInsertRowid)) as RawPlayerMail)
}
// //// /创建管理员发放的玩家邮件 ////

export function getPlayerMailsSync(playerId: number, page = 1, pageSize = 20): PlayerMailPage {
    if (!Number.isSafeInteger(playerId) || playerId <= 0) throw new Error("playerId must be a positive integer.")
    if (!Number.isSafeInteger(page) || page < 1 || page > MAX_MAIL_PAGE) throw new Error("page must be between 1 and 1000000.")
    if (!Number.isSafeInteger(pageSize) || pageSize < 1 || pageSize > MAX_MAIL_PAGE_SIZE) throw new Error("pageSize must be between 1 and 100.")
    const now = getServerTime()
    const rows = db.prepare("SELECT id, player_id, title, body, sender, rewards_json, created_at, expires_at, received_at FROM player_mails WHERE player_id = ? AND received_at IS NULL AND (expires_at IS NULL OR expires_at > ?) ORDER BY id DESC LIMIT ? OFFSET ?").all(playerId, now, pageSize, (page - 1) * pageSize) as RawPlayerMail[]
    return { mails: rows.map(buildPlayerMail), total: getRemainingMailCount(playerId, now) }
}

function emptyClaimResult(): PlayerMailClaimResult {
    return { mailIds: [], itemList: {}, equipmentList: [], characterList: [], expiredMailCount: 0, remainingCount: 0 }
}

function addItem(result: PlayerMailClaimResult, itemId: string, count: number): void {
    result.itemList[itemId] = (result.itemList[itemId] ?? 0) + count
}

function applyRewards(playerId: number, rewards: PlayerMailReward, result: PlayerMailClaimResult): void {
    for (const [itemId, count] of Object.entries(rewards.itemList)) {
        givePlayerItemSync(playerId, itemId, count)
        addItem(result, itemId, count)
    }
    for (const [equipmentId, count] of Object.entries(rewards.equipmentList)) {
        result.equipmentList.push(givePlayerEquipmentSync(playerId, Number(equipmentId), count))
    }
    for (const characterId of rewards.characterList) {
        const given = givePlayerCharacterSync(playerId, characterId)
        if (given === null) throw new Error(`character ${characterId} does not exist.`)
        result.characterList.push(given.character)
        if (given.item !== undefined) addItem(result, String(given.item.id), given.item.count)
    }
    const player = getPlayerSync(playerId) as Player
    updatePlayerSync({
        id: playerId,
        freeMana: player.freeMana + rewards.freeMana,
        paidMana: player.paidMana + rewards.paidMana,
        freeVmoney: player.freeVmoney + rewards.freeVmoney,
        vmoney: player.vmoney + rewards.vmoney,
        expPool: player.expPool + rewards.expPool,
    })
}

function claimMailInTransaction(playerId: number, mailId: number, result: PlayerMailClaimResult, now: number): void {
    const raw = selectMail(mailId, playerId)
    if (raw === undefined || raw.received_at !== null) return
    db.prepare("UPDATE player_mails SET received_at = ? WHERE id = ? AND player_id = ? AND received_at IS NULL").run(now, mailId, playerId)
    result.mailIds.push(mailId)
    if (raw.expires_at !== null && raw.expires_at <= now) {
        result.expiredMailCount += 1
        return
    }
    applyRewards(playerId, normalizeRewards(JSON.parse(raw.rewards_json)), result)
}

function claimMailsSync(playerId: number, mailIds?: number[]): PlayerMailClaimResult {
    if (!Number.isSafeInteger(playerId) || playerId <= 0) throw new Error("playerId must be a positive integer.")
    if (getPlayerSync(playerId) === null) throw new Error("player not found.")
    const now = getServerTime()
    const result = emptyClaimResult()
    const transaction = db.transaction(() => {
        const ids = mailIds ?? (db.prepare("SELECT id FROM player_mails WHERE player_id = ? AND received_at IS NULL").all(playerId) as { id: number }[]).map((row) => row.id)
        for (const mailId of ids) {
            if (!Number.isSafeInteger(mailId) || mailId <= 0) throw new Error("mailIds must contain positive integers.")
            claimMailInTransaction(playerId, mailId, result, now)
        }
        result.remainingCount = getRemainingMailCount(playerId, now)
    })
    transaction()
    return result
}

export function claimPlayerMailSync(playerId: number, mailId: number): PlayerMailClaimResult {
    return claimMailsSync(playerId, [mailId])
}

export function claimAllPlayerMailsSync(playerId: number, mailIds?: number[]): PlayerMailClaimResult {
    return claimMailsSync(playerId, mailIds)
}
