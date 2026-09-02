// audience: external
// # transfer-data-routes
// 此模块使用壳 token 管理同账号的槽 token.
// 此模块使用槽 token 上传和下载一个可移植存档槽.
// 此模块不读取或返回实例登录凭据和好友数据.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { getSaveSlotActivationBlock } from "../../control/saveSlotActivity"
import {
    CreateSlotTransferTokenInput,
    TransferTokenMetadata,
    managementAccessStore,
} from "../../control/managementAccess"
import {
    commitSaveRevisionChangeSync,
    createSaveRevisionSync,
    getCurrentSaveRevisionSync,
    hasSaveRevisionConflict,
    normalizeRevisionEtag,
} from "../../data/saveRevisions"
import {
    capturePortablePlayerSnapshotSync,
    deserializePlayerData,
    getPortableSerializedData,
} from "../../data/utils"
import {
    getAccountFromPlayerIdSync,
    getAccountPlayersSync,
    getActivePlayerIdSync,
    getPlayerSync,
    replacePlayerDataSync,
} from "../../data/wdfpData"
import {
    createStarpointSavePackage,
    parseStarpointSavePackage,
} from "../../games/starpoint/portableSave"

interface PlayerParams {
    playerId: string
}

interface TokenParams extends PlayerParams {
    tokenId: string
}

interface ShellSlotTokenBody {
    playerId?: number
    permission?: "upload" | "download" | "both"
    expiresAt?: string | null
    deviceName?: string | null
}

interface ShellSlotTokenInput extends CreateSlotTransferTokenInput {
    playerId: number
}

const SAVE_BODY_LIMIT_BYTES = 8 * 1024 * 1024

function parsePlayerId(value: string): number | null {
    const playerId = Number(value)
    return Number.isInteger(playerId) && playerId > 0 ? playerId : null
}

function parseTokenId(value: string): string | null {
    return /^[a-f0-9]{32}$/.test(value) ? value : null
}

function getBearerToken(request: FastifyRequest): string | null {
    const value = request.headers.authorization
    if (typeof value !== "string" || !value.startsWith("Bearer ")) return null
    const token = value.slice("Bearer ".length)
    return token.length > 0 ? token : null
}

function sendUnauthorizedTransferToken(reply: FastifyReply): null {
    reply.header("www-authenticate", "Bearer")
    reply.status(401).send({ error: "transfer_token_required" })
    return null
}

function getShellToken(request: FastifyRequest, reply: FastifyReply): TransferTokenMetadata | null {
    const token = getBearerToken(request)
    if (token === null) return sendUnauthorizedTransferToken(reply)
    const metadata = managementAccessStore.resolveShellTransferToken(token)
    return metadata ?? sendUnauthorizedTransferToken(reply)
}

function getSlotToken(
    request: FastifyRequest,
    reply: FastifyReply,
    playerId: number,
    permission: "upload" | "download",
): TransferTokenMetadata | null {
    const token = getBearerToken(request)
    if (token === null) return sendUnauthorizedTransferToken(reply)
    const metadata = managementAccessStore.resolveSlotTransferToken(token, playerId, permission)
    return metadata ?? sendUnauthorizedTransferToken(reply)
}

function slotBelongsToAccount(playerId: number, accountId: number): boolean {
    return getAccountFromPlayerIdSync(playerId)?.id === accountId
}

function setTransferIdentityHeaders(
    reply: FastifyReply,
    token: TransferTokenMetadata,
    playerId: number,
): void {
    reply.header("x-starpoint-instance-id", managementAccessStore.getTransferInstanceId())
    reply.header("x-starpoint-shell-id", token.accountId.toString())
    reply.header("x-starpoint-slot-id", playerId.toString())
}

