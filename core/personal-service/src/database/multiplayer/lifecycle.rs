// audience: internal
// # personal-service-multiplayer-lifecycle
//
// 该模块提交房间发布, 大厅开始, 战斗开始, 结束, 终止和解散状态.
// 非战斗房间按真实墙钟锚点产生剩余时间和解散事件.

use super::{
    multiplayer_database_error, MultiplayerRoomEvent, MultiplayerRoomEventKind, MAX_ROOM_MEMBERS,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, OptionalExtension, Transaction};

const INCOMPLETE_ROOM_LIFETIME_MS: i64 = 15 * 60 * 1_000;
const FULL_ROOM_LIFETIME_MS: i64 = 30 * 60 * 1_000;
const REMAINING_NOTICE_MS: i64 = 30 * 1_000;

struct RoomExpiryCandidate {
    room_number: String,
    room_sequence: i64,
    expiry_anchor_ms: i64,
    member_count: i64,
}

impl ServiceDatabase {
    // //// 清理到期房间并提取会话通知 [@x380kkm 2026-08-23] ////
    pub(crate) fn poll_multiplayer_room_events(
        &mut self,
        wall_time_ms: i64,
    ) -> Result<Vec<MultiplayerRoomEvent>, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let mut events = read_queued_dismissals(&transaction)?;
        transaction
            .execute("DELETE FROM multiplayer_room_dismissals", [])
            .map_err(multiplayer_database_error)?;

        for candidate in read_expiry_candidates(&transaction)? {
            let lifetime_ms = if candidate.member_count < MAX_ROOM_MEMBERS {
                INCOMPLETE_ROOM_LIFETIME_MS
            } else {
                FULL_ROOM_LIFETIME_MS
            };
            let deadline_ms = candidate
                .expiry_anchor_ms
                .checked_add(lifetime_ms)
                .ok_or_else(|| PersonalServiceError::new("multiplayer room deadline overflow"))?;
            let remaining_ms = deadline_ms.saturating_sub(wall_time_ms);
            if remaining_ms <= 0 {
                delete_room_state(&transaction, &candidate.room_number)?;
                events.push(MultiplayerRoomEvent {
                    room_number: candidate.room_number,
                    room_sequence: candidate.room_sequence,
                    kind: MultiplayerRoomEventKind::Dismissed,
                });
                continue;
            }
            if remaining_ms > REMAINING_NOTICE_MS {
                continue;
            }
            let notified_deadline = transaction
                .query_row(
                    "SELECT deadline_ms FROM multiplayer_room_expiry_notices
                     WHERE room_number = ?1 AND room_sequence = ?2",
                    params![candidate.room_number, candidate.room_sequence],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(multiplayer_database_error)?;
            if notified_deadline == Some(deadline_ms) {
                continue;
            }
            events.push(MultiplayerRoomEvent {
                room_number: candidate.room_number,
                room_sequence: candidate.room_sequence,
                kind: MultiplayerRoomEventKind::Remaining {
                    seconds: (remaining_ms + 999) / 1_000,
                    deadline_ms,
                },
            });
        }
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(events)
    }
    // //// /清理到期房间并提取会话通知 ////

