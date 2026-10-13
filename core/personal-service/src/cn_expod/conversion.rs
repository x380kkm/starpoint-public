// audience: internal
// # personal-service-cn-expod-conversion
// 角色副本按星级转换为 EXP, 来源为 0 的角色同时产生星粒. 批量转换只处理突破已满且未保护的角色, 奖励与请求凭据共同保存.

use super::settlement::{conversion_history, ExpodSettlement};
use super::{
    character_rarity, error_response, read_character, require_character, serialize_character,
    stack_conversion, CHARACTER_EXP_CAPS, REWARD_ITEM_ID,
};
use crate::cn::{decode_request, server_time};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{decode_player_data, player_snapshot, require_object, require_root};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const CHARACTER_SOURCES: &str = include_str!("../../assets/cn-character-obtain-sources.json");
static CHARACTER_SOURCE_DATA: OnceLock<Result<BTreeMap<i64, i64>, String>> = OnceLock::new();

// //// 按角色来源计算每个副本的 EXP 和星粒 [@x380kkm 2026-09-23] ////
pub(crate) fn calculate_conversion_rewards(
    character_id: i64,
    rarity: i64,
) -> Result<(i64, i64), PersonalServiceError> {
    let sources = CHARACTER_SOURCE_DATA
        .get_or_init(|| serde_json::from_str(CHARACTER_SOURCES).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let source = sources
        .get(&character_id)
        .ok_or_else(|| PersonalServiceError::new("CN character obtain source is missing"))?;
    let (experience, star_grain) = stack_conversion(rarity)?;
    Ok((experience, if *source == 0 { star_grain } else { 0 }))
}
// //// /按角色来源计算每个副本的 EXP 和星粒 ////

#[derive(Deserialize)]
struct StackToExpRequest {
    character_id: i64,
    api_count: Option<i64>,
    number: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct BulkStackToExpRequest {
    viewer_id: i64,
    api_count: Option<i64>,
}

// //// 将已满突破角色的全部副本批量转换 [@x380kkm 2026-08-22] ////
pub(super) fn bulk(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkStackToExpRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count.map_or(true, |count| count >= 0) => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "expod",
            "bulk-stack-to-exp",
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
    let character_snapshot = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("stored CN character list is missing"))?;
    let mut conversions = Vec::new();
    let mut total_exp = 0_i64;
    let mut total_items = 0_i64;

    for (character_id, character) in character_snapshot {
        let Ok(character_id) = character_id.parse::<i64>() else {
            continue;
        };
        let Some(character) = character.as_object() else {
            continue;
        };
        let stack = character
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if stack <= 0
            || character
                .get("protection")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let Ok(rarity) = character_rarity(character_id) else {
            continue;
        };
        let max_over_limit = CHARACTER_EXP_CAPS
            .get(rarity as usize)
            .map(|levels| levels.len().saturating_sub(1) as i64)
            .unwrap_or_default();
        let over_limit = character
            .get("over_limit_step")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if over_limit < max_over_limit {
            continue;
        }
        let (exp_per_stack, item_per_stack) = calculate_conversion_rewards(character_id, rarity)?;
        total_exp =
            total_exp
                .checked_add(exp_per_stack.checked_mul(stack).ok_or_else(|| {
                    PersonalServiceError::new("CN bulk converted EXP exceeds range")
                })?)
                .ok_or_else(|| PersonalServiceError::new("CN bulk converted EXP exceeds range"))?;
        total_items = total_items
            .checked_add(item_per_stack.checked_mul(stack).ok_or_else(|| {
                PersonalServiceError::new("CN bulk converted item count exceeds range")
            })?)
            .ok_or_else(|| {
                PersonalServiceError::new("CN bulk converted item count exceeds range")
            })?;
        conversions.push(character_id);
    }

    crate::cn_exp_pool::settle(root, response_time)?;
    let user_info = require_object(root, "user_info")?;
    let current_exp_pool = user_info
        .get("exp_pool")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let exp_pooled_time = user_info
        .get("exp_pooled_time")
        .cloned()
        .unwrap_or(Value::from(response_time));
    let new_exp_pool = current_exp_pool
        .checked_add(total_exp)
        .ok_or_else(|| PersonalServiceError::new("CN EXP pool exceeds range"))?;
    user_info.insert("exp_pool".to_owned(), Value::from(new_exp_pool));

    let item_list = require_object(root, "item_list")?;
    let current_items = item_list
        .get(REWARD_ITEM_ID)
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let new_items = current_items
        .checked_add(total_items)
        .ok_or_else(|| PersonalServiceError::new("CN EXP item count exceeds range"))?;
    if total_items > 0 {
        item_list.insert(REWARD_ITEM_ID.to_owned(), Value::from(new_items));
    }

    let mut response_characters = Vec::with_capacity(conversions.len());
    for character_id in conversions {
        let character = require_character(root, character_id)?;
        character.insert("stack".to_owned(), Value::from(0));
        response_characters.push(serialize_character(
            body.viewer_id,
            character_id,
            character,
            response_time,
        ));
    }
    let response_items = root
        .get("item_list")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("stored CN item list is missing"))?;
    let response = json!({
        "character_list": response_characters,
        "converted_exp_info": {"add_exp": total_exp},
        "item_list": response_items,
        "user_info": {
            "exp_pool": new_exp_pool,
            "exp_pooled_time": exp_pooled_time,
        },
        "mail_arrived": false,
    });
    ExpodSettlement {
        viewer_id: body.viewer_id,
        account_id: snapshot.account_id,
        response_time,
        receipt,
    }
    .save(
        database,
        &mut player_data,
        response,
        &conversion_history(total_exp, total_items),
    )
}
// //// /将已满突破角色的全部副本批量转换 ////

