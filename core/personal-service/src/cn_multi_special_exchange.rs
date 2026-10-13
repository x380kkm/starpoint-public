// audience: internal
// # personal-service-cn-multi-special-exchange
// 活动状态、票券、角色和原结算响应随玩家快照提交, 管理活动时窗与客户端候选主表共同约束兑换.

mod catalog;
mod state;

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_character_reward::{grant_character, record_character_encyclopedia_state};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use catalog::{definitions, Campaign, FAMILY};
use serde::Deserialize;
use serde_json::{json, Map, Value};

const CAMPAIGN_ERROR_RESULT_CODE: i64 = 4901;
const ALREADY_DRAWN_RESULT_CODE: i64 = 4902;

#[derive(Deserialize)]
struct DrawTicketRequest {
    viewer_id: i64,
    campaign_id: i64,
    #[serde(default)]
    api_count: Option<i64>,
}

#[derive(Deserialize)]
struct ExchangeCharacterRequest {
    viewer_id: i64,
    campaign_id: i64,
    character_id: i64,
    ticket_item_id: i64,
    api_count: i64,
}

enum TicketClaim {
    Grant(i64),
    Recover(i64),
}

type TicketSelector = fn(
    &Map<String, Value>,
    &ServiceDatabase,
    i64,
    &Campaign,
) -> Result<Result<TicketClaim, i64>, PersonalServiceError>;

// //// 将有效活动状态投影到客户端列表 [@x380kkm 2026-09-22] ////
pub(crate) fn refresh_campaigns(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
) -> Result<(), PersonalServiceError> {
    let mut campaigns = Vec::new();
    for (id, campaign) in &definitions()?.campaigns {
        state::initialize(root, *id, campaign)?;
        if campaign.can_exchange(database, *id)? {
            campaigns.push(state::response_state(root, *id));
        }
    }
    root.insert(
        "multi_special_exchange_campaign_list".to_owned(),
        json!(campaigns),
    );
    Ok(())
}
// //// /将有效活动状态投影到客户端列表 ////

pub(crate) fn has_campaign(activity_id: &str) -> Result<bool, PersonalServiceError> {
    Ok(campaign_for_activity(activity_id)?.is_some())
}

pub(crate) fn reward_period_millis(activity_id: &str) -> Result<Option<i64>, PersonalServiceError> {
    Ok(campaign_for_activity(activity_id)?
        .map(|campaign| campaign.exchange_end_at_ms - campaign.end_at_ms))
}

fn campaign_for_activity(
    activity_id: &str,
) -> Result<Option<&'static Campaign>, PersonalServiceError> {
    let Some(id) = activity_id
        .strip_prefix(&format!("{FAMILY}:"))
        .and_then(|id| id.parse::<i64>().ok())
    else {
        return Ok(None);
    };
    Ok(definitions()?.campaigns.get(&id))
}

// //// 分派单人领券、多人补领和角色兑换 [@x380kkm 2026-09-22] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        "/api/index.php/multi_special_exchange/single_draw_ticket" => {
            Some(draw_ticket(request, database, select_single_ticket))
        }
        "/api/index.php/multi_special_exchange/multi_draw_ticket" => {
            Some(draw_ticket(request, database, select_multi_ticket))
        }
        "/api/index.php/multi_special_exchange/exchange_character" => {
            Some(exchange_character(request, database))
        }
        _ => None,
    }
}
// //// /分派单人领券、多人补领和角色兑换 ////

// //// 按单人流程发放主表首组票券 [@x380kkm 2026-09-22] ////
fn select_single_ticket(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    id: i64,
    campaign: &Campaign,
) -> Result<Result<TicketClaim, i64>, PersonalServiceError> {
    if !campaign.can_draw(database, id)? {
        return Ok(Err(CAMPAIGN_ERROR_RESULT_CODE));
    }
    Ok(match state::status(root, id) {
        1 => campaign
            .ticket_ids
            .first()
            .copied()
            .map(TicketClaim::Grant)
            .ok_or(CAMPAIGN_ERROR_RESULT_CODE),
        2..=4 => Err(ALREADY_DRAWN_RESULT_CODE),
        _ => Err(CAMPAIGN_ERROR_RESULT_CODE),
    })
}
// //// /按单人流程发放主表首组票券 ////

