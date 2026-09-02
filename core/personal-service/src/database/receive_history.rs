// audience: internal
// # personal-service-receive-history-storage
//
// 该模块原子保存玩家奖励和领取记录. event_key 使同一奖励提交的记录保持幂等.

use super::mails::MailReward;
use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, Transaction};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReceiveHistoryEntry {
    pub(crate) kind: i64,
    pub(crate) type_id: Option<i64>,
    pub(crate) number: i64,
    pub(crate) reason_id: i64,
    pub(crate) description: Option<String>,
    pub(crate) subject: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredReceiveHistory {
    pub(crate) create_time: i64,
    pub(crate) description: Option<String>,
    pub(crate) number: i64,
    pub(crate) reason_id: i64,
    pub(crate) subject: Option<String>,
    pub(crate) kind: i64,
    pub(crate) type_id: Option<i64>,
}

impl ReceiveHistoryEntry {
    pub(crate) fn reward(kind: i64, type_id: Option<i64>, number: i64) -> Self {
        Self {
            kind,
            type_id,
            number,
            reason_id: 0,
            description: None,
            subject: None,
        }
    }
}

// //// 创建统一领取记录并迁移已领取邮件 [@x380kkm 2026-08-22] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS player_receive_history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 account_id INTEGER NOT NULL,
                 event_key TEXT NOT NULL CHECK (length(event_key) BETWEEN 1 AND 200),
                 entry_index INTEGER NOT NULL CHECK (entry_index >= 0),
                 kind INTEGER NOT NULL CHECK (kind > 0),
                 type_id INTEGER,
                 number INTEGER NOT NULL CHECK (number > 0),
                 reason_id INTEGER NOT NULL DEFAULT 0,
                 description TEXT,
                 subject TEXT,
                 created_at INTEGER NOT NULL CHECK (created_at >= 0),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE,
                 UNIQUE (account_id, event_key, entry_index)
             );
             CREATE INDEX IF NOT EXISTS player_receive_history_recent
                 ON player_receive_history (account_id, created_at DESC, id DESC);",
        )
        .map_err(history_database_error)?;
    backfill_received_mails(connection)
}
// //// /创建统一领取记录并迁移已领取邮件 ////

impl ServiceDatabase {
    // //// 原子保存玩家快照和领取记录 [@x380kkm 2026-08-22] ////
    pub(crate) fn save_player_snapshot_with_receive_history(
        &mut self,
        account_id: i64,
        data: &str,
        event_key: &str,
        created_at: i64,
        entries: &[ReceiveHistoryEntry],
    ) -> Result<(), PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(history_database_error)?;
        save_player_snapshot(&transaction, account_id, data)?;
        insert_receive_history_in_transaction(
            &transaction,
            account_id,
            event_key,
            created_at,
            entries,
        )?;
        transaction.commit().map_err(history_database_error)
    }
    // //// /原子保存玩家快照和领取记录 ////

    // //// 原子保存任务奖励, 领取记录和任务进度 [@x380kkm 2026-08-22] ////
    pub(crate) fn save_mission_rewards_with_receive_history(
        &mut self,
        account_id: i64,
        data: &str,
        progress: &BTreeMap<(i64, i64), i64>,
        event_key: &str,
        created_at: i64,
        entries: &[ReceiveHistoryEntry],
    ) -> Result<(), PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(history_database_error)?;
        save_player_snapshot(&transaction, account_id, data)?;
        for ((category, mission_id), value) in progress {
            transaction
                .execute(
                    "INSERT INTO player_mission_progress (account_id, category, mission_id, value)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(account_id, category, mission_id)
                     DO UPDATE SET value = excluded.value",
                    params![account_id, category, mission_id, value],
                )
                .map_err(history_database_error)?;
        }
        insert_receive_history_in_transaction(
            &transaction,
            account_id,
            event_key,
            created_at,
            entries,
        )?;
        transaction.commit().map_err(history_database_error)
    }
    // //// /原子保存任务奖励, 领取记录和任务进度 ////

    // //// 读取玩家领取记录 [@x380kkm 2026-08-22] ////
    pub(crate) fn receive_history(
        &self,
        account_id: i64,
        limit: i64,
    ) -> Result<Vec<StoredReceiveHistory>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT created_at, description, number, reason_id, subject, kind, type_id
                 FROM player_receive_history
                 WHERE account_id = ?1
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?2",
            )
            .map_err(history_database_error)?;
        let history = statement
            .query_map(params![account_id, limit], |row| {
                Ok(StoredReceiveHistory {
                    create_time: row.get(0)?,
                    description: row.get(1)?,
                    number: row.get(2)?,
                    reason_id: row.get(3)?,
                    subject: row.get(4)?,
                    kind: row.get(5)?,
                    type_id: row.get(6)?,
                })
            })
            .map_err(history_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(history_database_error)?;
        Ok(history)
    }
    // //// /读取玩家领取记录 ////
}

