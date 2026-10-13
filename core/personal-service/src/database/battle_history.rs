// audience: internal
// # personal-service-battle-history-storage
// 战斗历史按账号和玩法保存. 快照提交事务同时归档新记录, 查询保持结算顺序.

use super::ServiceDatabase;
use crate::cn_battle_history::SNAPSHOT_FIELD;
use crate::PersonalServiceError;
use rusqlite::{params, Connection};
use serde_json::{Map, Value};

// //// 创建逐场战斗历史存储 [@x380kkm 2026-09-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS player_battle_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL,
            category INTEGER NOT NULL,
            event_id INTEGER,
            play_id TEXT NOT NULL,
            record_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
            UNIQUE(account_id, category, play_id)
        );
        CREATE INDEX IF NOT EXISTS player_battle_history_recent
        ON player_battle_history(account_id, category, id DESC);",
        )
        .map_err(history_error)
}

// //// 在玩家快照事务中归档新增战斗记录 [@x380kkm 2026-09-23] ////
pub(super) fn record_snapshot(
    connection: &Connection,
    account_id: i64,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Result<(), PersonalServiceError> {
    let Some(entry) = after.get(SNAPSHOT_FIELD) else {
        return Ok(());
    };
    if before.get(SNAPSHOT_FIELD) == Some(entry) {
        return Ok(());
    }
    let category = entry
        .get("category")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("battle history category is missing"))?;
    let play_id = entry
        .get("play_id")
        .and_then(Value::as_str)
        .ok_or_else(|| PersonalServiceError::new("battle history play id is missing"))?;
    let created_at = entry
        .get("created_at")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("battle history time is missing"))?;
    let record = entry
        .get("record")
        .filter(|record| record.is_object())
        .ok_or_else(|| PersonalServiceError::new("battle history record is missing"))?;
    connection
        .execute(
            "INSERT INTO player_battle_history
        (account_id,category,event_id,play_id,record_json,created_at)
        VALUES (?1,?2,?3,?4,?5,?6)
        ON CONFLICT(account_id,category,play_id) DO NOTHING",
            params![
                account_id,
                category,
                entry.get("event_id").and_then(Value::as_i64),
                play_id,
                record.to_string(),
                created_at
            ],
        )
        .map_err(history_error)?;
    Ok(())
}

impl ServiceDatabase {
    // //// 按账号和玩法读取当场历史快照 [@x380kkm 2026-09-23] ////
    pub(crate) fn battle_history(
        &self,
        account_id: i64,
        category: i64,
        event_id: Option<i64>,
    ) -> Result<Vec<Value>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT record_json FROM player_battle_history
             WHERE account_id=?1 AND category=?2 AND (?3 IS NULL OR event_id=?3)
             ORDER BY id DESC",
            )
            .map_err(history_error)?;
        let records = statement
            .query_map(params![account_id, category, event_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(history_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(history_error)?;
        records
            .into_iter()
            .map(|record| {
                serde_json::from_str(&record).map_err(|error| {
                    PersonalServiceError::new(format!("invalid stored battle history: {error}"))
                })
            })
            .collect()
    }
}

fn history_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("battle history storage failed: {error}"))
}
