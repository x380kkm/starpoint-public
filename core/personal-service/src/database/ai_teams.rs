// audience: internal
// # personal-service-ai-team-storage
//
// 该模块保存从本地存档提取的两个不可变 AI 队伍快照, 槽位自动选择设置和实例来源选择.
// 当前 head 只表示每个槽位的两个活动快照. 历史快照不会被更新.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub(crate) struct AiTeamSnapshot {
    pub(crate) id: String,
    pub(crate) slot_id: i64,
    pub(crate) team_index: i64,
    pub(crate) party_id: i64,
    pub(crate) source_revision_id: String,
    pub(crate) data_json: String,
    pub(crate) created_at: String,
}

pub(crate) struct AiTeamSnapshotInput {
    pub(crate) team_index: i64,
    pub(crate) party_id: i64,
    pub(crate) data_json: String,
}

#[derive(Debug)]
pub(crate) enum AiTeamStoreError {
    NotFound,
    InvalidState,
    Storage(PersonalServiceError),
}

// //// 创建不可变 AI 队伍快照表 [@x380kkm 2026-08-18] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS ai_team_snapshots (
                 id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 32),
                 slot_id INTEGER NOT NULL,
                 team_index INTEGER NOT NULL CHECK (team_index IN (0, 1)),
                 party_id INTEGER NOT NULL CHECK (party_id > 0),
                 source_revision_id TEXT NOT NULL,
                 data_json TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 UNIQUE (id, slot_id, team_index),
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE,
                 FOREIGN KEY (source_revision_id) REFERENCES local_save_revisions (id) ON DELETE RESTRICT
             );
             CREATE INDEX IF NOT EXISTS ai_team_snapshots_slot_index
             ON ai_team_snapshots (slot_id, team_index, created_at DESC, id DESC);
             CREATE TABLE IF NOT EXISTS ai_team_snapshot_heads (
                 slot_id INTEGER NOT NULL,
                 team_index INTEGER NOT NULL CHECK (team_index IN (0, 1)),
                 snapshot_id TEXT NOT NULL UNIQUE,
                 PRIMARY KEY (slot_id, team_index),
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE,
                 FOREIGN KEY (snapshot_id) REFERENCES ai_team_snapshots (id) ON DELETE RESTRICT
             );
             CREATE TABLE IF NOT EXISTS ai_team_slot_settings (
                 slot_id INTEGER PRIMARY KEY NOT NULL,
                 automatic_selection_enabled INTEGER NOT NULL
                     CHECK (automatic_selection_enabled IN (0, 1)),
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS ai_team_source (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 slot_id INTEGER,
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE SET NULL
             );
             CREATE TRIGGER IF NOT EXISTS ai_team_snapshots_immutable
             BEFORE UPDATE ON ai_team_snapshots
             BEGIN
                 SELECT RAISE(ABORT, 'AI team snapshots are immutable');
             END;",
        )
        .map_err(ai_team_database_error)
}
// //// /创建不可变 AI 队伍快照表 ////

