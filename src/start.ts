// audience: internal | external
// # server-start
// 此入口在加载任何游戏数据库模块前应用待恢复文件.
// 恢复失败时保留原数据库并继续启动, 待恢复状态供管理页面诊断.

import { managementStore } from "./control/management"

// //// 在游戏数据库加载前应用已校验恢复文件 [@x380kkm 2026-07-22] ////
async function applyRestoreBeforeDatabaseLoad(): Promise<void> {
    try {
        const applied = await managementStore.applyPendingRestore()
        if (applied === null) return
        console.log(`Applied backup ${applied.backupId} before database initialization.`)
        if (applied.rollbackRetained) {
            console.warn("The restored database is active, but the rollback file could not be removed.")
        }
    } catch (error) {
        console.error("Pending database restore was not applied; the existing database remains active.", error)
    }
}
// //// /在游戏数据库加载前应用已校验恢复文件 ////

// //// 恢复完成后加载 HTTP 服务 [@x380kkm 2026-07-22] ////
async function startServerProcess(): Promise<void> {
    await applyRestoreBeforeDatabaseLoad()
    await import("./server")
}
// //// /恢复完成后加载 HTTP 服务 ////

startServerProcess().catch((error: unknown) => {
    console.error(error)
    process.exit(1)
})
