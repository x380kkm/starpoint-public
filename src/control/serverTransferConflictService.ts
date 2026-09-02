// audience: internal
// # server-transfer-conflict-service
//
// 此模块用本地覆盖, 远端覆盖或保留双方分支解决服务器传输冲突.
// 本地和目标槽在远端请求期间的变化都由 ETag 拒绝.

import {
    downloadServerTransferSave,
    uploadServerTransferSave,
} from "./serverTransferBindingClient"
import {
    getServerTransferConflictSync,
    refreshServerTransferConflictSync,
    resolveServerTransferConflictSync,
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
    DownloadedServerTransferSave,
    ResolvedServerTransferConflict,
    ServerTransferBinding,
    ServerTransferConflict,
    ServerTransferConflictResolution,
    ServerTransferSave,
} from "./serverTransferBindingTypes"
import { commitSaveRevisionChangeSync } from "../data/saveRevisions"
import {
    capturePortablePlayerSnapshotSync,
    deserializePlayerData,
} from "../data/utils"
import { replacePlayerDataSync } from "../data/wdfpData"

function requireOpenConflict(bindingId: string, conflictId: string): ServerTransferConflict {
    const conflict = getServerTransferConflictSync(conflictId)
    if (conflict === null || conflict.bindingId !== bindingId || conflict.status !== "open") {
        throw new ServerTransferBindingOperationError("conflict_not_found")
    }
    return conflict
}

function readResolvedConflict(conflictId: string): ServerTransferConflict {
    return getServerTransferConflictSync(conflictId) as ServerTransferConflict
}

// //// 使用远端分支原子覆盖本地槽 [@x380kkm 2026-08-04] ////
function refreshRemoteWinsConflict(
    binding: ServerTransferBinding,
    conflict: ServerTransferConflict,
    source: ServerTransferSave,
    target: DownloadedServerTransferSave,
): never {
    refreshServerTransferConflictSync(
        binding,
        conflict,
        source.revisionId,
        source.etag,
        target.revisionId,
        target.etag,
    )
    throw new ServerTransferBindingOperationError("conflict_changed")
}

function commitRemoteWins(
    binding: ServerTransferBinding,
    conflict: ServerTransferConflict,
    target: DownloadedServerTransferSave,
): ResolvedServerTransferConflict {
    const current = getOrCreateServerTransferSourceSave(
        binding.sourcePlayerId,
        "Conflict source check",
    )
    if (current.etag !== conflict.sourceEtag) {
        throw new ServerTransferBindingOperationError("conflict_changed")
    }
    ensureServerTransferSourceCanBeReplaced(binding.sourcePlayerId)
    const parsed = deserializePlayerData(binding.sourcePlayerId, target.package.data)
    return commitSaveRevisionChangeSync(() => {
        const transactionSource = getOrCreateServerTransferSourceSave(
            binding.sourcePlayerId,
            "Before remote conflict resolution",
        )
        if (transactionSource.etag !== conflict.sourceEtag) {
            throw new ServerTransferBindingOperationError("conflict_changed")
        }
        replacePlayerDataSync(parsed)
        capturePortablePlayerSnapshotSync(binding.sourcePlayerId, target.package.data)
        const revision = getOrCreateServerTransferSourceSave(
            binding.sourcePlayerId,
            "Remote conflict resolution",
        )
        const resolvedBinding = resolveServerTransferConflictSync(
            binding,
            conflict,
            "resolved_remote_wins",
            { common: target.etag, source: revision.etag, target: target.etag },
        )
        return {
            conflict: readResolvedConflict(conflict.id),
            binding: resolvedBinding,
        }
    })
}
// //// /使用远端分支原子覆盖本地槽 ////

