// audience: internal
// # server-transfer-binding-service
//
// 此模块同步完整服务器的一个玩家槽和一个明确的远端实例槽.
// 共同 ETag 判断单侧变化, 双侧变化只记录冲突并保留双方 current.
// 下载覆盖和冲突解决在同一 WDFP SQLite 事务中追加 revision.

import {
    downloadServerTransferSave,
    normalizeServerTransferBaseUrl,
    uploadServerTransferSave,
} from "./serverTransferBindingClient"
import {
    createServerTransferBindingSync,
    recordServerTransferBindingFailureSync,
    recordServerTransferBindingSuccessSync,
} from "./serverTransferBindingStore"
import {
    getOpenServerTransferConflictSync,
    recordServerTransferConflictSync,
} from "./serverTransferConflictStore"
import {
    ServerTransferBindingOperationError,
    getOrCreateServerTransferSourceSave,
    ensureServerTransferSourceCanBeReplaced,
    getServerTransferEndpoint,
    mapServerTransferOperationError,
    requireServerTransferBinding,
    runExclusiveServerTransferOperation,
} from "./serverTransferBindingOperations"
import {
    CreateServerTransferBindingInput,
    DownloadedServerTransferSave,
    ServerTransferBinding,
    ServerTransferConflictOutcome,
    ServerTransferEndpoint,
    ServerTransferSave,
    ServerTransferSyncDirection,
    ServerTransferSyncOutcome,
    ServerTransferSyncTrigger,
} from "./serverTransferBindingTypes"
import { commitSaveRevisionChangeSync } from "../data/saveRevisions"
import {
    capturePortablePlayerSnapshotSync,
    deserializePlayerData,
} from "../data/utils"
import { replacePlayerDataSync } from "../data/wdfpData"

export interface CreateServerTransferBindingRequest {
    sourcePlayerId: number
    targetBaseUrl: string
    targetInstanceId: string
    targetPlayerId: number
    targetToken: string
    uploadMode: "manual" | "interval"
    pullMode: "manual" | "interval"
    conflictPolicy: "local_wins" | "remote_wins" | "ask"
    intervalSeconds: number
    enabled: boolean
}

type ServerTransferDecision =
    | { kind: "unchanged", target: DownloadedServerTransferSave }
    | { kind: "uploaded", targetEtag: string }
    | { kind: "downloaded", target: DownloadedServerTransferSave }
    | { kind: "conflict", target: DownloadedServerTransferSave }
    | { kind: "deferred", target: DownloadedServerTransferSave }

function shouldUpload(
    binding: ServerTransferBinding,
    trigger: ServerTransferSyncTrigger,
    direction: ServerTransferSyncDirection,
): boolean {
    if (direction === "upload") return true
    if (direction === "pull") return false
    return trigger === "manual" || binding.uploadMode === "interval"
}

function shouldPull(
    binding: ServerTransferBinding,
    trigger: ServerTransferSyncTrigger,
    direction: ServerTransferSyncDirection,
): boolean {
    if (direction === "pull") return true
    if (direction === "upload") return false
    return trigger === "manual" || binding.pullMode === "interval"
}

// //// 创建并验证目标绑定 [@x380kkm 2026-08-04] ////
export async function createServerTransferBinding(
    request: CreateServerTransferBindingRequest,
): Promise<ServerTransferBinding> {
    const targetBaseUrl = normalizeServerTransferBaseUrl(request.targetBaseUrl)
    const source = getOrCreateServerTransferSourceSave(request.sourcePlayerId, "Transfer binding source")
    const endpoint: ServerTransferEndpoint = {
        baseUrl: targetBaseUrl,
        instanceId: request.targetInstanceId,
        playerId: request.targetPlayerId,
        token: request.targetToken,
    }
    let target: DownloadedServerTransferSave
    try {
        target = await downloadServerTransferSave(endpoint)
    } catch (error) {
        throw mapServerTransferOperationError(error)
    }
    const input: CreateServerTransferBindingInput = {
        ...request,
        targetBaseUrl,
        targetShellId: target.shellId,
        observedSourceEtag: source.etag,
        observedTargetEtag: target.etag,
    }
    try {
        return createServerTransferBindingSync(input)
    } catch (error) {
        throw mapServerTransferOperationError(error)
    }
}
// //// /创建并验证目标绑定 ////

