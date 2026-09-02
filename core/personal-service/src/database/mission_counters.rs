// audience: internal
// # personal-service-mission-progress-storage
//
// 该模块按账号保存 CN 任务模式计数和 master 任务进度. 一次客户端更新使用同一 SQLite 事务.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

// //// 创建 CN 任务进度表 [@x380kkm 2026-08-22] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS player_mission_counters (
                 account_id INTEGER NOT NULL,
                 pattern TEXT NOT NULL CHECK (length(trim(pattern)) BETWEEN 1 AND 128),
                 value INTEGER NOT NULL CHECK (value >= 0),
                 PRIMARY KEY (account_id, pattern),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );

             CREATE TABLE IF NOT EXISTS player_mission_progress (
                 account_id INTEGER NOT NULL,
                 category INTEGER NOT NULL CHECK (category > 0),
                 mission_id INTEGER NOT NULL CHECK (mission_id > 0),
                 value INTEGER NOT NULL CHECK (value >= 0),
                 PRIMARY KEY (account_id, category, mission_id),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );",
        )
        .map_err(mission_database_error)
}
// //// /创建 CN 任务进度表 ////

impl ServiceDatabase {
    pub(crate) fn mission_counters(
        &self,
        account_id: i64,
    ) -> Result<BTreeMap<String, i64>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT pattern, value FROM player_mission_counters
                 WHERE account_id = ?1 ORDER BY pattern",
            )
            .map_err(mission_database_error)?;
        let counters = statement
            .query_map(params![account_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(mission_database_error)?
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(mission_database_error)?;
        Ok(counters)
    }

    pub(crate) fn mission_progress(
        &self,
        account_id: i64,
    ) -> Result<BTreeMap<(i64, i64), i64>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT category, mission_id, value FROM player_mission_progress
                 WHERE account_id = ?1 ORDER BY category, mission_id",
            )
            .map_err(mission_database_error)?;
        let progress = statement
            .query_map(params![account_id], |row| {
                Ok(((row.get(0)?, row.get(1)?), row.get(2)?))
            })
            .map_err(mission_database_error)?
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(mission_database_error)?;
        Ok(progress)
    }

    // //// 保存模式计数和所有匹配任务的进度 [@x380kkm 2026-08-22] ////
    pub(crate) fn set_mission_progress(
        &mut self,
        account_id: i64,
        counters: &BTreeMap<String, i64>,
        missions: &BTreeMap<(i64, i64), i64>,
    ) -> Result<(), PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(mission_database_error)?;
        for (pattern, value) in counters {
            transaction
                .execute(
                    "INSERT INTO player_mission_counters (account_id, pattern, value)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(account_id, pattern) DO UPDATE SET value = excluded.value",
                    params![account_id, pattern, value],
                )
                .map_err(mission_database_error)?;
        }
        for ((category, mission_id), value) in missions {
            transaction
                .execute(
                    "INSERT INTO player_mission_progress (account_id, category, mission_id, value)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(account_id, category, mission_id)
                     DO UPDATE SET value = excluded.value",
                    params![account_id, category, mission_id, value],
                )
                .map_err(mission_database_error)?;
        }
        transaction.commit().map_err(mission_database_error)
    }
    // //// /保存模式计数和所有匹配任务的进度 ////
}

fn mission_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "mission progress database operation failed: {error}"
    ))
}
