// audience: internal
// # personal-service-cn-multi-special-exchange-state
// 每期活动保存票券状态和原结算响应, 活动重置归档原轮次后清除该期状态.

use super::catalog::{Campaign, FAMILY};
use crate::cn::msgpack_response_at;
use crate::cn_activity::{event_state, event_state_mut};
use crate::database::ReceiveHistoryEntry;
use crate::http::HttpResponse;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

pub(super) struct Receipt<'a> {
    pub(super) request: Value,
    pub(super) response: &'a Value,
    pub(super) rewards: &'a [ReceiveHistoryEntry],
    pub(super) consumed_ticket: Option<i64>,
    pub(super) received_at: i64,
}

// //// 导入玩家持有的合法活动状态 [@x380kkm 2026-09-22] ////
pub(super) fn initialize(
    root: &mut Map<String, Value>,
    id: i64,
    campaign: &Campaign,
) -> Result<(), PersonalServiceError> {
    let legacy = root
        .get("multi_special_exchange_campaign_list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| entry.get("campaign_id").and_then(Value::as_i64) == Some(id))
        .cloned();
    let event = event_state_mut(root, FAMILY, id)?;
    if !event.contains_key("status") {
        let legacy = legacy
            .filter(|_| event.get("legacy_state_imported").and_then(Value::as_bool) != Some(true));
        let mut status = legacy
            .as_ref()
            .and_then(|entry| entry.get("status"))
            .and_then(Value::as_i64)
            .filter(|status| (1..=4).contains(status))
            .unwrap_or(1);
        let ticket = legacy
            .as_ref()
            .and_then(|entry| entry.get("ticket_item_id"))
            .and_then(Value::as_i64)
            .filter(|ticket| campaign.ticket_ids.contains(ticket));
        if status == 3 && ticket.is_none() {
            status = 1;
        }
        event.insert("status".to_owned(), json!(status));
        if let Some(ticket) = ticket {
            event.insert("ticket_item_id".to_owned(), json!(ticket));
        }
    }
    event.insert("legacy_state_imported".to_owned(), json!(true));
    Ok(())
}
// //// /导入玩家持有的合法活动状态 ////

pub(super) fn status(root: &Map<String, Value>, id: i64) -> i64 {
    event_state(root, FAMILY, id)
        .and_then(|event| event.get("status"))
        .and_then(Value::as_i64)
        .unwrap_or(1)
}

pub(super) fn ticket(root: &Map<String, Value>, id: i64) -> Option<i64> {
    event_state(root, FAMILY, id)
        .and_then(|event| event.get("ticket_item_id"))
        .and_then(Value::as_i64)
}

pub(super) fn response_state(root: &Map<String, Value>, id: i64) -> Value {
    let mut response = json!({"campaign_id":id,"status":status(root, id)});
    if let Some(ticket) = ticket(root, id) {
        response["ticket_item_id"] = json!(ticket);
    }
    response
}

// //// 保存领券和兑换后的客户端活动状态 [@x380kkm 2026-09-22] ////
pub(super) fn set_status(
    root: &mut Map<String, Value>,
    id: i64,
    status: i64,
    ticket: Option<i64>,
) -> Result<(), PersonalServiceError> {
    let event = event_state_mut(root, FAMILY, id)?;
    event.insert("status".to_owned(), json!(status));
    if let Some(ticket) = ticket {
        event.insert("ticket_item_id".to_owned(), json!(ticket));
    } else {
        event.remove("ticket_item_id");
    }
    let response = response_state(root, id);
    let campaigns = root
        .entry("multi_special_exchange_campaign_list".to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            PersonalServiceError::new("invalid CN multi special exchange campaign list")
        })?;
    if let Some(entry) = campaigns
        .iter_mut()
        .find(|entry| entry.get("campaign_id").and_then(Value::as_i64) == Some(id))
    {
        *entry = response;
    } else {
        campaigns.push(response);
    }
    Ok(())
}
// //// /保存领券和兑换后的客户端活动状态 ////

// //// 重放同一会话和请求计数的原结算结果 [@x380kkm 2026-09-22] ////
pub(super) fn replay(
    root: &Map<String, Value>,
    request: &Value,
) -> Result<Option<HttpResponse>, PersonalServiceError> {
    if request["api_count"].is_null() {
        return Ok(None);
    }
    let events = root
        .get("cn_activity_state")
        .and_then(|state| state.get("event_families"))
        .and_then(|families| families.get(FAMILY))
        .and_then(Value::as_object);
    for event in events.into_iter().flat_map(|events| events.values()) {
        for field in ["draw_receipt", "exchange_receipt"] {
            let Some(receipt) = event.get(field) else {
                continue;
            };
            let previous = &receipt["request"];
            if previous["action"] != request["action"]
                || previous["viewer_id"] != request["viewer_id"]
                || previous["api_count"] != request["api_count"]
            {
                continue;
            }
            if previous != request {
                return Ok(Some(super::error_response(
                    "400 Bad Request",
                    "multi_special_exchange_request_conflict",
                )));
            }
            let response_time = receipt
                .get("received_at")
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    PersonalServiceError::new("invalid CN multi special exchange receipt time")
                })?;
            let mut response = receipt.get("response").cloned().ok_or_else(|| {
                PersonalServiceError::new("missing CN multi special exchange receipt response")
            })?;
            crate::cn_character::complete_character_response_times(&mut response, root, response_time);
            return Ok(Some(msgpack_response_at(
                request["viewer_id"].as_i64().unwrap_or_default(),
                false,
                response_time,
                response,
            )?));
        }
    }
    Ok(None)
}
// //// /重放同一会话和请求计数的原结算结果 ////

// //// 保存实发奖励、消耗票券和原响应以供活动历史归档 [@x380kkm 2026-09-22] ////
pub(super) fn record(
    root: &mut Map<String, Value>,
    id: i64,
    field: &str,
    receipt: Receipt<'_>,
) -> Result<String, PersonalServiceError> {
    let activity_id = format!("{FAMILY}:{id}");
    let round = crate::activity_history::participation_round(root, &activity_id);
    let rewards = receipt
        .rewards
        .iter()
        .map(|reward| json!({"kind":reward.kind,"type_id":reward.type_id,"number":reward.number}))
        .collect::<Vec<_>>();
    let costs = receipt
        .consumed_ticket
        .into_iter()
        .map(|ticket| json!({"item_id":ticket,"number":1}))
        .collect::<Vec<_>>();
    event_state_mut(root, FAMILY, id)?.insert(
        field.to_owned(),
        json!({
            "request":receipt.request,"response":receipt.response,"received_at":receipt.received_at,
            "costs":costs,"rewards":rewards,
        }),
    );
    Ok(format!("{activity_id}:{round}:{field}"))
}
// //// /保存实发奖励、消耗票券和原响应以供活动历史归档 ////
