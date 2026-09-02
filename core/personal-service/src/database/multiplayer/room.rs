// audience: internal
// # personal-service-multiplayer-rooms
//
// 该模块创建, 查询和加入持久化本地联机房间.

use super::{multiplayer_database_error, MultiplayerRoom, MAX_ROOM_MEMBERS};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

impl ServiceDatabase {
    // //// 创建持久化本地联机房间 [@x380kkm 2026-08-22] ////
    pub(crate) fn create_multiplayer_room(
        &mut self,
        host_account_id: i64,
        host_viewer_id: i64,
        host_party_id: i64,
        host_main_character_id: i64,
        category_id: i64,
        quest_id: i64,
        created_at_ms: i64,
        expiry_anchor_ms: i64,
    ) -> Result<MultiplayerRoom, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let host_player_id = transaction
            .query_row(
                "SELECT id FROM players WHERE account_id = ?1",
                params![host_account_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(multiplayer_database_error)?;
        transaction
            .execute(
                "DELETE FROM multiplayer_rooms WHERE host_account_id = ?1",
                params![host_account_id],
            )
            .map_err(multiplayer_database_error)?;
        let (room_number, sequence) = loop {
            let sequence = allocate_room_sequence(&transaction)?;
            let room_number = format!("{:06}", 100_000 + sequence.rem_euclid(900_000));
            let exists = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM multiplayer_rooms WHERE room_number = ?1)",
                    params![room_number],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(multiplayer_database_error)?;
            if !exists {
                break (room_number, sequence);
            }
        };
        let access_token = transaction
            .query_row("SELECT lower(hex(randomblob(16)))", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(multiplayer_database_error)?;
        let host_entry_time = created_at_ms / 1_000;
        transaction
            .execute(
                "INSERT INTO multiplayer_rooms (
                     room_number, room_sequence, access_token, host_account_id,
                     host_viewer_id, host_party_id, host_main_character_id,
                     category_id, quest_id, raising_state, created_at_ms,
                     host_entry_time, expiry_anchor_ms, share_room_options
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 2, ?10, ?11, ?12, 0)",
                params![
                    room_number,
                    sequence,
                    access_token,
                    host_account_id,
                    host_viewer_id,
                    host_party_id,
                    host_main_character_id,
                    category_id,
                    quest_id,
                    created_at_ms,
                    host_entry_time,
                    expiry_anchor_ms,
                ],
            )
            .map_err(multiplayer_database_error)?;
        transaction
            .execute(
                "INSERT INTO multiplayer_room_members (
                     room_number, account_id, viewer_id, party_id, member_index
                 ) VALUES (?1, ?2, ?3, ?4, 0)",
                params![room_number, host_account_id, host_viewer_id, host_party_id],
            )
            .map_err(multiplayer_database_error)?;
        transaction.commit().map_err(multiplayer_database_error)?;
        self.multiplayer_room(&room_number)?.ok_or_else(|| {
            PersonalServiceError::new(format!(
                "created multiplayer room {room_number} is missing for player {host_player_id}"
            ))
        })
    }
    // //// /创建持久化本地联机房间 ////

    pub(crate) fn multiplayer_room(
        &self,
        room_number: &str,
    ) -> Result<Option<MultiplayerRoom>, PersonalServiceError> {
        read_room_by(&self.connection, "rooms.room_number = ?1", room_number)
    }

    pub(crate) fn multiplayer_room_by_token(
        &self,
        access_token: &str,
    ) -> Result<Option<MultiplayerRoom>, PersonalServiceError> {
        read_room_by(&self.connection, "rooms.access_token = ?1", access_token)
    }

    pub(crate) fn list_multiplayer_rooms(
        &self,
        category_id: i64,
    ) -> Result<Vec<MultiplayerRoom>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "{} WHERE rooms.category_id = ?1 ORDER BY rooms.created_at_ms, rooms.room_number",
                room_select()
            ))
            .map_err(multiplayer_database_error)?;
        let rooms = statement
            .query_map(params![category_id], read_room)
            .map_err(multiplayer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(multiplayer_database_error)?;
        Ok(rooms)
    }

    pub(crate) fn join_multiplayer_room(
        &mut self,
        room_number: &str,
        account_id: i64,
        viewer_id: i64,
        party_id: i64,
    ) -> Result<Option<MultiplayerRoom>, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let room_exists = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM multiplayer_rooms
                     WHERE room_number = ?1 AND battle_started = 0
                 )",
                params![room_number],
                |row| row.get::<_, bool>(0),
            )
            .map_err(multiplayer_database_error)?;
        if !room_exists {
            return Ok(None);
        }
        let existing = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM multiplayer_room_members
                     WHERE room_number = ?1 AND viewer_id = ?2 AND account_id = ?3
                 )",
                params![room_number, viewer_id, account_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(multiplayer_database_error)?;
        if !existing {
            let human_count = transaction
                .query_row(
                    "SELECT COUNT(*) FROM multiplayer_room_members WHERE room_number = ?1",
                    params![room_number],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(multiplayer_database_error)?;
            let Some(member_index) = available_member_index(&transaction, room_number)? else {
                return Ok(None);
            };
            let ai_capacity = MAX_ROOM_MEMBERS - human_count - 1;
            transaction
                .execute(
                    "DELETE FROM multiplayer_ai_mates
                     WHERE room_number = ?1 AND position > ?2",
                    params![room_number, ai_capacity],
                )
                .map_err(multiplayer_database_error)?;
            transaction
                .execute(
                    "INSERT INTO multiplayer_room_members (
                         room_number, account_id, viewer_id, party_id, member_index
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![room_number, account_id, viewer_id, party_id, member_index],
                )
                .map_err(multiplayer_database_error)?;
        }
        transaction.commit().map_err(multiplayer_database_error)?;
        self.multiplayer_room(room_number)
    }
}

