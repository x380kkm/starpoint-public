// audience: internal
// # personal-service-transfer-conflicts
//
// 该模块提交传输共同基线并保存双分支冲突.
// 本地下载覆盖, 冲突解决与绑定状态在同一 SQLite 事务提交.

use super::{
    find_transfer_binding, parse_database_value, transfer_database_error, transfer_storage_error,
    transfer_storage_message, TransferBinding, TransferBindingEtagUpdate,
    TransferBindingStoreError, TransferConflict, TransferConflictStatus,
};
use crate::database::local_saves::{replace_local_save_data_in_transaction, LocalSaveStoreError};
use crate::database::ServiceDatabase;
use crate::portable_save;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};

impl ServiceDatabase {
    // //// 提交传输结果或记录传输失败 [@x380kkm 2026-08-03] ////
    pub(crate) fn record_transfer_binding_success(
        &mut self,
        binding_id: &str,
        expected_revision: i64,
        common_etag: &str,
        source_etag: &str,
        target_etag: &str,
    ) -> Result<TransferBinding, TransferBindingStoreError> {
        update_transfer_binding_success(
            &self.connection,
            binding_id,
            expected_revision,
            common_etag,
            source_etag,
            target_etag,
        )
    }

    pub(crate) fn commit_transfer_binding_download(
        &mut self,
        binding_id: &str,
        expected_revision: i64,
        source_slot_id: i64,
        data_json: &str,
        target_etag: &str,
    ) -> Result<TransferBinding, TransferBindingStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(transfer_storage_error)?;
        validate_enabled_transfer_binding_download(
            &transaction,
            binding_id,
            expected_revision,
            source_slot_id,
        )?;
        replace_local_save_data_in_transaction(&transaction, source_slot_id, data_json)
            .map_err(map_local_save_error)?;
        let binding = update_transfer_binding_success(
            &transaction,
            binding_id,
            expected_revision,
            target_etag,
            target_etag,
            target_etag,
        )?;
        transaction.commit().map_err(transfer_storage_error)?;
        Ok(binding)
    }

    pub(crate) fn record_transfer_binding_failure(
        &mut self,
        binding_id: &str,
        expected_revision: i64,
        source_etag: &str,
        target_etag: Option<&str>,
        error_code: &str,
        retry_seconds: i64,
    ) -> Result<(), TransferBindingStoreError> {
        let updated = self
            .connection
            .execute(
                "UPDATE transfer_bindings
                 SET last_source_etag = ?1,
                     last_target_etag = COALESCE(?2, last_target_etag),
                     pending_direction = 'none',
                     last_error = ?3,
                     next_run_at = strftime(
                         '%Y-%m-%dT%H:%M:%fZ', 'now', printf('+%d seconds', ?4)
                     ),
                     revision = revision + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?5 AND revision = ?6",
                params![
                    source_etag,
                    target_etag,
                    error_code,
                    retry_seconds,
                    binding_id,
                    expected_revision,
                ],
            )
            .map_err(transfer_storage_error)?;
        if updated == 1 {
            Ok(())
        } else {
            Err(TransferBindingStoreError::StaleBinding)
        }
    }
    // //// /提交传输结果或记录传输失败 ////

    // //// 保存并列出双分支冲突 [@x380kkm 2026-08-03] ////
    pub(crate) fn record_transfer_conflict(
        &mut self,
        binding_id: &str,
        expected_revision: i64,
        source_revision_id: &str,
        source_etag: &str,
        target_revision_id: Option<&str>,
        target_etag: &str,
    ) -> Result<TransferConflict, TransferBindingStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(transfer_storage_error)?;
        if let Some(conflict) = find_open_transfer_conflict(&transaction, binding_id)? {
            return Ok(conflict);
        }
        let updated = transaction
            .execute(
                "UPDATE transfer_bindings
                 SET last_source_etag = ?1,
                     last_target_etag = ?2,
                     pending_direction = 'conflict',
                     last_error = 'transfer_conflict',
                     revision = revision + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?3 AND revision = ?4",
                params![source_etag, target_etag, binding_id, expected_revision],
            )
            .map_err(transfer_storage_error)?;
        if updated != 1 {
            return Err(TransferBindingStoreError::StaleBinding);
        }
        let conflict_id = transaction
            .query_row(
                "INSERT INTO transfer_conflicts (
                     id, binding_id, source_revision_id, source_etag,
                     target_revision_id, target_etag, detected_at, status, resolved_at
                 ) VALUES (
                     lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'open', NULL
                 ) RETURNING id",
                params![
                    binding_id,
                    source_revision_id,
                    source_etag,
                    target_revision_id,
                    target_etag,
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(transfer_storage_error)?;
        let conflict = find_transfer_conflict(&transaction, binding_id, &conflict_id)?
            .ok_or_else(|| transfer_storage_message("created transfer conflict is missing"))?;
        transaction.commit().map_err(transfer_storage_error)?;
        Ok(conflict)
    }

    pub(crate) fn list_transfer_conflicts(
        &self,
        binding_id: &str,
    ) -> Result<Option<Vec<TransferConflict>>, PersonalServiceError> {
        if find_transfer_binding(&self.connection, binding_id)
            .map_err(|error| match error {
                TransferBindingStoreError::Storage(error) => error,
                _ => PersonalServiceError::new("failed to read transfer binding"),
            })?
            .is_none()
        {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, binding_id, source_revision_id, source_etag,
                        target_revision_id, target_etag, detected_at, status, resolved_at
                 FROM transfer_conflicts
                 WHERE binding_id = ?1
                 ORDER BY detected_at DESC, id DESC",
            )
            .map_err(transfer_database_error)?;
        let conflicts = statement
            .query_map(params![binding_id], read_transfer_conflict)
            .map_err(transfer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(transfer_database_error)?;
        Ok(Some(conflicts))
    }

    pub(crate) fn get_open_transfer_conflict(
        &self,
        binding_id: &str,
    ) -> Result<Option<TransferConflict>, PersonalServiceError> {
        find_open_transfer_conflict(&self.connection, binding_id).map_err(|error| match error {
            TransferBindingStoreError::Storage(error) => error,
            _ => PersonalServiceError::new("failed to read transfer conflict"),
        })
    }
    // //// /保存并列出双分支冲突 ////

    // //// 原子解决传输冲突并更新绑定状态 [@x380kkm 2026-08-03] ////
    pub(crate) fn resolve_transfer_conflict(
        &mut self,
        binding_id: &str,
        conflict_id: &str,
        expected_revision: i64,
        status: TransferConflictStatus,
        etags: TransferBindingEtagUpdate<'_>,
    ) -> Result<TransferConflict, TransferBindingStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(transfer_storage_error)?;
        let resolved = resolve_transfer_conflict_in_transaction(
            &transaction,
            binding_id,
            conflict_id,
            expected_revision,
            status,
            etags,
        )?;
        transaction.commit().map_err(transfer_storage_error)?;
        Ok(resolved)
    }

    pub(crate) fn resolve_transfer_conflict_with_download(
        &mut self,
        binding_id: &str,
        conflict_id: &str,
        expected_revision: i64,
        source_slot_id: i64,
        expected_source_etag: &str,
        data_json: &str,
        target_etag: &str,
    ) -> Result<TransferConflict, TransferBindingStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(transfer_storage_error)?;
        validate_transfer_conflict_download(
            &transaction,
            binding_id,
            expected_revision,
            source_slot_id,
            expected_source_etag,
        )?;
        let resolved = resolve_transfer_conflict_in_transaction(
            &transaction,
            binding_id,
            conflict_id,
            expected_revision,
            TransferConflictStatus::ResolvedRemoteWins,
            TransferBindingEtagUpdate::synchronized(target_etag),
        )?;
        replace_local_save_data_in_transaction(&transaction, source_slot_id, data_json)
            .map_err(map_local_save_error)?;
        transaction.commit().map_err(transfer_storage_error)?;
        Ok(resolved)
    }
}

