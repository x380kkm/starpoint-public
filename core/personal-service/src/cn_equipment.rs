// audience: internal
// # personal-service-cn-equipment
//
// 该模块实现 CN 装备升级和分解协议. 升级消耗装备栈和 wrightpiece, 分解按主数据返还物品.

mod dissolve;

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};

const WRIGHTPIECE_ITEM_ID: i64 = 100_000;
const MAX_EQUIPMENT_LEVEL: i64 = 5;
const UPGRADE_COST_BY_RARITY: [i64; 5] = [5, 10, 15, 20, 25];

#[derive(Deserialize)]
struct UpgradeRequest {
    viewer_id: i64,
    equipment_id: i64,
    upgrade_count: i64,
    use_stack: bool,
    item_id: Option<i64>,
    #[serde(default)]
    api_count: i64,
}

#[derive(Deserialize)]
struct SetProtectionRequest {
    viewer_id: i64,
    protection: bool,
    equipment_ids: Vec<i64>,
    #[serde(default)]
    api_count: i64,
}

#[derive(Deserialize)]
struct BulkEquipmentRequest {
    viewer_id: i64,
    #[serde(default)]
    api_count: i64,
    equipment_ids: Vec<i64>,
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
        "/api/index.php/equipment/upgrade" => Some(upgrade(request, database)),
        "/api/index.php/equipment/bulk_upgrade" => Some(bulk_upgrade(request, database)),
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

// //// 批量消耗装备副本完成觉醒 [@x380kkm 2026-08-22] ////
fn bulk_upgrade(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkEquipmentRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count >= 0
                && !body.equipment_ids.is_empty()
                && body.equipment_ids.len() <= 100 =>
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
    let equipment_snapshot = root
        .get("user_equipment_list")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| PersonalServiceError::new("stored CN equipment list is missing"))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut upgrades = Vec::new();
    let mut total_cost = 0_i64;

    for equipment_id in body.equipment_ids {
        if equipment_id <= 0 || !seen.insert(equipment_id) {
            continue;
        }
        let Some(equipment) = equipment_snapshot
            .get(&equipment_id.to_string())
            .and_then(Value::as_object)
        else {
            continue;
        };
        let level = equipment.get("level").and_then(Value::as_i64).unwrap_or(1);
        let stack = equipment
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let count = MAX_EQUIPMENT_LEVEL.saturating_sub(level).min(stack);
        if count <= 0 {
            continue;
        }
        let unit_cost = equipment_upgrade_cost(equipment_id);
        total_cost =
            total_cost
                .checked_add(unit_cost.checked_mul(count).ok_or_else(|| {
                    PersonalServiceError::new("CN bulk equipment cost exceeds range")
                })?)
                .ok_or_else(|| PersonalServiceError::new("CN bulk equipment cost exceeds range"))?;
        upgrades.push((equipment_id, level, stack, count));
    }

    if upgrades.is_empty() {
        return msgpack_response_at(
            body.viewer_id,
            false,
            server_time(database)?,
            json!({"equipment_list": [], "item_list": {}, "mail_arrived": false}),
        );
    }

    let item_list = require_object(root, "item_list")?;
    let current_wrightpieces = item_list
        .get(&WRIGHTPIECE_ITEM_ID.to_string())
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let remaining_wrightpieces = match current_wrightpieces.checked_sub(total_cost) {
        Some(value) if value >= 0 => value,
        _ => return Ok(error_response("400 Bad Request", "not_enough_wrightpiece")),
    };
    let mut response_items = Map::new();
    item_list.insert(
        WRIGHTPIECE_ITEM_ID.to_string(),
        Value::from(remaining_wrightpieces),
    );
    response_items.insert(
        WRIGHTPIECE_ITEM_ID.to_string(),
        Value::from(remaining_wrightpieces),
    );
    let mut history_entries = Vec::new();
    for (equipment_id, _, _, count) in &upgrades {
        for (item_id, reward_count) in dissolve::ability_soul_rewards(*equipment_id, *count)? {
            add_item(item_list, &mut response_items, item_id, reward_count)?;
            history_entries.push(ReceiveHistoryEntry::reward(1, Some(item_id), reward_count));
        }
    }

    let equipment_list = require_object(root, "user_equipment_list")?;
    let mut response_equipment = Vec::with_capacity(upgrades.len());
    for (equipment_id, level, stack, count) in upgrades {
        let equipment = equipment_list
            .get_mut(&equipment_id.to_string())
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
        equipment.insert("level".to_owned(), Value::from(level + count));
        equipment.insert("stack".to_owned(), Value::from(stack - count));
        response_equipment.push(serialize_equipment(equipment_id, equipment));
    }
    let response_time = server_time(database)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("equipment:bulk-upgrade:{}", body.api_count),
        response_time,
        &history_entries,
    )?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "equipment_list": response_equipment,
            "item_list": response_items,
            "mail_arrived": false,
        }),
    )
}
// //// /批量消耗装备副本完成觉醒 ////

// //// /分派 CN 装备养成请求 ////

