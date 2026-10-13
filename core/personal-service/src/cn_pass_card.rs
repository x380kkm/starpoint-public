// audience: internal
// # personal-service-cn-pass-card
//
// 该模块实现 CN Pass Card 查询和领取协议.

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::json;

mod catalog;
mod claim;
mod state;
pub(crate) use state::{
    add_points, add_points_for_event, grant_purchase, point_purchase_target, purchase_event_id, record_login_day_for_event,
};

// //// 从修行活动主表读取领奖宽限时间 [@x380kkm 2026-09-25] ////
pub(crate) fn reward_period_millis(activity_id: &str) -> Result<Option<i64>, PersonalServiceError> {
    let Some(id) = activity_id.strip_prefix("pass-card:").and_then(|id| id.parse::<i64>().ok()) else {
        return Ok(None);
    };
    Ok(catalog::catalog()?.events.get(&id).map(|event| {
        event.force_end_time.saturating_sub(event.end_time).max(0).saturating_mul(1000)
    }))
}

#[derive(Deserialize)]
struct PassCardRequest {
    viewer_id: i64,
    pass_card_id: i64,
}

// //// 分派 CN Pass Card 请求 [@x380kkm 2026-08-21] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        "/api/index.php/Pass_card/get_pass_card" => Some(get(request, database)),
        "/api/index.php/Pass_card/receive_all" => Some(claim::receive(request, database)),
        _ => return None,
    }
}
// //// /分派 CN Pass Card 请求 ////

// //// 返回活动的积分和双轨领取状态 [@x380kkm 2026-09-22] ////
fn get(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let now = server_time(database)?;
    let body = match decode_request::<PassCardRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        _ => return msgpack_result_code_response_at(0, now, 20001),
    };
    let Some(event) = catalog::catalog()?.events.get(&body.pass_card_id) else {
        return msgpack_result_code_response_at(body.viewer_id, now, 20001);
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    let state = state::read_state(root, event.id)?;
    state::select_event(root, event.id)?;
    let response = json!({"point":state.point,"is_buy":event.is_free || state.is_buy,"all_received_record":state.all_received_record});
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&data)?)?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