fn update_transfer_binding_success(
    connection: &Connection,
    binding_id: &str,
    expected_revision: i64,
    common_etag: &str,
    source_etag: &str,
    target_etag: &str,
) -> Result<TransferBinding, TransferBindingStoreError> {
    let updated = connection
        .execute(
            "UPDATE transfer_bindings
             SET last_common_etag = ?1,
                 last_source_etag = ?2,
                 last_target_etag = ?3,
                 pending_direction = 'none',
                 last_synced_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 last_error = NULL,
                 next_run_at = strftime(
                     '%Y-%m-%dT%H:%M:%fZ', 'now', printf('+%d seconds', interval_seconds)
                 ),
                 revision = revision + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?4 AND revision = ?5",
            params![
                common_etag,
                source_etag,
                target_etag,
                binding_id,
                expected_revision
            ],
        )
        .map_err(transfer_storage_error)?;
    if updated != 1 {
        return Err(TransferBindingStoreError::StaleBinding);
    }
    find_transfer_binding(connection, binding_id)?
        .ok_or_else(|| transfer_storage_message("synchronized transfer binding is missing"))
}

fn validate_enabled_transfer_binding_download(
    transaction: &Transaction<'_>,
    binding_id: &str,
    expected_revision: i64,
    source_slot_id: i64,
) -> Result<(), TransferBindingStoreError> {
    let state = read_transfer_binding_replace_state(transaction, binding_id)?;
    if state.source_slot_id != source_slot_id
        || state.revision != expected_revision
        || !state.enabled
    {
        return Err(TransferBindingStoreError::StaleBinding);
    }
    if find_open_transfer_conflict(transaction, binding_id)?.is_some() {
        return Err(TransferBindingStoreError::StaleBinding);
    }
    ensure_local_save_is_idle(transaction, source_slot_id)
}