fn upgrade(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpgradeRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.equipment_id > 0
                && body.upgrade_count >= 0
                && body.api_count >= 0 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let upgrade_count = body.upgrade_count.max(1);
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let equipment_key = body.equipment_id.to_string();
    let equipment = match require_object(root, "user_equipment_list")?.get(&equipment_key) {
        Some(equipment) => equipment.clone(),
        None => return Ok(error_response("400 Bad Request", "equipment_not_owned")),
    };
    let equipment_object = equipment
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
    let current_level = equipment_object
        .get("level")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let current_stack = equipment_object
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let new_level = current_level
        .checked_add(upgrade_count)
        .ok_or_else(|| PersonalServiceError::new("CN equipment level exceeds supported range"))?;
    if new_level > MAX_EQUIPMENT_LEVEL {
        return Ok(error_response("400 Bad Request", "equipment_level_limit"));
    }
    let new_stack = if body.use_stack {
        match current_stack.checked_sub(upgrade_count) {
            Some(value) if value >= 0 => value,
            _ => {
                return Ok(error_response(
                    "400 Bad Request",
                    "not_enough_equipment_stack",
                ))
            }
        }
    } else {
        current_stack
    };
    let rarity = body.equipment_id / 1_000_000 - 1;
    let upgrade_cost = usize::try_from(rarity)
        .ok()
        .and_then(|index| UPGRADE_COST_BY_RARITY.get(index).copied())
        .unwrap_or_default();
    let item_list = require_object(root, "item_list")?;
    let wrightpieces = item_list
        .get(&WRIGHTPIECE_ITEM_ID.to_string())
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let wrightpiece_cost = upgrade_cost
        .checked_mul(upgrade_count)
        .ok_or_else(|| PersonalServiceError::new("CN wrightpiece cost exceeds range"))?;
    let new_wrightpieces = match wrightpieces.checked_sub(wrightpiece_cost) {
        Some(value) if value >= 0 => value,
        _ => return Ok(error_response("400 Bad Request", "not_enough_wrightpiece")),
    };
    let mut response_items = Map::new();
    item_list.insert(
        WRIGHTPIECE_ITEM_ID.to_string(),
        Value::from(new_wrightpieces),
    );
    response_items.insert(
        WRIGHTPIECE_ITEM_ID.to_string(),
        Value::from(new_wrightpieces),
    );
    if !body.use_stack {
        if let Some(item_id) = body.item_id {
            if item_id <= 0 {
                return Ok(error_response("400 Bad Request", "invalid_item_id"));
            }
            let item_key = item_id.to_string();
            let current = item_list
                .get(&item_key)
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let new_amount = match current.checked_sub(upgrade_count) {
                Some(value) if value >= 0 => value,
                _ => return Ok(error_response("400 Bad Request", "not_enough_item")),
            };
            item_list.insert(item_key.clone(), Value::from(new_amount));
            response_items.insert(item_key, Value::from(new_amount));
        }
    }
    let mut history_entries = Vec::new();
    for (item_id, reward_count) in dissolve::ability_soul_rewards(body.equipment_id, upgrade_count)?
    {
        add_item(item_list, &mut response_items, item_id, reward_count)?;
        history_entries.push(ReceiveHistoryEntry::reward(1, Some(item_id), reward_count));
    }
    let mut updated_equipment = equipment_object.clone();
    updated_equipment.insert("level".to_owned(), Value::from(new_level));
    updated_equipment.insert("stack".to_owned(), Value::from(new_stack));
    require_object(root, "user_equipment_list")?.insert(
        equipment_key.clone(),
        Value::Object(updated_equipment.clone()),
    );
    let mission_delta = crate::cn_mission::record_equipment_upgrade_action(
        root,
        database,
        snapshot.account_id,
        upgrade_count,
        i64::from(current_level < MAX_EQUIPMENT_LEVEL && new_level >= MAX_EQUIPMENT_LEVEL),
    )?;
    let server_time = server_time(database)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("equipment:upgrade:{}", body.api_count),
        server_time,
        &history_entries,
    )?;
    let mut response = json!({
        "equipment_list": [{
            "equipment_id": body.equipment_id,
            "protection": updated_equipment.get("protection").and_then(Value::as_bool).unwrap_or(false),
            "level": new_level,
            "enhancement_level": updated_equipment.get("enhancement_level").and_then(Value::as_i64).unwrap_or_default(),
            "stack": new_stack,
        }],
        "item_list": response_items,
        "active_mission_list": mission_delta.active_mission_list,
        "mail_arrived": false,
    });
    if !mission_delta.mission_info.is_empty() {
        response
            .as_object_mut()
            .expect("CN equipment upgrade response is an object")
            .insert(
                "mission_info".to_owned(),
                Value::Array(mission_delta.mission_info),
            );
    }
    msgpack_response_at(body.viewer_id, false, server_time, response)
}

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

fn equipment_upgrade_cost(equipment_id: i64) -> i64 {
    let rarity = equipment_id / 1_000_000 - 1;
    usize::try_from(rarity)
        .ok()
        .and_then(|index| UPGRADE_COST_BY_RARITY.get(index).copied())
        .unwrap_or_default()
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
