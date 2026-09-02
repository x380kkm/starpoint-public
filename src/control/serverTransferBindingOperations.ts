// audience: internal
// # server-transfer-binding-operations
//
// 此模块提供服务器传输同步和冲突解决共用的本地槽边界.
// 同一 binding 的远端操作在进程内串行执行.

import { getSaveSlotActivationBlock } from "./saveSlotActivity"
import {
    ServerTransferClientError,
    ServerTransferClientErrorCode,
} from "./serverTransferBindingClient"
import {
    getServerTransferBindingSync,
} from "./serverTransferBindingStore"
import {
    ServerTransferBindingStoreError,
    ServerTransferBindingStoreErrorCode,
} from "./serverTransferStoreSupport"
import {
    ServerTransferBinding,
    ServerTransferEndpoint,
    ServerTransferSave,
} from "./serverTransferBindingTypes"
import { createSaveRevisionSync } from "../data/saveRevisions"
import { getPortableSerializedData } from "../data/utils"
import {
    getAccountFromPlayerIdSync,
    getActivePlayerIdSync,
    getPlayerSync,
} from "../data/wdfpData"
import { createStarpointSavePackage } from "../games/starpoint/portableSave"

export type ServerTransferBindingOperationErrorCode =
    | ServerTransferClientErrorCode
    | ServerTransferBindingStoreErrorCode
    | "transfer_binding_disabled"
    | "transfer_binding_busy"
    | "transfer_conflict_open"
    | "local_save_not_found"
    | "local_save_import_blocked"
    | "transfer_direction_not_scheduled"

export class ServerTransferBindingOperationError extends Error {
    constructor(
        readonly code: ServerTransferBindingOperationErrorCode,
        readonly retryable: boolean = false,
    ) {
        super(code)
    }
}

export function mapServerTransferOperationError(
    error: unknown,
): ServerTransferBindingOperationError {
    if (error instanceof ServerTransferBindingOperationError) return error
    if (error instanceof ServerTransferClientError) {
        return new ServerTransferBindingOperationError(
            error.code,
            error.code === "transfer_target_unavailable",
        )
    }
    if (error instanceof ServerTransferBindingStoreError) {
        return new ServerTransferBindingOperationError(error.code)
    }
    throw error
}

export function requireServerTransferBinding(bindingId: string): ServerTransferBinding {
    const binding = getServerTransferBindingSync(bindingId)
    if (binding === null) throw new ServerTransferBindingOperationError("binding_not_found")
    return binding
}

export function getServerTransferEndpoint(
    binding: ServerTransferBinding,
): ServerTransferEndpoint {
    return {
        baseUrl: binding.targetBaseUrl,
        instanceId: binding.targetInstanceId,
        shellId: binding.targetShellId,
        playerId: binding.targetPlayerId,
        token: binding.targetToken,
    }
}

// //// 读取本地来源槽的可移植 revision [@x380kkm 2026-08-04] ////
export function getOrCreateServerTransferSourceSave(
    playerId: number,
    label: string,
): ServerTransferSave {
    const data = getPortableSerializedData(playerId, {
        serializeRushEventData: true,
        viewerId: 0,
    })
    if (data === null) throw new ServerTransferBindingOperationError("local_save_not_found")
    const revision = createSaveRevisionSync({ playerId, data, label })
    return {
        package: createStarpointSavePackage({
            data,
            createdAt: new Date().toISOString(),
            source: {
                instanceKind: "remote",
                slotId: String(playerId),
                slotName: getPlayerSync(playerId)?.name ?? null,
                revisionId: revision.id,
            },
        }),
        revisionId: revision.id,
        etag: revision.etag,
    }
}

export function ensureServerTransferSourceCanBeReplaced(playerId: number): void {
    const account = getAccountFromPlayerIdSync(playerId)
    if (account === null) throw new ServerTransferBindingOperationError("local_save_not_found")
    if (getActivePlayerIdSync(account.id) !== playerId) return
    if (getSaveSlotActivationBlock(account.id, playerId) !== null) {
        throw new ServerTransferBindingOperationError("local_save_import_blocked")
    }
}
// //// /读取本地来源槽的可移植 revision ////

// //// 串行执行同一绑定的远端操作 [@x380kkm 2026-08-04] ////
const activeBindingOperations = new Set<string>()

export async function runExclusiveServerTransferOperation<T>(
    bindingId: string,
    operation: () => Promise<T>,
): Promise<T> {
    if (activeBindingOperations.has(bindingId)) {
        throw new ServerTransferBindingOperationError("transfer_binding_busy")
    }
    activeBindingOperations.add(bindingId)
    try {
        return await operation()
    } finally {
        activeBindingOperations.delete(bindingId)
    }
}

export function isServerTransferBindingBusy(bindingId: string): boolean {
    return activeBindingOperations.has(bindingId)
}
// //// /串行执行同一绑定的远端操作 ////
