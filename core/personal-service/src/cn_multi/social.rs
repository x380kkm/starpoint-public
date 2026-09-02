// audience: internal
// # personal-service-cn-multi-social
//
// 该模块实现参考 CN 服务的联机邀请 token, 房间社区和房间发布响应.

use super::msgpack_response;
use crate::cn::decode_request;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::json;

#[derive(Default, Deserialize)]
struct ViewerRequest {
    #[serde(default)]
    viewer_id: i64,
}

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/multi_battle_quest/verify_access_token" => {
            verify_access_token(request, database)
        }
        "/api/index.php/multi_battle_quest/micro_community" => micro_community(request, database),
        "/api/index.php/multi_battle_quest/publish_room" => publish_room(request, database),
        _ => return None,
    };
    Some(response)
}

fn verify_access_token(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response(
        database,
        decode_viewer_id(request),
        json!({ "is_valid": true }),
    )
}

fn micro_community(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response(database, decode_viewer_id(request), json!({}))
}

fn publish_room(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    msgpack_response(database, decode_viewer_id(request), json!({}))
}

fn decode_viewer_id(request: &HttpRequest) -> i64 {
    decode_request::<ViewerRequest>(request)
        .unwrap_or_default()
        .viewer_id
}
