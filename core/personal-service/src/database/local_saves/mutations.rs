// audience: internal
// # personal-service-local-save-mutations
//
// 该文件创建, 复制和恢复本地存档槽.
// 该文件创建, 查询和恢复不可变 revision.
// 每次恢复先保存当前状态.
// 恢复会清除不再匹配恢复状态的进行中单机战斗.
// 自动快照按来源标记限制数量.

use super::{
    grant_local_save_access_in_connection, local_save_storage_error, read_slot, read_snapshot,
    LocalSaveRevision, LocalSaveSlot, LocalSaveSnapshot, LocalSaveStoreError,
};
use crate::database::ServiceDatabase;
use crate::portable_save;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;

impl ServiceDatabase {
    // //// 创建或复用槽位当前 revision [@x380kkm 2026-07-27] ////
    pub(crate) fn ensure_local_save_revision(
        &mut self,
        slot_id: i64,
        label: &str,
    ) -> Result<LocalSaveRevision, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let data_json = transaction
            .query_row(
                "SELECT player_snapshots.data_json
                 FROM local_save_slots
                 JOIN player_snapshots ON player_snapshots.account_id = local_save_slots.account_id
                 WHERE local_save_slots.id = ?1",
                params![slot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(local_save_storage_error)?
            .ok_or(LocalSaveStoreError::NotFound)?;
        let revision = insert_local_save_revision(&transaction, slot_id, &data_json, label)?;
        transaction.commit().map_err(local_save_storage_error)?;
        Ok(revision)
    }
    // //// /创建或复用槽位当前 revision ////

    // //// 列出槽位 revision [@x380kkm 2026-07-27] ////
    pub(crate) fn list_local_save_revisions(
        &self,
        slot_id: i64,
    ) -> Result<Option<Vec<LocalSaveRevision>>, LocalSaveStoreError> {
        if find_local_save_slot(&self.connection, slot_id)?.is_none() {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, slot_id, parent_revision_id, payload_sha256,
                        label, created_at, pinned
                 FROM local_save_revisions
                 WHERE slot_id = ?1
                 ORDER BY created_at DESC, id DESC",
            )
            .map_err(local_save_storage_error)?;
        let revisions = statement
            .query_map(params![slot_id], read_local_save_revision)
            .map_err(local_save_storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_storage_error)?;
        Ok(Some(revisions))
    }
    // //// /列出槽位 revision ////

    // //// 从当前槽复制新的本地存档槽 [@x380kkm 2026-07-23] ////
    pub(crate) fn copy_local_save(
        &mut self,
        source_slot_id: i64,
        name: &str,
    ) -> Result<LocalSaveSlot, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let (source_account_id, data_json) = transaction
            .query_row(
                "SELECT local_save_slots.account_id, player_snapshots.data_json
                 FROM local_save_slots
                 JOIN player_snapshots ON player_snapshots.account_id = local_save_slots.account_id
                 WHERE local_save_slots.id = ?1",
                params![source_slot_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(local_save_storage_error)?
            .ok_or(LocalSaveStoreError::NotFound)?;
        let slot_id = create_local_save_slot(&transaction, name, &data_json)?;
        transaction
            .execute(
                "INSERT INTO local_save_access (account_id, slot_id) VALUES (?1, ?2)",
                params![source_account_id, slot_id],
            )
            .map_err(local_save_storage_error)?;
        transaction.commit().map_err(local_save_storage_error)?;
        find_local_save_slot(&self.connection, slot_id)?.ok_or(LocalSaveStoreError::InvalidState)
    }
    // //// /从当前槽复制新的本地存档槽 ////

    // //// 从已验证数据创建本地存档槽 [@x380kkm 2026-07-23] ////
    pub(crate) fn import_local_save(
        &mut self,
        name: &str,
        data_json: &str,
    ) -> Result<LocalSaveSlot, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let available_name = available_local_save_name(&transaction, name)?;
        let slot_id = create_local_save_slot(&transaction, &available_name, data_json)?;
        transaction.commit().map_err(local_save_storage_error)?;
        find_local_save_slot(&self.connection, slot_id)?.ok_or(LocalSaveStoreError::InvalidState)
    }

    pub(crate) fn import_local_save_for_account(
        &mut self,
        account_id: i64,
        name: &str,
        data_json: &str,
    ) -> Result<LocalSaveSlot, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let available_name = available_local_save_name(&transaction, name)?;
        let slot_id = create_local_save_slot(&transaction, &available_name, data_json)?;
        grant_local_save_access_in_connection(&transaction, account_id, slot_id)?;
        transaction.commit().map_err(local_save_storage_error)?;
        find_local_save_slot(&self.connection, slot_id)?.ok_or(LocalSaveStoreError::InvalidState)
    }
    // //// /从已验证数据创建本地存档槽 ////

