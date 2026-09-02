// audience: internal
// # personal-service-cn-multiplayer-lobby-player
//
// 该模块校验真人大厅载荷, COM 回传内容和握手标识.

use crate::database::{MultiplayerRoom, ServiceDatabase, ViewerSessionPlayer};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

// //// 归一化客户端真人大厅成员 [@x380kkm 2026-08-22] ////
pub(super) fn normalize_lobby_player(
    database: &ServiceDatabase,
    command: &[Value],
    room: &MultiplayerRoom,
    viewer_id: i64,
    connection_id: &str,
    is_host: bool,
) -> Result<Option<Value>, PersonalServiceError> {
    if command.len() == 2 {
        return normalize_legacy_lobby_player(
            database,
            &command[1],
            room,
            viewer_id,
            connection_id,
            is_host,
        );
    }
    if command.len() != 3 {
        return Ok(None);
    }
    let party_id = command[2].as_i64().unwrap_or_default();
    let member = database.multiplayer_member(&room.room_number, viewer_id)?;
    if party_id <= 0 || member.as_ref().map(|member| member.party_id) != Some(party_id) {
        return Ok(None);
    }
    normalize_modern_lobby_player(
        database,
        &command[1],
        room,
        viewer_id,
        connection_id,
        is_host,
        party_id,
    )
}

pub(super) fn normalize_changed_lobby_player(
    database: &ServiceDatabase,
    player: &Value,
    room: &MultiplayerRoom,
    viewer_id: i64,
    connection_id: &str,
    is_host: bool,
    party_id: i64,
) -> Result<Option<Value>, PersonalServiceError> {
    if party_id <= 0 {
        return Ok(None);
    }
    normalize_modern_lobby_player(
        database,
        player,
        room,
        viewer_id,
        connection_id,
        is_host,
        party_id,
    )
}

fn normalize_modern_lobby_player(
    database: &ServiceDatabase,
    player: &Value,
    room: &MultiplayerRoom,
    viewer_id: i64,
    connection_id: &str,
    is_host: bool,
    party_id: i64,
) -> Result<Option<Value>, PersonalServiceError> {
    let Some(player) = player.as_object() else {
        return Ok(None);
    };
    if player.get("viewerId").and_then(Value::as_i64) != Some(viewer_id)
        || player.get("connectionId").and_then(Value::as_str) != Some(connection_id)
        || player.get("currentPartyId").and_then(Value::as_i64) != Some(party_id)
        || player.get("comId").is_some_and(|value| !value.is_null())
        || player
            .get("entryTime")
            .and_then(Value::as_f64)
            .map_or(true, |value| !value.is_finite())
    {
        return Ok(None);
    }
    let Some(name) = player
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    else {
        return Ok(None);
    };
    let Some(rank) = positive_i64(player, "rank") else {
        return Ok(None);
    };
    let Some(degree_id) = positive_i64(player, "degreeId") else {
        return Ok(None);
    };
    let Some(player_role_kind) = positive_i64(player, "playerRoleKind") else {
        return Ok(None);
    };
    let Some(autoskill_mode) = nonnegative_i64(player, "autoskillMode") else {
        return Ok(None);
    };
    let Some(auto_speed_level) = nonnegative_i64(player, "autoSpeedLevel") else {
        return Ok(None);
    };
    let Some(skill_ability_behavior_mode) = nonnegative_i64(player, "skillAbilityBehaviorMode")
    else {
        return Ok(None);
    };
    let Some(dash_behavior_mode) = nonnegative_i64(player, "dashBehaviorMode") else {
        return Ok(None);
    };
    let (Some(is_newbie), Some(autoplay_mode), Some(auto_start), Some(allow_heal)) = (
        player.get("isNewbie").and_then(Value::as_bool),
        player.get("autoplayMode").and_then(Value::as_bool),
        player.get("autoStart").and_then(Value::as_bool),
        player
            .get("allowHealFromOtherPlayers")
            .and_then(Value::as_bool),
    ) else {
        return Ok(None);
    };
    let Some(state) = player.get("state").and_then(Value::as_array) else {
        return Ok(None);
    };
    if state.len() != 1 || !matches!(state[0].as_i64(), Some(0 | 1)) {
        return Ok(None);
    }
    let Some(party) = player.get("party").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(main_character_id) = lobby_main_character_id(party) else {
        return Ok(None);
    };
    let snapshot = match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => snapshot,
        _ => return Ok(None),
    };
    let stored_player = serde_json::from_str::<Value>(&snapshot.data).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to decode multiplayer party owner snapshot: {error}"
        ))
    })?;
    if crate::cn_multi::party_leader_id(&stored_player, party_id) != Some(main_character_id) {
        return Ok(None);
    }
    let player_id = next_player_id(database, &room.room_number, viewer_id)?;
    Ok(Some(json!({
        "viewerId": viewer_id,
        "playerId": player_id,
        "name": name,
        "rank": rank,
        "degreeId": degree_id,
        "mainCharacterId": main_character_id,
        "party": party,
        "connectionId": connection_id,
        "playerRoleKind": player_role_kind,
        "isNewbie": is_newbie,
        "isHost": is_host,
        "entryTime": database.current_server_time_millis()?,
        "currentPartyId": party_id,
        "autoplayMode": autoplay_mode,
        "autoskillMode": autoskill_mode,
        "autoSpeedLevel": auto_speed_level,
        "autoStart": auto_start,
        "skillAbilityBehaviorMode": skill_ability_behavior_mode,
        "dashBehaviorMode": dash_behavior_mode,
        "allowHealFromOtherPlayers": allow_heal,
        "state": state,
    })))
}
// //// /归一化客户端真人大厅成员 ////

