// audience: internal
// # local-save-occupancy
// 传输覆盖与设备切换检查单机战斗和房间占用, 历史恢复检查房间占用.

use super::{local_save_storage_error, LocalSaveStoreError};
use rusqlite::{params, Transaction};

// //// 在写入事务中检查存档的战斗和房间占用 [@x380kkm 2026-09-26] ////
pub(super) fn ensure_local_save_is_idle(
    transaction: &Transaction<'_>,
    slot_id: i64,
) -> Result<(), LocalSaveStoreError> {
    ensure_local_save_is_outside_room(transaction, slot_id)?;
    let is_busy = transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM local_save_slots AS slots
                 WHERE slots.id = ?1 AND
                     EXISTS(SELECT 1 FROM active_single_quests WHERE account_id = slots.account_id)
             )",
            params![slot_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(local_save_storage_error)?;
    if is_busy {
        Err(LocalSaveStoreError::Busy)
    } else {
        Ok(())
    }
}
// //// /在写入事务中检查存档的战斗和房间占用 ////

// //// 在写入事务中保护房主和成员使用的存档 [@x380kkm 2026-09-27] ////
pub(super) fn ensure_local_save_is_outside_room(
    transaction: &Transaction<'_>,
    slot_id: i64,
) -> Result<(), LocalSaveStoreError> {
    let is_busy = transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM local_save_slots AS slots
                 WHERE slots.id = ?1 AND (
                     EXISTS(SELECT 1 FROM multiplayer_rooms WHERE host_account_id = slots.account_id)
                     OR EXISTS(SELECT 1 FROM multiplayer_room_members WHERE account_id = slots.account_id)
                 )
             )",
            params![slot_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(local_save_storage_error)?;
    if is_busy {
        Err(LocalSaveStoreError::Busy)
    } else {
        Ok(())
    }
}
// //// /在写入事务中保护房主和成员使用的存档 ////
