// audience: internal
// # personal-service-cn-carnival-event
//
// 该模块返回 CN 嘉年华活动记录和持久化活动队伍.
// 成绩响应的空角色使用 null, 历史成绩和原始队伍保存在玩家快照中.

use super::{closed_activity_response, error_response, party, state};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
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
    let records = records(root, body.event_id);
    let party_groups = party::carnival_party_groups(root, FAMILY, body.event_id)?;
    let activity_id = format!("{FAMILY}:{}", body.event_id);
    let round = crate::activity_history::participation_round(root, &activity_id);
    player.save(database)?;
    database.record_activity_summary(player.account_id, &activity_id, round, &json!({"records":records}))?;
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

// //// 读取嘉年华成绩并整理展示队伍的角色引用 [@x380kkm 2026-10-02] ////
pub(crate) fn records(root: &Map<String, Value>, event_id: i64) -> Value {
    let records = state::event_state(root, FAMILY, event_id)
        .and_then(|event| event.get("carnival_records"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(present_record)
        .collect();
    Value::Array(records)
}

fn present_record(value: &Value) -> Option<Value> {
    let record = value.as_object()?;
    i32::try_from(record.get("folder_id")?.as_i64()?).ok()?;
    let mut record = record.clone();
    for field in ["best_score", "previous_score"] {
        if let Some(score) = record.get_mut(field) {
            if !score.is_number() {
                *score = Value::Null;
            }
        }
    }
    for field in ["previous_character_ids", "previous_unison_character_ids"] {
        let slots = record.entry(field.to_owned()).or_insert(Value::Null);
        if let Some(slots) = slots.as_array_mut() {
            slots.iter_mut().for_each(
                crate::cn_character_presentation::normalize_optional_character_reference,
            );
        } else {
            *slots = json!([null, null, null]);
        }
    }
    Some(Value::Object(record))
}
// //// /读取嘉年华成绩并整理展示队伍的角色引用 ////

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