fn normalize_legacy_lobby_player(
    database: &ServiceDatabase,
    enter_data: &Value,
    room: &MultiplayerRoom,
    viewer_id: i64,
    connection_id: &str,
    is_host: bool,
) -> Result<Option<Value>, PersonalServiceError> {
    let Some(enter_data) = enter_data.as_object() else {
        return Ok(None);
    };
    let Some(member) = database.multiplayer_member(&room.room_number, viewer_id)? else {
        return Ok(None);
    };
    let Some(party) = enter_data.get("party").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(main_character_id) = lobby_main_character_id(party) else {
        return Ok(None);
    };
    let snapshot = match database.lookup_viewer_session_player(viewer_id)? {
        ViewerSessionPlayer::Present(snapshot) => snapshot,
        _ => return Ok(None),
    };
    let stored_player = serde_json::from_str::<Value>(&snapshot.data).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to decode multiplayer party owner snapshot: {error}"
        ))
    })?;
    if crate::cn_multi::party_leader_id(&stored_player, member.party_id) != Some(main_character_id)
    {
        return Ok(None);
    }
    let Some(user_info) = stored_player.get("user_info").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(name) = user_info
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    else {
        return Ok(None);
    };
    let degree_id = user_info
        .get("degree_id")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .max(1);
    let state = enter_data
        .get("state")
        .and_then(Value::as_array)
        .filter(|state| state.len() == 1 && matches!(state[0].as_i64(), Some(0 | 1)))
        .cloned()
        .unwrap_or_else(|| vec![Value::from(0)]);
    Ok(Some(json!({
        "viewerId": viewer_id,
        "playerId": next_player_id(database, &room.room_number, viewer_id)?,
        "name": name,
        "rank": degree_id,
        "degreeId": degree_id,
        "mainCharacterId": main_character_id,
        "party": party,
        "connectionId": connection_id,
        "playerRoleKind": user_info.get("role").and_then(Value::as_i64).unwrap_or(1).max(1),
        "isNewbie": user_info.get("is_newbie").and_then(Value::as_bool).unwrap_or(false),
        "isHost": is_host,
        "entryTime": database.current_server_time_millis()?,
        "currentPartyId": member.party_id,
        "autoplayMode": enter_data.get("autoplayMode").and_then(Value::as_bool).unwrap_or(false),
        "autoskillMode": enter_data.get("autoskillMode").and_then(Value::as_i64).unwrap_or(1).max(0),
        "autoSpeedLevel": enter_data.get("autoSpeedLevel").and_then(Value::as_i64).unwrap_or(1).max(0),
        "autoStart": enter_data.get("autoStart").and_then(Value::as_bool).unwrap_or(false),
        "skillAbilityBehaviorMode": enter_data.get("skillAbilityBehaviorMode").and_then(Value::as_i64).unwrap_or(1).max(0),
        "dashBehaviorMode": enter_data.get("dashBehaviorMode").and_then(Value::as_i64).unwrap_or(1).max(0),
        "allowHealFromOtherPlayers": enter_data.get("allowHealFromOtherPlayers").and_then(Value::as_bool).unwrap_or(true),
        "state": state,
    })))
}

