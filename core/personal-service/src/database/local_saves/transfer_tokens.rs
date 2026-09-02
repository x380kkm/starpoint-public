// audience: internal
// # local-save-transfer-tokens
// 此模块为本地账号壳和本地存档槽保存 transfer token 摘要.
// token 明文只在签发结果中出现.
// 槽 token 的查询只匹配一个槽位和一个权限方向.

use super::{
    local_save_database_error, local_save_storage_error, LocalIssuedTransferToken,
    LocalSaveStoreError, LocalTransferPermission, LocalTransferTokenMetadata,
};
use crate::database::{parse_iso_timestamp, ServiceDatabase};
use crate::PersonalServiceError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use sha2::{Digest, Sha256};

const TRANSFER_TOKEN_BYTES: usize = 32;

impl LocalTransferPermission {
    pub(crate) fn allows(self, requested: Self) -> bool {
        self == Self::Both || self == requested
    }

    pub(crate) fn as_database_value(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Both => "both",
        }
    }

    pub(crate) fn from_database_value(value: &str) -> Option<Self> {
        match value {
            "upload" => Some(Self::Upload),
            "download" => Some(Self::Download),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

// //// 生成和校验 transfer token 输入 [@x380kkm 2026-07-27] ////
fn generate_transfer_token(kind: &str) -> Result<String, PersonalServiceError> {
    let mut bytes = [0_u8; TRANSFER_TOKEN_BYTES];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to generate transfer token: {error}"))
    })?;
    Ok(format!("spt_{kind}_{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn generate_transfer_token_id() -> Result<String, PersonalServiceError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to generate transfer token id: {error}"))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn hash_transfer_token(kind: &str, token: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("starpoint-transfer-{kind}:{token}").as_bytes())
    )
}

fn normalize_device_name(value: Option<&str>) -> Result<Option<String>, LocalSaveStoreError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = value.trim();
    if normalized.is_empty()
        || normalized.chars().count() > 64
        || normalized.chars().any(char::is_control)
    {
        return Err(LocalSaveStoreError::InvalidState);
    }
    Ok(Some(normalized.to_owned()))
}

fn has_canonical_milliseconds(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && [4, 7, 10, 13, 16, 19, 23].iter().all(|index| {
            matches!(
                (index, bytes[*index]),
                (4 | 7, b'-') | (10, b'T') | (13 | 16, b':') | (19, b'.') | (23, b'Z')
            )
        })
        && bytes
            .iter()
            .enumerate()
            .filter(|(index, _)| ![4, 7, 10, 13, 16, 19, 23].contains(index))
            .all(|(_, byte)| byte.is_ascii_digit())
}

fn normalize_expiration(
    value: Option<&str>,
    now: &str,
) -> Result<Option<String>, LocalSaveStoreError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !has_canonical_milliseconds(value) || parse_iso_timestamp(value).is_none() || value <= now {
        return Err(LocalSaveStoreError::InvalidState);
    }
    Ok(Some(value.to_owned()))
}
// //// /生成和校验 transfer token 输入 ////

fn read_metadata(row: &Row<'_>) -> rusqlite::Result<LocalTransferTokenMetadata> {
    let permission = row
        .get::<_, Option<String>>(3)?
        .map(|value| {
            LocalTransferPermission::from_database_value(&value)
                .ok_or(rusqlite::Error::InvalidQuery)
        })
        .transpose()?;
    Ok(LocalTransferTokenMetadata {
        id: row.get(0)?,
        account_id: row.get(1)?,
        slot_id: row.get(2)?,
        permission,
        device_name: row.get(4)?,
        created_at: row.get(5)?,
        expires_at: row.get(6)?,
        revoked_at: row.get(7)?,
    })
}

fn slot_account_id(
    connection: &Connection,
    slot_id: i64,
) -> Result<Option<i64>, LocalSaveStoreError> {
    connection
        .query_row(
            "SELECT account_id FROM local_save_slots WHERE id = ?1",
            params![slot_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(local_save_storage_error)
}

fn transfer_timestamp(transaction: &Transaction<'_>) -> Result<String, LocalSaveStoreError> {
    transaction
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
            row.get(0)
        })
        .map_err(local_save_storage_error)
}

