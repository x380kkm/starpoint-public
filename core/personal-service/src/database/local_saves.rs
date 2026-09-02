// audience: internal
// # personal-service-local-saves
//
// 该模块把本地账号映射为可管理的存档槽.
// 设备只保存当前槽引用.
// player_snapshots 保存单一当前状态.
// local_save_snapshots 保存按保留策略管理的历史快照.
// local_save_revisions 保存不可变版本.
// local_save_heads 保存槽位当前 revision 指针.

use super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};

mod mutations;
mod transfer_tokens;

pub(super) use mutations::{
    create_local_save_slot, find_local_save_slot, replace_local_save_data_in_transaction,
};

#[derive(Debug)]
pub(crate) struct LocalSaveSlot {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) snapshot_count: i64,
}

#[derive(Debug)]
pub(crate) struct LocalSaveDevice {
    pub(crate) device_id: i64,
    pub(crate) active_slot_id: i64,
}

pub(crate) struct LocalSaveState {
    pub(crate) slots: Vec<LocalSaveSlot>,
    pub(crate) devices: Vec<LocalSaveDevice>,
}

pub(crate) struct LocalSaveContext {
    pub(crate) slot: LocalSaveSlot,
    pub(crate) account_id: i64,
    pub(crate) viewer_id: Option<i64>,
    pub(crate) active_device_ids: Vec<i64>,
}

pub(crate) struct LocalSaveExport {
    pub(crate) slot: LocalSaveSlot,
    pub(crate) data_json: String,
    pub(crate) revision_id: String,
    pub(crate) etag: String,
}

