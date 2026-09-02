// audience: internal
// # personal-service-cn-carnival-event
//
// 该模块返回 CN 嘉年华活动记录和持久化活动队伍.

use super::{closed_activity_response, error_response, party, state};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const FAMILY: &str = "carnival";

#[derive(Deserialize)]
struct IndexRequest {
    event_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct PartyRequest {
    event_id: Option<i64>,
    viewer_id: i64,
}

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/carnival_event/index" => index(request, database, asset_root),
        "/api/index.php/carnival_event/get_party" => get_party(request, database, asset_root),
        _ => return None,
    };
    Some(response)
}

// //// 返回嘉年华活动首页 [@x380kkm 2026-08-22] ////
fn index(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<IndexRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    if let Some(response) =
        closed_activity_response(database, asset_root, &format!("{FAMILY}:{}", body.event_id))?
    {
        return Ok(response);
    }
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    let records = state::event_state(root, FAMILY, body.event_id)
        .and_then(|event| event.get("carnival_records"))
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let party_groups = party::carnival_party_groups(root, FAMILY, body.event_id)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "records": records,
            "user_party_group_list": party_groups,
        }),
    )
}
// //// /返回嘉年华活动首页 ////

// //// 返回嘉年华活动队伍 [@x380kkm 2026-08-22] ////
fn get_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PartyRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let current_event = state::current_event_id(player.root()?, FAMILY);
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id = body
        .event_id
        .filter(|event_id| *event_id > 0)
        .or(current_event)
        .or(managed_event);
    let has_activity_context = event_id.is_some();
    let event_id = event_id.unwrap_or(1);
    if has_activity_context {
        if let Some(response) =
            closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
        {
            return Ok(response);
        }
    }
    let root = player.root_mut()?;
    if has_activity_context {
        state::set_current_event(root, FAMILY, event_id)?;
    }
    let party_groups = party::carnival_party_groups(root, FAMILY, event_id)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "user_party_group_list": party_groups }),
    )
}
// //// /返回嘉年华活动队伍 ////
