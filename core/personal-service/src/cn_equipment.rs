// audience: internal
// # personal-service-cn-equipment
//
// 该模块实现 CN 装备升级和分解协议. 觉醒消耗副本或星铁钢和锻造玉, 分解按主数据返还物品.

mod dissolve;
mod reward;
mod upgrade;

pub(crate) use reward::grant as grant_equipment;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_root,
};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const WRIGHTPIECE_ITEM_ID: i64 = 100_000;

#[derive(Deserialize)]
struct SetProtectionRequest {
    viewer_id: i64,
    protection: bool,
    equipment_ids: Vec<i64>,
    #[serde(default)]
    api_count: i64,
}

// //// 分派 CN 装备养成请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    match request.path() {
        "/api/index.php/equipment/upgrade" => Some(upgrade::single(request, database)),
        "/api/index.php/equipment/bulk_upgrade" => Some(upgrade::bulk(request, database)),
        "/api/index.php/equipment/set_protection" => Some(set_protection(request, database)),
        "/api/index.php/equipment/sell_equipment" => {
            Some(dissolve::sell_equipment(request, database))
        }
        "/api/index.php/equipment/sell_stack" => Some(dissolve::sell_stack(request, database)),
        "/api/index.php/equipment/bulk_sell_stack" => {
            Some(dissolve::bulk_sell_stack(request, database))
        }
        _ => None,
    }
}

// //// /分派 CN 装备养成请求 ////

fn set_protection(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SetProtectionRequest>(request) {
        Ok(body)
            if body.viewer_id > 0 && body.api_count >= 0 && body.equipment_ids.len() <= 100 =>
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
    let equipment_list = root
        .get_mut("user_equipment_list")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_equipment_list is missing"))?;
    for equipment_id in &body.equipment_ids {
        if *equipment_id <= 0 {
            return Ok(error_response("400 Bad Request", "invalid_equipment_id"));
        }
        let Some(equipment) = equipment_list.get_mut(&equipment_id.to_string()) else {
            continue;
        };
        let equipment = equipment
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
        equipment.insert("protection".to_owned(), Value::from(body.protection));
    }
    let response_time = server_time(database)?;
    database.save_player_snapshot(snapshot.account_id, &encode_player_data(&player_data)?)?;
    msgpack_response_at(body.viewer_id, false, response_time, json!({}))
}

fn add_item(
    item_list: &mut Map<String, Value>,
    response_items: &mut Map<String, Value>,
    item_id: i64,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    if amount == 0 {
        return Ok(());
    }
    let key = item_id.to_string();
    let current = item_list
        .get(&key)
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let total = current
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN item count exceeds range"))?;
    item_list.insert(key.clone(), Value::from(total));
    response_items.insert(key, Value::from(total));
    Ok(())
}

fn serialize_equipment(equipment_id: i64, equipment: &Map<String, Value>) -> Value {
    json!({
        "equipment_id": equipment_id,
        "protection": equipment.get("protection").and_then(Value::as_bool).unwrap_or(false),
        "level": equipment.get("level").and_then(Value::as_i64).unwrap_or(1),
        "enhancement_level": equipment.get("enhancement_level").and_then(Value::as_i64).unwrap_or_default(),
        "stack": equipment.get("stack").and_then(Value::as_i64).unwrap_or_default(),
    })
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}