// //// 把角色副本转换为 EXP 和奖励道具 [@x380kkm 2026-07-24] ////
pub(super) fn single(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<StackToExpRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.character_id > 0
                && body.api_count.map_or(true, |count| count >= 0)
                && body.number > 0 =>
        {
            body
        }
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
            "expod",
            "stack-to-exp",
            body.viewer_id,
            count,
            json!({"character_id":body.character_id,"number":body.number}),
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
    let rarity = match character_rarity(body.character_id) {
        Ok(rarity) => rarity,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_found")),
    };
    let character_data = match read_character(root, body.character_id) {
        Ok(character) => character,
        Err(_) => return Ok(error_response("400 Bad Request", "character_not_owned")),
    };
    if character_data
        .get("protection")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(error_response("400 Bad Request", "character_protected"));
    }
    let stack = character_data
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let after_stack = stack.checked_sub(body.number).ok_or_else(|| {
        PersonalServiceError::new("CN character stack exceeds the supported range")
    })?;
    if after_stack < 0 {
        return Ok(error_response("400 Bad Request", "not_enough_stack"));
    }
    let (exp_per_stack, item_per_stack) = calculate_conversion_rewards(body.character_id, rarity)?;
    let increase_exp = exp_per_stack
        .checked_mul(body.number)
        .ok_or_else(|| PersonalServiceError::new("CN converted EXP exceeds the supported range"))?;
    let increase_items = item_per_stack.checked_mul(body.number).ok_or_else(|| {
        PersonalServiceError::new("CN converted item count exceeds the supported range")
    })?;
    let server_time = server_time(database)?;
    crate::cn_exp_pool::settle(root, server_time)?;
    let user_info = require_object(root, "user_info")?;
    let exp_pool = user_info
        .get("exp_pool")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let after_exp_pool = exp_pool
        .checked_add(increase_exp)
        .ok_or_else(|| PersonalServiceError::new("CN EXP pool exceeds the supported range"))?;
    user_info.insert("exp_pool".to_owned(), Value::from(after_exp_pool));
    let exp_pooled_time = user_info
        .get("exp_pooled_time")
        .cloned()
        .unwrap_or(Value::from(server_time));
    let item_list = require_object(root, "item_list")?;
    let current_items = item_list
        .get(REWARD_ITEM_ID)
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let after_items = current_items.checked_add(increase_items).ok_or_else(|| {
        PersonalServiceError::new("CN EXP item count exceeds the supported range")
    })?;
    if increase_items > 0 {
        item_list.insert(REWARD_ITEM_ID.to_owned(), Value::from(after_items));
    }
    let character = require_character(root, body.character_id)?;
    character.insert("stack".to_owned(), Value::from(after_stack));
    let character_response =
        serialize_character(body.viewer_id, body.character_id, character, server_time);
    let response = json!({
        "user_info": {
            "exp_pool": after_exp_pool,
            "exp_pooled_time": exp_pooled_time,
        },
        "character_list": [character_response],
        "converted_exp_info": {"add_exp": increase_exp},
        "item_list": {REWARD_ITEM_ID: after_items},
        "mail_arrived": false,
    });
    ExpodSettlement {
        viewer_id: body.viewer_id,
        account_id: snapshot.account_id,
        response_time: server_time,
        receipt,
    }
    .save(
        database,
        &mut player_data,
        response,
        &conversion_history(increase_exp, increase_items),
    )
}
// //// /把角色副本转换为 EXP 和奖励道具 ////
