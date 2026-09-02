// audience: internal
// # personal-service-cn-multiplayer-meeting
//
// 该模块校验大厅与战斗握手并处理大厅成员状态.

use super::lobby_player;
use super::transport::{
    all_human_members_ready, discard_pending_writes, lobby_room, lobby_roster, meeting_command,
    queue_frame, random_connection_id, value_i64, value_string,
};
use super::{MultiplayerSessionListener, SessionState};
use crate::database::{ServiceDatabase, ViewerSessionPlayer};
use crate::PersonalServiceError;
use serde_json::{json, Value};

impl MultiplayerSessionListener {
    pub(super) fn handle_handshake(
        &mut self,
        client_index: usize,
        frame: Value,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        let Some(frame) = frame.as_object() else {
            return self.deny(client_index, "DENIED");
        };
        match frame.get("socklet").and_then(Value::as_str) {
            Some("cooperation_room") => {
                let Some(viewer_id) = value_i64(frame, &["viewerId", "viewer_id"]) else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                let Some(room_number) = value_string(frame, &["roomNumber", "room_number"]) else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                if !lobby_player::is_room_number(&room_number)
                    || frame.get("reconnected").and_then(Value::as_i64).is_none()
                {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                }
                let Some(category_id) = value_i64(frame, &["questCategory", "quest_category"])
                else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                let Some(quest_id) = value_i64(frame, &["questId", "quest_id"]) else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                let room = database.multiplayer_room(&room_number)?;
                let member = database.multiplayer_member(&room_number, viewer_id)?;
                let (Some(room), Some(member)) = (room, member) else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                if room.category_id != category_id || room.quest_id != quest_id {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                }
                if room.host_viewer_id == viewer_id && member.party_id != room.host_party_id {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                }
                let account_id = match database.lookup_viewer_session_player(viewer_id)? {
                    ViewerSessionPlayer::Present(snapshot)
                        if snapshot.account_id == member.account_id =>
                    {
                        snapshot.account_id
                    }
                    _ => return self.deny(client_index, "HANDSHAKE_DENIED"),
                };
                let connection_id = random_connection_id()?;
                self.disconnect_previous_viewer_sessions(client_index, &room_number, viewer_id);
                if !database.set_multiplayer_member_connection(
                    &room_number,
                    viewer_id,
                    &connection_id,
                )? {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                }
                self.clients[client_index].state = SessionState::Lobby {
                    room_number: room_number.clone(),
                    room_sequence: room.room_sequence,
                    viewer_id,
                    account_id,
                    connection_id: connection_id.clone(),
                    is_host: room.host_account_id == account_id,
                    legacy_protocol: true,
                };
                queue_frame(
                    &mut self.clients[client_index],
                    &json!([0, connection_id, room_number]),
                )?;
                Ok(())
            }
            Some("cooperation_battle") => {
                let Some(room_number) = value_string(frame, &["roomNumber", "room_number"]) else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                let Some(connection_id) = value_string(frame, &["connectionId", "connection_id"])
                else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                if !lobby_player::is_room_number(&room_number)
                    || !lobby_player::is_connection_id(&connection_id)
                    || frame.get("reconnected").and_then(Value::as_i64).is_none()
                {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                }
                let room = database.multiplayer_room(&room_number)?;
                let member =
                    database.multiplayer_member_by_connection(&room_number, &connection_id)?;
                let (Some(room), Some(member)) = (room, member) else {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                };
                if !room.battle_started || !room.lobby_started {
                    return self.deny(client_index, "HANDSHAKE_DENIED");
                }
                self.disconnect_previous_battle_session(client_index, &room_number, &connection_id);
                database.restore_multiplayer_battle_expected_viewer(
                    &room_number,
                    room.room_sequence,
                    member.viewer_id,
                )?;
                database.set_multiplayer_member_scene_ready(
                    &room_number,
                    member.viewer_id,
                    false,
                )?;
                self.clients[client_index].state = SessionState::Battle {
                    room_number: room_number.clone(),
                    room_sequence: room.room_sequence,
                    viewer_id: member.viewer_id,
                    connection_id,
                    scene_ready: false,
                    finalized: false,
                };
                queue_frame(
                    &mut self.clients[client_index],
                    &json!([0, room_number, ""]),
                )?;
                Ok(())
            }
            _ => self.deny(client_index, "DENIED"),
        }
    }

