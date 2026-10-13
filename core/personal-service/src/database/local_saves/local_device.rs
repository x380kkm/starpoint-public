// audience: internal
// # local-save-device
// 本机默认槽绑定服务实例, 客户端设备标识在首次注册时继承该选择.

use super::{ensure_device_can_switch_local_save, local_save_storage_error, LocalSaveStoreError};
use crate::database::ServiceDatabase;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

// //// 为唯一的未绑定本地存档初始化默认槽 [@x380kkm 2026-09-26] ////
pub(super) fn initialize_single_local_save_default(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO local_save_default (id, slot_id)
         SELECT 1, slots.id FROM local_save_slots AS slots
         JOIN accounts ON accounts.id = slots.account_id
         WHERE accounts.app_id = 'wf_cn' AND accounts.idp_code = 'local'
           AND (SELECT COUNT(*) FROM local_save_slots) = 1
           AND NOT EXISTS(SELECT 1 FROM active_local_save_slots)
         ON CONFLICT(id) DO NOTHING",
        [],
    )?;
    Ok(())
}

pub(super) fn local_slot_id(connection: &Connection) -> rusqlite::Result<Option<i64>> {
    connection
        .query_row(
            "SELECT slot_id FROM local_save_default WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
}

// //// 为本机设置默认存档并同步已知设备 [@x380kkm 2026-09-26] ////
impl ServiceDatabase {
    pub(crate) fn activate_local_default_save(
        &mut self,
        slot_id: i64,
    ) -> Result<(), LocalSaveStoreError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_save_slots WHERE id = ?1)",
                params![slot_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(local_save_storage_error)?;
        if !exists {
            return Err(LocalSaveStoreError::NotFound);
        }
        let devices = {
            let mut statement = transaction
                .prepare("SELECT device_id FROM active_local_save_slots")
                .map_err(local_save_storage_error)?;
            let devices = statement
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(local_save_storage_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(local_save_storage_error)?;
            devices
        };
        for device_id in devices {
            ensure_device_can_switch_local_save(&transaction, device_id, slot_id)?;
        }
        transaction
            .execute(
                "UPDATE active_local_save_slots SET slot_id = ?1",
                params![slot_id],
            )
            .map_err(local_save_storage_error)?;
        transaction.execute(
            "INSERT INTO local_save_default (id, slot_id) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET slot_id = excluded.slot_id",
            params![slot_id],
        ).map_err(local_save_storage_error)?;
        transaction.commit().map_err(local_save_storage_error)
    }
}

// //// 按设备选择和本机默认槽解析注册账号 [@x380kkm 2026-09-26] ////
pub(in crate::database) fn selected_account_id(
    transaction: &Transaction<'_>,
    device_id: i64,
) -> rusqlite::Result<Option<i64>> {
    let selected = super::active_local_save_account_id(transaction, device_id)?;
    if selected.is_some() {
        return Ok(selected);
    }
    transaction.query_row(
        "SELECT slots.account_id FROM local_save_default AS selection JOIN local_save_slots AS slots ON slots.id = selection.slot_id WHERE selection.id = 1",
        [], |row| row.get(0),
    ).optional()
}
