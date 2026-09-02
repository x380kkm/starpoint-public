// audience: external
// # server-transfer-binding-routes
//
// 此模块管理完整服务器到明确目标实例槽的持久化绑定和冲突.
// 响应返回目标地址和身份, 但不返回远端槽 token.

import { FastifyInstance, FastifyReply, FastifyRequest } from "fastify"
import { managementAccessStore } from "../../control/managementAccess"
import { resolveServerTransferConflict } from "../../control/serverTransferConflictService"
import {
    ServerTransferBindingOperationError,
    isServerTransferBindingBusy,
} from "../../control/serverTransferBindingOperations"
import {
    createServerTransferBinding,
    synchronizeServerTransferBinding,
} from "../../control/serverTransferBindingService"
import {
    deleteServerTransferBindingSync,
    getServerTransferBindingSync,
    listServerTransferBindingsSync,
    updateServerTransferBindingSync,
} from "../../control/serverTransferBindingStore"
import { listServerTransferConflictsSync } from "../../control/serverTransferConflictStore"
import { ServerTransferBindingStoreError } from "../../control/serverTransferStoreSupport"
import {
    ServerTransferBinding,
    ServerTransferConflict,
    ServerTransferConflictPolicy,
    ServerTransferConflictResolution,
    ServerTransferScheduleMode,
    ServerTransferSyncDirection,
    UpdateServerTransferBindingInput,
} from "../../control/serverTransferBindingTypes"
import { getPlayerSync } from "../../data/wdfpData"
import {
    getManagementPrincipal,
    parseManagementId,
    requireManagementAdmin,
} from "./access"

interface PlayerParams {
    playerId: string
}

interface BindingParams extends PlayerParams {
    bindingId: string
}

interface ConflictParams extends BindingParams {
    conflictId: string
}

interface CreateBindingBody {
    targetBaseUrl?: unknown
    targetInstanceId?: unknown
    targetPlayerId?: unknown
    targetToken?: unknown
    uploadMode?: unknown
    pullMode?: unknown
    conflictPolicy?: unknown
    intervalSeconds?: unknown
    enabled?: unknown
}

interface UpdateBindingBody {
    uploadMode?: unknown
    pullMode?: unknown
    conflictPolicy?: unknown
    intervalSeconds?: unknown
    enabled?: unknown
    targetToken?: unknown
}

interface SyncBindingBody {
    direction?: unknown
}

interface ResolveConflictBody {
    resolution?: unknown
}

function isObjectId(value: string): boolean {
    return /^[a-f0-9]{32}$/.test(value)
}

function isScheduleMode(value: unknown): value is ServerTransferScheduleMode {
    return value === "manual" || value === "interval"
}

function isConflictPolicy(value: unknown): value is ServerTransferConflictPolicy {
    return value === "local_wins" || value === "remote_wins" || value === "ask"
}

function isSyncDirection(value: unknown): value is ServerTransferSyncDirection {
    return value === "auto" || value === "upload" || value === "pull"
}

function isConflictResolution(value: unknown): value is ServerTransferConflictResolution {
    return value === "local_wins" || value === "remote_wins" || value === "keep_both"
}

function isIntervalSeconds(value: unknown): value is number {
    return Number.isInteger(value) && Number(value) >= 1 && Number(value) <= 2_592_000
}

function isTargetToken(value: unknown): value is string {
    return typeof value === "string"
        && value.length <= 512
        && /^spt_slot_[A-Za-z0-9_-]{40,}$/.test(value)
}

function parseCreateBindingBody(body: CreateBindingBody): Omit<
    Parameters<typeof createServerTransferBinding>[0],
    "sourcePlayerId"
> | null {
    if (
        typeof body.targetBaseUrl !== "string"
        || !/^[a-f0-9]{32}$/.test(String(body.targetInstanceId))
        || !Number.isInteger(body.targetPlayerId)
        || Number(body.targetPlayerId) <= 0
        || !isTargetToken(body.targetToken)
        || !isScheduleMode(body.uploadMode)
        || !isScheduleMode(body.pullMode)
        || !isConflictPolicy(body.conflictPolicy)
        || !isIntervalSeconds(body.intervalSeconds)
        || typeof body.enabled !== "boolean"
    ) {
        return null
    }
    return {
        targetBaseUrl: body.targetBaseUrl,
        targetInstanceId: String(body.targetInstanceId),
        targetPlayerId: Number(body.targetPlayerId),
        targetToken: body.targetToken,
        uploadMode: body.uploadMode,
        pullMode: body.pullMode,
        conflictPolicy: body.conflictPolicy,
        intervalSeconds: body.intervalSeconds,
        enabled: body.enabled,
    }
}

