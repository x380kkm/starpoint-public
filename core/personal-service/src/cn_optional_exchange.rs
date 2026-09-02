// audience: internal
// # personal-service-cn-optional-exchange
//
// 该模块返回客户端可解析的已关闭活动兑换状态.

use crate::cn::{decode_request, deserialize_optional_i64, msgpack_response_at, server_time};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Deserialize)]
struct ViewerRequest {
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    viewer_id: Option<i64>,
}

// //// 分派 CN 已关闭活动兑换请求 [@x380kkm 2026-08-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let data = match request.path() {
        "/api/index.php/special_exchange/exchange_character" => {
            closed_character_exchange_data("special_exchange_campaign_list")
        }
        "/api/index.php/special_exchange/exchange_equipment"
        | "/api/index.php/special_exchange/enter_campaign"
        | "/api/index.php/special_exchange/buy_ticket" => {
            json!({"special_exchange_campaign_list": []})
        }
        "/api/index.php/start_dash_exchange/exchange_character" => {
            closed_character_exchange_data("start_dash_exchange_campaign_list")
        }
        "/api/index.php/start_dash_exchange/buy_ticket" => {
            json!({"start_dash_exchange_campaign_list": []})
        }
        "/api/index.php/growth_fund/receive_fund_bonus" => {
            json!({"fund_receive_list": []})
        }
        "/api/index.php/treasure_vault_exchange/exchange_equipment" => json!({}),
        _ => return None,
    };
    Some(respond(request, database, data))
}
// //// /分派 CN 已关闭活动兑换请求 ////

// //// 构造关闭状态下的角色兑换响应 [@x380kkm 2026-08-24] ////
fn closed_character_exchange_data(campaign_list_key: &str) -> Value {
    let mut data = Map::from_iter([
        ("character_list".to_owned(), json!([])),
        ("crazy_gacha_result_list".to_owned(), json!([])),
        ("equipment_list".to_owned(), json!([])),
        ("fund_receive_list".to_owned(), json!([])),
        ("item_list".to_owned(), json!({})),
        ("mail_arrived".to_owned(), Value::Bool(false)),
        ("monthly_charge_bonus_info".to_owned(), Value::Null),
        ("user_daily_challenge_point_list".to_owned(), json!([])),
        ("user_info".to_owned(), Value::Null),
    ]);
    data.insert(campaign_list_key.to_owned(), json!([]));
    Value::Object(data)
}
// //// /构造关闭状态下的角色兑换响应 ////

fn respond(
    request: &HttpRequest,
    database: &ServiceDatabase,
    data: serde_json::Value,
) -> Result<HttpResponse, PersonalServiceError> {
    let viewer_id = decode_request::<ViewerRequest>(request)
        .map(|body| body.viewer_id.unwrap_or_default().max(0))
        .map_err(|_| PersonalServiceError::new("invalid CN optional exchange request body"))?;
    msgpack_response_at(viewer_id, false, server_time(database)?, data)
}