// //// 补领多人已决定的票券或恢复已到账票券 [@x380kkm 2026-09-22] ////
fn select_multi_ticket(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    id: i64,
    campaign: &Campaign,
) -> Result<Result<TicketClaim, i64>, PersonalServiceError> {
    if !campaign.can_exchange(database, id)? {
        return Ok(Err(CAMPAIGN_ERROR_RESULT_CODE));
    }
    if state::status(root, id) == 4 {
        return Ok(Err(ALREADY_DRAWN_RESULT_CODE));
    }
    let Some(ticket) =
        state::ticket(root, id).filter(|ticket| campaign.ticket_ids.contains(ticket))
    else {
        return Ok(Err(CAMPAIGN_ERROR_RESULT_CODE));
    };
    Ok(match state::status(root, id) {
        2 => Ok(TicketClaim::Grant(ticket)),
        3 => Ok(TicketClaim::Recover(ticket)),
        _ => Err(CAMPAIGN_ERROR_RESULT_CODE),
    })
}
// //// /补领多人已决定的票券或恢复已到账票券 ////

// //// 结算票券并持久保存领取凭据 [@x380kkm 2026-09-22] ////
fn draw_ticket(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    select_ticket: TicketSelector,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<DrawTicketRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.campaign_id > 0
                && body.api_count.map_or(true, |count| count >= 0) =>
        {
            body
        }
        _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let identity = json!({"action":request.path(),"viewer_id":body.viewer_id,"api_count":body.api_count,"campaign_id":body.campaign_id});
    if let Some(response) = state::replay(root, &identity)? {
        return Ok(response);
    }
    let now = server_time(database)?;
    let Some(campaign) = definitions()?.campaigns.get(&body.campaign_id) else {
        return msgpack_result_code_response_at(body.viewer_id, now, CAMPAIGN_ERROR_RESULT_CODE);
    };
    state::initialize(root, body.campaign_id, campaign)?;
    let ticket = match select_ticket(root, database, body.campaign_id, campaign)? {
        Ok(TicketClaim::Grant(ticket)) => ticket,
        Ok(TicketClaim::Recover(ticket)) => {
            return msgpack_response_at(
                body.viewer_id,
                false,
                now,
                ticket_response(root, body.campaign_id, ticket)?,
            )
        }
        Err(code) => return msgpack_result_code_response_at(body.viewer_id, now, code),
    };
    adjust_item_count(root, ticket, 1)?;
    state::set_status(root, body.campaign_id, 3, Some(ticket))?;
    let response = ticket_response(root, body.campaign_id, ticket)?;
    let rewards = [ReceiveHistoryEntry::reward(1, Some(ticket), 1)];
    let event_key = state::record(
        root,
        body.campaign_id,
        "draw_receipt",
        state::Receipt {
            request: identity,
            response: &response,
            rewards: &rewards,
            consumed_ticket: None,
            received_at: now,
        },
    )?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &event_key,
        now,
        &rewards,
    )?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
// //// /结算票券并持久保存领取凭据 ////

fn ticket_response(
    root: &Map<String, Value>,
    id: i64,
    ticket: i64,
) -> Result<Value, PersonalServiceError> {
    Ok(json!({
        "multi_special_exchange_campaign_list":[state::response_state(root, id)],
        "item_list":{ticket.to_string():item_count(root, ticket)?},"mail_arrived":false,
    }))
}