function parseUpdateBindingBody(body: UpdateBindingBody): UpdateServerTransferBindingInput | null {
    if (
        !isScheduleMode(body.uploadMode)
        || !isScheduleMode(body.pullMode)
        || !isConflictPolicy(body.conflictPolicy)
        || !isIntervalSeconds(body.intervalSeconds)
        || typeof body.enabled !== "boolean"
        || (body.targetToken !== undefined && !isTargetToken(body.targetToken))
    ) {
        return null
    }
    return {
        uploadMode: body.uploadMode,
        pullMode: body.pullMode,
        conflictPolicy: body.conflictPolicy,
        intervalSeconds: body.intervalSeconds,
        enabled: body.enabled,
        targetToken: body.targetToken,
    }
}

function bindingView(binding: ServerTransferBinding) {
    return {
        bindingId: binding.id,
        source: { playerId: binding.sourcePlayerId },
        target: {
            baseUrl: binding.targetBaseUrl,
            instanceId: binding.targetInstanceId,
            shellId: binding.targetShellId,
            playerId: binding.targetPlayerId,
        },
        uploadMode: binding.uploadMode,
        pullMode: binding.pullMode,
        conflictPolicy: binding.conflictPolicy,
        intervalSeconds: binding.intervalSeconds,
        enabled: binding.enabled,
        lastCommonEtag: binding.lastCommonEtag,
        lastSourceEtag: binding.lastSourceEtag,
        lastTargetEtag: binding.lastTargetEtag,
        pendingDirection: binding.pendingDirection,
        nextRunAt: binding.nextRunAt,
        lastSyncedAt: binding.lastSyncedAt,
        lastError: binding.lastError,
        revision: binding.revision,
        createdAt: binding.createdAt,
        updatedAt: binding.updatedAt,
    }
}

function conflictView(conflict: ServerTransferConflict) {
    return {
        conflictId: conflict.id,
        bindingId: conflict.bindingId,
        sourceRevisionId: conflict.sourceRevisionId,
        sourceEtag: conflict.sourceEtag,
        targetRevisionId: conflict.targetRevisionId,
        targetEtag: conflict.targetEtag,
        detectedAt: conflict.detectedAt,
        status: conflict.status,
        resolvedAt: conflict.resolvedAt,
    }
}

function getAccessiblePlayerId(
    request: FastifyRequest,
    reply: FastifyReply,
    value: string,
): number | null {
    let playerId: number
    try {
        playerId = parseManagementId(value, "playerId")
    } catch {
        reply.status(400).send({ error: "invalid_save_slot_id" })
        return null
    }
    const principal = getManagementPrincipal(request)
    if (!managementAccessStore.canAccessPlayer(principal, playerId)) {
        reply.status(403).send({ error: "management_player_forbidden" })
        return null
    }
    if (getPlayerSync(playerId) === null) {
        reply.status(404).send({ error: "save_slot_not_found" })
        return null
    }
    return playerId
}

function getAccessibleBinding(
    request: FastifyRequest,
    reply: FastifyReply,
    params: BindingParams,
): ServerTransferBinding | null {
    const playerId = getAccessiblePlayerId(request, reply, params.playerId)
    if (playerId === null) return null
    if (!isObjectId(params.bindingId)) {
        reply.status(400).send({ error: "invalid_transfer_binding_id" })
        return null
    }
    const binding = getServerTransferBindingSync(params.bindingId)
    if (binding === null || binding.sourcePlayerId !== playerId) {
        reply.status(404).send({ error: "transfer_binding_not_found" })
        return null
    }
    return binding
}

function sendBindingError(reply: FastifyReply, error: unknown) {
    const code = error instanceof ServerTransferBindingOperationError
        || error instanceof ServerTransferBindingStoreError
        ? error.code
        : "transfer_storage_failed"
    if (code === "binding_not_found" || code === "source_player_not_found") {
        return reply.status(404).send({ error: "transfer_binding_not_found" })
    }
    if (code === "conflict_not_found") {
        return reply.status(404).send({ error: "transfer_conflict_not_found" })
    }
    if (code === "transfer_target_unavailable") {
        return reply.status(503).send({ error: code })
    }
    if (
        code === "transfer_target_authentication_failed"
        || code === "transfer_target_identity_mismatch"
        || code === "transfer_target_invalid_response"
        || code === "transfer_target_slot_not_found"
    ) {
        return reply.status(502).send({ error: code })
    }
    if (
        code === "binding_changed"
        || code === "conflict_changed"
        || code === "duplicate_binding"
        || code === "transfer_binding_disabled"
        || code === "transfer_binding_busy"
        || code === "transfer_conflict_open"
        || code === "transfer_target_revision_conflict"
        || code === "local_save_import_blocked"
    ) {
        return reply.status(409).send({ error: code })
    }
    if (code === "transfer_target_invalid") {
        return reply.status(400).send({ error: code })
    }
    return reply.status(500).send({ error: code })
}