    // //// 确认房间剩余时间已投递 [@x380kkm 2026-08-23] ////
    pub(crate) fn mark_multiplayer_remaining_notified(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        deadline_ms: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "INSERT INTO multiplayer_room_expiry_notices (
                     room_number, room_sequence, deadline_ms
                 )
                 SELECT room_number, room_sequence, ?1
                 FROM multiplayer_rooms
                 WHERE room_number = ?2 AND room_sequence = ?3
                   AND battle_started = 0 AND raising_state != 4
                 ON CONFLICT(room_number) DO UPDATE SET
                     room_sequence = excluded.room_sequence,
                     deadline_ms = excluded.deadline_ms",
                params![deadline_ms, room_number, room_sequence],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }
    // //// /确认房间剩余时间已投递 ////

    pub(crate) fn set_multiplayer_lobby_started(
        &mut self,
        room_number: &str,
        host_account_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_rooms SET lobby_started = 1
                 WHERE room_number = ?1 AND host_account_id = ?2",
                params![room_number, host_account_id],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }

    pub(crate) fn update_multiplayer_host_entry_time(
        &mut self,
        room_number: &str,
        host_account_id: i64,
        host_entry_time: i64,
        expiry_anchor_ms: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_rooms
                 SET host_entry_time = ?1, expiry_anchor_ms = ?2
                 WHERE room_number = ?3 AND host_account_id = ?4",
                params![
                    host_entry_time,
                    expiry_anchor_ms,
                    room_number,
                    host_account_id
                ],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }

    #[cfg(test)]
    pub(crate) fn finish_multiplayer_room_if_complete(
        &mut self,
        room_number: &str,
        expiry_anchor_ms: i64,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_rooms
                 SET raising_state = 1, battle_started = 0, lobby_started = 0,
                     play_id = NULL, expiry_anchor_ms = ?2
                 WHERE room_number = ?1
                   AND NOT EXISTS (
                       SELECT 1 FROM multiplayer_battle_players AS battle_players
                       WHERE battle_players.room_number = multiplayer_rooms.room_number
                   )",
                params![room_number, expiry_anchor_ms],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }

    pub(crate) fn disband_multiplayer_room(
        &mut self,
        room_number: &str,
        host_account_id: i64,
    ) -> Result<bool, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let room_sequence = transaction
            .query_row(
                "SELECT room_sequence FROM multiplayer_rooms
                 WHERE room_number = ?1 AND host_account_id = ?2",
                params![room_number, host_account_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(multiplayer_database_error)?;
        let Some(room_sequence) = room_sequence else {
            return Ok(false);
        };
        transaction
            .execute(
                "INSERT INTO multiplayer_room_dismissals (room_number, room_sequence)
                 VALUES (?1, ?2)",
                params![room_number, room_sequence],
            )
            .map_err(multiplayer_database_error)?;
        delete_room_state(&transaction, room_number)?;
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(true)
    }
}

// //// 读取房间解散事件 [@x380kkm 2026-08-23] ////
fn read_queued_dismissals(
    transaction: &Transaction<'_>,
) -> Result<Vec<MultiplayerRoomEvent>, PersonalServiceError> {
    let mut statement = transaction
        .prepare(
            "SELECT room_number, room_sequence
             FROM multiplayer_room_dismissals ORDER BY id",
        )
        .map_err(multiplayer_database_error)?;
    let events = statement
        .query_map([], |row| {
            Ok(MultiplayerRoomEvent {
                room_number: row.get(0)?,
                room_sequence: row.get(1)?,
                kind: MultiplayerRoomEventKind::Dismissed,
            })
        })
        .map_err(multiplayer_database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(multiplayer_database_error)?;
    Ok(events)
}
// //// /读取房间解散事件 ////

// //// 读取非战斗房间的到期依据 [@x380kkm 2026-08-23] ////
fn read_expiry_candidates(
    transaction: &Transaction<'_>,
) -> Result<Vec<RoomExpiryCandidate>, PersonalServiceError> {
    let mut statement = transaction
        .prepare(
            "SELECT rooms.room_number, rooms.room_sequence, rooms.expiry_anchor_ms,
                    (SELECT COUNT(*) FROM multiplayer_room_members AS members
                     WHERE members.room_number = rooms.room_number AND members.entered = 1)
                      + (SELECT COUNT(*) FROM multiplayer_ai_mates AS mates
                         WHERE mates.room_number = rooms.room_number)
             FROM multiplayer_rooms AS rooms
             WHERE rooms.battle_started = 0 AND rooms.raising_state != 4",
        )
        .map_err(multiplayer_database_error)?;
    let candidates = statement
        .query_map([], |row| {
            Ok(RoomExpiryCandidate {
                room_number: row.get(0)?,
                room_sequence: row.get(1)?,
                expiry_anchor_ms: row.get(2)?,
                member_count: row.get(3)?,
            })
        })
        .map_err(multiplayer_database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(multiplayer_database_error)?;
    Ok(candidates)
}
// //// /读取非战斗房间的到期依据 ////

// //// 删除房间关联的战斗状态 [@x380kkm 2026-08-23] ////
fn delete_room_state(
    transaction: &Transaction<'_>,
    room_number: &str,
) -> Result<(), PersonalServiceError> {
    transaction
        .execute(
            "DELETE FROM active_single_quests
             WHERE EXISTS (
                 SELECT 1 FROM multiplayer_battle_players AS battle_players
                 WHERE battle_players.room_number = ?1
                   AND battle_players.account_id = active_single_quests.account_id
                   AND battle_players.play_id = active_single_quests.play_id
             )",
            params![room_number],
        )
        .map_err(multiplayer_database_error)?;
    transaction
        .execute(
            "DELETE FROM multiplayer_rooms WHERE room_number = ?1",
            params![room_number],
        )
        .map_err(multiplayer_database_error)?;
    Ok(())
}
// //// /删除房间关联的战斗状态 ////
