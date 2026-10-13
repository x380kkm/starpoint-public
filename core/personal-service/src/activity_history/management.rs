// audience: internal
// # personal-service-activity-history-api
// 管理 API 按指定存档和活动查询历史, 并使用当前参与轮次校验重置请求.

use crate::database::ServiceDatabase;
use crate::http::{decode_path_segment, HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetRequest {
    expected_round: i64,
}

// //// 分派存档活动历史和重置管理请求 [@x380kkm 2026-09-21] ////
pub(crate) fn route(request: &HttpRequest, database: &mut ServiceDatabase, asset_root: &Path)
    -> Option<Result<HttpResponse, PersonalServiceError>> {
    let path = request.path().strip_prefix("/v1/local-saves/")?;
    let (slot, rest) = path.split_once("/activities/")?;
    let (activity, action) = rest.rsplit_once('/')?;
    if !matches!(action, "history" | "reset") { return None; }
    if !management::is_authorized(request, database) { return Some(Ok(management::unauthorized_response())); }
    let Some(slot_id) = slot.parse::<i64>().ok().filter(|id| *id > 0) else {
        return Some(Ok(error("400 Bad Request", "invalid_local_save_id")));
    };
    let Some(activity_id) = decode_path_segment(activity).filter(|id| crate::database::is_valid_activity_id(id)) else {
        return Some(Ok(error("400 Bad Request", "invalid_activity_id")));
    };
    Some(handle(request, database, asset_root, slot_id, &activity_id, action))
}

fn handle(request: &HttpRequest, database: &mut ServiceDatabase, asset_root: &Path,
    slot_id: i64, activity_id: &str, action: &str) -> Result<HttpResponse, PersonalServiceError> {
    let Some(context) = database.local_save_context(slot_id)? else {
        return Ok(error("404 Not Found", "local_save_not_found"));
    };
    match (request.method(), action) {
        ("GET", "history") => {
            let before_id = request.target().split_once('?').map(|(_,query)| query).unwrap_or("")
                .split('&').find_map(|part| part.strip_prefix("before_id="));
            let before_id = match before_id {
                Some(value) => match value.parse::<i64>() { Ok(id) if id > 0 => Some(id), _ => return Ok(error("400 Bad Request", "invalid_history_cursor")) },
                None => None,
            };
            let mut history = database.activity_history(context.account_id, activity_id, before_id)?;
            history["slot_id"] = Value::from(slot_id);
            history["slot_name"] = Value::from(context.slot.name);
            history["can_reset"] = Value::Bool(supports_reset(activity_id)?);
            Ok(HttpResponse::json("200 OK", history.to_string()))
        }
        ("POST", "reset") => {
            if !crate::activity_catalog::contains_activity_definition(asset_root, activity_id) {
                return Ok(error("404 Not Found", "activity_not_found"));
            }
            if !supports_reset(activity_id)? { return Ok(error("400 Bad Request", "activity_reset_unavailable")); }
            let body = match serde_json::from_slice::<ResetRequest>(request.body()) {
                Ok(body) if body.expected_round >= 0 => body,
                _ => return Ok(error("400 Bad Request", "invalid_request_body")),
            };
            match database.reset_save_activity(context.account_id, activity_id, body.expected_round)? {
                Ok(details) => Ok(HttpResponse::json("200 OK", details.to_string())),
                Err(code) => Ok(error("409 Conflict", code)),
            }
        }
        _ => Ok(error("405 Method Not Allowed", "method_not_allowed")),
    }
}

fn supports_reset(activity_id: &str) -> Result<bool, PersonalServiceError> {
    Ok(crate::cn_battle_assets::load_battle_fixture()?.quests.values()
        .any(|quest| quest.activity_id.as_deref() == Some(activity_id))
        || activity_id.starts_with("active-mission:") || activity_id.starts_with("collect-item:")
        || activity_id.starts_with("pass-card:")
        || crate::cn_shop::has_activity_shop(activity_id)?
        || crate::cn_multi_special_exchange::has_campaign(activity_id)?
        || crate::cn_character_election::has_election(activity_id)?
        || crate::cn_player_history::default_activity_window(activity_id)?.is_some()
        || crate::cn_optional_exchange::has_campaign(activity_id)?)
}

fn error(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, json!({"error":code}).to_string())
}
