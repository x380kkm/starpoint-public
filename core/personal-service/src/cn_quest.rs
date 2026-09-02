// audience: internal
// # personal-service-cn-quest
//
// 该模块返回 CN 任务助战队伍兼容数据.

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::player_snapshot;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct RecentPartyRequest {
    viewer_id: i64,
}

// //// 分派 CN 任务助战队伍请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST"
        || request.path() != "/api/index.php/quest/get_recent_other_player_party"
    {
        return None;
    }
    Some(get_recent_other_player_party(request, database))
}
// //// /分派 CN 任务助战队伍请求 ////

// //// 返回类型化的空助战队伍列表 [@x380kkm 2026-08-22] ////
fn get_recent_other_player_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<RecentPartyRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    match player_snapshot(database, body.viewer_id)? {
        Ok(_) => {}
        Err(response) => return Ok(response),
    }
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"recent_other_player_party": []}),
    )
}
// //// /返回类型化的空助战队伍列表 ////

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
