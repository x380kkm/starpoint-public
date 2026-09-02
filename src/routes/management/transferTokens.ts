// audience: internal
// # management-transfer-tokens
// 此模块为已认证的账号壳签发和撤销 transfer token.
// 壳 token 管理同一账号壳的槽 token.
// 槽 token 只授予一个槽位的上传或下载权限.

import { FastifyInstance, FastifyReply } from "fastify"
import {
    CreateSlotTransferTokenInput,
    CreateTransferTokenInput,
    IssuedTransferToken,
    managementAccessStore,
} from "../../control/managementAccess"
import { getAccountFromPlayerIdSync } from "../../data/wdfpData"
import { getManagementPrincipal, parseManagementId } from "./access"

interface PlayerParams {
    playerId: string
}

interface TokenParams extends PlayerParams {
    tokenId: string
}

function parseTokenId(value: string): string {
    if (!/^[a-f0-9]{32}$/.test(value)) throw new Error("tokenId is invalid.")
    return value
}

function getManagedAccountId(
    request: Parameters<typeof getManagementPrincipal>[0],
    playerId: number,
    reply: FastifyReply,
): number | null {
    const account = getAccountFromPlayerIdSync(playerId)
    if (account === null) {
        reply.status(404).send({ error: "player_not_found" })
        return null
    }
    const principal = getManagementPrincipal(request)
    if (principal.role === "admin") return account.id
    const ownsAccount = managementAccessStore.getBoundPlayerIds(principal.id)
        .map(getAccountFromPlayerIdSync)
        .some((boundAccount) => boundAccount?.id === account.id)
    if (ownsAccount) return account.id
    reply.status(403).send({ error: "player_forbidden" })
    return null
}

function issueResponse(kind: "shell" | "slot", issued: IssuedTransferToken) {
    return {
        token: issued.token,
        tokenType: kind,
        instanceId: issued.instanceId,
        metadata: issued.metadata,
    }
}

function parseShellTokenInput(value: unknown): CreateTransferTokenInput {
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
        throw new Error("Transfer token request is invalid.")
    }
    const body = value as Record<string, unknown>
    if ((body.expiresAt !== undefined && body.expiresAt !== null && typeof body.expiresAt !== "string")
        || (body.deviceName !== undefined && body.deviceName !== null && typeof body.deviceName !== "string")) {
        throw new Error("Transfer token request is invalid.")
    }
    return { expiresAt: body.expiresAt as string | null | undefined, deviceName: body.deviceName as string | null | undefined }
}

function parseSlotTokenInput(value: unknown): CreateSlotTransferTokenInput {
    const input = parseShellTokenInput(value)
    const permission = (value as Record<string, unknown>).permission
    if (permission !== "upload" && permission !== "download" && permission !== "both") {
        throw new Error("Transfer token permission is invalid.")
    }
    return { ...input, permission }
}

// //// 注册账号壳和槽 transfer token 管理接口 [@x380kkm 2026-07-27] ////
export async function registerManagementTransferTokenRoutes(fastify: FastifyInstance): Promise<void> {
    fastify.post<{ Params: PlayerParams }>("/transfer/shells/:playerId/tokens", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            const accountId = getManagedAccountId(request, playerId, reply)
            if (accountId === null) return
            return reply.status(201).send(issueResponse(
                "shell",
                managementAccessStore.issueShellTransferToken(accountId, parseShellTokenInput(request.body)),
            ))
        } catch (error) {
            return reply.status(400).send({ error: "transfer_shell_token_create_failed", message: (error as Error).message })
        }
    })

    fastify.get<{ Params: PlayerParams }>("/transfer/shells/:playerId/tokens", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            const accountId = getManagedAccountId(request, playerId, reply)
            if (accountId === null) return
            return {
                instanceId: managementAccessStore.getTransferInstanceId(),
                tokens: managementAccessStore.listShellTransferTokens(accountId),
            }
        } catch (error) {
            return reply.status(400).send({ error: "transfer_shell_token_list_failed", message: (error as Error).message })
        }
    })

    fastify.delete<{ Params: TokenParams }>("/transfer/shells/:playerId/tokens/:tokenId", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            const accountId = getManagedAccountId(request, playerId, reply)
            if (accountId === null) return
            const revoked = managementAccessStore.revokeShellTransferToken(accountId, parseTokenId(request.params.tokenId))
            return revoked ? { revoked: true } : reply.status(404).send({ error: "transfer_token_not_found" })
        } catch (error) {
            return reply.status(400).send({ error: "transfer_shell_token_revoke_failed", message: (error as Error).message })
        }
    })

    fastify.post<{ Params: PlayerParams }>("/transfer/slots/:playerId/tokens", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            const accountId = getManagedAccountId(request, playerId, reply)
            if (accountId === null) return
            return reply.status(201).send(issueResponse(
                "slot",
                managementAccessStore.issueSlotTransferToken(accountId, playerId, parseSlotTokenInput(request.body)),
            ))
        } catch (error) {
            return reply.status(400).send({ error: "transfer_slot_token_create_failed", message: (error as Error).message })
        }
    })

    fastify.get<{ Params: PlayerParams }>("/transfer/slots/:playerId/tokens", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            const accountId = getManagedAccountId(request, playerId, reply)
            if (accountId === null) return
            return {
                instanceId: managementAccessStore.getTransferInstanceId(),
                tokens: managementAccessStore.listSlotTransferTokens(accountId, playerId),
            }
        } catch (error) {
            return reply.status(400).send({ error: "transfer_slot_token_list_failed", message: (error as Error).message })
        }
    })

    fastify.delete<{ Params: TokenParams }>("/transfer/slots/:playerId/tokens/:tokenId", async (request, reply) => {
        try {
            const playerId = parseManagementId(request.params.playerId, "playerId")
            const accountId = getManagedAccountId(request, playerId, reply)
            if (accountId === null) return
            const revoked = managementAccessStore.revokeSlotTransferToken(
                accountId,
                playerId,
                parseTokenId(request.params.tokenId),
            )
            return revoked ? { revoked: true } : reply.status(404).send({ error: "transfer_token_not_found" })
        } catch (error) {
            return reply.status(400).send({ error: "transfer_slot_token_revoke_failed", message: (error as Error).message })
        }
    })
}
// //// /注册账号壳和槽 transfer token 管理接口 ////
