// audience: internal
// # personal-service-cn-multi-special-exchange
//
// 该模块处理 CN 多选角色兑换活动. 活动状态, 票券余额和角色奖励在同一玩家快照中提交.

use crate::cn::{
    decode_request, msgpack_response_at, msgpack_result_code_response_at, server_time,
};
use crate::cn_character_reward::grant_character;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const CAMPAIGN_ID: i64 = 5;
const TICKET_ITEM_ID: i64 = 980_007;
const CAMPAIGN_ERROR_RESULT_CODE: i64 = 4901;
const ALREADY_DRAWN_RESULT_CODE: i64 = 4902;

#[derive(Deserialize)]
struct SingleDrawTicketRequest {
    viewer_id: i64,
    campaign_id: i64,
    api_count: i64,
}

#[derive(Deserialize)]
struct MultiDrawTicketRequest {
    viewer_id: i64,
    campaign_id: i64,
}

#[derive(Deserialize)]
struct ExchangeCharacterRequest {
    viewer_id: i64,
    campaign_id: i64,
    character_id: i64,
    ticket_item_id: i64,
    api_count: i64,
}

// //// 分派 CN 多选角色兑换请求 [@x380kkm 2026-08-23] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        "/api/index.php/multi_special_exchange/single_draw_ticket" => {
            Some(single_draw_ticket(request, database))
        }
        "/api/index.php/multi_special_exchange/multi_draw_ticket" => {
            Some(multi_draw_ticket(request, database))
        }
        "/api/index.php/multi_special_exchange/exchange_character" => {
            Some(exchange_character(request, database))
        }
        _ => None,
    }
}
// //// /分派 CN 多选角色兑换请求 ////

// //// 领取 CN 多选角色兑换票券 [@x380kkm 2026-08-23] ////
fn single_draw_ticket(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SingleDrawTicketRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    draw_ticket(database, body.viewer_id, body.campaign_id)
}
// //// /领取 CN 多选角色兑换票券 ////

// //// 领取 CN 多抽角色兑换票券 [@x380kkm 2026-08-24] ////
fn multi_draw_ticket(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<MultiDrawTicketRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    draw_ticket(database, body.viewer_id, body.campaign_id)
}
// //// /领取 CN 多抽角色兑换票券 ////

fn draw_ticket(
    database: &mut ServiceDatabase,
    viewer_id: i64,
    campaign_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let snapshot = match player_snapshot(database, viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let status = if campaign_id == CAMPAIGN_ID {
        campaign_status(root, campaign_id)?
    } else {
        None
    };
    match status {
        Some(2) => {}
        Some(3 | 4) => {
            return msgpack_result_code_response_at(
                viewer_id,
                response_time,
                ALREADY_DRAWN_RESULT_CODE,
            )
        }
        _ => {
            return msgpack_result_code_response_at(
                viewer_id,
                response_time,
                CAMPAIGN_ERROR_RESULT_CODE,
            )
        }
    }

    let ticket_count = adjust_item_count(root, TICKET_ITEM_ID, 1)?;
    set_campaign_status(root, campaign_id, 3, Some(TICKET_ITEM_ID))?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        viewer_id,
        false,
        response_time,
        json!({
            "multi_special_exchange_campaign_list": [{
                "campaign_id": campaign_id,
                "status": 3,
                "ticket_item_id": TICKET_ITEM_ID,
            }],
            "item_list": {TICKET_ITEM_ID.to_string(): ticket_count},
            "mail_arrived": false,
        }),
    )
}

// //// 使用 CN 多选角色兑换票券 [@x380kkm 2026-08-23] ////
fn exchange_character(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ExchangeCharacterRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.character_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let request_matches_campaign =
        body.campaign_id == CAMPAIGN_ID && body.ticket_item_id == TICKET_ITEM_ID;
    let status = if request_matches_campaign {
        campaign_status(root, body.campaign_id)?
    } else {
        None
    };
    match status {
        Some(3) => {}
        Some(4) => {
            return msgpack_result_code_response_at(
                body.viewer_id,
                response_time,
                ALREADY_DRAWN_RESULT_CODE,
            )
        }
        _ => {
            return msgpack_result_code_response_at(
                body.viewer_id,
                response_time,
                CAMPAIGN_ERROR_RESULT_CODE,
            )
        }
    }
    if item_count(root, TICKET_ITEM_ID)? < 1 {
        return msgpack_result_code_response_at(
            body.viewer_id,
            response_time,
            CAMPAIGN_ERROR_RESULT_CODE,
        );
    }

    let ticket_count = adjust_item_count(root, TICKET_ITEM_ID, -1)?;
    let reward = grant_character(root, body.viewer_id, body.character_id, response_time)?;
    let mut encyclopedia_info = Map::new();
    if reward.joined
        && crate::cn_gacha::record_character_encyclopedia_state(root, body.character_id)?
    {
        encyclopedia_info.insert(format!("1{}01", body.character_id), json!({"read": false}));
    }
    set_campaign_status(root, body.campaign_id, 4, None)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "multi_special_exchange_campaign_list": [{
                "campaign_id": body.campaign_id,
                "status": 4,
            }],
            "character_list": [reward.character],
            "item_list": {TICKET_ITEM_ID.to_string(): ticket_count},
            "encyclopedia_info": encyclopedia_info,
            "mail_arrived": false,
        }),
    )
}
// //// /使用 CN 多选角色兑换票券 ////

fn campaign_status(
    root: &Map<String, Value>,
    campaign_id: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    let campaigns = root
        .get("multi_special_exchange_campaign_list")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            PersonalServiceError::new("stored CN multi special exchange campaigns are missing")
        })?;
    Ok(campaigns.iter().find_map(|campaign| {
        (campaign.get("campaign_id").and_then(Value::as_i64) == Some(campaign_id))
            .then(|| campaign.get("status").and_then(Value::as_i64))
            .flatten()
    }))
}

fn set_campaign_status(
    root: &mut Map<String, Value>,
    campaign_id: i64,
    status: i64,
    ticket_item_id: Option<i64>,
) -> Result<(), PersonalServiceError> {
    let campaigns = root
        .get_mut("multi_special_exchange_campaign_list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            PersonalServiceError::new("stored CN multi special exchange campaigns are missing")
        })?;
    let campaign = campaigns
        .iter_mut()
        .find(|campaign| campaign.get("campaign_id").and_then(Value::as_i64) == Some(campaign_id))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            PersonalServiceError::new("stored CN multi special exchange campaign is missing")
        })?;
    campaign.insert("status".to_owned(), Value::from(status));
    match ticket_item_id {
        Some(ticket_item_id) => {
            campaign.insert("ticket_item_id".to_owned(), Value::from(ticket_item_id));
        }
        None => {
            campaign.remove("ticket_item_id");
        }
    }
    Ok(())
}

fn item_count(root: &mut Map<String, Value>, item_id: i64) -> Result<i64, PersonalServiceError> {
    Ok(require_object(root, "item_list")?
        .get(&item_id.to_string())
        .and_then(Value::as_i64)
        .unwrap_or_default())
}

fn adjust_item_count(
    root: &mut Map<String, Value>,
    item_id: i64,
    change: i64,
) -> Result<i64, PersonalServiceError> {
    let items = require_object(root, "item_list")?;
    let key = item_id.to_string();
    let total = items
        .get(&key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(change)
        .filter(|total| *total >= 0)
        .ok_or_else(|| {
            PersonalServiceError::new("CN multi special exchange ticket count exceeds range")
        })?;
    items.insert(key, Value::from(total));
    Ok(total)
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
