// audience: internal
// # personal-service-cn-reference-item
//
// 道具出售收益遵循免费与付费玛纳合计上限, 资产和请求回执共同持久化.

use super::common::{add_user_info, error_response, json_document, required_i64};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const ITEM_SALE: &str = include_str!("../../assets/cn-item-sale.json");
static ITEM_SALE_DOCUMENT: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Deserialize)]
struct ItemSellRequest {
    viewer_id: i64,
    api_count: Option<i64>,
    item_id: i64,
    sell_number: i64,
}

// //// 出售可出售物品并增加玛纳 [@x380kkm 2026-08-22] ////
pub(super) fn sell(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<ItemSellRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.item_id > 0 && body.sell_number > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let document = json_document(&ITEM_SALE_DOCUMENT, ITEM_SALE, "item sale")?;
    let Some(definition) = document.get(body.item_id.to_string()) else {
        return Ok(error_response("400 Bad Request", "item_not_sellable"));
    };
    if !definition
        .get("sellable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(error_response("400 Bad Request", "item_not_sellable"));
    }
    let price = definition
        .get("sale_price")
        .and_then(Value::as_i64)
        .filter(|price| *price >= 0)
        .ok_or_else(|| PersonalServiceError::new("CN item sale price is invalid"))?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "item",
            "sell",
            body.viewer_id,
            count,
            json!({"item_id":body.item_id,"sell_number":body.sell_number}),
        )
    });
    if let Some(receipt) = &receipt {
        if let Some(response) = receipt.replay(root)? {
            return Ok(response);
        }
    }
    let items = require_object(root, "item_list")?;
    let key = body.item_id.to_string();
    let current = items.get(&key).and_then(Value::as_i64).unwrap_or_default();
    if current < body.sell_number {
        return Ok(error_response("400 Bad Request", "not_enough_items"));
    }
    let mana_gain = price.checked_mul(body.sell_number).ok_or_else(|| {
        PersonalServiceError::new("CN item sale mana exceeds the supported range")
    })?;
    let updated = current - body.sell_number;
    let user_info = require_object(root, "user_info")?;
    let held_mana = required_i64(user_info, "free_mana")?
        .checked_add(required_i64(user_info, "paid_mana")?)
        .ok_or_else(|| PersonalServiceError::new("CN mana balance exceeds the supported range"))?;
    if !crate::cn_reward_capacity::has_room(
        held_mana,
        mana_gain,
        crate::cn_reward_capacity::capacities()?.mana,
    ) {
        return Ok(error_response("400 Bad Request", "mana_capacity_exceeded"));
    }
    require_object(root, "item_list")?.insert(key.clone(), Value::from(updated));
    add_user_info(root, "free_mana", mana_gain)?;
    let response_time = server_time(database)?;
    let missions = crate::cn_mission::record_acquired_mana(
        root,
        database,
        snapshot.account_id,
        mana_gain,
        response_time,
    )?;
    let free_mana = required_i64(require_object(root, "user_info")?, "free_mana")?;
    let item_updates = BTreeMap::from([(key, updated)]);
    let mut response = json!({
        "item_list": item_updates,
        "user_info": {"free_mana": free_mana},
        "mission_info": missions,
        "mail_arrived": false,
    });
    crate::cn_mission::apply_mission_reward_response(
        &mut response,
        root,
        &missions,
        body.viewer_id,
        response_time,
    )?;
    if let Some(receipt) = receipt {
        receipt.record(root, &response, response_time)?;
    }
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, response)
}
// //// /出售可出售物品并增加玛纳 ////
