// audience: internal
// # server-transfer-store-support
//
// 此模块提供服务器传输存储共用的 WDFP SQLite, 调度时间和并发错误边界.
// binding revision 的条件更新必须恰好修改一行.

import getDatabase, { Database } from "../data"

// //// 提供服务器传输存储的共享边界 [@x380kkm 2026-08-04] ////
export const serverTransferDatabase = getDatabase(Database.WDFP_DATA)

export type ServerTransferBindingStoreErrorCode =
    | "binding_not_found"
    | "binding_changed"
    | "conflict_not_found"
    | "conflict_changed"
    | "duplicate_binding"
    | "source_player_not_found"

export class ServerTransferBindingStoreError extends Error {
    constructor(readonly code: ServerTransferBindingStoreErrorCode) {
        super(code)
    }
}

export function getNextServerTransferRunAt(
    intervalSeconds: number,
    now: Date = new Date(),
): string {
    return new Date(now.getTime() + intervalSeconds * 1000).toISOString()
}

export function requireServerTransferBindingRevision(changes: number): void {
    if (changes !== 1) throw new ServerTransferBindingStoreError("binding_changed")
}
// //// /提供服务器传输存储的共享边界 ////
