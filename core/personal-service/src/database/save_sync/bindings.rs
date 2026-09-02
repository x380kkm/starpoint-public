// audience: internal
// # personal-service-save-sync-bindings
//
// 该模块保存本地存档槽和远端密文对象的唯一绑定. 下载导入和绑定在同一事务提交.

use super::{sync_database_error, SaveSyncBinding};
use crate::database::local_saves::{create_local_save_slot, find_local_save_slot};
use crate::database::{LocalSaveSlot, LocalSaveStoreError, ServiceDatabase};
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

impl ServiceDatabase {
    // //// 读取和更新本地槽位的远端绑定 [@x380kkm 2026-07-23] ////
    pub(crate) fn get_local_save_sync_binding(
        &self,
        target_id: i64,
        slot_id: i64,
    ) -> Result<Option<SaveSyncBinding>, PersonalServiceError> {
        find_sync_binding(&self.connection, target_id, slot_id)
    }

    pub(crate) fn list_local_save_sync_bindings(
        &self,
        slot_id: i64,
    ) -> Result<Option<Vec<SaveSyncBinding>>, PersonalServiceError> {
        let slot_exists = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(sync_database_error)?;
        if !slot_exists {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT target_id, object_id, remote_etag, last_synced_at
                 FROM local_save_sync_bindings
                 WHERE slot_id = ?1 ORDER BY target_id",
            )
            .map_err(sync_database_error)?;
        let bindings = statement
            .query_map(params![slot_id], |row| {
                Ok(SaveSyncBinding {
                    target_id: row.get(0)?,
                    object_id: row.get(1)?,
                    remote_etag: row.get(2)?,
                    last_synced_at: row.get(3)?,
                })
            })
            .map_err(sync_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sync_database_error)?;
        Ok(Some(bindings))
    }

    pub(crate) fn list_local_save_sync_bindings_for_account(
        &self,
        account_id: i64,
        slot_id: i64,
    ) -> Result<Option<Vec<SaveSyncBinding>>, PersonalServiceError> {
        let slot_accessible = self
            .connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM local_save_access
                     WHERE account_id = ?1 AND slot_id = ?2
                 )",
                params![account_id, slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(sync_database_error)?;
        if !slot_accessible {
            return Ok(None);
        }
        self.list_local_save_sync_bindings(slot_id)
    }