// //// 消耗活动票券兑换候选角色并推进对应任务 [@x380kkm 2026-09-22] ////
fn exchange_character(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ExchangeCharacterRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.campaign_id > 0
                && body.character_id > 0
                && body.api_count >= 0 =>
        {
            body
        }
        _ => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let identity = json!({
        "action":request.path(),"viewer_id":body.viewer_id,"api_count":body.api_count,
        "campaign_id":body.campaign_id,"ticket_item_id":body.ticket_item_id,"character_id":body.character_id,
    });
    if let Some(response) = state::replay(root, &identity)? {
        return Ok(response);
    }
    let now = server_time(database)?;
    let catalog = definitions()?;
    let Some(campaign) = catalog.campaigns.get(&body.campaign_id) else {
        return msgpack_result_code_response_at(body.viewer_id, now, CAMPAIGN_ERROR_RESULT_CODE);
    };
    state::initialize(root, body.campaign_id, campaign)?;
    if !campaign.can_exchange(database, body.campaign_id)? {
        return msgpack_result_code_response_at(body.viewer_id, now, CAMPAIGN_ERROR_RESULT_CODE);
    }
    if state::status(root, body.campaign_id) == 4 {
        return msgpack_result_code_response_at(body.viewer_id, now, ALREADY_DRAWN_RESULT_CODE);
    }
    let candidate = catalog
        .tickets
        .get(&body.ticket_item_id)
        .is_some_and(|characters| characters.contains(&body.character_id));
    if state::status(root, body.campaign_id) != 3
        || state::ticket(root, body.campaign_id) != Some(body.ticket_item_id)
        || !campaign.ticket_ids.contains(&body.ticket_item_id)
        || !candidate
        || item_count(root, body.ticket_item_id)? < 1
    {
        return msgpack_result_code_response_at(body.viewer_id, now, CAMPAIGN_ERROR_RESULT_CODE);
    }
    let count = adjust_item_count(root, body.ticket_item_id, -1)?;
    let reward = grant_character(root, body.viewer_id, body.character_id, now)?;
    let encyclopedia = record_character_encyclopedia_state(root, body.character_id)?;
    let mut missions = crate::cn_mission::record_active_campaign_action(
        root,
        database,
        "multi_special_exchange",
        body.campaign_id,
        1,
    )?;
    missions.extend(crate::cn_mission::refresh_active_missions(root, database)?);
    let mut items = Map::from_iter([(body.ticket_item_id.to_string(), json!(count))]);
    let mut rewards = vec![ReceiveHistoryEntry::reward(5, Some(body.character_id), 1)];
    if let Some(duplicate) = reward.duplicate_item {
        items.insert(duplicate.id.to_string(), json!(duplicate.total));
        rewards.push(ReceiveHistoryEntry::reward(
            1,
            Some(duplicate.id),
            duplicate.count,
        ));
    }
    state::set_status(root, body.campaign_id, 4, None)?;
    let mut response = json!({
        "multi_special_exchange_campaign_list":[state::response_state(root, body.campaign_id)],
        "character_list":[reward.character],"item_list":items,"encyclopedia_info":encyclopedia,
        "active_mission_list":missions,"mail_arrived":false,
    });
    crate::cn_reward_overflow::apply_response(&mut response, root)?;
    let event_key = state::record(
        root,
        body.campaign_id,
        "exchange_receipt",
        state::Receipt {
            request: identity,
            response: &response,
            rewards: &rewards,
            consumed_ticket: Some(body.ticket_item_id),
            received_at: now,
        },
    )?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &event_key,
        now,
        &rewards,
    )?;
    msgpack_response_at(body.viewer_id, false, now, response)
}
// //// /消耗活动票券兑换候选角色并推进对应任务 ////

fn item_count(root: &Map<String, Value>, item_id: i64) -> Result<i64, PersonalServiceError> {
    let items = root
        .get("item_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("missing CN item inventory"))?;
    items.get(&item_id.to_string()).map_or(Ok(0), |count| {
        count.as_i64().filter(|count| *count >= 0).ok_or_else(|| {
            PersonalServiceError::new("invalid CN multi special exchange ticket count")
        })
    })
}

fn adjust_item_count(
    root: &mut Map<String, Value>,
    item_id: i64,
    change: i64,
) -> Result<i64, PersonalServiceError> {
    let total = item_count(root, item_id)?
        .checked_add(change)
        .filter(|count| *count >= 0)
        .ok_or_else(|| {
            PersonalServiceError::new("CN multi special exchange ticket count exceeds range")
        })?;
    require_object(root, "item_list")?.insert(item_id.to_string(), json!(total));
    Ok(total)
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, json!({"error":code}).to_string())
}