fn next_player_id(
    database: &ServiceDatabase,
    room_number: &str,
    viewer_id: i64,
) -> Result<i64, PersonalServiceError> {
    let used_player_ids = database
        .list_multiplayer_members(room_number)?
        .into_iter()
        .filter(|member| member.entered && member.viewer_id != viewer_id)
        .filter_map(|member| member.lobby_player_json)
        .filter_map(|serialized| serde_json::from_str::<Value>(&serialized).ok())
        .filter_map(|player| player.get("playerId").and_then(Value::as_i64))
        .collect::<BTreeSet<_>>();
    let mut player_id = 2;
    while used_player_ids.contains(&player_id) {
        player_id += 1;
    }
    Ok(player_id)
}

pub(super) fn update_lobby_player_modes(
    database: &mut ServiceDatabase,
    room_number: &str,
    viewer_id: i64,
    updates: &[(&str, Value)],
) -> Result<bool, PersonalServiceError> {
    let Some(member) = database.multiplayer_member(room_number, viewer_id)? else {
        return Ok(false);
    };
    let Some(serialized) = member.lobby_player_json else {
        return Ok(false);
    };
    let mut player = serde_json::from_str::<Value>(&serialized).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to decode multiplayer lobby player: {error}"
        ))
    })?;
    let player = player
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("multiplayer lobby player is not an object"))?;
    for (key, value) in updates {
        player.insert((*key).to_owned(), value.clone());
    }
    let encoded = serde_json::to_string(&player).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode multiplayer lobby player: {error}"
        ))
    })?;
    database.enter_multiplayer_lobby(room_number, viewer_id, &encoded)
}

pub(super) fn validate_ai_requests(
    database: &ServiceDatabase,
    room_number: &str,
    requests: &Value,
) -> Result<Option<Vec<String>>, PersonalServiceError> {
    let Some(requests) = requests.as_array() else {
        return Ok(None);
    };
    let mates = database.list_multiplayer_ai_mates(room_number)?;
    if requests.len() < mates.len() {
        return Ok(None);
    }
    let mut names = Vec::with_capacity(mates.len());
    for (request, mate) in requests.iter().take(mates.len()).zip(mates) {
        let Some(request) = request.as_object() else {
            return Ok(None);
        };
        if request.len() != 5 {
            return Ok(None);
        }
        let Some(name) = request
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            return Ok(None);
        };
        let lobby_player =
            serde_json::from_str::<Value>(&mate.lobby_player_json).map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to decode multiplayer AI lobby player: {error}"
                ))
            })?;
        if ["degreeId", "rank", "comId", "party"]
            .iter()
            .any(|key| request.get(*key) != lobby_player.get(*key))
        {
            return Ok(None);
        }
        names.push(name.to_owned());
    }
    Ok(Some(names))
}

pub(super) fn validate_ai_names(
    database: &ServiceDatabase,
    room_number: &str,
    requests: &Value,
) -> Result<Option<Vec<String>>, PersonalServiceError> {
    let Some(requests) = requests.as_array() else {
        return Ok(None);
    };
    let mate_count = database.list_multiplayer_ai_mates(room_number)?.len();
    if requests.len() < mate_count {
        return Ok(None);
    }
    let names = requests
        .iter()
        .take(mate_count)
        .map(|request| {
            request
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .collect::<Option<Vec<_>>>();
    Ok(names)
}

pub(super) fn is_connection_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn is_room_number(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn positive_i64(object: &Map<String, Value>, key: &str) -> Option<i64> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
}

fn nonnegative_i64(object: &Map<String, Value>, key: &str) -> Option<i64> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .filter(|value| *value >= 0)
}

fn lobby_main_character_id(party: &Map<String, Value>) -> Option<i64> {
    let first = party
        .get("characters")
        .and_then(Value::as_array)?
        .first()?
        .as_array()?;
    (first.len() == 2 && first[0].as_i64() == Some(0))
        .then(|| first[1].get("id").and_then(Value::as_i64))
        .flatten()
        .filter(|character_id| *character_id > 0)
}
