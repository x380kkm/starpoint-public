// audience: internal
// # personal-service-cn-reference-state-misc
//
// 该模块分派参考 CN 服务中的主动任务领奖, 任务解锁和物品出售.

mod active_mission;
mod common;
mod item;
mod incentive;
mod quest;

pub(crate) use active_mission::remove_unknown_active_missions_from_load;
pub(crate) use active_mission::recover_recorded_active_mission_rewards;
pub(crate) use incentive::{active_mission_ids, active_mission_event_id};

use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;

// //// 分派参考 CN 状态请求 [@x380kkm 2026-08-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        "/api/index.php/active_mission/receive" => Some(active_mission::receive(request, database)),
        "/api/index.php/active_mission/receive_incentive" => {
            Some(incentive::receive(request, database))
        }
        "/api/index.php/quest/unlock" => Some(quest::unlock(request, database)),
        "/api/index.php/item/sell" => Some(item::sell(request, database)),
        _ => None,
    }
}
// //// /分派参考 CN 状态请求 ////
