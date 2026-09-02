// audience: internal
// # personal-service-save-automation
//
// 该模块持久化每个本地存档槽的自动快照周期和可选密文上传目标.
// 到期时间使用 UTC 文本保存, 服务暂停或重启后仍执行已经到期的任务.
// 修订号防止较早开始的上传清除较新快照的待上传状态.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Row};

pub(crate) const DEFAULT_AUTOMATION_INTERVAL_SECONDS: i64 = 900;
pub(crate) const MIN_AUTOMATION_INTERVAL_SECONDS: i64 = 60;
pub(crate) const MAX_AUTOMATION_INTERVAL_SECONDS: i64 = 2_592_000;
const AUTOMATIC_UPLOAD_RETRY_SECONDS: i64 = 5;

#[derive(Debug)]
pub(crate) struct LocalSaveAutomation {
    pub(crate) slot_id: i64,
    pub(crate) enabled: bool,
    pub(crate) interval_seconds: i64,
    pub(crate) target_id: Option<i64>,
    pub(crate) object_id: Option<String>,
    pub(crate) next_run_at: String,
    pub(crate) last_snapshot_at: Option<String>,
    pub(crate) last_upload_at: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) pending_upload: bool,
    pub(crate) snapshot_due: bool,
    pub(crate) revision: i64,
}

pub(crate) struct LocalSaveAutomationInput {
    pub(crate) enabled: bool,
    pub(crate) interval_seconds: i64,
    pub(crate) target_id: Option<i64>,
    pub(crate) object_id: Option<String>,
}

pub(crate) enum LocalSaveAutomationStoreError {
    LocalSaveNotFound,
    TargetNotFound,
    Storage(PersonalServiceError),
}

// //// 创建自动快照和上传计划表 [@x380kkm 2026-07-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS local_save_automations (
                 slot_id INTEGER PRIMARY KEY,
                 enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                 interval_seconds INTEGER NOT NULL
                     CHECK (interval_seconds BETWEEN 60 AND 2592000),
                 target_id INTEGER,
                 object_id TEXT CHECK (
                     object_id IS NULL OR length(object_id) BETWEEN 1 AND 64
                 ),
                 next_run_at TEXT NOT NULL,
                 last_snapshot_at TEXT,
                 last_upload_at TEXT,
                 last_error TEXT,
                 pending_upload INTEGER NOT NULL DEFAULT 0
                     CHECK (pending_upload IN (0, 1)),
                 revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
                 updated_at TEXT NOT NULL,
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE,
                 FOREIGN KEY (target_id) REFERENCES save_sync_targets (id) ON DELETE SET NULL
             );
             CREATE TRIGGER IF NOT EXISTS local_save_automation_clear_deleted_target
             AFTER UPDATE OF target_id ON local_save_automations
             WHEN NEW.target_id IS NULL
             BEGIN
                 UPDATE local_save_automations
                 SET object_id = NULL, pending_upload = 0,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE slot_id = NEW.slot_id;
             END;",
        )
        .map_err(automation_database_error)
}
// //// /创建自动快照和上传计划表 ////

impl ServiceDatabase {
    // //// 读取和设置自动快照计划 [@x380kkm 2026-07-23] ////
    pub(crate) fn get_local_save_automation(
        &self,
        slot_id: i64,
    ) -> Result<Option<LocalSaveAutomation>, LocalSaveAutomationStoreError> {
        if !local_save_exists(&self.connection, slot_id)? {
            return Err(LocalSaveAutomationStoreError::LocalSaveNotFound);
        }
        find_automation(&self.connection, slot_id, false)
    }

