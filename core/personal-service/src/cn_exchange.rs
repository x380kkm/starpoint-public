// audience: internal
// # personal-service-cn-exchange
// 星屑和羁绊证兑换使用客户端目录, 费用, 奖励与请求凭据共同保存.

mod catalog;
mod settlement;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, player_snapshot, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct ViewerRequest {
    viewer_id: i64,
}

// //// 按兑换目录分派查询与原子结算 [@x380kkm 2026-09-23] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let resolver: settlement::OfferResolver = match request.path() {
        "/api/index.php/exchange/get_bond_token_exchange_list" => {
            return Some(bond_token_exchange_list(request, database));
        }
        "/api/index.php/exchange/star_crumb" => catalog::star_crumb_offer,
        "/api/index.php/exchange/bond_token" => catalog::bond_token_offer,
        _ => return None,
    };
    Some(
        database
            .with_player_settlement(|database| {
                settlement::exchange(request, database, resolver).map(Some)
            })
            .and_then(|response| {
                response.ok_or_else(|| PersonalServiceError::new("CN exchange response is missing"))
            }),
    )
}
// //// /按兑换目录分派查询与原子结算 ////

// //// 返回羁绊证装备目录的持久化兑换次数 [@x380kkm 2026-09-23] ////
fn bond_token_exchange_list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ViewerRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        _ => return Ok(error_response("invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    let entries = catalog::bond_token_offers()?
        .map(|offer| {
            json!({
                "equipment_id":offer.reward.target_id,
                "exchange_count":offer.purchase_count(root),
            })
        })
        .collect::<Vec<_>>();
    msgpack_response_at(body.viewer_id, false, server_time(database)?, entries)
}
// //// /返回羁绊证装备目录的持久化兑换次数 ////

fn error_response(code: &str) -> HttpResponse {
    HttpResponse::json("400 Bad Request", json!({"error":code}).to_string())
}
