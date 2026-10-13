// audience: internal
// # personal-service-activity-history-storage
// 历史按存档账号, 活动和参与轮次归档. 业务事件键保证重试只产生一条记录.
// 邮件领取状态由邮件表读取, 旧轮次记录随活动重置继续保留.

use super::ServiceDatabase;
use crate::activity_history::{collect_snapshot_events, participation_round, ActivityHistoryEntry};
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

mod reset;

// //// 创建活动历史存储 [@x380kkm 2026-09-21] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS player_activity_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL,
            activity_id TEXT NOT NULL,
            participation_round INTEGER NOT NULL CHECK(participation_round >= 0),
            event_key TEXT NOT NULL,
            kind TEXT NOT NULL,
            details_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            mail_id INTEGER,
            FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE,
            FOREIGN KEY(mail_id) REFERENCES player_mails(id) ON DELETE SET NULL,
            UNIQUE(account_id, activity_id, participation_round, kind, event_key)
        );
        CREATE INDEX IF NOT EXISTS player_activity_history_recent
        ON player_activity_history(account_id, activity_id, id DESC);"
    ).map_err(history_error)
}

// //// 在资产事务内保存活动历史 [@x380kkm 2026-09-21] ////
pub(in crate::database) fn insert_event(
    connection: &Connection, account_id: i64, event: &ActivityHistoryEntry,
) -> Result<(), PersonalServiceError> {
    connection.execute(
        "INSERT INTO player_activity_history
        (account_id,activity_id,participation_round,event_key,kind,details_json,created_at,mail_id)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
        ON CONFLICT(account_id,activity_id,participation_round,kind,event_key) DO NOTHING",
        params![account_id,event.activity_id,event.participation_round,event.event_key,event.kind,
            event.details.to_string(),event.created_at,event.mail_id],
    ).map_err(history_error)?;
    Ok(())
}

// //// 归档快照中新产生的凭据和逐场统计 [@x380kkm 2026-09-21] ////
pub(in crate::database) fn record_snapshot_events(
    connection: &Connection, account_id: i64, data: &str, captured_at: i64,
) -> Result<(), PersonalServiceError> {
    let before: Option<String> = connection.query_row(
        "SELECT data_json FROM player_snapshots WHERE account_id=?1", [account_id], |row| row.get(0),
    ).optional().map_err(history_error)?;
    let before = decode_root(before.as_deref().unwrap_or("{}"))?;
    let after = decode_root(data)?;
    super::battle_history::record_snapshot(connection, account_id, &before, &after)?;
    for history in crate::cn_mission::collect_mission_reward_history(&before, &after)? {
        super::receive_history::insert_receive_history_in_transaction(
            connection, account_id, &history.event_key, history.received_at, &history.entries,
        )?;
    }
    for event in collect_snapshot_events(&before, &after, captured_at)? {
        insert_event(connection, account_id, &event)?;
    }
    Ok(())
}

pub(in crate::database) fn decode_root(data: &str) -> Result<Map<String, Value>, PersonalServiceError> {
    serde_json::from_str(data).map_err(|error| PersonalServiceError::new(format!("invalid activity snapshot: {error}")))
}

impl ServiceDatabase {
    // //// 按存档活动分页读取历史和邮件到账状态 [@x380kkm 2026-09-21] ////
    pub(crate) fn activity_history(
        &self, account_id: i64, activity_id: &str, before_id: Option<i64>,
    ) -> Result<Value, PersonalServiceError> {
        let mut statement = self.connection.prepare(
            "SELECT h.id,h.participation_round,h.kind,h.details_json,h.created_at,h.mail_id,m.received_at
            FROM player_activity_history h LEFT JOIN player_mails m ON m.id=h.mail_id
            WHERE h.account_id=?1 AND h.activity_id=?2 AND (?3 IS NULL OR h.id<?3)
            ORDER BY h.id DESC LIMIT 101"
        ).map_err(history_error)?;
        let records = statement.query_map(params![account_id,activity_id,before_id], |row| {
            Ok((row.get::<_,i64>(0)?,row.get::<_,i64>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,
                row.get::<_,i64>(4)?,row.get::<_,Option<i64>>(5)?,row.get::<_,Option<i64>>(6)?))
        }).map_err(history_error)?.collect::<Result<Vec<_>,_>>().map_err(history_error)?;
        let has_more = records.len() > 100;
        let mut entries = Vec::new();
        for (id,round,kind,details,time,mail_id,received_at) in records.into_iter().take(100) {
            let details: Value = serde_json::from_str(&details).map_err(|error| PersonalServiceError::new(error.to_string()))?;
            entries.push(json!({"id":id,"participation_round":round,"kind":kind,"details":details,
                "created_at":time,"mail_id":mail_id,"received_at":received_at}));
        }
        let snapshot = self.activity_player_snapshot(account_id)?;
        let root = decode_root(&snapshot)?;
        Ok(json!({"activity_id":activity_id,"participation_round":participation_round(&root,activity_id),
            "next_before_id":if has_more { entries.last().and_then(|entry| entry.get("id")).cloned() } else { None },
            "entries":entries}))
    }

    pub(crate) fn activity_player_snapshot(&self, account_id: i64) -> Result<String, PersonalServiceError> {
        self.connection.query_row("SELECT data_json FROM player_snapshots WHERE account_id=?1",
            [account_id], |row| row.get(0)).map_err(history_error)
    }

    // //// 归档发生变化的排名统计 [@x380kkm 2026-09-21] ////
    pub(crate) fn record_activity_summary(
        &mut self, account_id: i64, activity_id: &str, round: i64, summary: &Value,
    ) -> Result<(), PersonalServiceError> {
        let created_at = self.current_server_time_seconds()?;
        let data = summary.to_string();
        let previous: Option<String> = self.connection.query_row(
            "SELECT details_json FROM player_activity_history
             WHERE account_id=?1 AND activity_id=?2 AND participation_round=?3 AND kind='summary'
             ORDER BY id DESC LIMIT 1", params![account_id,activity_id,round], |row| row.get(0),
        ).optional().map_err(history_error)?;
        if previous.as_deref() == Some(&data) { return Ok(()); }
        self.connection.execute(
            "INSERT INTO player_activity_history (account_id,activity_id,participation_round,event_key,kind,details_json,created_at)
             SELECT ?1,?2,?3,printf('summary:%d',COALESCE(MAX(id),0)+1),'summary',?4,?5 FROM player_activity_history",
            params![account_id,activity_id,round,data,created_at],
        ).map_err(history_error)?;
        Ok(())
    }
}

pub(super) fn history_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("activity history storage failed: {error}"))
}