#[derive(Debug)]
pub(crate) struct LocalSaveRevision {
    pub(crate) id: String,
    pub(crate) slot_id: i64,
    pub(crate) parent_revision_id: Option<String>,
    pub(crate) etag: String,
    pub(crate) label: String,
    pub(crate) created_at: String,
    pub(crate) pinned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalTransferPermission {
    Upload,
    Download,
    Both,
}

#[derive(Debug)]
pub(crate) struct LocalTransferTokenMetadata {
    pub(crate) id: String,
    pub(crate) account_id: i64,
    pub(crate) slot_id: Option<i64>,
    pub(crate) permission: Option<LocalTransferPermission>,
    pub(crate) device_name: Option<String>,
    pub(crate) created_at: String,
    pub(crate) expires_at: Option<String>,
    pub(crate) revoked_at: Option<String>,
}

pub(crate) struct LocalIssuedTransferToken {
    pub(crate) token: String,
    pub(crate) instance_id: String,
    pub(crate) metadata: LocalTransferTokenMetadata,
}

#[derive(Debug)]
pub(crate) struct LocalSaveSnapshot {
    pub(crate) id: i64,
    pub(crate) slot_id: i64,
    pub(crate) label: String,
    pub(crate) created_at: String,
}

pub(crate) enum LocalSaveStoreError {
    NotFound,
    Busy,
    InvalidState,
    Storage(PersonalServiceError),
}

// //// 创建本地存档槽和可区分来源的快照表 [@x380kkm 2026-07-23] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS local_save_slots (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 account_id INTEGER NOT NULL UNIQUE,
                 name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 64),
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS active_local_save_slots (
                 device_id INTEGER PRIMARY KEY CHECK (device_id > 0),
                 slot_id INTEGER NOT NULL,
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE RESTRICT
             );
             CREATE TABLE IF NOT EXISTS local_save_access (
                 account_id INTEGER NOT NULL,
                 slot_id INTEGER NOT NULL,
                 PRIMARY KEY (account_id, slot_id),
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE,
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE
             );
             INSERT OR IGNORE INTO local_save_access (account_id, slot_id)
             SELECT account_id, id FROM local_save_slots;
             CREATE TABLE IF NOT EXISTS local_save_snapshots (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 slot_id INTEGER NOT NULL,
                 label TEXT NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 64),
                 data_json TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 is_automatic INTEGER NOT NULL DEFAULT 0 CHECK (is_automatic IN (0, 1)),
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS local_save_revisions (
                 id TEXT PRIMARY KEY NOT NULL,
                 slot_id INTEGER NOT NULL,
                 parent_revision_id TEXT,
                 payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64),
                 data_json TEXT NOT NULL,
                 label TEXT NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 64),
                 created_at TEXT NOT NULL,
                 pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE,
                 FOREIGN KEY (parent_revision_id) REFERENCES local_save_revisions (id) ON DELETE RESTRICT
             );
             CREATE INDEX IF NOT EXISTS local_save_revisions_slot_created
             ON local_save_revisions (slot_id, created_at DESC, id DESC);
             CREATE TABLE IF NOT EXISTS local_save_heads (
                 slot_id INTEGER PRIMARY KEY NOT NULL,
                 revision_id TEXT NOT NULL UNIQUE,
                 updated_at TEXT NOT NULL,
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE,
                 FOREIGN KEY (revision_id) REFERENCES local_save_revisions (id) ON DELETE RESTRICT
             );
             CREATE TABLE IF NOT EXISTS local_transfer_token_state (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 instance_id TEXT NOT NULL
             );
             INSERT OR IGNORE INTO local_transfer_token_state (id, instance_id)
             VALUES (1, lower(hex(randomblob(16))));
             CREATE TABLE IF NOT EXISTS local_shell_transfer_tokens (
                 id TEXT PRIMARY KEY NOT NULL,
                 token_hash TEXT NOT NULL UNIQUE,
                 account_id INTEGER NOT NULL,
                 device_name TEXT,
                 created_at TEXT NOT NULL,
                 expires_at TEXT,
                 revoked_at TEXT,
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS local_shell_transfer_tokens_account
             ON local_shell_transfer_tokens (account_id, created_at DESC);
             CREATE TABLE IF NOT EXISTS local_slot_transfer_tokens (
                 id TEXT PRIMARY KEY NOT NULL,
                 token_hash TEXT NOT NULL UNIQUE,
                 account_id INTEGER NOT NULL,
                 slot_id INTEGER NOT NULL,
                 permission TEXT NOT NULL CHECK (permission IN ('upload', 'download', 'both')),
                 device_name TEXT,
                 created_at TEXT NOT NULL,
                 expires_at TEXT,
                 revoked_at TEXT,
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE,
                 FOREIGN KEY (slot_id) REFERENCES local_save_slots (id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS local_slot_transfer_tokens_slot
             ON local_slot_transfer_tokens (account_id, slot_id, created_at DESC);
             CREATE TRIGGER IF NOT EXISTS local_save_revisions_immutable
             BEFORE UPDATE OF id, slot_id, parent_revision_id, payload_sha256, data_json, label, created_at
             ON local_save_revisions
             BEGIN
                 SELECT RAISE(ABORT, 'save revisions are immutable');
             END;
             INSERT OR IGNORE INTO local_save_slots (
                 account_id, name, created_at, updated_at
             )
             SELECT id, printf('Save %d', id), reg_time, last_login_time FROM accounts;
             INSERT OR IGNORE INTO active_local_save_slots (device_id, slot_id)
             SELECT CAST(substr(accounts.idp_id, 4) AS INTEGER), slots.id
             FROM accounts
             JOIN local_save_slots AS slots ON slots.account_id = accounts.id
             WHERE accounts.idp_id GLOB 'cn:[0-9]*'
               AND CAST(substr(accounts.idp_id, 4) AS INTEGER) > 0;
             CREATE TRIGGER IF NOT EXISTS local_save_slot_touch_after_snapshot_insert
             AFTER INSERT ON player_snapshots
             BEGIN
                 UPDATE local_save_slots
                 SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE account_id = NEW.account_id;
             END;
             CREATE TRIGGER IF NOT EXISTS local_save_slot_touch_after_snapshot_update
             AFTER UPDATE OF data_json ON player_snapshots
             BEGIN
                 UPDATE local_save_slots
                 SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE account_id = NEW.account_id;
             END;",
        )
        .map_err(local_save_database_error)?;
    let has_automatic_marker = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('local_save_snapshots')
                 WHERE name = 'is_automatic'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(local_save_database_error)?;
    if !has_automatic_marker {
        connection
            .execute_batch(
                "ALTER TABLE local_save_snapshots
                 ADD COLUMN is_automatic INTEGER NOT NULL DEFAULT 0
                     CHECK (is_automatic IN (0, 1));",
            )
            .map_err(local_save_database_error)?;
    }
    Ok(())
}
// //// /创建本地存档槽和可区分来源的快照表 ////

impl ServiceDatabase {
    // //// 列出本地存档槽和设备选择 [@x380kkm 2026-07-23] ////
    pub(crate) fn list_local_saves(&self) -> Result<LocalSaveState, PersonalServiceError> {
        let mut slot_statement = self
            .connection
            .prepare(
                "SELECT slots.id, slots.name, slots.created_at, slots.updated_at,
                        (SELECT COUNT(*) FROM local_save_snapshots
                         WHERE local_save_snapshots.slot_id = slots.id)
                 FROM local_save_slots AS slots
                 ORDER BY slots.updated_at DESC, slots.id",
            )
            .map_err(local_save_database_error)?;
        let slots = slot_statement
            .query_map([], read_slot)
            .map_err(local_save_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_database_error)?;
        let mut device_statement = self
            .connection
            .prepare("SELECT device_id, slot_id FROM active_local_save_slots ORDER BY device_id")
            .map_err(local_save_database_error)?;
        let devices = device_statement
            .query_map([], |row| {
                Ok(LocalSaveDevice {
                    device_id: row.get(0)?,
                    active_slot_id: row.get(1)?,
                })
            })
            .map_err(local_save_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_database_error)?;
        Ok(LocalSaveState { slots, devices })
    }
    // //// /列出本地存档槽和设备选择 ////

    // //// 列出玩家被授予的存档槽和设备选择 [@x380kkm 2026-07-24] ////
    pub(crate) fn list_local_saves_for_account(
        &self,
        account_id: i64,
    ) -> Result<LocalSaveState, PersonalServiceError> {
        let mut slot_statement = self
            .connection
            .prepare(
                "SELECT slots.id, slots.name, slots.created_at, slots.updated_at,
                        (SELECT COUNT(*) FROM local_save_snapshots
                         WHERE local_save_snapshots.slot_id = slots.id)
                 FROM local_save_slots AS slots
                 LEFT JOIN local_save_access AS access
                   ON access.slot_id = slots.id AND access.account_id = ?1
                 WHERE slots.account_id = ?1 OR access.account_id IS NOT NULL
                 ORDER BY slots.updated_at DESC, slots.id",
            )
            .map_err(local_save_database_error)?;
        let slots = slot_statement
            .query_map(params![account_id], read_slot)
            .map_err(local_save_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_database_error)?;
        let mut device_statement = self
            .connection
            .prepare(
                "SELECT active.device_id, active.slot_id
                 FROM active_local_save_slots AS active
                 JOIN local_save_access AS access ON access.slot_id = active.slot_id
                 WHERE access.account_id = ?1
                 ORDER BY active.device_id",
            )
            .map_err(local_save_database_error)?;
        let devices = device_statement
            .query_map(params![account_id], |row| {
                Ok(LocalSaveDevice {
                    device_id: row.get(0)?,
                    active_slot_id: row.get(1)?,
                })
            })
            .map_err(local_save_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_database_error)?;
        Ok(LocalSaveState { slots, devices })
    }
    // //// /列出玩家被授予的存档槽和设备选择 ////

    // //// 读取本地存档槽的账号和当前 viewer 上下文 [@x380kkm 2026-08-18] ////
    pub(crate) fn local_save_context(
        &self,
        slot_id: i64,
    ) -> Result<Option<LocalSaveContext>, PersonalServiceError> {
        let context = self
            .connection
            .query_row(
                "SELECT slots.id, slots.name, slots.created_at, slots.updated_at,
                        (SELECT COUNT(*) FROM local_save_snapshots
                         WHERE local_save_snapshots.slot_id = slots.id),
                        slots.account_id,
                        (SELECT sessions.token FROM sessions
                         WHERE sessions.account_id = slots.account_id AND sessions.type = 2
                         LIMIT 1)
                 FROM local_save_slots AS slots
                 WHERE slots.id = ?1",
                params![slot_id],
                |row| {
                    let viewer_token = row.get::<_, Option<String>>(6)?;
                    Ok((
                        LocalSaveSlot {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            created_at: row.get(2)?,
                            updated_at: row.get(3)?,
                            snapshot_count: row.get(4)?,
                        },
                        row.get(5)?,
                        viewer_token
                            .and_then(|token| token.parse::<i64>().ok().filter(|value| *value > 0)),
                    ))
                },
            )
            .optional()
            .map_err(local_save_database_error)?;
        let Some((slot, account_id, viewer_id)) = context else {
            return Ok(None);
        };
        let mut statement = self
            .connection
            .prepare(
                "SELECT device_id FROM active_local_save_slots
                 WHERE slot_id = ?1 ORDER BY device_id",
            )
            .map_err(local_save_database_error)?;
        let active_device_ids = statement
            .query_map(params![slot_id], |row| row.get(0))
            .map_err(local_save_database_error)?
            .collect::<Result<Vec<i64>, _>>()
            .map_err(local_save_database_error)?;
        Ok(Some(LocalSaveContext {
            slot,
            account_id,
            viewer_id,
            active_device_ids,
        }))
    }
    // //// /读取本地存档槽的账号和当前 viewer 上下文 ////

    // //// 查询账号壳拥有的本地存档槽 [@x380kkm 2026-07-27] ////
    pub(crate) fn list_owned_local_saves(
        &self,
        account_id: i64,
    ) -> Result<Vec<LocalSaveSlot>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT slots.id, slots.name, slots.created_at, slots.updated_at,
                        (SELECT COUNT(*) FROM local_save_snapshots
                         WHERE local_save_snapshots.slot_id = slots.id)
                 FROM local_save_slots AS slots
                 JOIN local_save_access AS access ON access.slot_id = slots.id
                 WHERE access.account_id = ?1
                 ORDER BY slots.updated_at DESC, slots.id",
            )
            .map_err(local_save_database_error)?;
        let slots = statement
            .query_map(params![account_id], read_slot)
            .map_err(local_save_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_database_error)?;
        Ok(slots)
    }

    pub(crate) fn local_save_is_owned_by_account(
        &self,
        account_id: i64,
        slot_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM local_save_slots AS slots
                     LEFT JOIN local_save_access AS access
                       ON access.slot_id = slots.id AND access.account_id = ?1
                     WHERE slots.id = ?2
                       AND (slots.account_id = ?1 OR access.account_id IS NOT NULL)
                 )",
                params![account_id, slot_id],
                |row| row.get(0),
            )
            .map_err(local_save_database_error)
    }

    pub(crate) fn local_save_has_active_single_quest(
        &self,
        slot_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM active_single_quests
                     JOIN local_save_slots
                       ON local_save_slots.account_id = active_single_quests.account_id
                     WHERE local_save_slots.id = ?1
                 )",
                params![slot_id],
                |row| row.get(0),
            )
            .map_err(local_save_database_error)
    }
    // //// /查询账号壳拥有的本地存档槽 ////

    // //// 导出本地存档槽当前状态 [@x380kkm 2026-07-23] ////
    pub(crate) fn export_local_save(
        &mut self,
        slot_id: i64,
    ) -> Result<Option<LocalSaveExport>, PersonalServiceError> {
        let revision = match self.ensure_local_save_revision(slot_id, "Export") {
            Ok(revision) => revision,
            Err(LocalSaveStoreError::NotFound) => return Ok(None),
            Err(LocalSaveStoreError::Busy) => {
                return Err(PersonalServiceError::new(
                    "local save is busy with an active single battle",
                ));
            }
            Err(LocalSaveStoreError::InvalidState) => {
                return Err(PersonalServiceError::new(
                    "local save revision state is invalid",
                ));
            }
            Err(LocalSaveStoreError::Storage(error)) => return Err(error),
        };
        self.connection
            .query_row(
                "SELECT slots.id, slots.name, slots.created_at, slots.updated_at,
                        (SELECT COUNT(*) FROM local_save_snapshots
                         WHERE local_save_snapshots.slot_id = slots.id),
                        player_snapshots.data_json
                 FROM local_save_slots AS slots
                 JOIN player_snapshots ON player_snapshots.account_id = slots.account_id
                 WHERE slots.id = ?1",
                params![slot_id],
                |row| {
                    Ok(LocalSaveExport {
                        slot: read_slot(row)?,
                        data_json: row.get(5)?,
                        revision_id: revision.id.clone(),
                        etag: revision.etag.clone(),
                    })
                },
            )
            .optional()
            .map_err(local_save_database_error)
    }
    // //// /导出本地存档槽当前状态 ////

    pub(crate) fn export_local_save_for_account(
        &mut self,
        account_id: i64,
        slot_id: i64,
    ) -> Result<Option<LocalSaveExport>, PersonalServiceError> {
        let can_access = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_access WHERE account_id = ?1 AND slot_id = ?2)",
                params![account_id, slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(local_save_database_error)?;
        if !can_access {
            return Ok(None);
        }
        let revision = match self.ensure_local_save_revision(slot_id, "Export") {
            Ok(revision) => revision,
            Err(LocalSaveStoreError::NotFound) => return Ok(None),
            Err(LocalSaveStoreError::Busy) => {
                return Err(PersonalServiceError::new(
                    "local save is busy with an active single battle",
                ));
            }
            Err(LocalSaveStoreError::InvalidState) => {
                return Err(PersonalServiceError::new(
                    "local save revision state is invalid",
                ));
            }
            Err(LocalSaveStoreError::Storage(error)) => return Err(error),
        };
        self.connection
            .query_row(
                "SELECT slots.id, slots.name, slots.created_at, slots.updated_at,
                        (SELECT COUNT(*) FROM local_save_snapshots
                         WHERE local_save_snapshots.slot_id = slots.id),
                        player_snapshots.data_json
                 FROM local_save_slots AS slots
                 JOIN local_save_access AS access ON access.slot_id = slots.id
                 JOIN player_snapshots ON player_snapshots.account_id = slots.account_id
                 WHERE access.account_id = ?1 AND slots.id = ?2",
                params![account_id, slot_id],
                |row| {
                    Ok(LocalSaveExport {
                        slot: read_slot(row)?,
                        data_json: row.get(5)?,
                        revision_id: revision.id.clone(),
                        etag: revision.etag.clone(),
                    })
                },
            )
            .optional()
            .map_err(local_save_database_error)
    }

    // //// 列出本地存档槽历史快照 [@x380kkm 2026-07-23] ////
    pub(crate) fn list_local_save_snapshots(
        &self,
        slot_id: i64,
    ) -> Result<Option<Vec<LocalSaveSnapshot>>, PersonalServiceError> {
        if !local_save_slot_exists(&self.connection, slot_id)? {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, slot_id, label, created_at
                 FROM local_save_snapshots WHERE slot_id = ?1
                 ORDER BY created_at DESC, id DESC",
            )
            .map_err(local_save_database_error)?;
        let snapshots = statement
            .query_map(params![slot_id], read_snapshot)
            .map_err(local_save_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(local_save_database_error)?;
        Ok(Some(snapshots))
    }
    // //// /列出本地存档槽历史快照 ////

    // //// 为设备切换当前本地存档槽 [@x380kkm 2026-07-23] ////
    pub(crate) fn activate_local_save(
        &mut self,
        slot_id: i64,
        device_id: i64,
    ) -> Result<(), LocalSaveStoreError> {
        if device_id <= 0 {
            return Err(LocalSaveStoreError::InvalidState);
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let slot_exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(local_save_storage_error)?;
        if !slot_exists {
            return Err(LocalSaveStoreError::NotFound);
        }
        ensure_device_can_switch_local_save(&transaction, device_id, slot_id)?;
        let updated = transaction
            .execute(
                "INSERT INTO active_local_save_slots (device_id, slot_id)
                 VALUES (?1, ?2)
                 ON CONFLICT(device_id) DO UPDATE SET slot_id = excluded.slot_id",
                params![device_id, slot_id],
            )
            .map_err(local_save_storage_error)?;
        if updated == 1 {
            transaction.commit().map_err(local_save_storage_error)
        } else {
            Err(LocalSaveStoreError::NotFound)
        }
    }
    // //// /为设备切换当前本地存档槽 ////

    pub(crate) fn activate_local_save_for_account(
        &mut self,
        account_id: i64,
        slot_id: i64,
        device_id: i64,
    ) -> Result<(), LocalSaveStoreError> {
        if device_id <= 0 {
            return Err(LocalSaveStoreError::InvalidState);
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let can_access = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM local_save_access
                     WHERE account_id = ?1 AND slot_id = ?2
                 )",
                params![account_id, slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(local_save_storage_error)?;
        if !can_access {
            return Err(LocalSaveStoreError::NotFound);
        }
        ensure_device_can_switch_local_save(&transaction, device_id, slot_id)?;
        let updated = transaction
            .execute(
                "INSERT INTO active_local_save_slots (device_id, slot_id)
                 VALUES (?1, ?2)
                 ON CONFLICT(device_id) DO UPDATE SET slot_id = excluded.slot_id",
                params![device_id, slot_id],
            )
            .map_err(local_save_storage_error)?;
        if updated == 1 {
            transaction.commit().map_err(local_save_storage_error)
        } else {
            Err(LocalSaveStoreError::NotFound)
        }
    }
}

pub(super) fn grant_local_save_access_in_connection(
    connection: &Connection,
    account_id: i64,
    slot_id: i64,
) -> Result<(), LocalSaveStoreError> {
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO local_save_access (account_id, slot_id)
             SELECT ?1, id FROM local_save_slots WHERE id = ?2",
            params![account_id, slot_id],
        )
        .map_err(local_save_storage_error)?;
    if inserted == 1
        || connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_access WHERE account_id = ?1 AND slot_id = ?2)",
                params![account_id, slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(local_save_storage_error)?
    {
        Ok(())
    } else {
        Err(LocalSaveStoreError::NotFound)
    }
}

// //// 拒绝在单人战斗期间切换设备存档槽 [@x380kkm 2026-08-18] ////
fn ensure_device_can_switch_local_save(
    transaction: &Transaction<'_>,
    device_id: i64,
    target_slot_id: i64,
) -> Result<(), LocalSaveStoreError> {
    let current = transaction
        .query_row(
            "SELECT active.slot_id,
                    EXISTS(
                        SELECT 1 FROM active_single_quests
                        JOIN local_save_slots
                          ON local_save_slots.account_id = active_single_quests.account_id
                        WHERE local_save_slots.id = active.slot_id
                    )
             FROM active_local_save_slots AS active
             WHERE active.device_id = ?1",
            params![device_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()
        .map_err(local_save_storage_error)?;
    if let Some((current_slot_id, has_active_single_quest)) = current {
        if current_slot_id != target_slot_id && has_active_single_quest {
            return Err(LocalSaveStoreError::Busy);
        }
    }
    Ok(())
}
// //// /拒绝在单人战斗期间切换设备存档槽 ////

pub(super) fn active_local_save_account_id(
    transaction: &Transaction<'_>,
    device_id: i64,
) -> rusqlite::Result<Option<i64>> {
    transaction
        .query_row(
            "SELECT slots.account_id
             FROM active_local_save_slots AS active
             JOIN local_save_slots AS slots ON slots.id = active.slot_id
             WHERE active.device_id = ?1",
            params![device_id],
            |row| row.get(0),
        )
        .optional()
}

pub(super) fn ensure_local_save_slot(
    transaction: &Transaction<'_>,
    account_id: i64,
    device_id: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT OR IGNORE INTO local_save_slots (
             account_id, name, created_at, updated_at
         )
         SELECT id, printf('Save %d', id), reg_time, last_login_time
         FROM accounts WHERE id = ?1",
        params![account_id],
    )?;
    transaction.execute(
        "INSERT INTO active_local_save_slots (device_id, slot_id)
         SELECT ?1, id FROM local_save_slots WHERE account_id = ?2
         ON CONFLICT(device_id) DO UPDATE SET slot_id = excluded.slot_id",
        params![device_id, account_id],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO local_save_access (account_id, slot_id)
         SELECT ?1, id FROM local_save_slots WHERE account_id = ?1",
        params![account_id],
    )?;
    Ok(())
}

pub(super) fn read_slot(row: &Row<'_>) -> rusqlite::Result<LocalSaveSlot> {
    Ok(LocalSaveSlot {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        snapshot_count: row.get(4)?,
    })
}

pub(super) fn read_snapshot(row: &Row<'_>) -> rusqlite::Result<LocalSaveSnapshot> {
    Ok(LocalSaveSnapshot {
        id: row.get(0)?,
        slot_id: row.get(1)?,
        label: row.get(2)?,
        created_at: row.get(3)?,
    })
}

pub(super) fn local_save_slot_exists(
    connection: &Connection,
    slot_id: i64,
) -> Result<bool, PersonalServiceError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
            params![slot_id],
            |row| row.get(0),
        )
        .map_err(local_save_database_error)
}

pub(super) fn local_save_database_error(error: rusqlite::Error) -> PersonalServiceError {
    PersonalServiceError::new(format!("failed to access local save storage: {error}"))
}

pub(super) fn local_save_storage_error(error: rusqlite::Error) -> LocalSaveStoreError {
    LocalSaveStoreError::Storage(local_save_database_error(error))
}
