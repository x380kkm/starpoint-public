// audience: internal
// # personal-service-cn-equipment-dissolve
//
// 该模块按装备分解主数据计算 craft point, star grain 和 ability soul 奖励.

use super::{add_item, error_response, serialize_equipment, WRIGHTPIECE_ITEM_ID};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const STAR_GRAIN_ITEM_ID: i64 = 990_008;
const CRAFT_POINTS_BY_RARITY: [i64; 5] = [1, 2, 3, 4, 5];
const STAR_GRAINS_BY_RARITY: [i64; 5] = [0, 0, 1, 5, 15];
const EQUIPMENT_DISSOLVE_ASSET: &str = include_str!("../../../../assets/equipment_dissolve.json");
static EQUIPMENT_DISSOLVE_DATA: OnceLock<Result<Value, String>> = OnceLock::new();

#[derive(Default)]
struct DissolveRewards {
    craft_points: i64,
    star_grains: i64,
    ability_souls: BTreeMap<i64, i64>,
}

impl DissolveRewards {
    fn merge(&mut self, rewards: Self) -> Result<(), PersonalServiceError> {
        self.craft_points = self
            .craft_points
            .checked_add(rewards.craft_points)
            .ok_or_else(|| PersonalServiceError::new("CN dissolve craft points exceed range"))?;
        self.star_grains = self
            .star_grains
            .checked_add(rewards.star_grains)
            .ok_or_else(|| PersonalServiceError::new("CN dissolve star grains exceed range"))?;
        for (item_id, count) in rewards.ability_souls {
            let current = self
                .ability_souls
                .get(&item_id)
                .copied()
                .unwrap_or_default();
            self.ability_souls.insert(
                item_id,
                current.checked_add(count).ok_or_else(|| {
                    PersonalServiceError::new("CN dissolve ability souls exceed range")
                })?,
            );
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct SellEquipmentRequest {
    viewer_id: i64,
    #[serde(default)]
    api_count: i64,
    equipment_list: Vec<SellEquipmentItem>,
}

#[derive(Deserialize)]
struct SellEquipmentItem {
    equipment_id: i64,
    number: Option<i64>,
}

#[derive(Deserialize)]
struct BulkSellRequest {
    viewer_id: i64,
    #[serde(default)]
    api_count: i64,
    equipment_ids: Vec<i64>,
}

// //// 将指定装备的全部副本批量分解 [@x380kkm 2026-08-22] ////
pub(super) fn bulk_sell_stack(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkSellRequest>(request) {
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
    let mut sales = Vec::new();
    let mut total_rewards = DissolveRewards::default();
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
        let stack = equipment
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if stack <= 0 {
            continue;
        }
        total_rewards.merge(calculate_dissolve_rewards(equipment_id, stack)?)?;
        sales.push(equipment_id);
    }
    if sales.is_empty() {
        return msgpack_response_at(
            body.viewer_id,
            false,
            server_time(database)?,
            json!({"equipment_list": [], "item_list": {}, "mail_arrived": false}),
        );
    }
    {
        let equipment_list = require_object(root, "user_equipment_list")?;
        for equipment_id in &sales {
            let equipment = equipment_list
                .get_mut(&equipment_id.to_string())
                .and_then(Value::as_object_mut)
                .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
            equipment.insert("stack".to_owned(), Value::from(0));
        }
    }
    let mut response_items = Map::new();
    let history_entries = apply_dissolve_rewards(root, &mut response_items, &total_rewards)?;
    let response_equipment = all_equipment(root)?;
    let response_time = server_time(database)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("equipment:bulk-sell-stack:{}", body.api_count),
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
// //// /将指定装备的全部副本批量分解 ////

pub(super) fn sell_equipment(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SellEquipmentRequest>(request) {
        Ok(body)
            if body.viewer_id > 0 && body.api_count >= 0 && body.equipment_list.len() <= 100 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let equipment_ids = match collect_equipment_ids(&body.equipment_list) {
        Ok(equipment_ids) => equipment_ids,
        Err(code) => return Ok(error_response("400 Bad Request", code)),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let equipment_list = require_object(root, "user_equipment_list")?;
    let mut sales = Vec::with_capacity(equipment_ids.len());
    let mut total_rewards = DissolveRewards::default();
    for equipment_id in equipment_ids {
        let Some(equipment) = equipment_list.get(&equipment_id.to_string()) else {
            return Ok(error_response("400 Bad Request", "equipment_not_owned"));
        };
        let equipment = equipment
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
        let stack = equipment
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if stack < 0 {
            return Ok(error_response("400 Bad Request", "invalid_equipment_stack"));
        }
        total_rewards.merge(calculate_dissolve_rewards(equipment_id, stack)?)?;
        sales.push(equipment_id);
    }
    {
        let equipment_list = require_object(root, "user_equipment_list")?;
        for equipment_id in &sales {
            equipment_list.remove(&equipment_id.to_string());
        }
    }
    let mut response_items = Map::new();
    let history_entries = apply_dissolve_rewards(root, &mut response_items, &total_rewards)?;
    let response_equipment = all_equipment(root)?;
    let response_time = server_time(database)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("equipment:sell-equipment:{}", body.api_count),
        response_time,
        &history_entries,
    )?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "item_list": response_items,
            "equipment_list": response_equipment,
            "mail_arrived": false,
        }),
    )
}

pub(super) fn sell_stack(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<SellEquipmentRequest>(request) {
        Ok(body)
            if body.viewer_id > 0 && body.api_count >= 0 && body.equipment_list.len() <= 100 =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let sale_counts = match collect_stack_sale_counts(&body.equipment_list) {
        Ok(counts) => counts,
        Err(code) => return Ok(error_response("400 Bad Request", code)),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = require_root(&mut player_data)?;
    let equipment_list = root
        .get("user_equipment_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_equipment_list is missing"))?;
    let mut sales = Vec::with_capacity(sale_counts.len());
    let mut total_rewards = DissolveRewards::default();
    for (equipment_id, requested) in &sale_counts {
        let Some(equipment) = equipment_list.get(&equipment_id.to_string()) else {
            return Ok(error_response("400 Bad Request", "equipment_not_owned"));
        };
        let equipment = equipment
            .as_object()
            .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
        let stack = equipment
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if stack < 0 {
            return Ok(error_response("400 Bad Request", "invalid_equipment_stack"));
        }
        let new_stack = match stack.checked_sub(*requested) {
            Some(value) if value >= 0 => value,
            _ => {
                return Ok(error_response(
                    "400 Bad Request",
                    "not_enough_equipment_stack",
                ));
            }
        };
        total_rewards.merge(calculate_dissolve_rewards(*equipment_id, *requested)?)?;
        sales.push((*equipment_id, new_stack));
    }
    {
        let equipment_list = require_object(root, "user_equipment_list")?;
        for (equipment_id, new_stack) in &sales {
            let equipment = equipment_list
                .get_mut(&equipment_id.to_string())
                .and_then(Value::as_object_mut)
                .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
            equipment.insert("stack".to_owned(), Value::from(*new_stack));
        }
    }
    let mut response_items = Map::new();
    let history_entries = apply_dissolve_rewards(root, &mut response_items, &total_rewards)?;
    let response_equipment = {
        let equipment_list = root
            .get("user_equipment_list")
            .and_then(Value::as_object)
            .ok_or_else(|| PersonalServiceError::new("stored CN equipment list is missing"))?;
        sales
            .iter()
            .map(|(equipment_id, _)| {
                let equipment = equipment_list
                    .get(&equipment_id.to_string())
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        PersonalServiceError::new("stored CN equipment data is invalid")
                    })?;
                Ok(serialize_equipment(*equipment_id, equipment))
            })
            .collect::<Result<Vec<_>, PersonalServiceError>>()?
    };
    let response_time = server_time(database)?;
    database.save_player_snapshot_with_receive_history(
        snapshot.account_id,
        &encode_player_data(&player_data)?,
        &format!("equipment:sell-stack:{}", body.api_count),
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

pub(super) fn ability_soul_rewards(
    equipment_id: i64,
    count: i64,
) -> Result<BTreeMap<i64, i64>, PersonalServiceError> {
    Ok(calculate_dissolve_rewards(equipment_id, count)?.ability_souls)
}

// //// 收集整件出售的唯一装备 [@x380kkm 2026-08-25] ////
fn collect_equipment_ids(items: &[SellEquipmentItem]) -> Result<BTreeSet<i64>, &'static str> {
    let mut equipment_ids = BTreeSet::new();
    for item in items {
        if item.equipment_id <= 0 {
            return Err("invalid_equipment_id");
        }
        equipment_ids.insert(item.equipment_id);
    }
    Ok(equipment_ids)
}
// //// /收集整件出售的唯一装备 ////

// //// 汇总部分出售的装备数量 [@x380kkm 2026-08-25] ////
fn collect_stack_sale_counts(
    items: &[SellEquipmentItem],
) -> Result<BTreeMap<i64, i64>, &'static str> {
    let mut counts = BTreeMap::<i64, i64>::new();
    for item in items {
        if item.equipment_id <= 0 {
            return Err("invalid_equipment_id");
        }
        let amount = match item.number {
            Some(amount) if amount > 0 => amount,
            Some(_) | None => return Err("invalid_equipment_sell_count"),
        };
        let entry = counts.entry(item.equipment_id).or_insert(0);
        *entry = (*entry)
            .checked_add(amount)
            .ok_or("equipment_sell_count_exceeds_range")?;
    }
    Ok(counts)
}
// //// /汇总部分出售的装备数量 ////

fn calculate_dissolve_rewards(
    equipment_id: i64,
    count: i64,
) -> Result<DissolveRewards, PersonalServiceError> {
    let rarity = usize::try_from(equipment_id / 1_000_000 - 1).ok();
    let craft_points = rarity
        .and_then(|index| CRAFT_POINTS_BY_RARITY.get(index).copied())
        .unwrap_or_default()
        .checked_mul(count)
        .ok_or_else(|| PersonalServiceError::new("CN dissolve craft points exceed range"))?;
    let document = EQUIPMENT_DISSOLVE_DATA.get_or_init(|| {
        serde_json::from_str::<Value>(EQUIPMENT_DISSOLVE_ASSET)
            .map_err(|error| format!("failed to decode CN equipment dissolve data: {error}"))
    });
    let document = document
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let definition = document.get(equipment_id.to_string());
    let star_grains = if definition
        .and_then(|value| value.get("obtain_source"))
        .and_then(Value::as_i64)
        == Some(0)
    {
        rarity
            .and_then(|index| STAR_GRAINS_BY_RARITY.get(index).copied())
            .unwrap_or_default()
            .checked_mul(count)
            .ok_or_else(|| PersonalServiceError::new("CN dissolve star grains exceed range"))?
    } else {
        0
    };
    let ability_souls = if definition
        .and_then(|value| value.get("generate_ability_soul"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        definition
            .and_then(|value| value.get("ability_soul_id"))
            .and_then(Value::as_i64)
            .filter(|item_id| *item_id > 0)
            .map(|item_id| BTreeMap::from([(item_id, count)]))
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    Ok(DissolveRewards {
        craft_points,
        star_grains,
        ability_souls,
    })
}

fn apply_dissolve_rewards(
    root: &mut Map<String, Value>,
    response_items: &mut Map<String, Value>,
    rewards: &DissolveRewards,
) -> Result<Vec<ReceiveHistoryEntry>, PersonalServiceError> {
    let mut reward_items = BTreeMap::from([
        (WRIGHTPIECE_ITEM_ID, rewards.craft_points),
        (STAR_GRAIN_ITEM_ID, rewards.star_grains),
    ]);
    for (item_id, count) in &rewards.ability_souls {
        reward_items.insert(*item_id, *count);
    }
    let item_list = require_object(root, "item_list")?;
    let mut history = Vec::new();
    for (item_id, count) in reward_items {
        if count <= 0 {
            continue;
        }
        add_item(item_list, response_items, item_id, count)?;
        history.push(ReceiveHistoryEntry::reward(1, Some(item_id), count));
    }
    Ok(history)
}

fn all_equipment(root: &Map<String, Value>) -> Result<Vec<Value>, PersonalServiceError> {
    root.get("user_equipment_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN equipment list is missing"))?
        .iter()
        .map(|(equipment_id, equipment)| {
            let equipment_id = equipment_id
                .parse::<i64>()
                .map_err(|_| PersonalServiceError::new("stored CN equipment id is invalid"))?;
            let equipment = equipment
                .as_object()
                .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
            Ok(serialize_equipment(equipment_id, equipment))
        })
        .collect()
}
