// audience: internal
// # personal-service-multiplayer-members
//
// 该模块读取并更新本地联机真人成员状态.

use super::{multiplayer_database_error, MultiplayerMember};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use rusqlite::{params, Connection, OptionalExtension};

impl ServiceDatabase {
    pub(crate) fn multiplayer_member(
        &self,
        room_number: &str,
        viewer_id: i64,
    ) -> Result<Option<MultiplayerMember>, PersonalServiceError> {
        self.connection
            .query_row(
                &format!(
                    "{} WHERE room_number = ?1 AND viewer_id = ?2",
                    member_select()
                ),
                params![room_number, viewer_id],
                read_member,
            )
            .optional()
            .map_err(multiplayer_database_error)
    }

    pub(crate) fn multiplayer_member_by_connection(
        &self,
        room_number: &str,
        connection_id: &str,
    ) -> Result<Option<MultiplayerMember>, PersonalServiceError> {
        self.connection
            .query_row(
                &format!(
                    "{} WHERE room_number = ?1 AND connection_id = ?2",
                    member_select()
                ),
                params![room_number, connection_id],
                read_member,
            )
            .optional()
            .map_err(multiplayer_database_error)
    }

    pub(crate) fn list_multiplayer_members(
        &self,
        room_number: &str,
    ) -> Result<Vec<MultiplayerMember>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "{} WHERE room_number = ?1 ORDER BY member_index",
                member_select()
            ))
            .map_err(multiplayer_database_error)?;
        let members = statement
            .query_map(params![room_number], read_member)
            .map_err(multiplayer_database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(multiplayer_database_error)?;
        Ok(members)
    }

    pub(crate) fn set_multiplayer_member_connection(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        connection_id: &str,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_room_members
                 SET connection_id = ?1, lobby_player_json = NULL, entered = 0,
                     ready = 0, autoplay = 0, auto_start = 0, scene_ready = 0
                 WHERE room_number = ?2 AND viewer_id = ?3",
                params![connection_id, room_number, viewer_id],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }

    pub(crate) fn enter_multiplayer_lobby(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        lobby_player_json: &str,
    ) -> Result<bool, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let updated = transaction
            .execute(
                "UPDATE multiplayer_room_members
                 SET lobby_player_json = ?1, entered = 1
                 WHERE room_number = ?2 AND viewer_id = ?3",
                params![lobby_player_json, room_number, viewer_id],
            )
            .map_err(multiplayer_database_error)?;
        if updated == 1 {
            transaction
                .execute(
                    "UPDATE multiplayer_rooms SET raising_state = 1
                     WHERE room_number = ?1 AND host_viewer_id = ?2",
                    params![room_number, viewer_id],
                )
                .map_err(multiplayer_database_error)?;
        }
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(updated == 1)
    }

    pub(crate) fn change_multiplayer_member_party(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        party_id: i64,
        main_character_id: i64,
        lobby_player_json: &str,
    ) -> Result<bool, PersonalServiceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(multiplayer_database_error)?;
        let updated = transaction
            .execute(
                "UPDATE multiplayer_room_members
                 SET party_id = ?1, lobby_player_json = ?2
                 WHERE room_number = ?3 AND viewer_id = ?4 AND entered = 1",
                params![party_id, lobby_player_json, room_number, viewer_id],
            )
            .map_err(multiplayer_database_error)?;
        if updated == 1 {
            transaction
                .execute(
                    "UPDATE multiplayer_rooms
                     SET host_party_id = ?1, host_main_character_id = ?2
                     WHERE room_number = ?3 AND host_viewer_id = ?4",
                    params![party_id, main_character_id, room_number, viewer_id],
                )
                .map_err(multiplayer_database_error)?;
        }
        transaction.commit().map_err(multiplayer_database_error)?;
        Ok(updated == 1)
    }

    pub(crate) fn leave_multiplayer_lobby(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        connection_id: &str,
    ) -> Result<bool, PersonalServiceError> {
        self.connection
            .execute(
                "UPDATE multiplayer_room_members
                 SET lobby_player_json = NULL, entered = 0, ready = 0,
                     autoplay = 0, auto_start = 0, scene_ready = 0
                 WHERE room_number = ?1 AND viewer_id = ?2 AND connection_id = ?3",
                params![room_number, viewer_id, connection_id],
            )
            .map(|updated| updated == 1)
            .map_err(multiplayer_database_error)
    }

    pub(crate) fn set_multiplayer_member_ready(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        ready: bool,
    ) -> Result<bool, PersonalServiceError> {
        update_member_bool(&self.connection, room_number, viewer_id, "ready", ready)
    }

    pub(crate) fn set_multiplayer_member_autoplay(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        autoplay: bool,
    ) -> Result<bool, PersonalServiceError> {
        update_member_bool(
            &self.connection,
            room_number,
            viewer_id,
            "autoplay",
            autoplay,
        )
    }

    pub(crate) fn set_multiplayer_member_auto_start(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        auto_start: bool,
    ) -> Result<bool, PersonalServiceError> {
        update_member_bool(
            &self.connection,
            room_number,
            viewer_id,
            "auto_start",
            auto_start,
        )
    }

    pub(crate) fn set_multiplayer_member_scene_ready(
        &mut self,
        room_number: &str,
        viewer_id: i64,
        scene_ready: bool,
    ) -> Result<bool, PersonalServiceError> {
        update_member_bool(
            &self.connection,
            room_number,
            viewer_id,
            "scene_ready",
            scene_ready,
        )
    }
}

fn member_select() -> &'static str {
    "SELECT account_id, viewer_id, party_id, connection_id,
            lobby_player_json, entered, ready
     FROM multiplayer_room_members"
}

fn read_member(row: &rusqlite::Row<'_>) -> rusqlite::Result<MultiplayerMember> {
    Ok(MultiplayerMember {
        account_id: row.get(0)?,
        viewer_id: row.get(1)?,
        party_id: row.get(2)?,
        connection_id: row.get(3)?,
        lobby_player_json: row.get(4)?,
        entered: row.get(5)?,
        ready: row.get(6)?,
    })
}

fn update_member_bool(
    connection: &Connection,
    room_number: &str,
    viewer_id: i64,
    field: &str,
    value: bool,
) -> Result<bool, PersonalServiceError> {
    if !matches!(field, "ready" | "autoplay" | "auto_start" | "scene_ready") {
        return Err(PersonalServiceError::new(
            "invalid multiplayer member boolean field",
        ));
    }
    connection
        .execute(
            &format!(
                "UPDATE multiplayer_room_members SET {field} = ?1
                 WHERE room_number = ?2 AND viewer_id = ?3"
            ),
            params![value, room_number, viewer_id],
        )
        .map(|updated| updated == 1)
        .map_err(multiplayer_database_error)
}
