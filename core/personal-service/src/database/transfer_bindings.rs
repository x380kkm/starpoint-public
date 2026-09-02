// audience: internal
// # personal-service-transfer-bindings
//
// 该模块持久化本地槽与另一服务实例槽之间的传输绑定.
// 出站槽 token 只供个人服务发起传输, 查询响应不返回该凭据.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Row};

mod conflicts;
mod types;

pub(crate) use types::*;

// //// 创建传输绑定和冲突表 [@x380kkm 2026-08-03] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS transfer_bindings (
                 id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 32),
                 source_slot_id INTEGER NOT NULL,
                 target_profile_id INTEGER NOT NULL,
                 target_instance_kind TEXT NOT NULL CHECK (target_instance_kind IN ('local', 'remote')),
                 target_instance_id TEXT NOT NULL CHECK (length(target_instance_id) = 32),
                 target_shell_id TEXT NOT NULL CHECK (length(target_shell_id) BETWEEN 1 AND 128),
                 target_slot_id INTEGER NOT NULL CHECK (target_slot_id > 0),
                 target_token TEXT NOT NULL CHECK (length(target_token) BETWEEN 1 AND 512),
                 upload_mode TEXT NOT NULL CHECK (upload_mode IN ('manual', 'on_connect', 'interval')),
                 pull_mode TEXT NOT NULL CHECK (pull_mode IN ('manual', 'on_disconnect', 'interval')),
                 conflict_policy TEXT NOT NULL CHECK (conflict_policy IN ('local_wins', 'remote_wins', 'ask')),
                 interval_seconds INTEGER NOT NULL CHECK (interval_seconds BETWEEN 60 AND 2592000),
                 enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
                 last_common_etag TEXT CHECK (last_common_etag IS NULL OR length(last_common_etag) = 64),
                 last_source_etag TEXT CHECK (last_source_etag IS NULL OR length(last_source_etag) = 64),
                 last_target_etag TEXT CHECK (last_target_etag IS NULL OR length(last_target_etag) = 64),
                 pending_direction TEXT NOT NULL CHECK (pending_direction IN ('none', 'upload', 'pull', 'conflict')),
                 next_run_at TEXT NOT NULL,
                 last_synced_at TEXT,
                 last_error TEXT,
                 revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 UNIQUE (source_slot_id, target_instance_id, target_slot_id),
                 FOREIGN KEY (source_slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE,
                 FOREIGN KEY (target_profile_id) REFERENCES server_profiles (id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS transfer_conflicts (
                 id TEXT PRIMARY KEY NOT NULL CHECK (length(id) = 32),
                 binding_id TEXT NOT NULL,
                 source_revision_id TEXT NOT NULL CHECK (length(source_revision_id) = 32),
                 source_etag TEXT NOT NULL CHECK (length(source_etag) = 64),
                 target_revision_id TEXT,
                 target_etag TEXT NOT NULL CHECK (length(target_etag) = 64),
                 detected_at TEXT NOT NULL,
                 status TEXT NOT NULL CHECK (status IN (
                     'open', 'resolved_local_wins', 'resolved_remote_wins', 'resolved_keep_both'
                 )),
                 resolved_at TEXT,
                 FOREIGN KEY (binding_id) REFERENCES transfer_bindings (id) ON DELETE CASCADE
             );
             CREATE UNIQUE INDEX IF NOT EXISTS transfer_conflicts_one_open_per_binding
                 ON transfer_conflicts (binding_id) WHERE status = 'open';",
        )
        .map_err(transfer_database_error)
}
// //// /创建传输绑定和冲突表 ////