function selectInitialTransfer(
    binding: ServerTransferBinding,
    source: ServerTransferSave,
    target: DownloadedServerTransferSave,
    trigger: ServerTransferSyncTrigger,
    direction: ServerTransferSyncDirection,
): "upload" | "pull" | "deferred" {
    if (shouldUpload(binding, trigger, direction)) return "upload"
    if (shouldPull(binding, trigger, direction)) return "pull"
    return "deferred"
}

async function uploadSourceOrReadConflict(
    binding: ServerTransferBinding,
    source: ServerTransferSave,
    target: DownloadedServerTransferSave,
): Promise<ServerTransferDecision> {
    try {
        const targetEtag = await uploadServerTransferSave(
            getServerTransferEndpoint(binding),
            source.package,
            target.etag,
        )
        return { kind: "uploaded", targetEtag }
    } catch (error) {
        const mapped = mapServerTransferOperationError(error)
        if (mapped.code !== "transfer_target_revision_conflict") throw mapped
        return {
            kind: "conflict",
            target: await downloadServerTransferSave(getServerTransferEndpoint(binding)),
        }
    }
}

async function selectTransferDecision(
    binding: ServerTransferBinding,
    source: ServerTransferSave,
    target: DownloadedServerTransferSave,
    trigger: ServerTransferSyncTrigger,
    direction: ServerTransferSyncDirection,
): Promise<ServerTransferDecision> {
    if (source.etag === target.etag) return { kind: "unchanged", target }
    const commonEtag = binding.lastCommonEtag
    if (commonEtag === null) {
        const initial = selectInitialTransfer(binding, source, target, trigger, direction)
        if (initial === "pull") return { kind: "downloaded", target }
        if (initial === "deferred") return { kind: "deferred", target }
        return uploadSourceOrReadConflict(binding, source, target)
    }
    const sourceChanged = source.etag !== commonEtag
    const targetChanged = target.etag !== commonEtag
    if (sourceChanged && targetChanged) {
        if (binding.conflictPolicy === "local_wins") {
            return uploadSourceOrReadConflict(binding, source, target)
        }
        if (binding.conflictPolicy === "remote_wins") {
            return { kind: "downloaded", target }
        }
        return { kind: "conflict", target }
    }
    if (direction === "upload") {
        return uploadSourceOrReadConflict(binding, source, target)
    }
    if (direction === "pull") {
        return { kind: "downloaded", target }
    }
    if (sourceChanged && shouldUpload(binding, trigger, direction)) {
        return uploadSourceOrReadConflict(binding, source, target)
    }
    if (targetChanged && shouldPull(binding, trigger, direction)) {
        return { kind: "downloaded", target }
    }
    if (!sourceChanged && !targetChanged) return { kind: "unchanged", target }
    return { kind: "deferred", target }
}

function recordConflict(
    binding: ServerTransferBinding,
    source: ServerTransferSave,
    target: DownloadedServerTransferSave,
): ServerTransferConflictOutcome {
    try {
        return {
            conflict: recordServerTransferConflictSync(
                binding,
                source.revisionId,
                source.etag,
                target.revisionId,
                target.etag,
            ),
        }
    } catch (error) {
        throw mapServerTransferOperationError(error)
    }
}