    pub(crate) fn set_local_save_automation(
        &mut self,
        slot_id: i64,
        input: &LocalSaveAutomationInput,
    ) -> Result<LocalSaveAutomation, LocalSaveAutomationStoreError> {
        if !local_save_exists(&self.connection, slot_id)? {
            return Err(LocalSaveAutomationStoreError::LocalSaveNotFound);
        }
        if let Some(target_id) = input.target_id {
            if !save_sync_target_exists(&self.connection, target_id)? {
                return Err(LocalSaveAutomationStoreError::TargetNotFound);
            }
        }
        self.connection
            .execute(
                "INSERT INTO local_save_automations (
                     slot_id, enabled, interval_seconds, target_id, object_id,
                     next_run_at, last_snapshot_at, last_upload_at, last_error,
                     pending_upload, updated_at
                 ) VALUES (
                     ?1, ?2, ?3, ?4, ?5,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('+%d seconds', ?3)),
                     NULL, NULL, NULL, 0,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )
                 ON CONFLICT(slot_id) DO UPDATE SET
                     enabled = excluded.enabled,
                     interval_seconds = excluded.interval_seconds,
                     target_id = excluded.target_id,
                     object_id = excluded.object_id,
                     next_run_at = excluded.next_run_at,
                     pending_upload = 0,
                     revision = local_save_automations.revision + 1,
                     last_error = NULL,
                     updated_at = excluded.updated_at",
                params![
                    slot_id,
                    input.enabled,
                    input.interval_seconds,
                    input.target_id,
                    input.object_id,
                ],
            )
            .map_err(automation_storage_error)?;
        find_automation(&self.connection, slot_id, false)?.ok_or_else(|| {
            LocalSaveAutomationStoreError::Storage(PersonalServiceError::new(
                "saved local save automation is missing",
            ))
        })
    }
    // //// /读取和设置自动快照计划 ////

    // //// 列出到期快照和待上传任务 [@x380kkm 2026-07-23] ////
    pub(crate) fn list_due_local_save_automations(
        &self,
    ) -> Result<Vec<LocalSaveAutomation>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT slot_id, enabled, interval_seconds, target_id, object_id,
                        next_run_at, last_snapshot_at, last_upload_at, last_error,
                        pending_upload, revision,
                        next_run_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 FROM local_save_automations
                 WHERE enabled = 1 AND (
                     next_run_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now') OR
                     (
                         pending_upload = 1 AND
                         updated_at <= strftime(
                             '%Y-%m-%dT%H:%M:%fZ', 'now', printf('-%d seconds', ?1)
                         )
                     )
                 )
                 ORDER BY next_run_at, slot_id",
            )
            .map_err(automation_database_error)?;
        let automations = statement
            .query_map(params![AUTOMATIC_UPLOAD_RETRY_SECONDS], read_automation)
            .map_err(automation_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(automation_database_error)?;
        Ok(automations)
    }
    // //// /列出到期快照和待上传任务 ////

    // //// 记录自动快照结果并推进周期 [@x380kkm 2026-07-23] ////
    pub(crate) fn record_automatic_snapshot_success(
        &mut self,
        slot_id: i64,
    ) -> Result<(), PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE local_save_automations
                 SET next_run_at = strftime(
                         '%Y-%m-%dT%H:%M:%fZ', 'now',
                         printf('+%d seconds', interval_seconds)
                     ),
                     last_snapshot_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     last_error = NULL,
                     pending_upload = CASE
                         WHEN target_id IS NOT NULL AND object_id IS NOT NULL THEN 1
                         ELSE 0
                     END,
                     revision = revision + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE slot_id = ?1 AND enabled = 1",
                params![slot_id],
            )
            .map_err(automation_database_error)?;
        Ok(())
    }

    pub(crate) fn record_automatic_snapshot_failure(
        &mut self,
        slot_id: i64,
        error_code: &str,
    ) -> Result<(), PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE local_save_automations
                 SET next_run_at = strftime(
                         '%Y-%m-%dT%H:%M:%fZ', 'now',
                         printf('+%d seconds', interval_seconds)
                     ),
                     last_error = ?2,
                     pending_upload = 0,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE slot_id = ?1",
                params![slot_id, error_code],
            )
            .map_err(automation_database_error)?;
        Ok(())
    }
    // //// /记录自动快照结果并推进周期 ////

    // //// 记录自动密文上传结果 [@x380kkm 2026-07-23] ////
    pub(crate) fn record_automatic_upload_success(
        &mut self,
        slot_id: i64,
        target_id: i64,
        object_id: &str,
        revision: i64,
    ) -> Result<(), PersonalServiceError> {
        update_upload_result(
            &self.connection,
            slot_id,
            target_id,
            object_id,
            revision,
            None,
        )
    }

    pub(crate) fn record_automatic_upload_failure(
        &mut self,
        slot_id: i64,
        target_id: i64,
        object_id: &str,
        revision: i64,
        error_code: &str,
    ) -> Result<(), PersonalServiceError> {
        update_upload_result(
            &self.connection,
            slot_id,
            target_id,
            object_id,
            revision,
            Some(error_code),
        )
    }

    pub(crate) fn schedule_automatic_upload_retry(
        &mut self,
        slot_id: i64,
        target_id: i64,
        object_id: &str,
        revision: i64,
        error_code: &str,
    ) -> Result<(), PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE local_save_automations
                 SET pending_upload = 1,
                     last_error = ?5,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE slot_id = ?1 AND target_id = ?2 AND object_id = ?3
                   AND revision = ?4",
                params![slot_id, target_id, object_id, revision, error_code],
            )
            .map_err(automation_database_error)?;
        Ok(())
    }
    // //// /记录自动密文上传结果 ////
}