pub(in crate::database) fn insert_receive_history_in_transaction(
    transaction: &Transaction<'_>,
    account_id: i64,
    event_key: &str,
    created_at: i64,
    entries: &[ReceiveHistoryEntry],
) -> Result<(), PersonalServiceError> {
    for (entry_index, entry) in entries.iter().enumerate() {
        if entry.number <= 0 {
            continue;
        }
        transaction
            .execute(
                "INSERT INTO player_receive_history (
                     account_id, event_key, entry_index, kind, type_id, number,
                     reason_id, description, subject, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(account_id, event_key, entry_index) DO NOTHING",
                params![
                    account_id,
                    event_key,
                    entry_index as i64,
                    entry.kind,
                    entry.type_id,
                    entry.number,
                    entry.reason_id,
                    entry.description,
                    entry.subject,
                    created_at,
                ],
            )
            .map_err(history_database_error)?;
    }
    Ok(())
}

pub(crate) fn mail_reward_history_entries(reward: &MailReward) -> Vec<ReceiveHistoryEntry> {
    let mut entries = Vec::new();
    for (type_id, number) in &reward.item_list {
        if let Ok(type_id) = type_id.parse::<i64>() {
            entries.push(ReceiveHistoryEntry::reward(1, Some(type_id), *number));
        }
    }
    for (type_id, number) in &reward.equipment_list {
        if let Ok(type_id) = type_id.parse::<i64>() {
            entries.push(ReceiveHistoryEntry::reward(6, Some(type_id), *number));
        }
    }
    for type_id in &reward.character_list {
        entries.push(ReceiveHistoryEntry::reward(5, Some(*type_id), 1));
    }
    for (kind, number) in [
        (8, reward.paid_mana),
        (3, reward.vmoney),
        (4, reward.free_vmoney),
        (8, reward.free_mana),
        (9, reward.exp_pool),
        (7, reward.star_crumb),
        (10, reward.bond_token),
        (11, reward.boss_boost_point),
        (12, reward.boost_point),
        (15, reward.rank_point),
    ] {
        entries.push(ReceiveHistoryEntry::reward(kind, None, number));
    }
    entries.retain(|entry| entry.number > 0);
    entries
}

fn save_player_snapshot(
    transaction: &Transaction<'_>,
    account_id: i64,
    data: &str,
) -> Result<(), PersonalServiceError> {
    transaction
        .execute(
            "INSERT INTO player_snapshots (account_id, data_json) VALUES (?1, ?2)
             ON CONFLICT(account_id) DO UPDATE SET data_json = excluded.data_json",
            params![account_id, data],
        )
        .map_err(history_database_error)?;
    Ok(())
}

fn backfill_received_mails(connection: &Connection) -> Result<(), PersonalServiceError> {
    let mut statement = connection
        .prepare(
            "SELECT id, account_id, rewards_json, received_at
             FROM player_mails
             WHERE received_at IS NOT NULL
             ORDER BY id",
        )
        .map_err(history_database_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(history_database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(history_database_error)?;
    drop(statement);
    for (mail_id, account_id, rewards_json, received_at) in rows {
        let reward = serde_json::from_str::<MailReward>(&rewards_json).map_err(|error| {
            PersonalServiceError::new(format!("stored mail rewards are invalid: {error}"))
        })?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(history_database_error)?;
        insert_receive_history_in_transaction(
            &transaction,
            account_id,
            &format!("mail:{mail_id}"),
            received_at,
            &mail_reward_history_entries(&reward),
        )?;
        transaction.commit().map_err(history_database_error)?;
    }
    Ok(())
}

fn history_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "receive history database operation failed: {error}"
    ))
}
