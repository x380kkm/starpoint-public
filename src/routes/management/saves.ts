// audience: external
// # management-save-routes
// 此模块列出, 导入, 导出和激活当前身份有权访问的游戏玩家存档槽.
// 管理员可以指定任意玩家, 普通用户访问绑定玩家所属账号的全部槽.

import { FastifyInstance, FastifyReply } from "fastify"
import { managementAccessStore } from "../../control/managementAccess"
import { getSaveSlotActivationBlock } from "../../control/saveSlotActivity"
import {
    commitSaveRevisionChangeSync,
    createSaveRevisionSync,
    getCurrentSaveRevisionSync,
    getSaveRevisionSync,
    hasSaveRevisionConflict,
    listSaveRevisionsSync,
    normalizeRevisionEtag,
} from "../../data/saveRevisions"
import {
    capturePortablePlayerSnapshotSync,
    deserializePlayerData,
    getPortableSerializedData,
} from "../../data/utils"
import {
    activateAccountPlayerSync,
    getAccountFromPlayerIdSync,
    getAccountPlayersSync,
    getAllPlayerIdsSync,
    getActivePlayerIdSync,
    getPlayerSync,
    importPlayerDataAsNewSlotSync,
    replacePlayerDataSync,
} from "../../data/wdfpData"
import {
    createStarpointSavePackage,
    parseStarpointSavePackage,
    sanitizePortableGameData,
    STARPOINT_SAVE_FORMAT,
} from "../../games/starpoint/portableSave"
import { getManagementPrincipal, parseManagementId } from "./access"

interface SaveParams {
    playerId: string
}

interface SaveRevisionParams extends SaveParams {
    revisionId: string
}

const SAVE_BODY_LIMIT_BYTES = 8 * 1024 * 1024

function getAccessiblePlayerIds(request: Parameters<typeof getManagementPrincipal>[0]): number[] {
    const principal = getManagementPrincipal(request)
    if (principal.role === "admin") return getAllPlayerIdsSync()
    const accountIds = new Set(
        managementAccessStore.getBoundPlayerIds(principal.id)
            .map(getAccountFromPlayerIdSync)
            .filter((account): account is NonNullable<typeof account> => account !== null)
            .map((account) => account.id),
    )
    return Array.from(accountIds)
        .flatMap(getAccountPlayersSync)
        .sort((left, right) => left - right)
}

function canAccessPlayerSlot(request: Parameters<typeof getManagementPrincipal>[0], playerId: number): boolean {
    const principal = getManagementPrincipal(request)
    if (principal.role === "admin" || managementAccessStore.canAccessPlayer(principal, playerId)) return true
    const account = getAccountFromPlayerIdSync(playerId)
    return account !== null && getAccessiblePlayerIds(request).includes(playerId)
}

function getPlayerOrSendAccessError(request: Parameters<typeof getManagementPrincipal>[0], playerId: number, reply: FastifyReply) {
    if (!canAccessPlayerSlot(request, playerId)) {
        reply.status(403).send({ error: "player_forbidden" })
        return null
    }
    const player = getPlayerSync(playerId)
    if (player === null) {
        reply.status(404).send({ error: "player_not_found" })
        return null
    }
    return player
}

function createCurrentSaveRevision(playerId: number, label: string) {
    const data = getPortableSerializedData(playerId, { serializeRushEventData: true, viewerId: 0 })
    if (data === null) throw new Error("Save does not exist.")
    return createSaveRevisionSync({ playerId, data, label })
}

function sendRevisionConflict(request: Parameters<typeof getManagementPrincipal>[0], playerId: number, reply: FastifyReply): boolean {
    const rawEtag = request.headers["if-match"]
    const expectedEtag = normalizeRevisionEtag(rawEtag)
    if (rawEtag !== undefined && expectedEtag === null) {
        reply.status(400).send({ error: "invalid_save_revision_etag" })
        return true
    }
    if (!hasSaveRevisionConflict(playerId, expectedEtag)) return false

    const current = getCurrentSaveRevisionSync(playerId)
    reply.status(409).send({
        error: "save_revision_conflict",
        currentRevisionId: current?.id ?? null,
        currentEtag: current?.etag ?? null,
    })
    return true
}

function summarizeSaveRevision(revision: ReturnType<typeof createCurrentSaveRevision>) {
    return {
        id: revision.id,
        playerId: revision.playerId,
        parentRevisionId: revision.parentRevisionId,
        etag: revision.etag,
        label: revision.label,
        createdAt: revision.createdAt,
        pinned: revision.pinned,
    }
}

