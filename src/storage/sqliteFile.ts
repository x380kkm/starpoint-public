// audience: internal
// # sqlite-file
// 此模块封装 better-sqlite3 文件操作, 其余模块只处理路径和校验结果.
// 在线备份和 WAL 检查点都在关闭临时连接后返回.

import Database from "better-sqlite3"

// //// 创建 SQLite 一致性在线备份 [@x380kkm 2026-07-22] ////
export async function createSqliteBackup(sourcePath: string, targetPath: string): Promise<void> {
    const source = new Database(sourcePath, { readonly: true, fileMustExist: true })
    try {
        await source.backup(targetPath)
    } finally {
        source.close()
    }
}
// //// /创建 SQLite 一致性在线备份 ////

// //// 将已提交 WAL 内容合并到主数据库文件 [@x380kkm 2026-07-22] ////
export function checkpointSqliteDatabase(databasePath: string): void {
    const database = new Database(databasePath, { fileMustExist: true })
    try {
        database.pragma("wal_checkpoint(TRUNCATE)")
    } finally {
        database.close()
    }
}
// //// /将已提交 WAL 内容合并到主数据库文件 ////

// //// 验证 SQLite 文件完整性 [@x380kkm 2026-07-22] ////
export function validateSqliteDatabase(databasePath: string): void {
    const database = new Database(databasePath, { readonly: true, fileMustExist: true })
    try {
        const rows = database.pragma("integrity_check") as Record<string, unknown>[]
        const results = rows.map((row) => String(Object.values(row)[0]))
        if (results.length !== 1 || results[0] !== "ok") {
            throw new Error(`SQLite integrity check failed: ${results.join(", ")}`)
        }
    } finally {
        database.close()
    }
}
// //// /验证 SQLite 文件完整性 ////