    // //// 创建普通和有保留上限的本地存档快照 [@x380kkm 2026-07-23] ////
    pub(crate) fn create_local_save_snapshot(
        &mut self,
        slot_id: i64,
        label: &str,
    ) -> Result<LocalSaveSnapshot, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let snapshot_id = insert_local_save_snapshot(&transaction, slot_id, label, false)?;
        transaction.commit().map_err(local_save_storage_error)?;
        find_local_save_snapshot(&self.connection, snapshot_id)?
            .ok_or(LocalSaveStoreError::InvalidState)
    }

    pub(crate) fn create_automatic_local_save_snapshot(
        &mut self,
        slot_id: i64,
        label: &str,
        retention_count: i64,
    ) -> Result<LocalSaveSnapshot, LocalSaveStoreError> {
        if retention_count <= 0 {
            return Err(LocalSaveStoreError::InvalidState);
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let snapshot_id = insert_local_save_snapshot(&transaction, slot_id, label, true)?;
        transaction
            .execute(
                "DELETE FROM local_save_snapshots
                 WHERE id IN (
                     SELECT id FROM local_save_snapshots
                     WHERE slot_id = ?1 AND is_automatic = 1
                     ORDER BY created_at DESC, id DESC
                     LIMIT -1 OFFSET ?2
                 )",
                params![slot_id, retention_count],
            )
            .map_err(local_save_storage_error)?;
        transaction.commit().map_err(local_save_storage_error)?;
        find_local_save_snapshot(&self.connection, snapshot_id)?
            .ok_or(LocalSaveStoreError::InvalidState)
    }
    // //// /创建普通和有保留上限的本地存档快照 ////

    // //// 回滚本地存档并保留回滚前状态 [@x380kkm 2026-07-23] ////
    pub(crate) fn restore_local_save_snapshot(
        &mut self,
        slot_id: i64,
        snapshot_id: i64,
    ) -> Result<LocalSaveSnapshot, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let target_data = transaction
            .query_row(
                "SELECT data_json FROM local_save_snapshots
                 WHERE id = ?1 AND slot_id = ?2",
                params![snapshot_id, slot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(local_save_storage_error)?
            .ok_or(LocalSaveStoreError::NotFound)?;
        let current_data = transaction
            .query_row(
                "SELECT player_snapshots.data_json
                 FROM local_save_slots
                 JOIN player_snapshots ON player_snapshots.account_id = local_save_slots.account_id
                 WHERE local_save_slots.id = ?1",
                params![slot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(local_save_storage_error)?
            .ok_or(LocalSaveStoreError::NotFound)?;
        insert_local_save_revision(&transaction, slot_id, &current_data, "Before restore")?;
        let safety_snapshot_id = insert_safety_snapshot(&transaction, slot_id)?;
        let updated = transaction
            .execute(
                "UPDATE player_snapshots
                 SET data_json = ?1
                 WHERE account_id = (
                     SELECT account_id FROM local_save_slots WHERE id = ?2
                 )",
                params![target_data, slot_id],
            )
            .map_err(local_save_storage_error)?;
        if updated != 1 {
            return Err(LocalSaveStoreError::InvalidState);
        }
        insert_local_save_revision(&transaction, slot_id, &target_data, "Snapshot restore")?;
        transaction
            .execute(
                "DELETE FROM active_single_quests
                 WHERE account_id = (
                     SELECT account_id FROM local_save_slots WHERE id = ?1
                 )",
                params![slot_id],
            )
            .map_err(local_save_storage_error)?;
        transaction.commit().map_err(local_save_storage_error)?;
        find_local_save_snapshot(&self.connection, safety_snapshot_id)?
            .ok_or(LocalSaveStoreError::InvalidState)
    }
    // //// /回滚本地存档并保留回滚前状态 ////

    // //// 恢复不可变 revision 并保留恢复前版本 [@x380kkm 2026-07-27] ////
    pub(crate) fn restore_local_save_revision(
        &mut self,
        slot_id: i64,
        revision_id: &str,
    ) -> Result<LocalSaveRevision, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let target_data = transaction
            .query_row(
                "SELECT data_json FROM local_save_revisions WHERE id = ?1 AND slot_id = ?2",
                params![revision_id, slot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(local_save_storage_error)?
            .ok_or(LocalSaveStoreError::NotFound)?;
        let current_data = transaction
            .query_row(
                "SELECT player_snapshots.data_json
                 FROM local_save_slots
                 JOIN player_snapshots ON player_snapshots.account_id = local_save_slots.account_id
                 WHERE local_save_slots.id = ?1",
                params![slot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(local_save_storage_error)?
            .ok_or(LocalSaveStoreError::NotFound)?;
        insert_local_save_revision(&transaction, slot_id, &current_data, "Before restore")?;
        let updated = transaction
            .execute(
                "UPDATE player_snapshots
                 SET data_json = ?1
                 WHERE account_id = (
                     SELECT account_id FROM local_save_slots WHERE id = ?2
                 )",
                params![target_data, slot_id],
            )
            .map_err(local_save_storage_error)?;
        if updated != 1 {
            return Err(LocalSaveStoreError::InvalidState);
        }
        transaction
            .execute(
                "DELETE FROM active_single_quests
                 WHERE account_id = (
                     SELECT account_id FROM local_save_slots WHERE id = ?1
                 )",
                params![slot_id],
            )
            .map_err(local_save_storage_error)?;
        let restored = insert_local_save_revision(
            &transaction,
            slot_id,
            &target_data,
            &format!("Restored {revision_id}"),
        )?;
        transaction.commit().map_err(local_save_storage_error)?;
        Ok(restored)
    }
    // //// /恢复不可变 revision 并保留恢复前版本 ////

    // //// 覆盖本地存档并追加 revision [@x380kkm 2026-07-27] ////
    pub(crate) fn replace_local_save_data(
        &mut self,
        slot_id: i64,
        data_json: &str,
    ) -> Result<LocalSaveRevision, LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let revision = replace_local_save_data_in_transaction(&transaction, slot_id, data_json)?;
        transaction.commit().map_err(local_save_storage_error)?;
        Ok(revision)
    }
}

pub(in crate::database) fn replace_local_save_data_in_transaction(
    transaction: &Transaction<'_>,
    slot_id: i64,
    data_json: &str,
) -> Result<LocalSaveRevision, LocalSaveStoreError> {
    let current_data = transaction
        .query_row(
            "SELECT player_snapshots.data_json
             FROM local_save_slots
             JOIN player_snapshots ON player_snapshots.account_id = local_save_slots.account_id
             WHERE local_save_slots.id = ?1",
            params![slot_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(local_save_storage_error)?
        .ok_or(LocalSaveStoreError::NotFound)?;
    insert_local_save_revision(
        transaction,
        slot_id,
        &current_data,
        "Before transfer upload",
    )?;
    let updated = transaction
        .execute(
            "UPDATE player_snapshots
             SET data_json = ?1
             WHERE account_id = (
                 SELECT account_id FROM local_save_slots WHERE id = ?2
             )",
            params![data_json, slot_id],
        )
        .map_err(local_save_storage_error)?;
    if updated != 1 {
        return Err(LocalSaveStoreError::InvalidState);
    }
    transaction
        .execute(
            "DELETE FROM active_single_quests
             WHERE account_id = (
                 SELECT account_id FROM local_save_slots WHERE id = ?1
             )",
            params![slot_id],
        )
        .map_err(local_save_storage_error)?;
    insert_local_save_revision(transaction, slot_id, data_json, "Transfer upload")
}
// //// /覆盖本地存档并追加 revision ////

fn insert_local_save_snapshot(
    connection: &Connection,
    slot_id: i64,
    label: &str,
    is_automatic: bool,
) -> Result<i64, LocalSaveStoreError> {
    let inserted = connection
        .execute(
            "INSERT INTO local_save_snapshots (
                 slot_id, label, data_json, created_at, is_automatic
             )
             SELECT slots.id, ?2, player_snapshots.data_json,
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?3
             FROM local_save_slots AS slots
             JOIN player_snapshots ON player_snapshots.account_id = slots.account_id
             WHERE slots.id = ?1",
            params![slot_id, label, is_automatic],
        )
        .map_err(local_save_storage_error)?;
    if inserted != 1 {
        return Err(LocalSaveStoreError::NotFound);
    }
    Ok(connection.last_insert_rowid())
}

// //// 生成未占用的本地存档显示名称 [@x380kkm 2026-08-31] ////
fn available_local_save_name(
    transaction: &Transaction<'_>,
    requested_name: &str,
) -> Result<String, LocalSaveStoreError> {
    if !local_save_name_exists(transaction, requested_name)? {
        return Ok(requested_name.to_owned());
    }
    for number in 2_u64.. {
        let suffix = format!(" ({number})");
        let prefix_length = 64_usize.saturating_sub(suffix.chars().count());
        let prefix = requested_name
            .chars()
            .take(prefix_length)
            .collect::<String>();
        let candidate = format!("{prefix}{suffix}");
        if !local_save_name_exists(transaction, &candidate)? {
            return Ok(candidate);
        }
    }
    unreachable!()
}

fn local_save_name_exists(
    transaction: &Transaction<'_>,
    name: &str,
) -> Result<bool, LocalSaveStoreError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE name = ?1)",
            params![name],
            |row| row.get(0),
        )
        .map_err(local_save_storage_error)
}
// //// /生成未占用的本地存档显示名称 ////

pub(in crate::database) fn create_local_save_slot(
    transaction: &Transaction<'_>,
    name: &str,
    data_json: &str,
) -> Result<i64, LocalSaveStoreError> {
    transaction
        .execute(
            "WITH identity(value) AS (
                 SELECT 'local-save:' || lower(hex(randomblob(16)))
             )
             INSERT INTO accounts (
                 app_id, first_login_time, idp_alias, idp_code, idp_id,
                 reg_time, last_login_time, status
             )
             SELECT 'wf_cn', now.value, identity.value, 'local', identity.value,
                    now.value, now.value, 'normal'
             FROM identity
             CROSS JOIN (
                 SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now') AS value
             ) AS now",
            [],
        )
        .map_err(local_save_storage_error)?;
    let account_id = transaction.last_insert_rowid();
    transaction
        .execute(
            "INSERT INTO players (account_id) VALUES (?1)",
            params![account_id],
        )
        .map_err(local_save_storage_error)?;
    transaction
        .execute(
            "INSERT INTO player_snapshots (account_id, data_json) VALUES (?1, ?2)",
            params![account_id, data_json],
        )
        .map_err(local_save_storage_error)?;
    transaction
        .execute(
            "INSERT INTO local_save_slots (account_id, name, created_at, updated_at)
             VALUES (
                 ?1, ?2,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )",
            params![account_id, name],
        )
        .map_err(local_save_storage_error)?;
    let slot_id = transaction.last_insert_rowid();
    insert_local_save_revision(transaction, slot_id, data_json, "Initial save")?;
    Ok(slot_id)
}

fn insert_local_save_revision(
    transaction: &Transaction<'_>,
    slot_id: i64,
    data_json: &str,
    label: &str,
) -> Result<LocalSaveRevision, LocalSaveStoreError> {
    let data = serde_json::from_str::<Value>(data_json).map_err(|error| {
        LocalSaveStoreError::Storage(crate::PersonalServiceError::new(format!(
            "failed to decode local save revision: {error}"
        )))
    })?;
    let etag =
        portable_save::calculate_payload_sha256(&data).map_err(LocalSaveStoreError::Storage)?;
    let current = transaction
        .query_row(
            "SELECT revisions.id, revisions.payload_sha256
             FROM local_save_heads AS heads
             JOIN local_save_revisions AS revisions ON revisions.id = heads.revision_id
             WHERE heads.slot_id = ?1",
            params![slot_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(local_save_storage_error)?;
    if let Some((revision_id, current_etag)) = current.as_ref() {
        if current_etag == &etag {
            return find_local_save_revision(transaction, slot_id, revision_id)?
                .ok_or(LocalSaveStoreError::InvalidState);
        }
    }
    let parent_revision_id = current.map(|(revision_id, _)| revision_id);
    let revision_id = transaction
        .query_row(
            "INSERT INTO local_save_revisions (
                 id, slot_id, parent_revision_id, payload_sha256,
                 data_json, label, created_at, pinned
             ) VALUES (
                 lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 0
             )
             RETURNING id",
            params![slot_id, parent_revision_id, etag, data_json, label],
            |row| row.get::<_, String>(0),
        )
        .map_err(local_save_storage_error)?;
    transaction
        .execute(
            "INSERT INTO local_save_heads (slot_id, revision_id, updated_at)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(slot_id) DO UPDATE SET
                 revision_id = excluded.revision_id,
                 updated_at = excluded.updated_at",
            params![slot_id, revision_id],
        )
        .map_err(local_save_storage_error)?;
    find_local_save_revision(transaction, slot_id, &revision_id)?
        .ok_or(LocalSaveStoreError::InvalidState)
}

fn find_local_save_revision(
    connection: &Connection,
    slot_id: i64,
    revision_id: &str,
) -> Result<Option<LocalSaveRevision>, LocalSaveStoreError> {
    connection
        .query_row(
            "SELECT id, slot_id, parent_revision_id, payload_sha256,
                    label, created_at, pinned
             FROM local_save_revisions
             WHERE slot_id = ?1 AND id = ?2",
            params![slot_id, revision_id],
            read_local_save_revision,
        )
        .optional()
        .map_err(local_save_storage_error)
}

fn read_local_save_revision(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalSaveRevision> {
    Ok(LocalSaveRevision {
        id: row.get(0)?,
        slot_id: row.get(1)?,
        parent_revision_id: row.get(2)?,
        etag: row.get(3)?,
        label: row.get(4)?,
        created_at: row.get(5)?,
        pinned: row.get(6)?,
    })
}

fn insert_safety_snapshot(
    transaction: &Transaction<'_>,
    slot_id: i64,
) -> Result<i64, LocalSaveStoreError> {
    let inserted = transaction
        .execute(
            "INSERT INTO local_save_snapshots (slot_id, label, data_json, created_at)
             SELECT slots.id, 'Before restore', player_snapshots.data_json,
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             FROM local_save_slots AS slots
             JOIN player_snapshots ON player_snapshots.account_id = slots.account_id
             WHERE slots.id = ?1",
            params![slot_id],
        )
        .map_err(local_save_storage_error)?;
    if inserted == 1 {
        Ok(transaction.last_insert_rowid())
    } else {
        Err(LocalSaveStoreError::NotFound)
    }
}

pub(in crate::database) fn find_local_save_slot(
    connection: &rusqlite::Connection,
    slot_id: i64,
) -> Result<Option<LocalSaveSlot>, LocalSaveStoreError> {
    connection
        .query_row(
            "SELECT slots.id, slots.name, slots.created_at, slots.updated_at,
                    (SELECT COUNT(*) FROM local_save_snapshots
                     WHERE local_save_snapshots.slot_id = slots.id)
             FROM local_save_slots AS slots WHERE slots.id = ?1",
            params![slot_id],
            read_slot,
        )
        .optional()
        .map_err(local_save_storage_error)
}

fn find_local_save_snapshot(
    connection: &rusqlite::Connection,
    snapshot_id: i64,
) -> Result<Option<LocalSaveSnapshot>, LocalSaveStoreError> {
    connection
        .query_row(
            "SELECT id, slot_id, label, created_at
             FROM local_save_snapshots WHERE id = ?1",
            params![snapshot_id],
            read_snapshot,
        )
        .optional()
        .map_err(local_save_storage_error)
}