// //// 注册本人存档槽管理接口 [@x380kkm 2026-07-27] ////
export async function registerManagementSaveRoutes(fastify: FastifyInstance): Promise<void> {
    fastify.get("/saves", async (request) => {
        const players = getAccessiblePlayerIds(request)
            .map(getPlayerSync)
            .filter((player): player is NonNullable<typeof player> => player !== null)
            .map((player) => {
                const account = getAccountFromPlayerIdSync(player.id)
                const activePlayerId = account === null ? null : getActivePlayerIdSync(account.id)
                const revision = createCurrentSaveRevision(player.id, "Current state")
                return {
                    id: player.id,
                    accountId: account?.id ?? null,
                    active: activePlayerId === player.id,
                    revisionId: revision?.id ?? null,
                    etag: revision?.etag ?? null,
                    name: player.name,
                    rankPoint: player.rankPoint,
                    lastLoginTime: player.lastLoginTime.toISOString(),
                }
            })
        return { players }
    })

    fastify.get<{ Params: SaveParams }>("/saves/:playerId", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            if (getPlayerOrSendAccessError(request, playerId, reply) === null) return
            const data = getPortableSerializedData(playerId, { serializeRushEventData: true, viewerId: 0 })
            if (data === null) return reply.status(404).send({ error: "save_not_found" })
            const revision = createSaveRevisionSync({ playerId, data, label: "Export" })
            reply.header("content-disposition", `attachment; filename="starpoint-player-${playerId}.json"`)
            reply.header("etag", `"${revision.etag}"`)
            return createStarpointSavePackage({
                data,
                createdAt: new Date().toISOString(),
                source: {
                    instanceKind: "remote",
                    slotId: playerId.toString(),
                    slotName: getPlayerSync(playerId)?.name ?? null,
                    revisionId: revision.id,
                },
            })
        } catch (error) {
            return reply.status(400).send({ error: "save_export_failed", message: (error as Error).message })
        }
    })

    fastify.post<{ Params: SaveParams }>("/saves/:playerId/slots", { bodyLimit: SAVE_BODY_LIMIT_BYTES }, async (request, reply) => {
        try {
            const sourcePlayerId = parseManagementId(request.params.playerId, "playerId")
            if (getPlayerOrSendAccessError(request, sourcePlayerId, reply) === null) return
            const account = getAccountFromPlayerIdSync(sourcePlayerId)
            if (account === null) return reply.status(404).send({ error: "account_not_found" })

            const portablePackage = parseStarpointSavePackage(request.body)
            if (portablePackage === null) return reply.status(400).send({ error: "invalid_save_package" })
            const importedData = deserializePlayerData(0, portablePackage.data)
            const { importedPlayer, revision } = commitSaveRevisionChangeSync(() => {
                const importedPlayer = importPlayerDataAsNewSlotSync(account.id, importedData)
                capturePortablePlayerSnapshotSync(importedPlayer.id, portablePackage.data)
                return {
                    importedPlayer,
                    revision: createCurrentSaveRevision(importedPlayer.id, "Imported slot"),
                }
            })

            return reply.status(201).send({
                imported: true,
                accountId: account.id,
                playerId: importedPlayer.id,
                active: getActivePlayerIdSync(account.id) === importedPlayer.id,
                name: importedPlayer.name,
                revision: summarizeSaveRevision(revision),
            })
        } catch (error) {
            return reply.status(400).send({ error: "save_slot_import_failed", message: (error as Error).message })
        }
    })

    fastify.post<{ Params: SaveParams }>("/saves/:playerId/activate", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            const player = getPlayerOrSendAccessError(request, playerId, reply)
            if (player === null) return
            const account = getAccountFromPlayerIdSync(playerId)
            if (account === null) return reply.status(404).send({ error: "account_not_found" })

            const activePlayerId = getActivePlayerIdSync(account.id)
            if (activePlayerId !== playerId) {
                const blockedBy = getSaveSlotActivationBlock(account.id, activePlayerId)
                if (blockedBy !== null) {
                    return reply.status(409).send({ error: "save_slot_activation_blocked", blockedBy })
                }
                activateAccountPlayerSync(account.id, playerId)
            }

            return { activated: true, accountId: account.id, playerId: player.id }
        } catch (error) {
            return reply.status(400).send({ error: "save_slot_activation_failed", message: (error as Error).message })
        }
    })

    fastify.get<{ Params: SaveParams }>("/saves/:playerId/revisions", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            if (getPlayerOrSendAccessError(request, playerId, reply) === null) return
            createCurrentSaveRevision(playerId, "Current state")
            return {
                currentRevisionId: getCurrentSaveRevisionSync(playerId)?.id ?? null,
                revisions: listSaveRevisionsSync(playerId).map(summarizeSaveRevision),
            }
        } catch (error) {
            return reply.status(400).send({ error: "save_revision_list_failed", message: (error as Error).message })
        }
    })

    fastify.post<{ Params: SaveRevisionParams }>(
        "/saves/:playerId/revisions/:revisionId/restore",
        async (request, reply) => {
            try {
                const playerId = parseManagementId(request.params.playerId, "playerId")
                if (getPlayerOrSendAccessError(request, playerId, reply) === null) return
                const activeData = getPortableSerializedData(playerId, { serializeRushEventData: true, viewerId: 0 })
                if (activeData === null) return reply.status(404).send({ error: "save_not_found" })
                createSaveRevisionSync({ playerId, data: activeData, label: "Before restore" })
                if (sendRevisionConflict(request, playerId, reply)) return

                const target = getSaveRevisionSync(playerId, request.params.revisionId)
                if (target === null) return reply.status(404).send({ error: "save_revision_not_found" })
                const portableData = sanitizePortableGameData(target.data)
                const parsed = deserializePlayerData(playerId, portableData)
                const revision = commitSaveRevisionChangeSync(() => {
                    createCurrentSaveRevision(playerId, "Before restore")
                    replacePlayerDataSync(parsed)
                    capturePortablePlayerSnapshotSync(playerId, portableData)
                    return createCurrentSaveRevision(playerId, `Restored ${target.id}`)
                })
                reply.header("etag", `"${revision.etag}"`)
                return { restored: true, playerId, revision: summarizeSaveRevision(revision) }
            } catch (error) {
                return reply.status(400).send({ error: "save_revision_restore_failed", message: (error as Error).message })
            }
        },
    )

    fastify.put<{ Params: SaveParams }>("/saves/:playerId", { bodyLimit: SAVE_BODY_LIMIT_BYTES }, async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            if (getPlayerOrSendAccessError(request, playerId, reply) === null) return
            const account = getAccountFromPlayerIdSync(playerId)
            if (account === null) return reply.status(404).send({ error: "account_not_found" })
            const activePlayerId = getActivePlayerIdSync(account.id)
            if (activePlayerId === playerId) {
                const blockedBy = getSaveSlotActivationBlock(account.id, activePlayerId)
                if (blockedBy !== null) {
                    return reply.status(409).send({ error: "save_slot_import_blocked", blockedBy })
                }
            }
            createCurrentSaveRevision(playerId, "Before overwrite")
            if (sendRevisionConflict(request, playerId, reply)) return
            const body = request.body
            if (body === null || typeof body !== "object" || Array.isArray(body)) {
                return reply.status(400).send({ error: "invalid_save" })
            }
            const record = body as Record<string, unknown>
            const portablePackage = record.format === STARPOINT_SAVE_FORMAT
                ? parseStarpointSavePackage(record)
                : null
            if (record.format === STARPOINT_SAVE_FORMAT && portablePackage === null) {
                return reply.status(400).send({ error: "invalid_save_package" })
            }
            const save = portablePackage?.data
                ?? (Object.prototype.hasOwnProperty.call(record, "data") ? record.data : record)
            const portableData = sanitizePortableGameData(save)
            const parsed = deserializePlayerData(playerId, portableData)
            const { player, revision } = commitSaveRevisionChangeSync(() => {
                createCurrentSaveRevision(playerId, "Before overwrite")
                replacePlayerDataSync(parsed)
                capturePortablePlayerSnapshotSync(playerId, portableData)
                return {
                    player: getPlayerSync(playerId),
                    revision: createCurrentSaveRevision(playerId, "Imported overwrite"),
                }
            })
            reply.header("etag", `"${revision.etag}"`)
            return { imported: true, playerId, name: player?.name ?? null, revision: summarizeSaveRevision(revision) }
        } catch (error) {
            return reply.status(400).send({ error: "save_import_failed", message: (error as Error).message })
        }
    })
}
// //// /注册本人存档槽管理接口 ////
