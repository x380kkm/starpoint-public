// audience: internal
// # personal-service-local-save-keys
//
// 该模块在个人服务 SQLite 中保存一个本地存档加密密钥. 密钥字节不进入 HTTP 响应.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};

const KEY_BYTES: usize = 32;
const KEY_ID_BYTES: usize = 12;
const PLAYER_SCOPE_BYTES: usize = 12;

pub(crate) struct LocalSaveEncryptionKey {
    pub(crate) key_id: String,
    pub(crate) key_bytes: [u8; KEY_BYTES],
}

// //// 创建本地存档加密密钥表 [@x380kkm 2026-07-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS local_save_encryption_keys (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 key_id TEXT NOT NULL UNIQUE CHECK (length(key_id) BETWEEN 1 AND 64),
                 key_bytes BLOB NOT NULL CHECK (length(key_bytes) = 32),
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS player_remote_scopes (
                 account_id INTEGER PRIMARY KEY,
                 scope TEXT NOT NULL UNIQUE CHECK (length(scope) BETWEEN 1 AND 64),
                 created_at TEXT NOT NULL,
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );",
        )
        .map_err(key_storage_error)
}
// //// /创建本地存档加密密钥表 ////

impl ServiceDatabase {
    // //// 读取或创建本地存档加密密钥 [@x380kkm 2026-07-23] ////
    pub(crate) fn get_or_create_local_save_encryption_key(
        &mut self,
    ) -> Result<LocalSaveEncryptionKey, PersonalServiceError> {
        if let Some(key) = read_key(&self.connection)? {
            return Ok(key);
        }
        let mut key_bytes = [0_u8; KEY_BYTES];
        let mut key_id_bytes = [0_u8; KEY_ID_BYTES];
        getrandom::getrandom(&mut key_bytes).map_err(|error| {
            PersonalServiceError::new(format!("failed to generate local save key: {error}"))
        })?;
        getrandom::getrandom(&mut key_id_bytes).map_err(|error| {
            PersonalServiceError::new(format!("failed to generate local save key id: {error}"))
        })?;
        let key_id = URL_SAFE_NO_PAD.encode(key_id_bytes);
        self.connection
            .execute(
                "INSERT OR IGNORE INTO local_save_encryption_keys (
                     id, key_id, key_bytes, created_at
                 ) VALUES (
                     1, ?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )",
                params![key_id, key_bytes.as_slice()],
            )
            .map_err(key_storage_error)?;
        read_key(&self.connection)?
            .ok_or_else(|| PersonalServiceError::new("failed to persist local save encryption key"))
    }
    // //// /读取或创建本地存档加密密钥 ////

    // //// 读取或创建稳定的玩家远端作用域 [@x380kkm 2026-07-24] ////
    pub(crate) fn get_or_create_player_remote_scope(
        &mut self,
        account_id: i64,
    ) -> Result<String, PersonalServiceError> {
        if let Some(scope) = read_player_scope(&self.connection, account_id)? {
            return Ok(scope);
        }
        let mut scope_bytes = [0_u8; PLAYER_SCOPE_BYTES];
        getrandom::getrandom(&mut scope_bytes).map_err(|error| {
            PersonalServiceError::new(format!("failed to generate player remote scope: {error}"))
        })?;
        let scope = URL_SAFE_NO_PAD.encode(scope_bytes);
        self.connection
            .execute(
                "INSERT OR IGNORE INTO player_remote_scopes (account_id, scope, created_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                params![account_id, scope],
            )
            .map_err(key_storage_error)?;
        read_player_scope(&self.connection, account_id)?
            .ok_or_else(|| PersonalServiceError::new("failed to persist player remote scope"))
    }
    // //// /读取或创建稳定的玩家远端作用域 ////

    // //// 导入跨设备恢复包中的本地密钥和远端作用域 [@x380kkm 2026-07-24] ////
    pub(crate) fn import_player_recovery_material(
        &mut self,
        account_id: i64,
        key_id: &str,
        key_bytes: [u8; KEY_BYTES],
        remote_scope: &str,
    ) -> Result<(), PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(key_storage_error)?;
        let existing_key = read_key(&transaction)?;
        if let Some(existing_key) = existing_key {
            if existing_key.key_id != key_id || existing_key.key_bytes != key_bytes {
                return Err(PersonalServiceError::new(
                    "local save encryption key conflict",
                ));
            }
        } else {
            transaction
                .execute(
                    "INSERT INTO local_save_encryption_keys (
                         id, key_id, key_bytes, created_at
                     ) VALUES (1, ?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                    params![key_id, key_bytes.as_slice()],
                )
                .map_err(key_storage_error)?;
        }
        if let Some(existing_scope) = read_player_scope(&transaction, account_id)? {
            if existing_scope != remote_scope {
                return Err(PersonalServiceError::new("player remote scope conflict"));
            }
            transaction.commit().map_err(key_storage_error)?;
            return Ok(());
        }
        transaction
            .execute(
                "INSERT INTO player_remote_scopes (account_id, scope, created_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                params![account_id, remote_scope],
            )
            .map_err(key_storage_error)?;
        transaction.commit().map_err(key_storage_error)?;
        Ok(())
    }
    // //// /导入跨设备恢复包中的本地密钥和远端作用域 ////
}

fn read_key(
    connection: &Connection,
) -> Result<Option<LocalSaveEncryptionKey>, PersonalServiceError> {
    let stored = connection
        .query_row(
            "SELECT key_id, key_bytes FROM local_save_encryption_keys WHERE id = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(key_storage_error)?;
    let Some((key_id, key_bytes)) = stored else {
        return Ok(None);
    };
    let key_bytes = key_bytes.try_into().map_err(|_| {
        PersonalServiceError::new("stored local save encryption key has an invalid length")
    })?;
    Ok(Some(LocalSaveEncryptionKey { key_id, key_bytes }))
}

fn key_storage_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!(
        "failed to access local save encryption key: {error}"
    ))
}

fn read_player_scope(
    connection: &Connection,
    account_id: i64,
) -> Result<Option<String>, PersonalServiceError> {
    connection
        .query_row(
            "SELECT scope FROM player_remote_scopes WHERE account_id = ?1",
            params![account_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(key_storage_error)
}
