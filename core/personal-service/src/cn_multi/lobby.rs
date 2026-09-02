// audience: internal
// # personal-service-cn-multi-lobby
//
// 该模块实现 CN 本地联机房间创建, 列表, 搜索和选择.

use super::{
    authenticate, bad_request, error_response, missing_room_connection, msgpack_response,
    party_leader_id, room_connection,
};
use crate::cn::decode_request;
use crate::cn_battle_assets::load_battle_fixture;
use crate::database::{MultiplayerRoom, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct CreateRoomRequest {
    category: i64,
    party_id: i64,
    quest_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct GetRoomsRequest {
    category_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct SearchRoomRequest {
    room_number: String,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct SelectRoomRequest {
    access_token: Option<String>,
    category: i64,
    party_id: i64,
    quest_id: i64,
    room_number: Option<String>,
    viewer_id: i64,
}

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/multi_battle_quest/create_room" => create_room(request, database),
        "/api/index.php/multi_battle_quest/get_rooms" => get_rooms(request, database),
        "/api/index.php/multi_battle_quest/search_room" => search_room(request, database),
        "/api/index.php/multi_battle_quest/select_room" => select_room(request, database),
        _ => return None,
    };
    Some(response)
}

// //// 创建 CN 本地联机房间 [@x380kkm 2026-08-22] ////
fn create_room(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<CreateRoomRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.category > 0
                && body.quest_id > 0
                && body.party_id > 0 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_room")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let Some(leader_id) = party_leader_id(&player.data, body.party_id) else {
        return Ok(error_response("400 Bad Request", "party_not_found"));
    };
    if !load_battle_fixture()?
        .quests
        .contains_key(&format!("{}:{}", body.category, body.quest_id))
    {
        return Ok(error_response("400 Bad Request", "quest_not_found"));
    }
    let created_at_ms = database.current_server_time_millis()?;
    let expiry_anchor_ms = database.current_wall_time_millis()?;
    let room = database.create_multiplayer_room(
        player.account_id,
        player.viewer_id,
        body.party_id,
        leader_id,
        body.category,
        body.quest_id,
        created_at_ms,
        expiry_anchor_ms,
    )?;
    msgpack_response(
        database,
        body.viewer_id,
        json!({
            "access_token": room.access_token,
            "room_number": room.room_number,
            "room_url": "",
        }),
    )
}
// //// /创建 CN 本地联机房间 ////

fn get_rooms(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<GetRoomsRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.category_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_room_query")),
    };
    if let Err(response) = authenticate(database, body.viewer_id)? {
        return Ok(response);
    }
    let mut rooms = Vec::new();
    for room in database.list_multiplayer_rooms(body.category_id)? {
        if room.host_viewer_id == body.viewer_id
            && database
                .multiplayer_member(&room.room_number, room.host_viewer_id)?
                .is_some_and(|member| member.entered)
        {
            let entered_members = database
                .list_multiplayer_members(&room.room_number)?
                .into_iter()
                .filter(|member| member.entered)
                .count();
            let ai_mates = database.list_multiplayer_ai_mates(&room.room_number)?.len();
            let mate_count = i64::try_from(entered_members.saturating_add(ai_mates))
                .map_err(|_| PersonalServiceError::new("multiplayer mate count exceeds range"))?;
            let mate_count = mate_count.min(room.member_count.max(0));
            rooms.push(serialize_room(&room, mate_count));
        }
    }
    msgpack_response(database, body.viewer_id, json!({ "rooms": rooms }))
}

fn search_room(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SearchRoomRequest>(request) {
        Ok(body) if body.viewer_id > 0 && is_room_number(&body.room_number) => body,
        Ok(_) | Err(_) => return Ok(bad_request("Invalid request body.")),
    };
    if let Err(response) = authenticate(database, body.viewer_id)? {
        return Ok(response);
    }
    let room = database.multiplayer_room(&body.room_number)?;
    msgpack_response(
        database,
        body.viewer_id,
        json!({
            "room_exists": room.is_some(),
            "category_id": room.as_ref().map(|room| room.category_id).unwrap_or_default(),
            "quest_id": room.as_ref().map(|room| room.quest_id).unwrap_or_default(),
            "room_number": room.as_ref().map(|room| room.room_number.as_str()).unwrap_or(&body.room_number),
            "establisher_viewer_id": room.as_ref().map(|room| room.host_viewer_id).unwrap_or_default(),
            "establisher_follow": 0,
        }),
    )
}

fn select_room(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SelectRoomRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.category > 0
                && body.quest_id > 0
                && body.party_id > 0 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_room")),
    };
    let player = match authenticate(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    if party_leader_id(&player.data, body.party_id).is_none() {
        return Ok(error_response("400 Bad Request", "party_not_found"));
    }
    let room = if let Some(room_number) = body.room_number.as_deref() {
        if !is_room_number(room_number) {
            return Ok(error_response("400 Bad Request", "invalid_room"));
        }
        database.multiplayer_room(room_number)?
    } else if let Some(access_token) = body.access_token.as_deref() {
        database.multiplayer_room_by_token(access_token)?
    } else {
        None
    };
    let Some(room) = room else {
        return msgpack_response(
            database,
            body.viewer_id,
            missing_room_connection(body.room_number.as_deref().unwrap_or_default()),
        );
    };
    if room.category_id != body.category || room.quest_id != body.quest_id {
        return Ok(error_response("409 Conflict", "room_mismatch"));
    }
    let joined = database.join_multiplayer_room(
        &room.room_number,
        player.account_id,
        body.viewer_id,
        body.party_id,
    )?;
    let Some(joined) = joined else {
        return Ok(error_response("409 Conflict", "room_full"));
    };
    msgpack_response(
        database,
        body.viewer_id,
        room_connection(database, &joined, body.viewer_id)?,
    )
}

fn serialize_room(room: &MultiplayerRoom, mate_count: i64) -> Value {
    json!({
        "access_token": room.access_token,
        "category_id": room.category_id,
        "clear_phase": 0,
        "establisher_character": room.host_main_character_id,
        "establisher_character_evolution_img_level": 0,
        "establisher_follow": 1,
        "establisher_name": format!("Player{}", room.host_viewer_id),
        "host_entry_time": room.host_entry_time,
        "host_main_character_id": room.host_main_character_id,
        "host_player_id": room.host_player_id,
        "host_viewer_id": room.host_viewer_id,
        "is_npc_mode": room.is_npc_mode,
        "is_pickup": false,
        "mates": mate_count,
        "quest_id": room.quest_id,
        "raising_state": room.raising_state,
        "room_number": room.room_number,
        "share_room_options": room.share_room_options,
        "room_sequence": room.room_sequence,
        "room_member_count": mate_count,
    })
}

pub(super) fn is_room_number(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit())
}
