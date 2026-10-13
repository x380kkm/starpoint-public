// audience: internal
// # personal-service-reward-history-api
// 管理 API 按本地存档展示到账资产, 商品领取和邮件投递状态. 响应仅包含展示字段.

use crate::database::{
    mail_reward_history_entries, RewardHistoryRecord, ServiceDatabase, StoredMail,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::{management, PersonalServiceError};
use serde_json::{json, Value};

// //// 分派存档的到账和投递记录请求 [@x380kkm 2026-09-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let (slot, action) = request
        .path()
        .strip_prefix("/v1/local-saves/")?
        .split_once('/')?;
    if !matches!(action, "reward-history" | "reward-deliveries" | "product-history") {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    if request.method() != "GET" {
        return Some(Ok(error("405 Method Not Allowed", "method_not_allowed")));
    }
    let Some(slot_id) = slot.parse::<i64>().ok().filter(|value| *value > 0) else {
        return Some(Ok(error("400 Bad Request", "invalid_local_save_id")));
    };
    Some(handle(request, database, slot_id, action))
}
// //// /分派存档的到账和投递记录请求 ////

// //// 使用当前存档关联账号读取奖励记录 [@x380kkm 2026-09-22] ////
fn handle(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: i64,
    action: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(context) = database.local_save_context(slot_id)? else {
        return Ok(error("404 Not Found", "local_save_not_found"));
    };
    let before_id = match query_before_id(request) {
        Ok(value) => value,
        Err(()) => return Ok(error("400 Bad Request", "invalid_history_cursor")),
    };
    let mut payload = match action {
        "product-history" => {
            let page = database.product_history_page(context.account_id, before_id)?;
            json!({"entries": page.entries, "next_before_id": page.next_before_id})
        }
        "reward-history" => {
            let page = database.reward_history_page(context.account_id, before_id)?;
            json!({"entries": page.entries.into_iter().map(serialize_received).collect::<Vec<_>>(), "next_before_id": page.next_before_id})
        }
        _ => {
            let now = crate::cn::server_time(database)?;
            let page = database.reward_delivery_page(context.account_id, before_id)?;
            json!({"entries": page.mails.into_iter().map(|mail| serialize_delivery(mail, now)).collect::<Vec<_>>(), "total": page.total, "next_before_id": page.next_before_id})
        }
    };
    payload["slot_id"] = Value::from(slot_id);
    payload["slot_name"] = Value::from(context.slot.name);
    Ok(HttpResponse::json("200 OK", payload.to_string()))
}
// //// /使用当前存档关联账号读取奖励记录 ////

fn query_before_id(request: &HttpRequest) -> Result<Option<i64>, ()> {
    let query = request
        .target()
        .split_once('?')
        .map(|(_, value)| value)
        .unwrap_or_default();
    let mut result = None;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key != "before_id" || result.is_some() {
            return Err(());
        }
        result = Some(
            value
                .parse::<i64>()
                .ok()
                .filter(|number| *number > 0)
                .ok_or(())?,
        );
    }
    Ok(result)
}

// //// 投影实际到账记录的展示字段 [@x380kkm 2026-09-22] ////
fn serialize_received(record: RewardHistoryRecord) -> Value {
    let source = match record.source.as_str() {
        "payment" => "product",
        "mail" => "mail",
        "login-bonus" | "subscription" => "login",
        "mission" | "mission-receipt" | "active-mission" | "pass-card" | "growth-fund" => "mission",
        "shop" | "exchange" | "multi-special-exchange" => "exchange",
        "gacha" => "gacha",
        _ => "reward",
    };
    let reward = record.reward;
    json!({"id": record.id, "created_at": reward.create_time, "source": source,
        "kind": reward.kind, "type_id": reward.type_id, "number": reward.number,
        "reason_id": reward.reason_id, "subject": reward.subject, "description": reward.description})
}
// //// /投影实际到账记录的展示字段 ////

// //// 展示邮件投递内容和当前领取状态 [@x380kkm 2026-09-22] ////
fn serialize_delivery(mail: StoredMail, now: i64) -> Value {
    let status = if mail.received_at.is_some() {
        "received"
    } else if mail.expires_at.is_some_and(|expiry| expiry <= now) {
        "expired"
    } else {
        "pending"
    };
    let rewards = mail_reward_history_entries(&mail.rewards).into_iter().map(|reward| {
        json!({"kind": reward.kind, "type_id": reward.type_id, "number": reward.number})
    }).collect::<Vec<_>>();
    json!({"id": mail.id, "title": mail.title, "body": mail.body, "sender": mail.sender,
        "created_at": mail.created_at, "expires_at": mail.expires_at, "received_at": mail.received_at,
        "status": status, "rewards": rewards})
}
// //// /展示邮件投递内容和当前领取状态 ////

fn error(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, json!({"error": code}).to_string())
}