impl ServiceDatabase {
    // //// 创建和读取明确的槽位传输绑定 [@x380kkm 2026-08-03] ////
    pub(crate) fn create_transfer_binding(
        &mut self,
        input: &CreateTransferBindingInput,
    ) -> Result<TransferBinding, TransferBindingStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(transfer_storage_error)?;
        let source_exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![input.source_slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(transfer_storage_error)?;
        if !source_exists {
            return Err(TransferBindingStoreError::SourceSlotNotFound);
        }
        let target_is_local = transaction
            .query_row(
                "SELECT mode = 'local' FROM server_profiles WHERE id = ?1",
                params![input.target_profile_id],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map_err(transfer_storage_error)?
            .ok_or(TransferBindingStoreError::TargetProfileNotFound)?;
        if target_is_local {
            return Err(TransferBindingStoreError::TargetProfileIsLocal);
        }
        let duplicate = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM transfer_bindings
                     WHERE source_slot_id = ?1 AND target_instance_id = ?2 AND target_slot_id = ?3
                 )",
                params![
                    input.source_slot_id,
                    input.target_instance_id,
                    input.target_slot_id
                ],
                |row| row.get::<_, bool>(0),
            )
            .map_err(transfer_storage_error)?;
        if duplicate {
            return Err(TransferBindingStoreError::DuplicateBinding);
        }
        let binding_id = transaction
            .query_row(
                "INSERT INTO transfer_bindings (
                     id, source_slot_id, target_profile_id, target_instance_kind,
                     target_instance_id, target_shell_id, target_slot_id, target_token,
                     upload_mode, pull_mode, conflict_policy, interval_seconds, enabled,
                     last_common_etag, last_source_etag, last_target_etag,
                     pending_direction, next_run_at, last_synced_at, last_error,
                     revision, created_at, updated_at
                 ) VALUES (
                     lower(hex(randomblob(16))), ?1, ?2, ?3,
                     ?4, ?5, ?6, ?7,
                     ?8, ?9, ?10, ?11, ?12,
                     NULL, ?13, ?14,
                     'none', strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('+%d seconds', ?11)),
                     NULL, NULL, 0,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 ) RETURNING id",
                params![
                    input.source_slot_id,
                    input.target_profile_id,
                    input.target_instance_kind.as_str(),
                    input.target_instance_id,
                    input.target_shell_id,
                    input.target_slot_id,
                    input.target_token,
                    input.upload_mode.as_str(),
                    input.pull_mode.as_str(),
                    input.conflict_policy.as_str(),
                    input.interval_seconds,
                    input.enabled,
                    input.observed_source_etag,
                    input.observed_target_etag,
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(transfer_storage_error)?;
        let binding = find_transfer_binding(&transaction, &binding_id)?
            .ok_or_else(|| transfer_storage_message("created transfer binding is missing"))?;
        transaction.commit().map_err(transfer_storage_error)?;
        Ok(binding)
    }

    pub(crate) fn get_transfer_binding(
        &self,
        binding_id: &str,
    ) -> Result<Option<TransferBinding>, PersonalServiceError> {
        find_transfer_binding(&self.connection, binding_id).map_err(|error| match error {
            TransferBindingStoreError::Storage(error) => error,
            _ => PersonalServiceError::new("failed to read transfer binding"),
        })
    }

    pub(crate) fn list_transfer_bindings(
        &self,
        source_slot_id: i64,
    ) -> Result<Option<Vec<TransferBinding>>, PersonalServiceError> {
        let source_exists = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![source_slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(transfer_database_error)?;
        if !source_exists {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare(&format!(
                "{} WHERE bindings.source_slot_id = ?1 ORDER BY bindings.created_at, bindings.id",
                transfer_binding_select()
            ))
            .map_err(transfer_database_error)?;
        let bindings = statement
            .query_map(params![source_slot_id], read_transfer_binding)
            .map_err(transfer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(transfer_database_error)?;
        Ok(Some(bindings))
    }
    // //// /创建和读取明确的槽位传输绑定 ////

    // //// 更新, 删除和调度传输绑定 [@x380kkm 2026-08-03] ////
    pub(crate) fn update_transfer_binding(
        &mut self,
        source_slot_id: i64,
        binding_id: &str,
        input: &UpdateTransferBindingInput,
    ) -> Result<TransferBinding, TransferBindingStoreError> {
        let updated = self
            .connection
            .execute(
                "UPDATE transfer_bindings
                 SET upload_mode = ?1, pull_mode = ?2, conflict_policy = ?3,
                     interval_seconds = ?4, enabled = ?5,
                     target_token = COALESCE(?6, target_token),
                     next_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('+%d seconds', ?4)),
                     revision = revision + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?7 AND source_slot_id = ?8",
                params![
                    input.upload_mode.as_str(),
                    input.pull_mode.as_str(),
                    input.conflict_policy.as_str(),
                    input.interval_seconds,
                    input.enabled,
                    input.target_token,
                    binding_id,
                    source_slot_id,
                ],
            )
            .map_err(transfer_storage_error)?;
        if updated != 1 {
            return Err(TransferBindingStoreError::BindingNotFound);
        }
        find_transfer_binding(&self.connection, binding_id)?
            .ok_or_else(|| transfer_storage_message("updated transfer binding is missing"))
    }

    pub(crate) fn delete_transfer_binding(
        &mut self,
        source_slot_id: i64,
        binding_id: &str,
    ) -> Result<(), TransferBindingStoreError> {
        let deleted = self
            .connection
            .execute(
                "DELETE FROM transfer_bindings WHERE id = ?1 AND source_slot_id = ?2",
                params![binding_id, source_slot_id],
            )
            .map_err(transfer_storage_error)?;
        if deleted == 1 {
            Ok(())
        } else {
            Err(TransferBindingStoreError::BindingNotFound)
        }
    }

    pub(crate) fn list_due_transfer_binding_ids(
        &self,
    ) -> Result<Vec<String>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT bindings.id
                 FROM transfer_bindings AS bindings
                 WHERE bindings.enabled = 1
                   AND (bindings.upload_mode = 'interval' OR bindings.pull_mode = 'interval')
                   AND bindings.next_run_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                   AND NOT EXISTS(
                       SELECT 1 FROM transfer_conflicts AS conflicts
                       WHERE conflicts.binding_id = bindings.id AND conflicts.status = 'open'
                   )
                 ORDER BY bindings.next_run_at, bindings.id",
            )
            .map_err(transfer_database_error)?;
        let binding_ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(transfer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(transfer_database_error)?;
        Ok(binding_ids)
    }
}

