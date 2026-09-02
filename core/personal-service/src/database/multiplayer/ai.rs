// audience: internal
// # personal-service-multiplayer-ai
//
// 该模块解析当前存档槽并冻结房间使用的 AI 队伍快照.

use super::{
    multiplayer_database_error, MultiplayerAiMate, MultiplayerAiMateInput, MAX_ROOM_MEMBERS,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

impl ServiceDatabase {
    pub(crate) fn multiplayer_current_slot_id(
        &self,
        account_id: i64,
    ) -> Result<Option<i64>, PersonalServiceError> {
        self.connection
            .query_row(
                "SELECT slots.id
                 FROM local_save_slots AS slots
                 JOIN local_save_access AS access
                   ON access.slot_id = slots.id AND access.account_id = ?1
                 JOIN accounts ON accounts.id = ?1
                 LEFT JOIN active_local_save_slots AS active
                   ON active.slot_id = slots.id
                  AND active.device_id = CAST(substr(accounts.idp_id, 4) AS INTEGER)
                 ORDER BY active.slot_id IS NULL, slots.updated_at DESC, slots.id
                 LIMIT 1",
                params![account_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(multiplayer_database_error)
    }

    pub(crate) fn stage_multiplayer_ai_mates(
        &mut self,
        room_number: &str,
        owner_account_id: i64,
        inputs: &[MultiplayerAiMateInput],
    ) -> Result<Option<Vec<MultiplayerAiMate>>, PersonalServiceError> {
        if inputs.len() > 2 {
            return Err(PersonalServiceError::new(
                "multiplayer AI mate count exceeds room capacity",
            ));
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let owns_room = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM multiplayer_rooms
                     WHERE room_number = ?1 AND host_account_id = ?2
                 )",
                params![room_number, owner_account_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(multiplayer_database_error)?;
        if !owns_room {
            return Ok(None);
        }
        let prepared_member_count = transaction
            .query_row(
                "SELECT COUNT(*) FROM multiplayer_room_members WHERE room_number = ?1",
                params![room_number],
                |row| row.get::<_, i64>(0),
            )
            .map_err(multiplayer_database_error)?;
        let available = (MAX_ROOM_MEMBERS - prepared_member_count).clamp(0, 2);
        if inputs.len() as i64 > available {
            return Err(PersonalServiceError::new(
                "multiplayer AI mate count exceeds prepared room capacity",
            ));
        }
        transaction
            .execute(
                "DELETE FROM multiplayer_ai_mates WHERE room_number = ?1",
                params![room_number],
            )
            .map_err(multiplayer_database_error)?;
        for (index, input) in inputs.iter().enumerate() {
            let owns_snapshot = transaction
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1
                         FROM ai_team_snapshot_heads AS heads
                         JOIN ai_team_snapshots AS snapshots
                           ON snapshots.id = heads.snapshot_id
                          AND snapshots.slot_id = heads.slot_id
                          AND snapshots.team_index = heads.team_index
                         JOIN local_save_access AS access
                           ON access.slot_id = snapshots.slot_id
                         WHERE snapshots.id = ?1 AND access.account_id = ?2
                     )",
                    params![input.snapshot_id, owner_account_id],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(multiplayer_database_error)?;
            if !owns_snapshot {
                return Err(PersonalServiceError::new(
                    "multiplayer AI snapshot does not belong to the room owner",
                ));
            }
            transaction
                .execute(
                    "INSERT INTO multiplayer_ai_mates (
                         room_number, position, owner_account_id, snapshot_id,
                         client_mate_json, lobby_player_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        room_number,
                        index as i64 + 1,
                        owner_account_id,
                        input.snapshot_id,
                        input.client_mate_json,
                        input.lobby_player_json,
                    ],
                )
                .map_err(multiplayer_database_error)?;
        }
        transaction
            .execute(
                "UPDATE multiplayer_rooms SET is_npc_mode = ?1
                 WHERE room_number = ?2",
                params![!inputs.is_empty(), room_number],
            )
            .map_err(multiplayer_database_error)?;
        transaction.commit().map_err(multiplayer_database_error)?;
        self.list_multiplayer_ai_mates(room_number).map(Some)
    }

    // //// 按已占位真人裁剪房间 AI [@x380kkm 2026-08-23] ////
    pub(crate) fn trim_multiplayer_ai_mates_to_capacity(
        &mut self,
        room_number: &str,
    ) -> Result<Vec<MultiplayerAiMate>, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let prepared_member_count = transaction
            .query_row(
                "SELECT COUNT(*) FROM multiplayer_room_members WHERE room_number = ?1",
                params![room_number],
                |row| row.get::<_, i64>(0),
            )
            .map_err(multiplayer_database_error)?;
        let available = (MAX_ROOM_MEMBERS - prepared_member_count).clamp(0, 2);
        transaction
            .execute(
                "DELETE FROM multiplayer_ai_mates
                 WHERE room_number = ?1 AND position > ?2",
                params![room_number, available],
            )
            .map_err(multiplayer_database_error)?;
        transaction
            .execute(
                "UPDATE multiplayer_rooms
                 SET is_npc_mode = EXISTS(
                     SELECT 1 FROM multiplayer_ai_mates WHERE room_number = ?1
                 )
                 WHERE room_number = ?1",
                params![room_number],
            )
            .map_err(multiplayer_database_error)?;
        transaction.commit().map_err(multiplayer_database_error)?;
        self.list_multiplayer_ai_mates(room_number)
    }
    // //// /按已占位真人裁剪房间 AI ////

    pub(crate) fn list_multiplayer_ai_mates(
        &self,
        room_number: &str,
    ) -> Result<Vec<MultiplayerAiMate>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT room_number, position, owner_account_id, snapshot_id,
                        client_mate_json, lobby_player_json
                 FROM multiplayer_ai_mates
                 WHERE room_number = ?1 ORDER BY position",
            )
            .map_err(multiplayer_database_error)?;
        let mates = statement
            .query_map(params![room_number], |row| {
                Ok(MultiplayerAiMate {
                    room_number: row.get(0)?,
                    position: row.get(1)?,
                    owner_account_id: row.get(2)?,
                    snapshot_id: row.get(3)?,
                    client_mate_json: row.get(4)?,
                    lobby_player_json: row.get(5)?,
                })
            })
            .map_err(multiplayer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(multiplayer_database_error)?;
        Ok(mates)
    }

    pub(crate) fn name_multiplayer_ai_mates(
        &mut self,
        room_number: &str,
        owner_account_id: i64,
        names: &[String],
    ) -> Result<bool, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let owns_room = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM multiplayer_rooms
                     WHERE room_number = ?1 AND host_account_id = ?2
                 )",
                params![room_number, owner_account_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(multiplayer_database_error)?;
        if !owns_room {
            return Ok(false);
        }
        let mut statement = transaction
            .prepare(
                "SELECT position, lobby_player_json FROM multiplayer_ai_mates
                 WHERE room_number = ?1 ORDER BY position",
            )
            .map_err(multiplayer_database_error)?;
        let players = statement
            .query_map(params![room_number], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(multiplayer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(multiplayer_database_error)?;
        drop(statement);
        if players.len() != names.len() {
            return Ok(false);
        }
        for ((position, serialized), name) in players.into_iter().zip(names) {
            let mut player = serde_json::from_str::<Value>(&serialized).map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to decode multiplayer AI lobby player: {error}"
                ))
            })?;
            player
                .as_object_mut()
                .ok_or_else(|| {
                    PersonalServiceError::new("multiplayer AI lobby player is not an object")
                })?
                .insert("name".to_owned(), Value::String(name.clone()));
            let encoded = serde_json::to_string(&player).map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to encode multiplayer AI lobby player: {error}"
                ))
            })?;
            transaction
                .execute(
                    "UPDATE multiplayer_ai_mates SET lobby_player_json = ?1
                     WHERE room_number = ?2 AND position = ?3",
                    params![encoded, room_number, position],
                )
                .map_err(multiplayer_database_error)?;
        }
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(true)
    }
}
