// audience: internal
// # personal-service-cn-reference-item
//
// 该模块出售参考 CN 可出售物品并保存道具和玛纳余额.

use super::common::{add_user_info, error_response, json_document, required_i64};
use crate::cn::{decode_request, msgpack_response_at, server_time};
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
    let items = require_object(root, "item_list")?;
    let key = body.item_id.to_string();
    let current = items.get(&key).and_then(Value::as_i64).unwrap_or_default();
    if current < body.sell_number {
        return Ok(error_response("400 Bad Request", "not_enough_items"));
    }
    let updated = current - body.sell_number;
    items.insert(key.clone(), Value::from(updated));
    let mana_gain = price.checked_mul(body.sell_number).ok_or_else(|| {
        PersonalServiceError::new("CN item sale mana exceeds the supported range")
    })?;
    add_user_info(root, "free_mana", mana_gain)?;
    let free_mana = required_i64(require_object(root, "user_info")?, "free_mana")?;
    let item_updates = BTreeMap::from([(key, updated)]);
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "item_list": item_updates,
            "user_info": {"free_mana": free_mana},
            "mail_arrived": false,
        }),
    )
}
// //// /出售可出售物品并增加玛纳 ////