fn transfer_instance_id(connection: &Connection) -> Result<String, PersonalServiceError> {
    connection
        .query_row(
            "SELECT instance_id FROM local_transfer_token_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(local_save_database_error)
}

// //// 签发本地壳和槽 transfer token [@x380kkm 2026-07-27] ////
impl ServiceDatabase {
    pub(crate) fn local_transfer_instance_id(&self) -> Result<String, PersonalServiceError> {
        transfer_instance_id(&self.connection)
    }

    pub(crate) fn issue_local_shell_transfer_token(
        &mut self,
        slot_id: i64,
        expires_at: Option<&str>,
        device_name: Option<&str>,
    ) -> Result<Option<LocalIssuedTransferToken>, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let Some(account_id) = slot_account_id(&transaction, slot_id)? else {
            return Ok(None);
        };
        let now = transfer_timestamp(&transaction)?;
        let expires_at = normalize_expiration(expires_at, &now)?;
        let device_name = normalize_device_name(device_name)?;
        let id = generate_transfer_token_id().map_err(LocalSaveStoreError::Storage)?;
        let token = generate_transfer_token("shell").map_err(LocalSaveStoreError::Storage)?;
        transaction
            .execute(
                "INSERT INTO local_shell_transfer_tokens (
                     id, token_hash, account_id, device_name, created_at, expires_at, revoked_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                params![
                    id,
                    hash_transfer_token("shell", &token),
                    account_id,
                    device_name,
                    now,
                    expires_at
                ],
            )
            .map_err(local_save_storage_error)?;
        let metadata = LocalTransferTokenMetadata {
            id,
            account_id,
            slot_id: None,
            permission: None,
            device_name,
            created_at: now,
            expires_at,
            revoked_at: None,
        };
        let instance_id =
            transfer_instance_id(&transaction).map_err(LocalSaveStoreError::Storage)?;
        transaction.commit().map_err(local_save_storage_error)?;
        Ok(Some(LocalIssuedTransferToken {
            token,
            instance_id,
            metadata,
        }))
    }

    pub(crate) fn issue_local_slot_transfer_token(
        &mut self,
        slot_id: i64,
        permission: LocalTransferPermission,
        expires_at: Option<&str>,
        device_name: Option<&str>,
    ) -> Result<Option<LocalIssuedTransferToken>, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let Some(account_id) = slot_account_id(&transaction, slot_id)? else {
            return Ok(None);
        };
        let now = transfer_timestamp(&transaction)?;
        let expires_at = normalize_expiration(expires_at, &now)?;
        let device_name = normalize_device_name(device_name)?;
        let id = generate_transfer_token_id().map_err(LocalSaveStoreError::Storage)?;
        let token = generate_transfer_token("slot").map_err(LocalSaveStoreError::Storage)?;
        transaction
            .execute(
                "INSERT INTO local_slot_transfer_tokens (
                     id, token_hash, account_id, slot_id, permission,
                     device_name, created_at, expires_at, revoked_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
                params![
                    id,
                    hash_transfer_token("slot", &token),
                    account_id,
                    slot_id,
                    permission.as_database_value(),
                    device_name,
                    now,
                    expires_at,
                ],
            )
            .map_err(local_save_storage_error)?;
        let metadata = LocalTransferTokenMetadata {
            id,
            account_id,
            slot_id: Some(slot_id),
            permission: Some(permission),
            device_name,
            created_at: now,
            expires_at,
            revoked_at: None,
        };
        let instance_id =
            transfer_instance_id(&transaction).map_err(LocalSaveStoreError::Storage)?;
        transaction.commit().map_err(local_save_storage_error)?;
        Ok(Some(LocalIssuedTransferToken {
            token,
            instance_id,
            metadata,
        }))
    }
    // //// /签发本地壳和槽 transfer token ////

    // //// 列出和撤销本地 transfer token [@x380kkm 2026-07-27] ////
    pub(crate) fn list_local_shell_transfer_tokens(
        &self,
        slot_id: i64,
    ) -> Result<Option<Vec<LocalTransferTokenMetadata>>, LocalSaveStoreError> {
        let Some(account_id) = slot_account_id(&self.connection, slot_id)? else {
            return Ok(None);
        };
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, account_id, NULL AS slot_id, NULL AS permission,
                    device_name, created_at, expires_at, revoked_at
             FROM local_shell_transfer_tokens
             WHERE account_id = ?1
             ORDER BY created_at DESC, id DESC",
            )
            .map_err(local_save_storage_error)?;
        let tokens = statement
            .query_map(params![account_id], read_metadata)
            .map_err(local_save_storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_storage_error)?;
        Ok(Some(tokens))
    }

    pub(crate) fn list_local_slot_transfer_tokens(
        &self,
        slot_id: i64,
    ) -> Result<Option<Vec<LocalTransferTokenMetadata>>, LocalSaveStoreError> {
        let Some(account_id) = slot_account_id(&self.connection, slot_id)? else {
            return Ok(None);
        };
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, account_id, slot_id, permission,
                    device_name, created_at, expires_at, revoked_at
             FROM local_slot_transfer_tokens
             WHERE account_id = ?1 AND slot_id = ?2
             ORDER BY created_at DESC, id DESC",
            )
            .map_err(local_save_storage_error)?;
        let tokens = statement
            .query_map(params![account_id, slot_id], read_metadata)
            .map_err(local_save_storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_storage_error)?;
        Ok(Some(tokens))
    }

    pub(crate) fn revoke_local_shell_transfer_token(
        &mut self,
        slot_id: i64,
        token_id: &str,
    ) -> Result<bool, LocalSaveStoreError> {
        let Some(account_id) = slot_account_id(&self.connection, slot_id)? else {
            return Ok(false);
        };
        let now = self
            .connection
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(local_save_storage_error)?;
        self.connection
            .execute(
                "UPDATE local_shell_transfer_tokens
             SET revoked_at = ?1
             WHERE id = ?2 AND account_id = ?3 AND revoked_at IS NULL",
                params![now, token_id, account_id],
            )
            .map_err(local_save_storage_error)
            .map(|changes| changes > 0)
    }

    pub(crate) fn revoke_local_slot_transfer_token(
        &mut self,
        slot_id: i64,
        token_id: &str,
    ) -> Result<bool, LocalSaveStoreError> {
        let Some(account_id) = slot_account_id(&self.connection, slot_id)? else {
            return Ok(false);
        };
        let now = self
            .connection
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(local_save_storage_error)?;
        self.connection
            .execute(
                "UPDATE local_slot_transfer_tokens
             SET revoked_at = ?1
             WHERE id = ?2 AND account_id = ?3 AND slot_id = ?4 AND revoked_at IS NULL",
                params![now, token_id, account_id, slot_id],
            )
            .map_err(local_save_storage_error)
            .map(|changes| changes > 0)
    }
    // //// /列出和撤销本地 transfer token ////

    // //// 验证本地壳和槽 transfer token [@x380kkm 2026-07-27] ////
    pub(crate) fn resolve_local_shell_transfer_token(
        &self,
        token: &str,
    ) -> Result<Option<LocalTransferTokenMetadata>, PersonalServiceError> {
        let now = self
            .connection
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(local_save_database_error)?;
        self.connection
            .query_row(
                "SELECT id, account_id, NULL AS slot_id, NULL AS permission,
                    device_name, created_at, expires_at, revoked_at
             FROM local_shell_transfer_tokens
             WHERE token_hash = ?1
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > ?2)",
                params![hash_transfer_token("shell", token), now],
                read_metadata,
            )
            .optional()
            .map_err(local_save_database_error)
    }

    pub(crate) fn resolve_local_slot_transfer_token(
        &self,
        token: &str,
        slot_id: i64,
        permission: LocalTransferPermission,
    ) -> Result<Option<LocalTransferTokenMetadata>, PersonalServiceError> {
        let now = self
            .connection
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(local_save_database_error)?;
        let metadata = self
            .connection
            .query_row(
                "SELECT id, account_id, slot_id, permission,
                    device_name, created_at, expires_at, revoked_at
             FROM local_slot_transfer_tokens
             WHERE token_hash = ?1
               AND slot_id = ?2
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > ?3)",
                params![hash_transfer_token("slot", token), slot_id, now],
                read_metadata,
            )
            .optional()
            .map_err(local_save_database_error)?;
        Ok(metadata.filter(|metadata| {
            metadata
                .permission
                .is_some_and(|stored| stored.allows(permission))
        }))
    }
    // //// /验证本地壳和槽 transfer token ////
}
