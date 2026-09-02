// audience: internal
// # personal-service-multiplayer-battle-expected
//
// 该模块持久化进入联机战斗时等待的真人集合.

use super::multiplayer_database_error;
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::params;
use std::collections::BTreeSet;

impl ServiceDatabase {
    // //// 冻结大厅等待的真人集合 [@x380kkm 2026-08-23] ////
    pub(crate) fn stage_multiplayer_battle_expected_viewers(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        host_account_id: i64,
        expected_viewers: &BTreeSet<i64>,
    ) -> Result<bool, PersonalServiceError> {
        if expected_viewers.is_empty() {
            return Ok(false);
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let accepts_expected = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM multiplayer_rooms
                     WHERE room_number = ?1 AND room_sequence = ?2 AND host_account_id = ?3
                       AND lobby_started = 0 AND battle_started = 0
                 )",
                params![room_number, room_sequence, host_account_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(multiplayer_database_error)?;
        if !accepts_expected {
            return Ok(false);
        }
        transaction
            .execute(
                "DELETE FROM multiplayer_battle_expected_members
                 WHERE room_number = ?1",
                params![room_number],
            )
            .map_err(multiplayer_database_error)?;
        for viewer_id in expected_viewers {
            let inserted = transaction
                .execute(
                    "INSERT INTO multiplayer_battle_expected_members (
                         room_number, room_sequence, viewer_id, delivered, required
                     )
                     SELECT room_number, ?1, viewer_id, 0, 1
                     FROM multiplayer_room_members
                     WHERE room_number = ?2 AND viewer_id = ?3 AND entered = 1",
                    params![room_sequence, room_number, viewer_id],
                )
                .map_err(multiplayer_database_error)?;
            if inserted != 1 {
                return Ok(false);
            }
        }
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(true)
    }
    // //// /冻结大厅等待的真人集合 ////

    // //// 读取仍需进入战斗的真人集合 [@x380kkm 2026-08-23] ////
    pub(crate) fn multiplayer_battle_expected_viewers(
        &self,
        room_number: &str,
        room_sequence: i64,
    ) -> Result<BTreeSet<i64>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT viewer_id
                  FROM multiplayer_battle_expected_members
                  WHERE room_number = ?1 AND room_sequence = ?2 AND required = 1
                 ORDER BY viewer_id",
            )
            .map_err(multiplayer_database_error)?;
        let viewers = statement
            .query_map(params![room_number, room_sequence], |row| row.get(0))
            .map_err(multiplayer_database_error)?
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(multiplayer_database_error)?;
        Ok(viewers)
    }
    // //// /读取仍需进入战斗的真人集合 ////

    // //// 读取尚未收到战斗开始帧的真人集合 [@x380kkm 2026-08-23] ////
    pub(crate) fn multiplayer_battle_undelivered_viewers(
        &self,
        room_number: &str,
        room_sequence: i64,
    ) -> Result<BTreeSet<i64>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT viewer_id
                  FROM multiplayer_battle_expected_members
                  WHERE room_number = ?1 AND room_sequence = ?2
                    AND delivered = 0 AND required = 1
                 ORDER BY viewer_id",
            )
            .map_err(multiplayer_database_error)?;
        let viewers = statement
            .query_map(params![room_number, room_sequence], |row| row.get(0))
            .map_err(multiplayer_database_error)?
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(multiplayer_database_error)?;
        Ok(viewers)
    }
    // //// /读取尚未收到战斗开始帧的真人集合 ////

    // //// 确认真人已收到战斗开始帧 [@x380kkm 2026-08-23] ////
    pub(crate) fn mark_multiplayer_battle_start_delivered(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        viewer_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_battle_expected_members
                 SET delivered = 1
                 WHERE room_number = ?1 AND room_sequence = ?2 AND viewer_id = ?3
                   AND delivered = 0",
                params![room_number, room_sequence, viewer_id],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }
    // //// /确认真人已收到战斗开始帧 ////

    // //// 恢复重新进入战斗的真人等待状态 [@x380kkm 2026-08-23] ////
    pub(crate) fn restore_multiplayer_battle_expected_viewer(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        viewer_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "INSERT INTO multiplayer_battle_expected_members (
                     room_number, room_sequence, viewer_id, delivered, required
                 )
                 SELECT rooms.room_number, rooms.room_sequence, members.viewer_id, 0, 1
                 FROM multiplayer_rooms AS rooms
                 JOIN multiplayer_room_members AS members
                   ON members.room_number = rooms.room_number
                 WHERE rooms.room_number = ?1 AND rooms.room_sequence = ?2
                   AND rooms.battle_started = 1 AND members.viewer_id = ?3
                 ON CONFLICT(room_number, room_sequence, viewer_id)
                 DO UPDATE SET required = 1",
                params![room_number, room_sequence, viewer_id],
            )
            .map(|inserted| inserted == 1)
            .map_err(multiplayer_database_error)
    }
    // //// /恢复重新进入战斗的真人等待状态 ////

    // //// 暂停重连宽限后离线的战斗成员 [@x380kkm 2026-08-23] ////
    pub(crate) fn suspend_multiplayer_battle_expected_viewer(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        viewer_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_battle_expected_members
                 SET required = 0
                 WHERE room_number = ?1 AND room_sequence = ?2 AND viewer_id = ?3
                   AND required = 1",
                params![room_number, room_sequence, viewer_id],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }
    // //// /暂停重连宽限后离线的战斗成员 ////
}