fn validate_transfer_conflict_download(
    transaction: &Transaction<'_>,
    binding_id: &str,
    expected_revision: i64,
    source_slot_id: i64,
    expected_source_etag: &str,
) -> Result<(), TransferBindingStoreError> {
    let state = read_transfer_binding_replace_state(transaction, binding_id)?;
    if state.source_slot_id != source_slot_id || state.revision != expected_revision {
        return Err(TransferBindingStoreError::StaleBinding);
    }
    let current_source_etag = calculate_local_save_etag(transaction, source_slot_id)?;
    if current_source_etag != expected_source_etag {
        return Err(TransferBindingStoreError::ConflictChanged);
    }
    ensure_local_save_is_idle(transaction, source_slot_id)
}

fn calculate_local_save_etag(
    connection: &Connection,
    source_slot_id: i64,
) -> Result<String, TransferBindingStoreError> {
    let data_json = connection
        .query_row(
            "SELECT player_snapshots.data_json
             FROM local_save_slots
             JOIN player_snapshots
               ON player_snapshots.account_id = local_save_slots.account_id
             WHERE local_save_slots.id = ?1",
            params![source_slot_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(transfer_storage_error)?
        .ok_or(TransferBindingStoreError::SourceSlotNotFound)?;
    let data = serde_json::from_str(&data_json).map_err(|error| {
        TransferBindingStoreError::Storage(PersonalServiceError::new(format!(
            "failed to decode local save during conflict resolution: {error}"
        )))
    })?;
    portable_save::calculate_payload_sha256(&data).map_err(TransferBindingStoreError::Storage)
}

struct TransferBindingReplaceState {
    source_slot_id: i64,
    revision: i64,
    enabled: bool,
}

fn read_transfer_binding_replace_state(
    connection: &Connection,
    binding_id: &str,
) -> Result<TransferBindingReplaceState, TransferBindingStoreError> {
    connection
        .query_row(
            "SELECT source_slot_id, revision, enabled
             FROM transfer_bindings WHERE id = ?1",
            params![binding_id],
            |row| {
                Ok(TransferBindingReplaceState {
                    source_slot_id: row.get(0)?,
                    revision: row.get(1)?,
                    enabled: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(transfer_storage_error)?
        .ok_or(TransferBindingStoreError::BindingNotFound)
}

fn ensure_local_save_is_idle(
    connection: &Connection,
    source_slot_id: i64,
) -> Result<(), TransferBindingStoreError> {
    let is_busy = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM active_single_quests
                 JOIN local_save_slots
                   ON local_save_slots.account_id = active_single_quests.account_id
                 WHERE local_save_slots.id = ?1
             )",
            params![source_slot_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(transfer_storage_error)?;
    if is_busy {
        Err(TransferBindingStoreError::LocalSaveBusy)
    } else {
        Ok(())
    }
}

fn map_local_save_error(error: LocalSaveStoreError) -> TransferBindingStoreError {
    match error {
        LocalSaveStoreError::NotFound => TransferBindingStoreError::SourceSlotNotFound,
        LocalSaveStoreError::Busy => TransferBindingStoreError::LocalSaveBusy,
        LocalSaveStoreError::InvalidState => {
            transfer_storage_message("local save replacement state is invalid")
        }
        LocalSaveStoreError::Storage(error) => TransferBindingStoreError::Storage(error),
    }
}

fn resolve_transfer_conflict_in_transaction(
    transaction: &Transaction<'_>,
    binding_id: &str,
    conflict_id: &str,
    expected_revision: i64,
    status: TransferConflictStatus,
    etags: TransferBindingEtagUpdate<'_>,
) -> Result<TransferConflict, TransferBindingStoreError> {
    if status == TransferConflictStatus::Open {
        return Err(TransferBindingStoreError::ConflictAlreadyResolved);
    }
    let conflict = find_transfer_conflict(transaction, binding_id, conflict_id)?
        .ok_or(TransferBindingStoreError::ConflictNotFound)?;
    if conflict.status != TransferConflictStatus::Open {
        return Err(TransferBindingStoreError::ConflictAlreadyResolved);
    }
    let updated = transaction
        .execute(
            "UPDATE transfer_conflicts
             SET status = ?1, resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?2 AND binding_id = ?3 AND status = 'open'",
            params![status.as_str(), conflict_id, binding_id],
        )
        .map_err(transfer_storage_error)?;
    if updated != 1 {
        return Err(TransferBindingStoreError::ConflictAlreadyResolved);
    }
    let disable_binding = status == TransferConflictStatus::ResolvedKeepBoth;
    let updated = transaction
        .execute(
            "UPDATE transfer_bindings
             SET last_common_etag = COALESCE(?1, last_common_etag),
                 last_source_etag = COALESCE(?2, last_source_etag),
                 last_target_etag = COALESCE(?3, last_target_etag),
                 pending_direction = 'none',
                 last_synced_at = CASE WHEN ?1 IS NULL THEN last_synced_at
                                       ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now') END,
                 last_error = NULL,
                 enabled = CASE WHEN ?4 THEN 0 ELSE enabled END,
                 next_run_at = strftime(
                     '%Y-%m-%dT%H:%M:%fZ', 'now', printf('+%d seconds', interval_seconds)
                 ),
                 revision = revision + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?5 AND revision = ?6",
            params![
                etags.common_etag,
                etags.source_etag,
                etags.target_etag,
                disable_binding,
                binding_id,
                expected_revision
            ],
        )
        .map_err(transfer_storage_error)?;
    if updated != 1 {
        return Err(TransferBindingStoreError::StaleBinding);
    }
    find_transfer_conflict(transaction, binding_id, conflict_id)?
        .ok_or_else(|| transfer_storage_message("resolved transfer conflict is missing"))
}

fn find_open_transfer_conflict(
    connection: &Connection,
    binding_id: &str,
) -> Result<Option<TransferConflict>, TransferBindingStoreError> {
    connection
        .query_row(
            "SELECT id, binding_id, source_revision_id, source_etag,
                    target_revision_id, target_etag, detected_at, status, resolved_at
             FROM transfer_conflicts
             WHERE binding_id = ?1 AND status = 'open'",
            params![binding_id],
            read_transfer_conflict,
        )
        .optional()
        .map_err(transfer_storage_error)
}

fn find_transfer_conflict(
    connection: &Connection,
    binding_id: &str,
    conflict_id: &str,
) -> Result<Option<TransferConflict>, TransferBindingStoreError> {
    connection
        .query_row(
            "SELECT id, binding_id, source_revision_id, source_etag,
                    target_revision_id, target_etag, detected_at, status, resolved_at
             FROM transfer_conflicts
             WHERE binding_id = ?1 AND id = ?2",
            params![binding_id, conflict_id],
            read_transfer_conflict,
        )
        .optional()
        .map_err(transfer_storage_error)
}

fn read_transfer_conflict(row: &Row<'_>) -> rusqlite::Result<TransferConflict> {
    let status = parse_database_value(row, 7, TransferConflictStatus::parse)?;
    Ok(TransferConflict {
        id: row.get(0)?,
        binding_id: row.get(1)?,
        source_revision_id: row.get(2)?,
        source_etag: row.get(3)?,
        target_revision_id: row.get(4)?,
        target_etag: row.get(5)?,
        detected_at: row.get(6)?,
        status,
        resolved_at: row.get(8)?,
    })
}
// //// /原子解决传输冲突并更新绑定状态 ////

#[cfg(test)]
mod tests;
