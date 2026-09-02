// audience: internal
// # personal-service-save-sync
//
// 该模块保存可更换的密文存档服务器配置和本地槽位的远端 ETag.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Row};

mod bindings;

pub(crate) struct SaveSyncTarget {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) scheme: String,
    pub(crate) host: String,
    pub(crate) port: i64,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

pub(crate) struct SaveSyncTargetInput {
    pub(crate) name: String,
    pub(crate) scheme: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) password: String,
}

pub(crate) struct SaveSyncBinding {
    pub(crate) target_id: i64,
    pub(crate) object_id: String,
    pub(crate) remote_etag: String,
    pub(crate) last_synced_at: String,
}

pub(crate) enum SaveSyncStoreError {
    NotFound,
    NameConflict,
    Storage(PersonalServiceError),
}

// //// 创建密文存档服务器和绑定表 [@x380kkm 2026-07-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS save_sync_targets (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL COLLATE NOCASE UNIQUE
                     CHECK (length(trim(name)) BETWEEN 1 AND 64),
                 scheme TEXT NOT NULL CHECK (scheme IN ('http', 'https')),
                 host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
                 port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
                 username TEXT NOT NULL CHECK (length(username) BETWEEN 1 AND 128),
                 password TEXT NOT NULL CHECK (length(password) BETWEEN 1 AND 256),
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS local_save_sync_bindings (
                 target_id INTEGER NOT NULL,
                 slot_id INTEGER NOT NULL,
                 object_id TEXT NOT NULL CHECK (length(object_id) BETWEEN 1 AND 64),
                 remote_etag TEXT NOT NULL CHECK (length(remote_etag) = 64),
                 last_synced_at TEXT NOT NULL,
                 PRIMARY KEY (target_id, slot_id),
                 UNIQUE (target_id, object_id),
                 FOREIGN KEY (target_id) REFERENCES save_sync_targets (id) ON DELETE CASCADE,
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE
             );",
        )
        .map_err(sync_database_error)
}
// //// /创建密文存档服务器和绑定表 ////

impl ServiceDatabase {
    // //// 创建和列出密文存档服务器 [@x380kkm 2026-07-23] ////
    pub(crate) fn create_save_sync_target(
        &mut self,
        input: &SaveSyncTargetInput,
    ) -> Result<SaveSyncTarget, SaveSyncStoreError> {
        if target_name_exists(&self.connection, &input.name, None)? {
            return Err(SaveSyncStoreError::NameConflict);
        }
        self.connection
            .execute(
                "INSERT INTO save_sync_targets (
                     name, scheme, host, port, username, password, created_at, updated_at
                 ) VALUES (
                     ?1, ?2, ?3, ?4, ?5, ?6,
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )",
                params![
                    input.name,
                    input.scheme,
                    input.host,
                    i64::from(input.port),
                    input.username,
                    input.password,
                ],
            )
            .map_err(sync_storage_error)?;
        find_target(&self.connection, self.connection.last_insert_rowid())?
            .ok_or_else(|| sync_storage_message("created save sync target is missing"))
    }

    pub(crate) fn list_save_sync_targets(
        &self,
    ) -> Result<Vec<SaveSyncTarget>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, name, scheme, host, port, username, password, created_at, updated_at
                 FROM save_sync_targets ORDER BY name COLLATE NOCASE, id",
            )
            .map_err(sync_database_error)?;
        let targets = statement
            .query_map([], read_target)
            .map_err(sync_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sync_database_error)?;
        Ok(targets)
    }
    // //// /创建和列出密文存档服务器 ////

    // //// 更新和删除密文存档服务器 [@x380kkm 2026-07-23] ////
    pub(crate) fn update_save_sync_target(
        &mut self,
        target_id: i64,
        input: &SaveSyncTargetInput,
    ) -> Result<SaveSyncTarget, SaveSyncStoreError> {
        let existing =
            find_target(&self.connection, target_id)?.ok_or(SaveSyncStoreError::NotFound)?;
        if target_name_exists(&self.connection, &input.name, Some(target_id))? {
            return Err(SaveSyncStoreError::NameConflict);
        }
        let identity_changed = existing.scheme != input.scheme
            || existing.host != input.host
            || existing.port != i64::from(input.port)
            || existing.username != input.username;
        let transaction = self.connection.transaction().map_err(sync_storage_error)?;
        transaction
            .execute(
                "UPDATE save_sync_targets
                 SET name = ?1, scheme = ?2, host = ?3, port = ?4,
                     username = ?5, password = ?6,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?7",
                params![
                    input.name,
                    input.scheme,
                    input.host,
                    i64::from(input.port),
                    input.username,
                    input.password,
                    target_id,
                ],
            )
            .map_err(sync_storage_error)?;
        if identity_changed {
            transaction
                .execute(
                    "DELETE FROM local_save_sync_bindings WHERE target_id = ?1",
                    params![target_id],
                )
                .map_err(sync_storage_error)?;
        }
        transaction.commit().map_err(sync_storage_error)?;
        find_target(&self.connection, target_id)?
            .ok_or_else(|| sync_storage_message("updated save sync target is missing"))
    }

    pub(crate) fn delete_save_sync_target(
        &mut self,
        target_id: i64,
    ) -> Result<(), SaveSyncStoreError> {
        let deleted = self
            .connection
            .execute(
                "DELETE FROM save_sync_targets WHERE id = ?1",
                params![target_id],
            )
            .map_err(sync_storage_error)?;
        if deleted == 1 {
            Ok(())
        } else {
            Err(SaveSyncStoreError::NotFound)
        }
    }
    // //// /更新和删除密文存档服务器 ////

    pub(crate) fn find_save_sync_target(
        &self,
        target_id: i64,
    ) -> Result<Option<SaveSyncTarget>, PersonalServiceError> {
        find_target(&self.connection, target_id).map_err(|error| match error {
            SaveSyncStoreError::Storage(error) => error,
            _ => PersonalServiceError::new("failed to read save sync target"),
        })
    }
}

fn find_target(
    connection: &Connection,
    target_id: i64,
) -> Result<Option<SaveSyncTarget>, SaveSyncStoreError> {
    connection
        .query_row(
            "SELECT id, name, scheme, host, port, username, password, created_at, updated_at
             FROM save_sync_targets WHERE id = ?1",
            params![target_id],
            read_target,
        )
        .optional()
        .map_err(sync_storage_error)
}

fn read_target(row: &Row<'_>) -> rusqlite::Result<SaveSyncTarget> {
    Ok(SaveSyncTarget {
        id: row.get(0)?,
        name: row.get(1)?,
        scheme: row.get(2)?,
        host: row.get(3)?,
        port: row.get(4)?,
        username: row.get(5)?,
        password: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn target_name_exists(
    connection: &Connection,
    name: &str,
    excluded_id: Option<i64>,
) -> Result<bool, SaveSyncStoreError> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM save_sync_targets
                 WHERE name = ?1 COLLATE NOCASE AND (?2 IS NULL OR id != ?2)
             )",
            params![name, excluded_id],
            |row| row.get(0),
        )
        .map_err(sync_storage_error)
}

fn sync_storage_error(error: rusqlite::Error) -> SaveSyncStoreError {
    SaveSyncStoreError::Storage(sync_database_error(error))
}

fn sync_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to access save sync storage: {error}"))
}

fn sync_storage_message(message: &str) -> SaveSyncStoreError {
    SaveSyncStoreError::Storage(PersonalServiceError::new(message))
}
