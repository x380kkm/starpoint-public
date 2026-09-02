// audience: internal
// # personal-service-cn-lounge
//
// 该模块把 CN lounge 请求连接到本地多人会话监听器.

use super::{authenticate, msgpack_response, SESSION_HOST};
use crate::cn::decode_request;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct LoungeRequest {
    viewer_id: i64,
}

// //// 分派 CN lounge 请求 [@x380kkm 2026-08-24] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/lounge/create" => create(request, database),
        "/api/index.php/lounge/prepare" => prepare(request, database),
        "/api/index.php/lounge/restore" => restore(request, database),
        "/api/index.php/lounge/search" => search(request, database),
        "/api/index.php/lounge/select" => select(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN lounge 请求 ////

// //// 返回本地 lounge 生命周期响应 [@x380kkm 2026-08-24] ////
fn create(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    msgpack_response(
        database,
        viewer_id,
        json!({"advice": "offline", "lounge_id": viewer_id}),
    )
}

fn prepare(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    msgpack_response(database, viewer_id, json!({}))
}

fn restore(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    msgpack_response(
        database,
        viewer_id,
        json!({
            "ip_address": SESSION_HOST,
            "port": database.multiplayer_session_port(),
            "raising_state": 2,
        }),
    )
}

fn search(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    msgpack_response(database, viewer_id, json!({"lounge_exists": false}))
}

fn select(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = match viewer_id(request, database)? {
        Ok(viewer_id) => viewer_id,
        Err(response) => return Ok(response),
    };
    msgpack_response(
        database,
        viewer_id,
        json!({
            "application_update_url": "",
            "ip_address": SESSION_HOST,
            "lounge_number": format!("{:06}", viewer_id.rem_euclid(1_000_000)),
            "port": database.multiplayer_session_port(),
            "raising_state": 2,
        }),
    )
}

fn viewer_id(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Result<Result<i64, HttpResponse>, PersonalServiceError> {
    let body = match decode_request::<LoungeRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) => return Ok(Err(super::bad_request("invalid_request_body"))),
        Err(response) => return Ok(Err(response)),
    };
    match authenticate(database, body.viewer_id)? {
        Ok(_) => Ok(Ok(body.viewer_id)),
        Err(response) => Ok(Err(response)),
    }
}
// //// /返回本地 lounge 生命周期响应 ////
