// audience: internal
// # personal-service-cn-pass-card
//
// 该模块实现 CN Pass Card 查询和领取协议.

use crate::cn::msgpack_response;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::json;

// //// 分派 CN Pass Card 请求 [@x380kkm 2026-08-21] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let data = match request.path() {
        "/api/index.php/Pass_card/get_pass_card" => {
            json!({"point": 0, "is_buy": false, "all_received_record": []})
        }
        "/api/index.php/Pass_card/receive_all" => json!({"all_received_record": []}),
        _ => return None,
    };
    Some(msgpack_response(database, 0, false, data))
}
// //// /分派 CN Pass Card 请求 ////