    pub(super) fn handle_lobby(
        &mut self,
        client_index: usize,
        frame: Value,
        database: &mut ServiceDatabase,
    ) -> Result<(), PersonalServiceError> {
        if frame
            .as_array()
            .filter(|data| {
                data.len() == 2
                    && data.first().and_then(Value::as_i64) == Some(1)
                    && data[1].is_array()
            })
            .is_some()
        {
            let (room_number, room_sequence) = match &self.clients[client_index].state {
                SessionState::Lobby {
                    room_number,
                    room_sequence,
                    ..
                } => (room_number.clone(), *room_sequence),
                _ => return Ok(()),
            };
            let room_is_current = database
                .multiplayer_room(&room_number)?
                .is_some_and(|room| room.room_sequence == room_sequence);
            if !room_is_current {
                return self.close_client(client_index);
            }
            self.broadcast_lobby(&room_number, room_sequence, &frame)?;
            return Ok(());
        }
        if frame
            .as_array()
            .filter(|data| {
                data.len() == 3
                    && data.first().and_then(Value::as_i64) == Some(2)
                    && data[1].as_i64().is_some_and(|viewer_id| viewer_id > 0)
            })
            .is_some()
        {
            let (room_number, room_sequence) = match &self.clients[client_index].state {
                SessionState::Lobby {
                    room_number,
                    room_sequence,
                    ..
                } => (room_number.clone(), *room_sequence),
                _ => return Ok(()),
            };
            let room_is_current = database
                .multiplayer_room(&room_number)?
                .is_some_and(|room| room.room_sequence == room_sequence);
            if !room_is_current {
                return self.close_client(client_index);
            }
            let target_viewer_id = frame[1]
                .as_i64()
                .expect("lobby send target viewer is validated");
            self.send_to_lobby_viewer(&room_number, room_sequence, target_viewer_id, &frame)?;
            return Ok(());
        }
        let Some(command) = meeting_command(&frame) else {
            return self.close_client(client_index);
        };
        let (
            room_number,
            room_sequence,
            viewer_id,
            account_id,
            connection_id,
            is_host,
            legacy_protocol,
        ) = match &self.clients[client_index].state {
            SessionState::Lobby {
                room_number,
                room_sequence,
                viewer_id,
                account_id,
                connection_id,
                is_host,
                legacy_protocol,
            } => (
                room_number.clone(),
                *room_sequence,
                *viewer_id,
                *account_id,
                connection_id.clone(),
                *is_host,
                *legacy_protocol,
            ),
            _ => return Ok(()),
        };
        let room = match database.multiplayer_room(&room_number)? {
            Some(room) if room.room_sequence == room_sequence => room,
            _ => return self.close_client(client_index),
        };
        match command.first().and_then(Value::as_i64) {
            Some(0) => {
                let Some(player) = lobby_player::normalize_lobby_player(
                    database,
                    command,
                    &room,
                    viewer_id,
                    &connection_id,
                    is_host,
                )?
                else {
                    return self.close_client(client_index);
                };
                let had_existing_players = database
                    .list_multiplayer_members(&room_number)?
                    .iter()
                    .any(|member| member.entered && member.viewer_id != viewer_id);
                let player_json = serde_json::to_string(&player).map_err(|error| {
                    PersonalServiceError::new(format!(
                        "failed to encode multiplayer lobby player: {error}"
                    ))
                })?;
                if !database.enter_multiplayer_lobby(&room_number, viewer_id, &player_json)? {
                    return self.close_client(client_index);
                }
                let ready = player
                    .get("state")
                    .and_then(Value::as_array)
                    .and_then(|state| state.first())
                    .and_then(Value::as_i64)
                    == Some(1);
                database.set_multiplayer_member_ready(&room_number, viewer_id, ready)?;
                let include_ai = !database
                    .trim_multiplayer_ai_mates_to_capacity(&room_number)?
                    .is_empty();
                let roster = lobby_roster(database, &room_number, include_ai, false)?;
                let room_data = lobby_room(&room, &connection_id, &roster);
                let welcome_context = if legacy_protocol {
                    player.clone()
                } else {
                    room_data
                };
                let welcome_roster = if legacy_protocol {
                    vec![player.clone()]
                } else {
                    roster.clone()
                };
                queue_frame(
                    &mut self.clients[client_index],
                    &json!([1, [0, welcome_context, welcome_roster]]),
                )?;
                if had_existing_players {
                    self.broadcast_lobby(&room_number, room_sequence, &json!([1, [1, roster]]))?;
                }
                if ready {
                    self.broadcast_lobby(
                        &room_number,
                        room_sequence,
                        &json!([1, [2, connection_id, [1]]]),
                    )?;
                }
                if room.lobby_started {
                    let roster = lobby_roster(database, &room_number, room.is_npc_mode, true)?;
                    queue_frame(&mut self.clients[client_index], &json!([1, [5, roster]]))?;
                }
            }
            Some(1) => {
                if command.len() != 1 {
                    return self.close_client(client_index);
                }
                let roster = if room.lobby_started {
                    lobby_roster(database, &room_number, room.is_npc_mode, true)?
                } else {
                    Vec::new()
                };
                queue_frame(&mut self.clients[client_index], &json!([1, [1, roster]]))?;
                self.clients[client_index].close_after_write = true;
            }
            Some(2) => {
                if command.len() != 4 || command[2].as_bool().is_none() {
                    return self.close_client(client_index);
                }
                let Some(party_id) = command[3].as_i64().filter(|party_id| *party_id > 0) else {
                    return self.close_client(client_index);
                };
                let Some(player) = lobby_player::normalize_changed_lobby_player(
                    database,
                    &command[1],
                    &room,
                    viewer_id,
                    &connection_id,
                    is_host,
                    party_id,
                )?
                else {
                    return self.close_client(client_index);
                };
                let Some(main_character_id) = player
                    .get("mainCharacterId")
                    .and_then(Value::as_i64)
                    .filter(|main_character_id| *main_character_id > 0)
                else {
                    return self.close_client(client_index);
                };
                let player_json = serde_json::to_string(&player).map_err(|error| {
                    PersonalServiceError::new(format!(
                        "failed to encode changed multiplayer lobby player: {error}"
                    ))
                })?;
                if !database.change_multiplayer_member_party(
                    &room_number,
                    viewer_id,
                    party_id,
                    main_character_id,
                    &player_json,
                )? {
                    return self.close_client(client_index);
                }
                let roster = lobby_roster(database, &room_number, room.is_npc_mode, false)?;
                self.broadcast_lobby(&room_number, room_sequence, &json!([1, [1, roster]]))?;
            }
            Some(3) => {
                if command.len() != 2
                    || command[1].as_array().map_or(true, |state| {
                        state.len() != 1 || !matches!(state[0].as_i64(), Some(0 | 1))
                    })
                {
                    return self.close_client(client_index);
                }
                let ready = command
                    .get(1)
                    .and_then(Value::as_array)
                    .and_then(|state| state.first())
                    .and_then(Value::as_i64)
                    == Some(1);
                database.set_multiplayer_member_ready(&room_number, viewer_id, ready)?;
                self.broadcast_lobby(
                    &room_number,
                    room_sequence,
                    &json!([1, [2, connection_id, [i64::from(ready)]]]),
                )?;
                self.evaluate_lobby_readiness(database, &room_number, room_sequence)?;
            }
            Some(4) => {
                if command.len() != 1 {
                    return self.close_client(client_index);
                }
                queue_frame(
                    &mut self.clients[client_index],
                    &json!([1, [if legacy_protocol { 11 } else { 10 }, connection_id]]),
                )?;
            }
            Some(5) => {
                if command.len() != 1 {
                    return self.close_client(client_index);
                }
                database.set_multiplayer_member_ready(&room_number, viewer_id, false)?;
                self.broadcast_lobby(
                    &room_number,
                    room_sequence,
                    &json!([1, [2, connection_id, [0]]]),
                )?;
            }
            Some(6) => {
                if command.len() != 1 || !is_host {
                    return self.close_client(client_index);
                }
                self.auto_starting_rooms
                    .remove(&(room_number.clone(), room_sequence));
                if room.lobby_started {
                    let roster = lobby_roster(database, &room_number, room.is_npc_mode, true)?;
                    self.broadcast_lobby(&room_number, room_sequence, &json!([1, [5, roster]]))?;
                    return Ok(());
                }
                if !all_human_members_ready(database, &room_number)? {
                    return Ok(());
                }
                let expected_viewers = database
                    .list_multiplayer_members(&room_number)?
                    .into_iter()
                    .filter(|member| member.entered)
                    .map(|member| member.viewer_id)
                    .collect::<std::collections::BTreeSet<_>>();
                if !database.stage_multiplayer_battle_expected_viewers(
                    &room_number,
                    room_sequence,
                    account_id,
                    &expected_viewers,
                )? {
                    return self.close_client(client_index);
                }
                if !database.set_multiplayer_lobby_started(&room_number, account_id)? {
                    return self.close_client(client_index);
                }
                let roster = lobby_roster(database, &room_number, room.is_npc_mode, true)?;
                self.broadcast_lobby(&room_number, room_sequence, &json!([1, [5, roster]]))?;
            }
            Some(7) => {
                if command.len() != 3 {
                    return self.close_client(client_index);
                }
                let Some(autoplay) = command.get(1).and_then(Value::as_bool) else {
                    return self.close_client(client_index);
                };
                let Some(speed_up) = command.get(2).and_then(Value::as_bool) else {
                    return self.close_client(client_index);
                };
                database.set_multiplayer_member_autoplay(&room_number, viewer_id, autoplay)?;
                let mut updates = vec![("autoplayMode", Value::Bool(autoplay))];
                if speed_up {
                    updates.push(("autoSpeedLevel", Value::from(1)));
                }
                lobby_player::update_lobby_player_modes(
                    database,
                    &room_number,
                    viewer_id,
                    &updates,
                )?;
                self.broadcast_lobby(
                    &room_number,
                    room_sequence,
                    &json!([1, [3, connection_id, autoplay, speed_up]]),
                )?;
            }
            Some(8) => {
                if command.len() != 2 {
                    return self.close_client(client_index);
                }
                let Some(auto_start) = command.get(1).and_then(Value::as_bool) else {
                    return self.close_client(client_index);
                };
                database.set_multiplayer_member_auto_start(&room_number, viewer_id, auto_start)?;
                lobby_player::update_lobby_player_modes(
                    database,
                    &room_number,
                    viewer_id,
                    &[("autoStart", Value::Bool(auto_start))],
                )?;
                self.broadcast_lobby(
                    &room_number,
                    room_sequence,
                    &json!([1, [4, connection_id, auto_start]]),
                )?;
            }
            Some(9) => {}
            Some(10) => {
                if command.len() != 2 || !is_host {
                    return self.close_client(client_index);
                }
                let names = if legacy_protocol {
                    lobby_player::validate_ai_names(database, &room_number, &command[1])?
                } else {
                    lobby_player::validate_ai_requests(database, &room_number, &command[1])?
                };
                let Some(names) = names else {
                    return self.close_client(client_index);
                };
                if !database.name_multiplayer_ai_mates(&room_number, account_id, &names)? {
                    return self.close_client(client_index);
                }
                if names.is_empty() {
                    self.evaluate_lobby_readiness(database, &room_number, room_sequence)?;
                } else {
                    self.schedule_npc_lobby_sequence(room_number.clone(), room_sequence);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn disconnect_previous_viewer_sessions(
        &mut self,
        current_index: usize,
        room_number: &str,
        viewer_id: i64,
    ) {
        for (index, client) in self.clients.iter_mut().enumerate() {
            if index == current_index {
                continue;
            }
            let same_viewer = matches!(
                &client.state,
                SessionState::Lobby {
                    room_number: client_room,
                    viewer_id: client_viewer,
                    ..
                } | SessionState::Battle {
                    room_number: client_room,
                    viewer_id: client_viewer,
                    ..
                } if client_room == room_number && *client_viewer == viewer_id
            );
            if same_viewer {
                let _ = client.stream.shutdown(std::net::Shutdown::Both);
                discard_pending_writes(client);
                client.peer_closed = true;
            }
        }
    }

    fn disconnect_previous_battle_session(
        &mut self,
        current_index: usize,
        room_number: &str,
        connection_id: &str,
    ) {
        for (index, client) in self.clients.iter_mut().enumerate() {
            if index == current_index {
                continue;
            }
            let supersedes = matches!(
                &client.state,
                SessionState::Battle {
                    room_number: client_room,
                    connection_id: client_connection,
                    ..
                } if client_room == room_number && client_connection == connection_id
            );
            if supersedes {
                let _ = client.stream.shutdown(std::net::Shutdown::Both);
                discard_pending_writes(client);
                client.peer_closed = true;
            }
        }
    }

    fn deny(&mut self, client_index: usize, reason: &str) -> Result<(), PersonalServiceError> {
        let tag = if reason == "HANDSHAKE_DENIED" { 3 } else { 1 };
        queue_frame(&mut self.clients[client_index], &json!([tag, reason]))?;
        self.clients[client_index].close_after_write = true;
        Ok(())
    }

    pub(super) fn broadcast_lobby(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        frame: &Value,
    ) -> Result<(), PersonalServiceError> {
        for client in &mut self.clients {
            if matches!(
                &client.state,
                SessionState::Lobby {
                    room_number: client_room,
                    room_sequence: client_sequence,
                    ..
                } if client_room == room_number && *client_sequence == room_sequence
            ) && !client.peer_closed
            {
                queue_frame(client, frame)?;
            }
        }
        Ok(())
    }

    pub(super) fn send_to_lobby_viewer(
        &mut self,
        room_number: &str,
        room_sequence: i64,
        viewer_id: i64,
        frame: &Value,
    ) -> Result<(), PersonalServiceError> {
        for client in &mut self.clients {
            if matches!(
                &client.state,
                SessionState::Lobby {
                    room_number: client_room,
                    room_sequence: client_sequence,
                    viewer_id: client_viewer,
                    ..
                } if client_room == room_number
                    && *client_sequence == room_sequence
                    && *client_viewer == viewer_id
            ) && !client.peer_closed
            {
                queue_frame(client, frame)?;
                break;
            }
        }
        Ok(())
    }
}
