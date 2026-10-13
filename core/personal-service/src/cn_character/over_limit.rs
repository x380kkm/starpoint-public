// audience: internal
// # personal-service-cn-character-over-limit
// 角色突破按副本或素材数量扣除消耗, 请求计数随 viewer 会话保存重放结果.

use super::{
    character_asset_data, character_limits, character_mut, character_time, character_value,
    error_response, serialize_character,
};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

pub(crate) const RECEIPTS_FIELD: &str = "character_request_receipts";

#[derive(Deserialize)]
struct OverLimitRequest {
    viewer_id: i64,
    character_id: i64,
    over_limit_count: i64,
    use_stack: bool,
    item_id: Option<i64>,
    api_count: Option<i64>,
}

#[derive(Deserialize)]
struct BulkOverLimitRequest {
    viewer_id: i64,
    api_count: Option<i64>,
}

// //// 消耗所有可用角色副本完成批量突破 [@x380kkm 2026-08-22] ////
pub(super) fn apply_bulk(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkOverLimitRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count.map_or(true, |count| count >= 0) => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "character",
            "bulk-over-limit",
            body.viewer_id,
            count,
            json!({}),
        )
    });
    if let Some(response) = receipt
        .as_ref()
        .map(|receipt| receipt.replay(root))
        .transpose()?
        .flatten()
    {
        return Ok(response);
    }
    let candidates = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN character list is missing"))?
        .iter()
        .filter_map(|(character_id, character)| {
            Some((
                character_id.parse::<i64>().ok()?,
                character.get("over_limit_step")?.as_i64()?,
                character.get("stack")?.as_i64()?,
            ))
        })
        .collect::<Vec<_>>();
    let response_time = server_time(database)?;
    let mut character_list = Vec::new();
    let mut total_over_limit_count = 0_i64;

    for (character_id, current_over_limit, current_stack) in candidates {
        let Ok((max_over_limit, _)) = character_limits(character_id) else {
            continue;
        };
        let count = current_stack.min(max_over_limit.saturating_sub(current_over_limit));
        if count <= 0 {
            continue;
        }
        total_over_limit_count = total_over_limit_count.checked_add(count).ok_or_else(|| {
            PersonalServiceError::new("CN bulk over-limit count exceeds supported range")
        })?;
        let character = character_mut(root, character_id).map_err(|_| {
            PersonalServiceError::new("stored CN character disappeared during bulk over limit")
        })?;
        character.insert(
            "over_limit_step".to_owned(),
            Value::from(current_over_limit + count),
        );
        character.insert("stack".to_owned(), Value::from(current_stack - count));
        character_list.push(serialize_character(
            body.viewer_id,
            character_id,
            character,
            response_time,
        ));
    }

    let mission_delta = crate::cn_mission::record_character_over_limit_action(
        root,
        database,
        snapshot.account_id,
        total_over_limit_count,
    )?;
    let mut response = json!({
        "character_list": character_list,
        "active_mission_list": mission_delta.active_mission_list,
        "mail_arrived": false,
    });
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &mission_delta.mission_info,
        body.viewer_id,
        response_time,
    )?;
    if !mission_delta.mission_info.is_empty() {
        response
            .as_object_mut()
            .expect("CN bulk over-limit response is an object")
            .insert(
                "mission_info".to_owned(),
                Value::Array(mission_delta.mission_info),
            );
    }
    if let Some(receipt) = receipt {
        receipt.record(root, &response, response_time)?;
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /消耗所有可用角色副本完成批量突破 ////

// //// 消耗选定素材突破角色并保存任务和请求凭据 [@x380kkm 2026-09-23] ////
pub(super) fn apply(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<OverLimitRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.character_id > 0
                && body.over_limit_count > 0
                && body.api_count.map_or(true, |count| count >= 0) =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    if !body.use_stack && body.item_id.is_none() {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "character",
            "over-limit",
            body.viewer_id,
            count,
            json!({
                "character_id":body.character_id,
                "over_limit_count":body.over_limit_count,
                "use_stack":body.use_stack,
                "item_id":body.item_id,
            }),
        )
    });
    if let Some(response) = receipt
        .as_ref()
        .map(|receipt| receipt.replay(root))
        .transpose()?
        .flatten()
    {
        return Ok(response);
    }
    let stored = match character_value(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    };
    let current_over_limit = stored
        .get("over_limit_step")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let new_over_limit = current_over_limit
        .checked_add(body.over_limit_count)
        .ok_or_else(|| PersonalServiceError::new("CN character over limit exceeds range"))?;
    let (max_over_limit, _) = match character_limits(body.character_id) {
        Ok(limits) => limits,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    if new_over_limit > max_over_limit {
        return Ok(error_response("400 Bad Request", "over_limit_exceeded"));
    }
    let current_stack = stored
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let mut response_items = Map::new();
    let after_stack = if body.use_stack {
        let after_stack = match current_stack.checked_sub(body.over_limit_count) {
            Some(after_stack) => after_stack,
            None => return Ok(error_response("400 Bad Request", "not_enough_stack")),
        };
        if after_stack < 0 {
            return Ok(error_response("400 Bad Request", "not_enough_stack"));
        }
        after_stack
    } else {
        let item_id = body.item_id.unwrap_or_default();
        let rarity = character_asset_data(body.character_id)?.rarity;
        if !matches!(
            (item_id, rarity),
            (10_001, 1..=3) | (10_002, 1..=4) | (10_003, 5)
        ) {
            return Ok(error_response("400 Bad Request", "invalid_uncapping_item"));
        }
        let item_list = require_object(root, "item_list")?;
        let current = item_list
            .get(&item_id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let after = match current.checked_sub(body.over_limit_count) {
            Some(after) => after,
            None => {
                return Ok(error_response(
                    "400 Bad Request",
                    "not_enough_uncapping_item",
                ))
            }
        };
        if after < 0 {
            return Ok(error_response(
                "400 Bad Request",
                "not_enough_uncapping_item",
            ));
        }
        item_list.insert(item_id.to_string(), Value::from(after));
        response_items.insert(item_id.to_string(), Value::from(after));
        current_stack
    };
    let character = match character_mut(root, body.character_id) {
        Ok(character) => character,
        Err(response) => return Ok(response),
    };
    character.insert("over_limit_step".to_owned(), Value::from(new_over_limit));
    character.insert("stack".to_owned(), Value::from(after_stack));
    let server_time = server_time(database)?;
    let response_character = json!({
        "character_id": body.character_id,
        "over_limit_step": new_over_limit,
        "stack": after_stack,
        "create_time": character_time(character.get("join_time"), server_time),
        "update_time": character_time(character.get("update_time"), server_time),
        "join_time": character_time(character.get("join_time"), server_time),
    });
    let mission_delta = crate::cn_mission::record_character_over_limit_action(
        root,
        database,
        snapshot.account_id,
        body.over_limit_count,
    )?;
    let mut response = json!({
        "character_list": [response_character],
        "item_list": response_items,
        "active_mission_list": mission_delta.active_mission_list,
        "mail_arrived": false,
    });
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &mission_delta.mission_info,
        body.viewer_id,
        server_time,
    )?;
    if !mission_delta.mission_info.is_empty() {
        response
            .as_object_mut()
            .expect("CN character over-limit response is an object")
            .insert(
                "mission_info".to_owned(),
                Value::Array(mission_delta.mission_info),
            );
    }
    if let Some(receipt) = receipt {
        receipt.record(root, &response, server_time)?;
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, server_time, response)
}
// //// /消耗选定素材突破角色并保存任务和请求凭据 ////