pub(super) fn transfer_binding_select() -> &'static str {
    "SELECT bindings.id,
            (SELECT instance_id FROM local_transfer_token_state WHERE id = 1),
            CAST(slots.account_id AS TEXT),
            bindings.source_slot_id,
            bindings.target_profile_id,
            bindings.target_instance_kind,
            bindings.target_instance_id,
            bindings.target_shell_id,
            bindings.target_slot_id,
            bindings.target_token,
            profiles.scheme,
            profiles.host,
            profiles.port,
            bindings.upload_mode,
            bindings.pull_mode,
            bindings.conflict_policy,
            bindings.interval_seconds,
            bindings.enabled,
            bindings.last_common_etag,
            bindings.last_source_etag,
            bindings.last_target_etag,
            bindings.pending_direction,
            bindings.next_run_at,
            bindings.last_synced_at,
            bindings.last_error,
            bindings.revision,
            bindings.created_at,
            bindings.updated_at
     FROM transfer_bindings AS bindings
     JOIN local_save_slots AS slots ON slots.id = bindings.source_slot_id
     JOIN server_profiles AS profiles ON profiles.id = bindings.target_profile_id"
}

pub(super) fn find_transfer_binding(
    connection: &Connection,
    binding_id: &str,
) -> Result<Option<TransferBinding>, TransferBindingStoreError> {
    connection
        .query_row(
            &format!("{} WHERE bindings.id = ?1", transfer_binding_select()),
            params![binding_id],
            read_transfer_binding,
        )
        .optional()
        .map_err(transfer_storage_error)
}

fn read_transfer_binding(row: &Row<'_>) -> rusqlite::Result<TransferBinding> {
    let target_instance_kind = parse_database_value(row, 5, TransferInstanceKind::parse)?;
    let upload_mode = parse_database_value(row, 13, TransferUploadMode::parse)?;
    let pull_mode = parse_database_value(row, 14, TransferPullMode::parse)?;
    let conflict_policy = parse_database_value(row, 15, TransferConflictPolicy::parse)?;
    Ok(TransferBinding {
        id: row.get(0)?,
        source_instance_id: row.get(1)?,
        source_shell_id: row.get(2)?,
        source_slot_id: row.get(3)?,
        target_profile_id: row.get(4)?,
        target_instance_kind,
        target_instance_id: row.get(6)?,
        target_shell_id: row.get(7)?,
        target_slot_id: row.get(8)?,
        target_token: row.get(9)?,
        target_scheme: row.get(10)?,
        target_host: row.get(11)?,
        target_port: row.get(12)?,
        upload_mode,
        pull_mode,
        conflict_policy,
        interval_seconds: row.get(16)?,
        enabled: row.get(17)?,
        last_common_etag: row.get(18)?,
        last_source_etag: row.get(19)?,
        last_target_etag: row.get(20)?,
        pending_direction: row.get(21)?,
        next_run_at: row.get(22)?,
        last_synced_at: row.get(23)?,
        last_error: row.get(24)?,
        revision: row.get(25)?,
        created_at: row.get(26)?,
        updated_at: row.get(27)?,
    })
}

pub(super) fn parse_database_value<T>(
    row: &Row<'_>,
    index: usize,
    parse: impl FnOnce(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let value = row.get::<_, String>(index)?;
    parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            format!("invalid transfer binding value: {value}").into(),
        )
    })
}

pub(super) fn transfer_storage_error(error: rusqlite::Error) -> TransferBindingStoreError {
    TransferBindingStoreError::Storage(transfer_database_error(error))
}

pub(super) fn transfer_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "failed to access transfer binding storage: {error}"
    ))
}

pub(super) fn transfer_storage_message(message: &str) -> TransferBindingStoreError {
    TransferBindingStoreError::Storage(PersonalServiceError::new(message))
}
// //// /更新, 删除和调度传输绑定 ////
