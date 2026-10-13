// audience: internal
// # personal-service-cn-optional-exchange-settlement
// 费用, 奖励, 活动资格, 任务和原请求响应在同一保存点提交.

use super::{catalog, delivery, state, PersonalServiceError, ServiceDatabase};
use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot, require_root};
use crate::http::{HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize, Serialize)]
struct ExchangeRequest {
    viewer_id: i64,
    campaign_id: i64,
    api_count: Option<i64>,
    character_id: Option<i64>,
    equipment_id: Option<i64>,
}

pub(super) fn handle(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    family: &str,
    action: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    database
        .with_player_settlement(|database| settle(request, database, family, action).map(Some))?
        .ok_or_else(|| PersonalServiceError::new("CN exchange settlement returned no response"))
}

// //// 验证自选资格并原子提交领取结果 [@x380kkm 2026-09-23] ////
fn settle(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    family: &str,
    action: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let now = server_time(database)?;
    let body = match decode_request::<ExchangeRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.campaign_id > 0
                && body.api_count.map_or(true, |count| count >= 0)
                && (family != "treasure" || body.api_count.is_some()) =>
        {
            body
        }
        _ => return msgpack_result_code_response_at(0, now, 20001),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut data)?;
    let mut identity = serde_json::to_value(&body)
        .map_err(|error| PersonalServiceError::new(error.to_string()))?;
    identity["family"] = json!(family);
    identity["action"] = json!(action);
    let activity_id = super::management::activity_id(family, body.campaign_id);
    identity["participation_round"] = json!(crate::activity_history::participation_round(
        root,
        &activity_id
    ));
    identity["activity_id"] = json!(activity_id);
    if let Some(response) = state::replay(root, &identity, now)? {
        return Ok(response);
    }
    state::refresh_campaigns(root, database)?;
    let Some(campaign) = catalog::resolve(root, family, body.campaign_id)? else {
        return msgpack_result_code_response_at(body.viewer_id, now, 20001);
    };
    let exchange = action == "character" || action == "equipment";
    if family == "start_dash" && (now < campaign.start_at || now > campaign.end_at) {
        return msgpack_result_code_response_at(body.viewer_id, now, 4802);
    }
    let (start, end) = if exchange {
        (campaign.exchange_start_at, campaign.exchange_end_at)
    } else {
        (campaign.start_at, campaign.end_at)
    };
    let is_open = if family == "start_dash" {
        super::management::is_start_dash_open(database, body.campaign_id)?
    } else {
        catalog::is_open(
            database,
            family,
            body.campaign_id,
            start,
            end,
            campaign.end_at,
        )?
    };
    if !is_open {
        let code = if family == "start_dash" {
            4802
        } else if action == "enter" {
            4715
        } else {
            4702
        };
        return msgpack_result_code_response_at(body.viewer_id, now, code);
    }
    let status = state::status(root, family, body.campaign_id);
    let previous_items = crate::cn_mission::capture_collect_item_counts(root)?;
    let mut response = delivery::response(root);
    let delivery = if exchange {
        let target = if action == "character" {
            body.character_id
        } else {
            body.equipment_id
        };
        let Some(target) = target.filter(|id| *id > 0 && campaign.candidates.contains(id)) else {
            return msgpack_result_code_response_at(body.viewer_id, now, 20001);
        };
        if (action == "character") != (campaign.kind == 0)
            || (family != "treasure" && status != 2)
            || state::item_count(root, campaign.ticket_id)? < 1
        {
            return msgpack_result_code_response_at(body.viewer_id, now, 20001);
        }
        let delivery =
            delivery::exchange(root, &campaign, body.viewer_id, target, now, &mut response)?;
        if family != "treasure" {
            state::set_status(root, family, body.campaign_id, 3)?;
        }
        Some(delivery)
    } else if status == 1 && (action == "buy" || campaign.product_source == 1) {
        let balance = root
            .get("user_info")
            .and_then(|info| info.get("vmoney"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if balance < campaign.price {
            return msgpack_result_code_response_at(body.viewer_id, now, 20001);
        }
        let delivery = delivery::issue_ticket(root, &campaign, &mut response)?;
        state::set_status(root, family, body.campaign_id, 2)?;
        Some(delivery)
    } else {
        None
    };
    if family != "treasure" {
        response[state::list_key(family)] =
            json!([state::campaign_state(root, family, body.campaign_id)
                .cloned()
                .unwrap_or_else(|| json!({"campaign_id":body.campaign_id,"status":status}))]);
    }
    if delivery.is_none() {
        response["item_list"] =
            json!({campaign.ticket_id.to_string():state::item_count(root, campaign.ticket_id)?});
    }
    let completed = crate::cn_mission::record_collected_items(
        root,
        database,
        snapshot.account_id,
        &previous_items,
    )?;
    response["mission_info"] = json!(completed);
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &completed,
        body.viewer_id,
        now,
    )?;
    response["active_mission_list"] =
        json!(crate::cn_mission::refresh_active_missions(root, database)?);
    response["mail_arrived"] = json!(response["mail_arrived"].as_bool().unwrap_or(false)
        || database.has_unreceived_mail(snapshot.account_id, now)?);
    let (history, costs) = delivery
        .map(|delivery| (delivery.history, delivery.costs))
        .unwrap_or_else(|| (Vec::new(), json!({})));
    let event_key = state::record(root, identity, &response, costs, &history, now)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&data)?,
        &event_key,
        now,
        &history,
    )?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
// //// /验证自选资格并原子提交领取结果 ////
