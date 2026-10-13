// audience: internal
// # local-save-deletion
//
// 删除事务同时移除槽位, 游戏账号及其本地备份. 设备活动槽和最后一个槽位保留.
// 删除顺序遵守 AI 快照, 版本父链和当前版本指针的外键约束.

use super::super::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, OptionalExtension, Transaction};

pub(crate) enum LocalSaveDeleteError {
    NotFound,
    ConfirmationMismatch,
    Active,
    LastSave,
    Busy,
    Storage(PersonalServiceError),
}

impl ServiceDatabase {
    // //// 按确认名称删除未使用的存档及关联数据 [@x380kkm 2026-09-20] ////
    pub(crate) fn delete_local_save(
        &mut self,
        slot_id: i64,
        confirmed_name: &str,
    ) -> Result<(), LocalSaveDeleteError> {
        let transaction = self.connection.transaction().map_err(storage_error)?;
        let (account_id, name) = transaction
            .query_row(
                "SELECT account_id, name FROM local_save_slots WHERE id = ?1",
                params![slot_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(LocalSaveDeleteError::NotFound)?;
        if name != confirmed_name {
            return Err(LocalSaveDeleteError::ConfirmationMismatch);
        }
        ensure_save_can_be_deleted(&transaction, slot_id, account_id)?;
        transaction
            .execute(
                "DELETE FROM ai_team_snapshot_heads WHERE slot_id = ?1",
                params![slot_id],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "DELETE FROM ai_team_snapshots WHERE slot_id = ?1",
                params![slot_id],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "DELETE FROM local_save_heads WHERE slot_id = ?1",
                params![slot_id],
            )
            .map_err(storage_error)?;
        delete_save_revisions(&transaction, slot_id)?;
        transaction
            .execute("DELETE FROM accounts WHERE id = ?1", params![account_id])
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }
}

// //// 验证设备选择和战斗引用处于可删除状态 [@x380kkm 2026-09-20] ////
fn ensure_save_can_be_deleted(
    transaction: &Transaction<'_>,
    slot_id: i64,
    account_id: i64,
) -> Result<(), LocalSaveDeleteError> {
    let (count, active, busy) = transaction
        .query_row(
            "SELECT (SELECT COUNT(*) FROM local_save_slots),
                    EXISTS(SELECT 1 FROM active_local_save_slots WHERE slot_id = ?1)
                    OR EXISTS(SELECT 1 FROM local_save_default WHERE slot_id = ?1),
                    EXISTS(SELECT 1 FROM active_single_quests WHERE account_id = ?2)
                    OR EXISTS(SELECT 1 FROM multiplayer_room_members WHERE account_id = ?2)
                    OR EXISTS(SELECT 1 FROM multiplayer_rooms WHERE host_account_id = ?2)
                    OR EXISTS(
                        SELECT 1 FROM multiplayer_ai_mates AS mates
                        JOIN ai_team_snapshots AS snapshots ON snapshots.id = mates.snapshot_id
                        WHERE snapshots.slot_id = ?1
                    )",
            params![slot_id, account_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .map_err(storage_error)?;
    if count <= 1 {
        return Err(LocalSaveDeleteError::LastSave);
    }
    if active {
        return Err(LocalSaveDeleteError::Active);
    }
    if busy {
        return Err(LocalSaveDeleteError::Busy);
    }
    Ok(())
}

// //// 从叶节点删除槽位版本树 [@x380kkm 2026-09-20] ////
fn delete_save_revisions(
    transaction: &Transaction<'_>,
    slot_id: i64,
) -> Result<(), LocalSaveDeleteError> {
    loop {
        let remaining = transaction
            .query_row(
                "SELECT COUNT(*) FROM local_save_revisions WHERE slot_id = ?1",
                params![slot_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(storage_error)?;
        if remaining == 0 {
            return Ok(());
        }
        let deleted = transaction
            .execute(
                "DELETE FROM local_save_revisions
                 WHERE slot_id = ?1 AND NOT EXISTS (
                     SELECT 1 FROM local_save_revisions AS children
                     WHERE children.parent_revision_id = local_save_revisions.id
                 )",
                params![slot_id],
            )
            .map_err(storage_error)?;
        if deleted == 0 {
            return Err(LocalSaveDeleteError::Storage(PersonalServiceError::new(
                "local save revisions contain a retained reference",
            )));
        }
    }
}

fn storage_error(error: rusqlite::Error) -> LocalSaveDeleteError {
    LocalSaveDeleteError::Storage(super::local_save_database_error(error))
}
