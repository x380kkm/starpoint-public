// audience: internal
// # personal-service-cn-multi
//
// 该模块分派 CN 本地联机 HTTP 路由并共享房间序列化与玩家身份校验.

mod battle;
mod lobby;
mod lounge;
mod room;
mod social;

use crate::cn::{msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, player_snapshot};
use crate::database::{MultiplayerRoom, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::{json, Value};

pub(super) const SESSION_HOST: &str = "127.0.0.1";

pub(super) struct AuthenticatedPlayer {
    pub(super) viewer_id: i64,
    pub(super) account_id: i64,
    pub(super) data: Value,
}

// //// 分派 CN 本地联机 HTTP 请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    if request.path().starts_with("/api/index.php/lounge/") {
        return lounge::route(request, database);
    }
    if !request
        .path()
        .starts_with("/api/index.php/multi_battle_quest/")
    {
        return None;
    }
    if let Some(response) = lobby::route(request, database) {
        return Some(response);
    }
    if let Some(response) = room::route(request, database) {
        return Some(response);
    }
    if let Some(response) = battle::route(request, database) {
        return Some(response);
    }
    social::route(request, database)
}
// //// /分派 CN 本地联机 HTTP 请求 ////

pub(super) fn authenticate(
    database: &ServiceDatabase,
    viewer_id: i64,
) -> Result<Result<AuthenticatedPlayer, HttpResponse>, PersonalServiceError> {
    if viewer_id <= 0 {
        return Ok(Err(error_response("400 Bad Request", "invalid_viewer_id")));
    }
    let snapshot = match player_snapshot(database, viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(Err(response)),
    };
    Ok(Ok(AuthenticatedPlayer {
        viewer_id,
        account_id: snapshot.account_id,
        data: decode_player_data(&snapshot.data)?,
    }))
}

pub(super) fn party_leader_id(player: &Value, party_id: i64) -> Option<i64> {
    player
        .get("user_party_group_list")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|groups| groups.values())
        .filter_map(|group| group.get("list").and_then(Value::as_object))
        .filter_map(|parties| parties.get(&party_id.to_string()))
        .filter_map(|party| party.get("character_ids").and_then(Value::as_array))
        .filter_map(|characters| characters.first().and_then(Value::as_i64))
        .find(|character_id| *character_id > 0)
}

pub(super) fn room_connection(
    database: &ServiceDatabase,
    room: &MultiplayerRoom,
    viewer_id: i64,
) -> Result<Value, PersonalServiceError> {
    let raising_state = if room.battle_started || room.raising_state == 4 {
        4
    } else if viewer_id == room.host_viewer_id {
        1
    } else {
        database
            .multiplayer_member(&room.room_number, room.host_viewer_id)?
            .filter(|member| member.entered)
            .map_or(2, |_| room.raising_state)
    };
    Ok(json!({
        "application_update_url": "",
        "category_id": room.category_id,
        "host_entry_time": room.host_entry_time,
        "ip_address": SESSION_HOST,
        "port": database.multiplayer_session_port(),
        "quest_id": room.quest_id,
        "raising_state": raising_state,
        "room_number": room.room_number,
        "room_sequence": room.room_sequence,
        "share_room_options": room.share_room_options,
        "is_pickup": null,
    }))
}

pub(super) fn missing_room_connection(room_number: &str) -> Value {
    json!({
        "application_update_url": "",
        "category_id": 0,
        "host_entry_time": 0,
        "ip_address": "",
        "port": 0,
        "quest_id": 0,
        "raising_state": 9,
        "room_number": room_number,
        "room_sequence": 0,
        "share_room_options": 0,
        "is_pickup": null,
    })
}

pub(super) fn msgpack_response(
    database: &ServiceDatabase,
    viewer_id: i64,
    data: Value,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response_at(viewer_id, false, server_time(database)?, data)
}

pub(super) fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

pub(super) fn bad_request(message: &str) -> HttpResponse {
    HttpResponse::json(
        "400 Bad Request",
        json!({ "error": "Bad Request", "message": message }).to_string(),
    )
}