async function resolveServerTransferConflictWithoutLock(
    bindingId: string,
    conflictId: string,
    resolution: ServerTransferConflictResolution,
): Promise<ResolvedServerTransferConflict> {
    const binding = requireServerTransferBinding(bindingId)
    const conflict = requireOpenConflict(bindingId, conflictId)
    if (resolution === "keep_both") {
        const resolvedBinding = resolveServerTransferConflictSync(
            binding,
            conflict,
            "resolved_keep_both",
            { common: null, source: null, target: null },
            true,
        )
        return {
            conflict: readResolvedConflict(conflict.id),
            binding: resolvedBinding,
        }
    }
    if (resolution === "local_wins") {
        const source = getOrCreateServerTransferSourceSave(
            binding.sourcePlayerId,
            "Local conflict resolution",
        )
        if (source.etag !== conflict.sourceEtag) {
            let target: DownloadedServerTransferSave
            try {
                target = await downloadServerTransferSave(getServerTransferEndpoint(binding))
            } catch (error) {
                throw mapServerTransferOperationError(error)
            }
            refreshServerTransferConflictSync(
                binding,
                conflict,
                source.revisionId,
                source.etag,
                target.revisionId,
                target.etag,
            )
            throw new ServerTransferBindingOperationError("conflict_changed")
        }
        let targetEtag: string
        try {
            targetEtag = await uploadServerTransferSave(
                getServerTransferEndpoint(binding),
                source.package,
                conflict.targetEtag,
            )
        } catch (error) {
            const mapped = mapServerTransferOperationError(error)
            if (mapped.code !== "transfer_target_revision_conflict") throw mapped
            let target: DownloadedServerTransferSave
            try {
                target = await downloadServerTransferSave(getServerTransferEndpoint(binding))
            } catch (downloadError) {
                throw mapServerTransferOperationError(downloadError)
            }
            const current = getOrCreateServerTransferSourceSave(
                binding.sourcePlayerId,
                "Local conflict refresh",
            )
            refreshServerTransferConflictSync(
                binding,
                conflict,
                current.revisionId,
                current.etag,
                target.revisionId,
                target.etag,
            )
            throw new ServerTransferBindingOperationError("conflict_changed")
        }
        const current = getOrCreateServerTransferSourceSave(
            binding.sourcePlayerId,
            "Local conflict current",
        )
        if (current.etag !== source.etag) {
            refreshServerTransferConflictSync(
                binding,
                conflict,
                current.revisionId,
                current.etag,
                null,
                targetEtag,
            )
            throw new ServerTransferBindingOperationError("conflict_changed")
        }
        const resolvedBinding = resolveServerTransferConflictSync(
            binding,
            conflict,
            "resolved_local_wins",
            { common: targetEtag, source: current.etag, target: targetEtag },
        )
        return {
            conflict: readResolvedConflict(conflict.id),
            binding: resolvedBinding,
        }
    }
    let target: DownloadedServerTransferSave
    try {
        target = await downloadServerTransferSave(getServerTransferEndpoint(binding))
    } catch (error) {
        throw mapServerTransferOperationError(error)
    }
    if (target.etag !== conflict.targetEtag) {
        return refreshRemoteWinsConflict(
            binding,
            conflict,
            getOrCreateServerTransferSourceSave(
                binding.sourcePlayerId,
                "Remote conflict refresh",
            ),
            target,
        )
    }
    try {
        return commitRemoteWins(binding, conflict, target)
    } catch (error) {
        const mapped = mapServerTransferOperationError(error)
        if (mapped.code !== "conflict_changed") throw mapped
        return refreshRemoteWinsConflict(
            binding,
            conflict,
            getOrCreateServerTransferSourceSave(
                binding.sourcePlayerId,
                "Remote conflict source refresh",
            ),
            target,
        )
    }
}

// //// 解决一个服务器传输冲突 [@x380kkm 2026-08-04] ////
export function resolveServerTransferConflict(
    bindingId: string,
    conflictId: string,
    resolution: ServerTransferConflictResolution,
): Promise<ResolvedServerTransferConflict> {
    return runExclusiveServerTransferOperation(
        bindingId,
        () => resolveServerTransferConflictWithoutLock(bindingId, conflictId, resolution),
    )
}
// //// /解决一个服务器传输冲突 ////