fn find_automation(
    connection: &Connection,
    slot_id: i64,
    snapshot_due: bool,
) -> Result<Option<LocalSaveAutomation>, LocalSaveAutomationStoreError> {
    connection
        .query_row(
            "SELECT slot_id, enabled, interval_seconds, target_id, object_id,
                    next_run_at, last_snapshot_at, last_upload_at, last_error,
                    pending_upload, revision, ?2
             FROM local_save_automations WHERE slot_id = ?1",
            params![slot_id, snapshot_due],
            read_automation,
        )
        .optional()
        .map_err(automation_storage_error)
}

fn read_automation(row: &Row<'_>) -> rusqlite::Result<LocalSaveAutomation> {
    Ok(LocalSaveAutomation {
        slot_id: row.get(0)?,
        enabled: row.get(1)?,
        interval_seconds: row.get(2)?,
        target_id: row.get(3)?,
        object_id: row.get(4)?,
        next_run_at: row.get(5)?,
        last_snapshot_at: row.get(6)?,
        last_upload_at: row.get(7)?,
        last_error: row.get(8)?,
        pending_upload: row.get(9)?,
        revision: row.get(10)?,
        snapshot_due: row.get(11)?,
    })
}

fn local_save_exists(
    connection: &Connection,
    slot_id: i64,
) -> Result<bool, LocalSaveAutomationStoreError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
            params![slot_id],
            |row| row.get(0),
        )
        .map_err(automation_storage_error)
}

fn save_sync_target_exists(
    connection: &Connection,
    target_id: i64,
) -> Result<bool, LocalSaveAutomationStoreError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM save_sync_targets WHERE id = ?1)",
            params![target_id],
            |row| row.get(0),
        )
        .map_err(automation_storage_error)
}

fn update_upload_result(
    connection: &Connection,
    slot_id: i64,
    target_id: i64,
    object_id: &str,
    revision: i64,
    error_code: Option<&str>,
) -> Result<(), PersonalServiceError> {
    connection
        .execute(
            "UPDATE local_save_automations
             SET pending_upload = 0,
                 last_upload_at = CASE
                     WHEN ?5 IS NULL THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     ELSE last_upload_at
                 END,
                 last_error = ?5,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE slot_id = ?1 AND target_id = ?2 AND object_id = ?3
               AND revision = ?4",
            params![slot_id, target_id, object_id, revision, error_code],
        )
        .map_err(automation_database_error)?;
    Ok(())
}

fn automation_storage_error(error: rusqlite::Error) -> LocalSaveAutomationStoreError {
    LocalSaveAutomationStoreError::Storage(automation_database_error(error))
}

fn automation_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "failed to access local save automation storage: {error}"
    ))
}