// //// 管理服务器传输绑定 [@x380kkm 2026-08-04] ////
export default async function registerManagementTransferBindingRoutes(
    fastify: FastifyInstance,
): Promise<void> {
    fastify.addHook("onRequest", requireManagementAdmin)

    fastify.get<{ Params: PlayerParams }>("/saves/:playerId/transfer-bindings", async (request, reply) => {
        const playerId = getAccessiblePlayerId(request, reply, request.params.playerId)
        if (playerId === null) return
        return listServerTransferBindingsSync(playerId).map(bindingView)
    })

    fastify.post<{ Params: PlayerParams, Body: CreateBindingBody }>("/saves/:playerId/transfer-bindings", async (request, reply) => {
        const playerId = getAccessiblePlayerId(request, reply, request.params.playerId)
        if (playerId === null) return
        const input = parseCreateBindingBody(request.body ?? {})
        if (input === null) return reply.status(400).send({ error: "invalid_transfer_binding" })
        try {
            const binding = await createServerTransferBinding({ sourcePlayerId: playerId, ...input })
            return reply.status(201).send(bindingView(binding))
        } catch (error) {
            return sendBindingError(reply, error)
        }
    })

    fastify.get<{ Params: BindingParams }>("/saves/:playerId/transfer-bindings/:bindingId", async (request, reply) => {
        const binding = getAccessibleBinding(request, reply, request.params)
        return binding === null ? undefined : bindingView(binding)
    })

    fastify.put<{ Params: BindingParams, Body: UpdateBindingBody }>("/saves/:playerId/transfer-bindings/:bindingId", async (request, reply) => {
        const binding = getAccessibleBinding(request, reply, request.params)
        if (binding === null) return
        if (isServerTransferBindingBusy(binding.id)) {
            return reply.status(409).send({ error: "transfer_binding_busy" })
        }
        const input = parseUpdateBindingBody(request.body ?? {})
        if (input === null) return reply.status(400).send({ error: "invalid_transfer_binding" })
        try {
            return bindingView(updateServerTransferBindingSync(binding, input))
        } catch (error) {
            return sendBindingError(reply, error)
        }
    })

    fastify.delete<{ Params: BindingParams }>("/saves/:playerId/transfer-bindings/:bindingId", async (request, reply) => {
        const binding = getAccessibleBinding(request, reply, request.params)
        if (binding === null) return
        if (isServerTransferBindingBusy(binding.id)) {
            return reply.status(409).send({ error: "transfer_binding_busy" })
        }
        return deleteServerTransferBindingSync(binding.sourcePlayerId, binding.id)
            ? { deleted: true }
            : reply.status(404).send({ error: "transfer_binding_not_found" })
    })

    fastify.post<{ Params: BindingParams, Body: SyncBindingBody }>("/saves/:playerId/transfer-bindings/:bindingId/sync", async (request, reply) => {
        const binding = getAccessibleBinding(request, reply, request.params)
        if (binding === null) return
        const direction = request.body?.direction ?? "auto"
        if (!isSyncDirection(direction)) {
            return reply.status(400).send({ error: "invalid_transfer_direction" })
        }
        try {
            const outcome = await synchronizeServerTransferBinding(binding.id, "manual", direction)
            if ("conflict" in outcome) {
                return reply.status(409).send({
                    error: "transfer_conflict",
                    conflict: conflictView(outcome.conflict),
                })
            }
            return { action: outcome.action, binding: bindingView(outcome.binding) }
        } catch (error) {
            return sendBindingError(reply, error)
        }
    })

    fastify.get<{ Params: BindingParams }>("/saves/:playerId/transfer-bindings/:bindingId/conflicts", async (request, reply) => {
        const binding = getAccessibleBinding(request, reply, request.params)
        if (binding === null) return
        return listServerTransferConflictsSync(binding.id).map(conflictView)
    })

    fastify.post<{ Params: ConflictParams, Body: ResolveConflictBody }>("/saves/:playerId/transfer-bindings/:bindingId/conflicts/:conflictId/resolve", async (request, reply) => {
        const binding = getAccessibleBinding(request, reply, request.params)
        if (binding === null) return
        if (!isObjectId(request.params.conflictId) || !isConflictResolution(request.body?.resolution)) {
            return reply.status(400).send({ error: "invalid_transfer_conflict_resolution" })
        }
        try {
            const resolved = await resolveServerTransferConflict(
                binding.id,
                request.params.conflictId,
                request.body.resolution,
            )
            return {
                conflict: conflictView(resolved.conflict),
                binding: bindingView(resolved.binding),
            }
        } catch (error) {
            return sendBindingError(reply, error)
        }
    })
}
// //// /管理服务器传输绑定 ////