// //// 分配持久单调房间序号 [@x380kkm 2026-08-23] ////
fn allocate_room_sequence(transaction: &Transaction<'_>) -> Result<i64, PersonalServiceError> {
    transaction
        .query_row(
            "UPDATE multiplayer_room_sequence
             SET next_value = next_value + 1
             WHERE id = 1
             RETURNING next_value - 1",
            [],
            |row| row.get(0),
        )
        .map_err(multiplayer_database_error)
}
// //// /分配持久单调房间序号 ////

fn room_select() -> &'static str {
    "SELECT rooms.room_number, rooms.room_sequence, rooms.access_token,
            rooms.host_account_id, rooms.host_viewer_id,
            (SELECT id FROM players WHERE account_id = rooms.host_account_id),
            rooms.host_party_id, rooms.host_main_character_id, rooms.category_id,
            rooms.quest_id, rooms.raising_state, rooms.host_entry_time,
            rooms.share_room_options, rooms.battle_started,
            rooms.lobby_started, rooms.is_npc_mode,
            (SELECT COUNT(*) FROM multiplayer_room_members AS members
             WHERE members.room_number = rooms.room_number)
              + (SELECT COUNT(*) FROM multiplayer_ai_mates AS mates
                 WHERE mates.room_number = rooms.room_number)
     FROM multiplayer_rooms AS rooms"
}

fn read_room_by(
    connection: &Connection,
    predicate: &str,
    value: &str,
) -> Result<Option<MultiplayerRoom>, PersonalServiceError> {
    connection
        .query_row(
            &format!("{} WHERE {predicate}", room_select()),
            params![value],
            read_room,
        )
        .optional()
        .map_err(multiplayer_database_error)
}

fn read_room(row: &rusqlite::Row<'_>) -> rusqlite::Result<MultiplayerRoom> {
    Ok(MultiplayerRoom {
        room_number: row.get(0)?,
        room_sequence: row.get(1)?,
        access_token: row.get(2)?,
        host_account_id: row.get(3)?,
        host_viewer_id: row.get(4)?,
        host_player_id: row.get(5)?,
        host_party_id: row.get(6)?,
        host_main_character_id: row.get(7)?,
        category_id: row.get(8)?,
        quest_id: row.get(9)?,
        raising_state: row.get(10)?,
        host_entry_time: row.get(11)?,
        share_room_options: row.get(12)?,
        battle_started: row.get(13)?,
        lobby_started: row.get(14)?,
        is_npc_mode: row.get(15)?,
        member_count: row.get(16)?,
    })
}

fn available_member_index(
    transaction: &Transaction<'_>,
    room_number: &str,
) -> Result<Option<i64>, PersonalServiceError> {
    let mut statement = transaction
        .prepare(
            "SELECT member_index FROM multiplayer_room_members
             WHERE room_number = ?1 ORDER BY member_index",
        )
        .map_err(multiplayer_database_error)?;
    let occupied = statement
        .query_map(params![room_number], |row| row.get::<_, i64>(0))
        .map_err(multiplayer_database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(multiplayer_database_error)?;
    Ok((0..MAX_ROOM_MEMBERS).find(|member_index| !occupied.contains(member_index)))
}
