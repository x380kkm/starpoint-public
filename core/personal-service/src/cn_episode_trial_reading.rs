// audience: internal
// # personal-service-cn-episode-trial-reading
//
// 该模块实现 CN 章节试读完成协议.

use crate::cn::msgpack_response;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::json;

// //// 分派 CN 章节试读请求 [@x380kkm 2026-08-21] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        "/api/index.php/episode_trial_reading/finish" => {
            Some(msgpack_response(database, 0, false, json!({})))
        }
        _ => None,
    }
}
// //// /分派 CN 章节试读请求 ////
