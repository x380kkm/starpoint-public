// audience: internal | external
// # server-transfer-binding-types
//
// 此模块定义完整服务器跨实例槽位绑定的持久化模型和管理契约.
// 公开视图不包含远端槽 token.

import { StarpointSavePackage } from "../games/starpoint/portableSave"

export type ServerTransferScheduleMode = "manual" | "interval"
export type ServerTransferConflictPolicy = "local_wins" | "remote_wins" | "ask"
export type ServerTransferPendingDirection = "none" | "upload" | "pull" | "conflict"
export type ServerTransferConflictStatus =
    | "open"
    | "resolved_local_wins"
    | "resolved_remote_wins"
    | "resolved_keep_both"
export type ServerTransferConflictResolution = "local_wins" | "remote_wins" | "keep_both"
export type ServerTransferSyncDirection = "auto" | "upload" | "pull"
export type ServerTransferSyncTrigger = "manual" | "interval"
export type ServerTransferSyncAction = "unchanged" | "uploaded" | "downloaded" | "deferred"

export interface ServerTransferBinding {
    id: string
    sourcePlayerId: number
    targetBaseUrl: string
    targetInstanceId: string
    targetShellId: string
    targetPlayerId: number
    targetToken: string
    uploadMode: ServerTransferScheduleMode
    pullMode: ServerTransferScheduleMode
    conflictPolicy: ServerTransferConflictPolicy
    intervalSeconds: number
    enabled: boolean
    lastCommonEtag: string | null
    lastSourceEtag: string | null
    lastTargetEtag: string | null
    pendingDirection: ServerTransferPendingDirection
    nextRunAt: string
    lastSyncedAt: string | null
    lastError: string | null
    revision: number
    createdAt: string
    updatedAt: string
}

export interface ServerTransferConflict {
    id: string
    bindingId: string
    sourceRevisionId: string
    sourceEtag: string
    targetRevisionId: string | null
    targetEtag: string
    detectedAt: string
    status: ServerTransferConflictStatus
    resolvedAt: string | null
}

export interface CreateServerTransferBindingInput {
    sourcePlayerId: number
    targetBaseUrl: string
    targetInstanceId: string
    targetShellId: string
    targetPlayerId: number
    targetToken: string
    uploadMode: ServerTransferScheduleMode
    pullMode: ServerTransferScheduleMode
    conflictPolicy: ServerTransferConflictPolicy
    intervalSeconds: number
    enabled: boolean
    observedSourceEtag: string
    observedTargetEtag: string
}

export interface UpdateServerTransferBindingInput {
    uploadMode: ServerTransferScheduleMode
    pullMode: ServerTransferScheduleMode
    conflictPolicy: ServerTransferConflictPolicy
    intervalSeconds: number
    enabled: boolean
    targetToken?: string
}

export interface ServerTransferEndpoint {
    baseUrl: string
    instanceId: string
    shellId?: string
    playerId: number
    token: string
}

export interface ServerTransferSave {
    package: StarpointSavePackage
    revisionId: string
    etag: string
}

export interface DownloadedServerTransferSave extends ServerTransferSave {
    shellId: string
}

export interface ServerTransferSyncOutcome {
    action: ServerTransferSyncAction
    binding: ServerTransferBinding
}

export interface ServerTransferConflictOutcome {
    conflict: ServerTransferConflict
}

export interface ResolvedServerTransferConflict {
    conflict: ServerTransferConflict
    binding: ServerTransferBinding
}
