// audience: internal
// # personal-service-management-access
//
// 该模块允许 loopback 页面在未提供 Authorization 时直接访问当前设备的管理 API. 请求显式提供 bearer token 时仍验证其有效性.

use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use serde::Deserialize;

const PLAYER_ACCESS_PATH: &str = "/v1/player-access";
const HTTP_OBSERVATIONS_PATH: &str = "/v1/http-observations";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerAccessRequest {
    viewer_id: i64,
}

// //// 分派管理员签发和撤销玩家访问 token [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, crate::PersonalServiceError>> {
    let path = request.path();
    if path == HTTP_OBSERVATIONS_PATH {
        if !is_authorized(request, database) {
            return Some(Ok(unauthorized_response()));
        }
        if request.method() != "GET" {
            return Some(Ok(HttpResponse::json(
                "405 Method Not Allowed",
                "{\"error\":\"method_not_allowed\"}".to_owned(),
            )));
        }
        return Some(http_observations_response(database));
    }
    if path != PLAYER_ACCESS_PATH && !path.starts_with(&format!("{PLAYER_ACCESS_PATH}/")) {
        return None;
    }
    if !is_authorized(request, database) {
        return Some(Ok(unauthorized_response()));
    }
    if path == PLAYER_ACCESS_PATH && request.method() == "POST" {
        let Some(body) = parse_request(request) else {
            return Some(Ok(HttpResponse::json(
                "400 Bad Request",
                "{\"error\":\"invalid_player_access_request\"}".to_owned(),
            )));
        };
        return Some(issue_player_access_token(database, body.viewer_id));
    }
    let viewer_id = path
        .strip_prefix(&format!("{PLAYER_ACCESS_PATH}/"))
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0);
    if request.method() == "DELETE" {
        let Some(viewer_id) = viewer_id else {
            return Some(Ok(HttpResponse::json(
                "400 Bad Request",
                "{\"error\":\"invalid_viewer_id\"}".to_owned(),
            )));
        };
        return Some(revoke_player_access_token(database, viewer_id));
    }
    Some(Ok(HttpResponse::json(
        "405 Method Not Allowed",
        "{\"error\":\"method_not_allowed\"}".to_owned(),
    )))
}
// //// /分派管理员签发和撤销玩家访问 token ////

// //// 返回最近的 HTTP 请求记录 [@x380kkm 2026-08-21] ////
fn http_observations_response(
    database: &ServiceDatabase,
) -> Result<HttpResponse, crate::PersonalServiceError> {
    let body = serde_json::to_string(&serde_json::json!({
        "observations": database.http_observations()?,
    }))
    .map_err(|error| {
        crate::PersonalServiceError::new(format!("failed to encode HTTP observations: {error}"))
    })?;
    Ok(HttpResponse::json("200 OK", body))
}
// //// /返回最近的 HTTP 请求记录 ////

fn parse_request(request: &HttpRequest) -> Option<PlayerAccessRequest> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return None;
    }
    serde_json::from_slice(request.body()).ok()
}

fn issue_player_access_token(
    database: &mut ServiceDatabase,
    viewer_id: i64,
) -> Result<HttpResponse, crate::PersonalServiceError> {
    let Some(token) = database.issue_player_access_token(viewer_id)? else {
        return Ok(HttpResponse::json(
            "404 Not Found",
            "{\"error\":\"viewer_not_found\"}".to_owned(),
        ));
    };
    Ok(HttpResponse::json(
        "201 Created",
        format!("{{\"viewer_id\":{viewer_id},\"token\":\"{token}\"}}"),
    ))
}

fn revoke_player_access_token(
    database: &mut ServiceDatabase,
    viewer_id: i64,
) -> Result<HttpResponse, crate::PersonalServiceError> {
    if !database.revoke_player_access_token(viewer_id)? {
        return Ok(HttpResponse::json(
            "404 Not Found",
            "{\"error\":\"viewer_not_found\"}".to_owned(),
        ));
    }
    Ok(HttpResponse::json(
        "200 OK",
        "{\"revoked\":true}".to_owned(),
    ))
}

// //// 接受本机直接访问并验证显式 bearer token [@x380kkm 2026-08-20] ////
pub(crate) fn is_authorized(request: &HttpRequest, database: &ServiceDatabase) -> bool {
    let Some(authorization) = request.header("authorization") else {
        return true;
    };
    let Some(provided_token) = authorization.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_equal(
        provided_token.as_bytes(),
        database.management_token().as_bytes(),
    )
}

pub(crate) fn player_account_id(request: &HttpRequest, database: &ServiceDatabase) -> Option<i64> {
    let token = request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())?;
    database.player_access_account_id(token).ok().flatten()
}

fn constant_time_equal(provided: &[u8], expected: &[u8]) -> bool {
    let mut difference = provided.len() ^ expected.len();
    for (index, expected_byte) in expected.iter().enumerate() {
        difference |= usize::from(provided.get(index).copied().unwrap_or_default() ^ expected_byte);
    }
    difference == 0
}
// //// /接受本机直接访问并验证显式 bearer token ////

// //// 拒绝未授权管理请求 [@x380kkm 2026-07-23] ////
pub(crate) fn unauthorized_response() -> HttpResponse {
    HttpResponse::json(
        "401 Unauthorized",
        "{\"error\":\"management_authorization_required\"}".to_owned(),
    )
}
// //// /拒绝未授权管理请求 ////

pub(crate) fn player_unauthorized_response() -> HttpResponse {
    HttpResponse::json(
        "401 Unauthorized",
        "{\"error\":\"player_authorization_required\"}".to_owned(),
    )
}