impl ServiceDatabase {
    // //// 读取实例 AI 来源存档 [@x380kkm 2026-09-29] ////
    pub(crate) fn ai_team_source_slot_id(&self) -> Result<Option<i64>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT slot_id FROM ai_team_source WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(ai_team_database_error)
    }

    // //// 将实例 AI 来源恢复为当前玩家存档 [@x380kkm 2026-09-29] ////
    pub(crate) fn follow_current_ai_team_source(&self) -> Result<(), PersonalServiceError> {
        self.connection
            .execute("DELETE FROM ai_team_source WHERE id = 1", [])
            .map_err(ai_team_database_error)?;
        Ok(())
    }

    // //// 读取槽位自动选择设置 [@x380kkm 2026-08-20] ////
    pub(crate) fn is_ai_team_automatic_selection_enabled(
        &self,
        slot_id: i64,
    ) -> Result<Option<bool>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT COALESCE(settings.automatic_selection_enabled, 1)
                 FROM local_save_slots AS slots
                 LEFT JOIN ai_team_slot_settings AS settings ON settings.slot_id = slots.id
                 WHERE slots.id = ?1",
                params![slot_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(ai_team_database_error)
    }
    // //// /读取槽位自动选择设置 ////

    pub(crate) fn local_save_revision_data(
        &self,
        slot_id: i64,
        revision_id: &str,
    ) -> Result<Option<String>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT data_json
                 FROM local_save_revisions
                 WHERE slot_id = ?1 AND id = ?2",
                params![slot_id, revision_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(ai_team_database_error)
    }

    pub(crate) fn replace_ai_team_snapshots(
        &mut self,
        slot_id: i64,
        source_revision_id: &str,
        snapshots: &[AiTeamSnapshotInput],
    ) -> Result<Vec<AiTeamSnapshot>, AiTeamStoreError> {
        self.stage_ai_team_snapshots(slot_id, source_revision_id, snapshots)?
            .commit()
            .map_err(ai_team_storage_error)?;
        self.list_ai_team_snapshots(slot_id)
            .map_err(AiTeamStoreError::Storage)
            .and_then(|snapshots| snapshots.ok_or(AiTeamStoreError::InvalidState))
    }

    // //// 原子保存两个快照并选择实例 AI 来源 [@x380kkm 2026-09-29] ////
    pub(crate) fn select_ai_team_snapshots(
        &mut self,
        slot_id: i64,
        source_revision_id: &str,
        snapshots: &[AiTeamSnapshotInput],
    ) -> Result<Vec<AiTeamSnapshot>, AiTeamStoreError> {
        let transaction = self.stage_ai_team_snapshots(slot_id, source_revision_id, snapshots)?;
        transaction
            .execute(
                "INSERT INTO ai_team_source (id, slot_id) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET slot_id = excluded.slot_id",
                params![slot_id],
            )
            .map_err(ai_team_storage_error)?;
        transaction.commit().map_err(ai_team_storage_error)?;
        self.list_ai_team_snapshots(slot_id)
            .map_err(AiTeamStoreError::Storage)
            .and_then(|snapshots| snapshots.ok_or(AiTeamStoreError::InvalidState))
    }

    // //// 在事务中建立槽位的两个 AI 快照 [@x380kkm 2026-09-29] ////
    fn stage_ai_team_snapshots(
        &mut self,
        slot_id: i64,
        source_revision_id: &str,
        snapshots: &[AiTeamSnapshotInput],
    ) -> Result<rusqlite::Transaction<'_>, AiTeamStoreError> {
        if snapshots.len() != 2
            || snapshots
                .iter()
                .map(|snapshot| snapshot.team_index)
                .collect::<std::collections::BTreeSet<_>>()
                != [0_i64, 1_i64]
                    .into_iter()
                    .collect::<std::collections::BTreeSet<_>>()
            || snapshots[0].party_id == snapshots[1].party_id
        {
            return Err(AiTeamStoreError::InvalidState);
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(ai_team_storage_error)?;
        let slot_exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(ai_team_storage_error)?;
        if !slot_exists {
            return Err(AiTeamStoreError::NotFound);
        }
        let revision_exists = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM local_save_revisions
                     WHERE id = ?1 AND slot_id = ?2
                 )",
                params![source_revision_id, slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(ai_team_storage_error)?;
        if !revision_exists {
            return Err(AiTeamStoreError::InvalidState);
        }
        for snapshot in snapshots {
            if snapshot.party_id <= 0 || snapshot.data_json.len() > 512 * 1024 {
                return Err(AiTeamStoreError::InvalidState);
            }
            serde_json::from_str::<serde_json::Value>(&snapshot.data_json)
                .map_err(|_| AiTeamStoreError::InvalidState)?;
            let snapshot_id = transaction
                .query_row(
                    "INSERT INTO ai_team_snapshots (
                         id, slot_id, team_index, party_id, source_revision_id,
                         data_json, created_at
                     ) VALUES (
                         lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5,
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     )
                     RETURNING id",
                    params![
                        slot_id,
                        snapshot.team_index,
                        snapshot.party_id,
                        source_revision_id,
                        snapshot.data_json,
                    ],
                    |row| row.get::<_, String>(0),
                )
                .map_err(ai_team_storage_error)?;
            transaction
                .execute(
                    "INSERT INTO ai_team_snapshot_heads (slot_id, team_index, snapshot_id)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(slot_id, team_index) DO UPDATE SET
                         snapshot_id = excluded.snapshot_id",
                    params![slot_id, snapshot.team_index, snapshot_id],
                )
                .map_err(ai_team_storage_error)?;
        }
        transaction
            .execute(
                "INSERT INTO ai_team_slot_settings (slot_id, automatic_selection_enabled)
                 VALUES (?1, 1)
                 ON CONFLICT(slot_id) DO UPDATE SET automatic_selection_enabled = 1",
                params![slot_id],
            )
            .map_err(ai_team_storage_error)?;
        Ok(transaction)
    }

    pub(crate) fn list_ai_team_snapshots(
        &self,
        slot_id: i64,
    ) -> Result<Option<Vec<AiTeamSnapshot>>, PersonalServiceError> {
        let slot_exists = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(ai_team_database_error)?;
        if !slot_exists {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT snapshots.id, snapshots.slot_id, snapshots.team_index,
                        snapshots.party_id, snapshots.source_revision_id,
                        snapshots.data_json, snapshots.created_at
                 FROM ai_team_snapshot_heads AS heads
                 JOIN ai_team_snapshots AS snapshots
                   ON snapshots.id = heads.snapshot_id
                  AND snapshots.slot_id = heads.slot_id
                  AND snapshots.team_index = heads.team_index
                 WHERE heads.slot_id = ?1
                 ORDER BY snapshots.team_index",
            )
            .map_err(ai_team_database_error)?;
        let snapshots = statement
            .query_map(params![slot_id], read_ai_team_snapshot)
            .map_err(ai_team_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ai_team_database_error)?;
        Ok(Some(snapshots))
    }

    pub(crate) fn clear_ai_team_snapshots(
        &mut self,
        slot_id: i64,
    ) -> Result<Option<usize>, AiTeamStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(ai_team_storage_error)?;
        let slot_exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(ai_team_storage_error)?;
        if !slot_exists {
            return Ok(None);
        }
        let deleted = transaction
            .execute(
                "DELETE FROM ai_team_snapshot_heads WHERE slot_id = ?1",
                params![slot_id],
            )
            .map_err(ai_team_storage_error)?;
        transaction
            .execute(
                "INSERT INTO ai_team_slot_settings (slot_id, automatic_selection_enabled)
                 VALUES (?1, 0)
                 ON CONFLICT(slot_id) DO UPDATE SET automatic_selection_enabled = 0",
                params![slot_id],
            )
            .map_err(ai_team_storage_error)?;
        transaction.commit().map_err(ai_team_storage_error)?;
        Ok(Some(deleted))
    }
}

fn read_ai_team_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiTeamSnapshot> {
    Ok(AiTeamSnapshot {
        id: row.get(0)?,
        slot_id: row.get(1)?,
        team_index: row.get(2)?,
        party_id: row.get(3)?,
        source_revision_id: row.get(4)?,
        data_json: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn ai_team_storage_error(error: rusqlite::Error) -> AiTeamStoreError {
    AiTeamStoreError::Storage(ai_team_database_error(error))
}

fn ai_team_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to access AI team storage: {error}"))
}
