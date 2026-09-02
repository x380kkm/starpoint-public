// audience: internal
// # database-registry
// 此模块按 DATABASE_PATH 注册游戏数据库, 并在首次访问时完成初始化和版本更新.
// 每个数据库在单个进程内只打开一次.

import sqlite3, { Database as BetterSqlite3Database } from 'better-sqlite3';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs';
import path from "path";
import { updateBeforeInit as updateWdfpDataBefore, updateAfterInit as updateWdfpDataAfter} from "./updaters/wdfpData";
import initWdfpData from "./initializers/wdfpData";

// //// 解析并创建游戏数据库目录 [@x380kkm 2026-07-22] ////
const rootDir = process.cwd();
const databasePath = path.resolve(process.env.DATABASE_PATH ?? path.join(rootDir, ".database", "wdfp_data.db"))
const databaseDirectory = path.dirname(databasePath)
const versionFileExtension = ".version"
if (!existsSync(databaseDirectory)) {
    try {
        mkdirSync(databaseDirectory, { recursive: true })
    } catch (error) {
        throw new Error(`Failed to create the data directory. Reason: ${(error as Error).message}`)
    }
}
// //// /解析并创建游戏数据库目录 ////

export const enum Database {
    WDFP_DATA
}

interface DatabaseMetadata {
    path: string
    latestVersion: number
    init?: (database: BetterSqlite3Database, exists: boolean) => void
    updateBefore?: (database: BetterSqlite3Database, currentVersion: number) => void
    updateAfter?: (database: BetterSqlite3Database, currentVersion: number) => void
}

const databasesMetadata: {[key in Database]: DatabaseMetadata} = {
    [Database.WDFP_DATA]: {
        path: databasePath,
        init: initWdfpData,
        updateBefore: updateWdfpDataBefore,
        updateAfter: updateWdfpDataAfter,
        latestVersion: 3
    }
}

const loadedDatabases: {
    [key in Database]?: BetterSqlite3Database
} = {}

export default function getDatabase(
    database: Database
): BetterSqlite3Database {
    // don't try to load an already-loaded database
    const isLoaded = loadedDatabases[database]
    if (isLoaded) return isLoaded

    // get metadata
    const metadata = databasesMetadata[database]

    const absoluteDatabasePath = metadata.path
    // check if the db already exists
    const dbExists = existsSync(absoluteDatabasePath)

    // get the database's version
    let currentVersion: number = 0
    const versionFilePath = `${absoluteDatabasePath}${versionFileExtension}`
    if (dbExists && existsSync(versionFilePath)) {
        const fileContents = readFileSync(versionFilePath).toString('utf-8')
        const versionNumber = Number(fileContents)
        currentVersion = isNaN(versionNumber) ? currentVersion : versionNumber
    }

    // create new db
    const db = new sqlite3(absoluteDatabasePath)

    // set pragma
    db.pragma('journal_mode = WAL')
    db.pragma('foreign_keys = ON')

    // call init & update function
    const init = metadata.init
    const updateBefore = metadata.updateBefore
    const updateAfter = metadata.updateAfter
    if (init !== undefined) {
        try {
            // try to update before initialization
            const latestVersion = metadata.latestVersion
            const updateRequired = dbExists && metadata.latestVersion > currentVersion
            if (updateRequired && updateBefore !== undefined) {
                console.log("Updating wdfp_data.db...")
                updateBefore(db, currentVersion)
            }

            // initialize
            init(db, dbExists)

            // try to update after initialization
            if (updateRequired && updateAfter !== undefined) {
                updateAfter(db, currentVersion)
                console.log("Successfully updated wdfp_data.db")
            }

            // write version file
            writeFileSync(versionFilePath, latestVersion.toString(), { encoding: 'utf-8' })
        } catch (error) {
            console.log(error)
            console.log(`Initalization failed for module ${metadata.path}. Error: ${error}`)
        }
    }

    // add to loaded databases
    loadedDatabases[database] = db

    return db
}