function sendRevisionConflict(request: FastifyRequest, playerId: number, reply: FastifyReply): boolean {
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

function createCurrentSaveRevision(playerId: number, label: string) {
    const data = getPortableSerializedData(playerId, { serializeRushEventData: true, viewerId: 0 })
    if (data === null) throw new Error("Save does not exist.")
    return createSaveRevisionSync({ playerId, data, label })
}

function parseShellSlotTokenInput(body: ShellSlotTokenBody): ShellSlotTokenInput | null {
    const playerId = body.playerId
    if (typeof playerId !== "number" || !Number.isInteger(playerId) || playerId <= 0) return null
    if (body.permission !== "upload" && body.permission !== "download" && body.permission !== "both") return null
    if ((body.expiresAt !== undefined && body.expiresAt !== null && typeof body.expiresAt !== "string")
        || (body.deviceName !== undefined && body.deviceName !== null && typeof body.deviceName !== "string")) {
        return null
    }
    return { playerId, permission: body.permission, expiresAt: body.expiresAt, deviceName: body.deviceName }
}

// //// 注册壳 token 的槽位管理接口 [@x380kkm 2026-07-27] ////
export async function registerTransferShellRoutes(fastify: FastifyInstance): Promise<void> {
    fastify.get("/shell/slots", async (request, reply) => {
        const shell = getShellToken(request, reply)
        if (shell === null) return
        const slots = getAccountPlayersSync(shell.accountId)
            .map((playerId) => {
                const player = getPlayerSync(playerId)
                const revision = player === null ? null : createCurrentSaveRevision(playerId, "Current state")
                return player === null ? null : {
                    id: player.id,
                    active: getActivePlayerIdSync(shell.accountId) === player.id,
                    name: player.name,
                    revisionId: revision?.id ?? null,
                    etag: revision?.etag ?? null,
                }
            })
            .filter((slot): slot is NonNullable<typeof slot> => slot !== null)
        return { instanceId: managementAccessStore.getTransferInstanceId(), slots }
    })

    fastify.post<{ Body: ShellSlotTokenBody }>("/shell/slot-tokens", async (request, reply) => {
        const shell = getShellToken(request, reply)
        if (shell === null) return
        const input = parseShellSlotTokenInput(request.body ?? {})
        if (input === null) return reply.status(400).send({ error: "invalid_transfer_slot_token" })
        if (!slotBelongsToAccount(input.playerId, shell.accountId)) {
            return reply.status(404).send({ error: "save_slot_not_found" })
        }
        try {
            const issued = managementAccessStore.issueSlotTransferToken(shell.accountId, input.playerId, input)
            return reply.status(201).send({
                token: issued.token,
                tokenType: "slot",
                instanceId: issued.instanceId,
                metadata: issued.metadata,
            })
        } catch (error) {
            return reply.status(400).send({ error: "transfer_slot_token_create_failed", message: (error as Error).message })
        }
    })

    fastify.delete<{ Params: TokenParams }>("/shell/slots/:playerId/tokens/:tokenId", async (request, reply) => {
        const shell = getShellToken(request, reply)
        if (shell === null) return
        const playerId = parsePlayerId(request.params.playerId)
        const tokenId = parseTokenId(request.params.tokenId)
        if (playerId === null || tokenId === null) return reply.status(400).send({ error: "invalid_transfer_token_id" })
        if (!slotBelongsToAccount(playerId, shell.accountId)) return reply.status(404).send({ error: "save_slot_not_found" })
        const revoked = managementAccessStore.revokeSlotTransferToken(shell.accountId, playerId, tokenId)
        return revoked ? { revoked: true } : reply.status(404).send({ error: "transfer_token_not_found" })
    })
}
// //// /注册壳 token 的槽位管理接口 ////

// //// 注册槽 token 的可移植存档传输接口 [@x380kkm 2026-07-27] ////
export async function registerTransferSlotRoutes(fastify: FastifyInstance): Promise<void> {
    fastify.addHook("onRequest", async (_request, reply) => {
        reply.header("cache-control", "no-store")
    })

    fastify.get<{ Params: PlayerParams }>("/slots/:playerId", async (request, reply) => {
        const playerId = parsePlayerId(request.params.playerId)
        if (playerId === null) return reply.status(400).send({ error: "invalid_save_slot_id" })
        const slotToken = getSlotToken(request, reply, playerId, "download")
        if (slotToken === null) return
        if (!slotBelongsToAccount(playerId, slotToken.accountId)) return reply.status(404).send({ error: "save_slot_not_found" })
        setTransferIdentityHeaders(reply, slotToken, playerId)
        const data = getPortableSerializedData(playerId, { serializeRushEventData: true, viewerId: 0 })
        if (data === null) return reply.status(404).send({ error: "save_not_found" })
        const revision = createSaveRevisionSync({ playerId, data, label: "Transfer download" })
        reply.header("etag", `"${revision.etag}"`)
        reply.header("content-disposition", `attachment; filename="starpoint-player-${playerId}.json"`)
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
    })

    fastify.put<{ Params: PlayerParams }>("/slots/:playerId", { bodyLimit: SAVE_BODY_LIMIT_BYTES }, async (request, reply) => {
        const playerId = parsePlayerId(request.params.playerId)
        if (playerId === null) return reply.status(400).send({ error: "invalid_save_slot_id" })
        const slotToken = getSlotToken(request, reply, playerId, "upload")
        if (slotToken === null) return
        const account = getAccountFromPlayerIdSync(playerId)
        if (account === null || account.id !== slotToken.accountId) return reply.status(404).send({ error: "save_slot_not_found" })
        setTransferIdentityHeaders(reply, slotToken, playerId)
        if (getActivePlayerIdSync(account.id) === playerId) {
            const blockedBy = getSaveSlotActivationBlock(account.id, playerId)
            if (blockedBy !== null) return reply.status(409).send({ error: "save_slot_import_blocked", blockedBy })
        }
        createCurrentSaveRevision(playerId, "Before transfer upload")
        if (sendRevisionConflict(request, playerId, reply)) return
        const portablePackage = parseStarpointSavePackage(request.body)
        if (portablePackage === null) return reply.status(400).send({ error: "invalid_save_package" })
        try {
            const parsed = deserializePlayerData(playerId, portablePackage.data)
            const revision = commitSaveRevisionChangeSync(() => {
                createCurrentSaveRevision(playerId, "Before transfer upload")
                replacePlayerDataSync(parsed)
                capturePortablePlayerSnapshotSync(playerId, portablePackage.data)
                return createCurrentSaveRevision(playerId, "Transfer upload")
            })
            reply.header("etag", `"${revision.etag}"`)
            return { imported: true, playerId, revisionId: revision.id, etag: revision.etag }
        } catch (error) {
            return reply.status(400).send({ error: "transfer_save_upload_failed", message: (error as Error).message })
        }
    })
}
// //// /注册槽 token 的可移植存档传输接口 ////