    pub(crate) fn save_local_save_sync_binding(
        &mut self,
        target_id: i64,
        slot_id: i64,
        object_id: &str,
        remote_etag: &str,
    ) -> Result<SaveSyncBinding, PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(sync_database_error)?;
        save_binding_in_transaction(&transaction, target_id, slot_id, object_id, remote_etag)?;
        let binding = find_sync_binding(&transaction, target_id, slot_id)?
            .ok_or_else(|| PersonalServiceError::new("saved local save sync binding is missing"))?;
        transaction.commit().map_err(sync_database_error)?;
        Ok(binding)
    }

    pub(crate) fn save_local_save_sync_binding_for_account(
        &mut self,
        account_id: i64,
        target_id: i64,
        slot_id: i64,
        object_id: &str,
        remote_etag: &str,
    ) -> Result<Option<SaveSyncBinding>, PersonalServiceError> {
        let slot_accessible = self
            .connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM local_save_access
                     WHERE account_id = ?1 AND slot_id = ?2
                 )",
                params![account_id, slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(sync_database_error)?;
        if !slot_accessible {
            return Ok(None);
        }
        self.save_local_save_sync_binding(target_id, slot_id, object_id, remote_etag)
            .map(Some)
    }

    pub(crate) fn import_synced_local_save(
        &mut self,
        name: &str,
        data_json: &str,
        target_id: i64,
        object_id: &str,
        remote_etag: &str,
    ) -> Result<(LocalSaveSlot, SaveSyncBinding), LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_sync_database_error)?;
        let slot_id = create_local_save_slot(&transaction, name, data_json)?;
        save_binding_in_transaction(&transaction, target_id, slot_id, object_id, remote_etag)
            .map_err(LocalSaveStoreError::Storage)?;
        let slot = find_local_save_slot(&transaction, slot_id)?
            .ok_or(LocalSaveStoreError::InvalidState)?;
        let binding = find_sync_binding(&transaction, target_id, slot_id)
            .map_err(LocalSaveStoreError::Storage)?
            .ok_or(LocalSaveStoreError::InvalidState)?;
        transaction
            .commit()
            .map_err(local_save_sync_database_error)?;
        Ok((slot, binding))
    }

    pub(crate) fn import_synced_local_save_for_account(
        &mut self,
        account_id: i64,
        name: &str,
        data_json: &str,
        target_id: i64,
        object_id: &str,
        remote_etag: &str,
    ) -> Result<(LocalSaveSlot, SaveSyncBinding), LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_sync_database_error)?;
        let slot_id = create_local_save_slot(&transaction, name, data_json)?;
        let granted = transaction
            .execute(
                "INSERT INTO local_save_access (account_id, slot_id)
                 VALUES (?1, ?2)",
                params![account_id, slot_id],
            )
            .map_err(|error| LocalSaveStoreError::Storage(sync_database_error(error)))?;
        if granted != 1 {
            return Err(LocalSaveStoreError::InvalidState);
        }
        save_binding_in_transaction(&transaction, target_id, slot_id, object_id, remote_etag)
            .map_err(LocalSaveStoreError::Storage)?;
        let slot = find_local_save_slot(&transaction, slot_id)?
            .ok_or(LocalSaveStoreError::InvalidState)?;
        let binding = find_sync_binding(&transaction, target_id, slot_id)
            .map_err(LocalSaveStoreError::Storage)?
            .ok_or(LocalSaveStoreError::InvalidState)?;
        transaction
            .commit()
            .map_err(local_save_sync_database_error)?;
        Ok((slot, binding))
    }
}

fn save_binding_in_transaction(
    transaction: &Transaction<'_>,
    target_id: i64,
    slot_id: i64,
    object_id: &str,
    remote_etag: &str,
) -> Result<(), PersonalServiceError> {
    transaction
        .execute(
            "DELETE FROM local_save_sync_bindings
             WHERE target_id = ?1 AND object_id = ?2 AND slot_id != ?3",
            params![target_id, object_id, slot_id],
        )
        .map_err(sync_database_error)?;
    transaction
        .execute(
            "INSERT INTO local_save_sync_bindings (
                 target_id, slot_id, object_id, remote_etag, last_synced_at
             ) VALUES (
                 ?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )
             ON CONFLICT(target_id, slot_id) DO UPDATE SET
                 object_id = excluded.object_id,
                 remote_etag = excluded.remote_etag,
                 last_synced_at = excluded.last_synced_at",
            params![target_id, slot_id, object_id, remote_etag],
        )
        .map_err(sync_database_error)?;
    Ok(())
}

fn find_sync_binding(
    connection: &Connection,
    target_id: i64,
    slot_id: i64,
) -> Result<Option<SaveSyncBinding>, PersonalServiceError> {
    connection
        .query_row(
            "SELECT target_id, object_id, remote_etag, last_synced_at
             FROM local_save_sync_bindings
             WHERE target_id = ?1 AND slot_id = ?2",
            params![target_id, slot_id],
            |row| {
                Ok(SaveSyncBinding {
                    target_id: row.get(0)?,
                    object_id: row.get(1)?,
                    remote_etag: row.get(2)?,
                    last_synced_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(sync_database_error)
}

fn local_save_sync_database_error(error: rusqlite::Error) -> LocalSaveStoreError {
    LocalSaveStoreError::Storage(sync_database_error(error))
}
// //// /读取和更新本地槽位的远端绑定 ////