function commitDownload(
    binding: ServerTransferBinding,
    preparedSource: ServerTransferSave,
    target: DownloadedServerTransferSave,
): ServerTransferSyncOutcome | ServerTransferConflictOutcome {
    const current = getOrCreateServerTransferSourceSave(binding.sourcePlayerId, "Transfer download check")
    if (current.etag !== preparedSource.etag) return recordConflict(binding, current, target)
    ensureServerTransferSourceCanBeReplaced(binding.sourcePlayerId)
    const parsed = deserializePlayerData(binding.sourcePlayerId, target.package.data)
    try {
        return commitSaveRevisionChangeSync(() => {
            const transactionSource = getOrCreateServerTransferSourceSave(
                binding.sourcePlayerId,
                "Before transfer download",
            )
            if (transactionSource.etag !== preparedSource.etag) {
                throw new ServerTransferBindingOperationError("conflict_changed")
            }
            replacePlayerDataSync(parsed)
            capturePortablePlayerSnapshotSync(binding.sourcePlayerId, target.package.data)
            const revision = getOrCreateServerTransferSourceSave(
                binding.sourcePlayerId,
                "Transfer download",
            )
            return {
                action: "downloaded" as const,
                binding: recordServerTransferBindingSuccessSync(
                    binding,
                    target.etag,
                    revision.etag,
                    target.etag,
                ),
            }
        })
    } catch (error) {
        const mapped = mapServerTransferOperationError(error)
        if (mapped.code !== "conflict_changed") throw mapped
        return recordConflict(
            binding,
            getOrCreateServerTransferSourceSave(binding.sourcePlayerId, "Transfer conflict source"),
            target,
        )
    }
}

// //// 同步一个持久化绑定 [@x380kkm 2026-08-04] ////
async function synchronizeServerTransferBindingWithoutLock(
    bindingId: string,
    trigger: ServerTransferSyncTrigger,
    direction: ServerTransferSyncDirection = "auto",
): Promise<ServerTransferSyncOutcome | ServerTransferConflictOutcome> {
    const binding = requireServerTransferBinding(bindingId)
    if (!binding.enabled) {
        throw new ServerTransferBindingOperationError("transfer_binding_disabled")
    }
    if (getOpenServerTransferConflictSync(binding.id) !== null) {
        throw new ServerTransferBindingOperationError("transfer_conflict_open")
    }
    const source = getOrCreateServerTransferSourceSave(binding.sourcePlayerId, "Transfer sync source")
    let target: DownloadedServerTransferSave | null = null
    try {
        target = await downloadServerTransferSave(getServerTransferEndpoint(binding))
        const decision = await selectTransferDecision(binding, source, target, trigger, direction)
        if (decision.kind === "conflict") return recordConflict(binding, source, decision.target)
        if (decision.kind === "downloaded") return commitDownload(binding, source, decision.target)
        if (decision.kind === "deferred") {
            recordServerTransferBindingFailureSync(
                binding,
                source.etag,
                decision.target.etag,
                "transfer_direction_not_scheduled",
            )
            return {
                action: "deferred",
                binding: requireServerTransferBinding(binding.id),
            }
        }
        const current = getOrCreateServerTransferSourceSave(binding.sourcePlayerId, "Transfer sync current")
        if (decision.kind === "uploaded" && current.etag !== source.etag) {
            return recordConflict(binding, current, {
                package: source.package,
                revisionId: source.revisionId,
                etag: decision.targetEtag,
                shellId: binding.targetShellId,
            })
        }
        const targetEtag = decision.kind === "uploaded"
            ? decision.targetEtag
            : decision.target.etag
        return {
            action: decision.kind,
            binding: recordServerTransferBindingSuccessSync(
                binding,
                targetEtag,
                current.etag,
                targetEtag,
            ),
        }
    } catch (error) {
        const mapped = mapServerTransferOperationError(error)
        try {
            recordServerTransferBindingFailureSync(
                binding,
                source.etag,
                target?.etag ?? null,
                mapped.code,
                mapped.retryable ? 5 : binding.intervalSeconds,
            )
        } catch (storeError) {
            throw mapServerTransferOperationError(storeError)
        }
        throw mapped
    }
}

export function synchronizeServerTransferBinding(
    bindingId: string,
    trigger: ServerTransferSyncTrigger,
    direction: ServerTransferSyncDirection = "auto",
): Promise<ServerTransferSyncOutcome | ServerTransferConflictOutcome> {
    return runExclusiveServerTransferOperation(
        bindingId,
        () => synchronizeServerTransferBindingWithoutLock(bindingId, trigger, direction),
    )
}
// //// /同步一个持久化绑定 ////
