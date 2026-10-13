// audience: internal
// # personal-service-cn-optional-exchange
//
// 自选活动使用客户端候选和票券, 玩家资格与奖励随同一存档事务提交.

use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde_json::{Map, Value};

mod catalog;
mod delivery;
mod growth_fund;
mod management;
mod settlement;
mod state;

pub(crate) use management::{
    has_campaign, project_campaigns, reset_campaign, reward_period_millis,
};
pub(crate) use state::refresh_campaigns;

// //// 分派 CN 活动兑换和成长基金请求 [@x380kkm 2026-09-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let (family, action) = match request.path() {
        "/api/index.php/special_exchange/exchange_character" => ("special", "character"),
        "/api/index.php/special_exchange/exchange_equipment" => ("special", "equipment"),
        "/api/index.php/special_exchange/enter_campaign" => ("special", "enter"),
        "/api/index.php/special_exchange/buy_ticket" => ("special", "buy"),
        "/api/index.php/start_dash_exchange/exchange_character" => ("start_dash", "character"),
        "/api/index.php/start_dash_exchange/buy_ticket" => ("start_dash", "buy"),
        "/api/index.php/growth_fund/receive_fund_bonus" => {
            return Some(growth_fund::receive(request, database));
        }
        "/api/index.php/treasure_vault_exchange/exchange_equipment" => ("treasure", "equipment"),
        _ => return None,
    };
    Some(settlement::handle(request, database, family, action))
}
// //// /分派 CN 活动兑换和成长基金请求 ////
