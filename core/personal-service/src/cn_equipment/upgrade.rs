// audience: internal
// # personal-service-cn-equipment-upgrade
// 装备觉醒消耗同名副本或对应星铁钢和锻造玉. 任务, 魂珠, 领取历史和请求凭据共同提交.

use super::{add_item, dissolve, error_response, serialize_equipment, WRIGHTPIECE_ITEM_ID};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_request_receipt::RequestReceipt;
use crate::cn_tutorial::{
    decode_player_data, encode_player_data, player_snapshot, require_object, require_root,
};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const UPGRADE_COST_BY_RARITY: [i64; 5] = [5, 10, 15, 20, 25];

#[derive(Deserialize)]
struct UpgradeRequest {
    viewer_id: i64,
    equipment_id: i64,
    upgrade_count: i64,
    use_stack: bool,
    item_id: Option<i64>,
    api_count: Option<i64>,
}

#[derive(Deserialize)]
struct BulkRequest {
    viewer_id: i64,
    equipment_ids: Vec<i64>,
    api_count: Option<i64>,
}

struct EquipmentUpgrade {
    equipment_id: i64,
    level: i64,
    stack: i64,
    count: i64,
    max_level: i64,
}

// //// 按所选副本或星铁钢生成单件觉醒结算 [@x380kkm 2026-09-23] ////
pub(super) fn single(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpgradeRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.equipment_id > 0
                && body.upgrade_count > 0
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
    let item_id = if body.use_stack { None } else { body.item_id };
    let receipt = body.api_count.map(|count| RequestReceipt::new("equipment", "upgrade", body.viewer_id, count, json!({
        "equipment_id":body.equipment_id,"upgrade_count":body.upgrade_count,"use_stack":body.use_stack,"item_id":item_id,
    })));
    if let Some(response) = receipt
        .as_ref()
        .map(|receipt| receipt.replay(root))
        .transpose()?
        .flatten()
    {
        return Ok(response);
    }
    let Some(equipment) = require_object(root, "user_equipment_list")?
        .get(&body.equipment_id.to_string())
        .and_then(Value::as_object)
    else {
        return Ok(error_response("400 Bad Request", "equipment_not_owned"));
    };
    let Some(max_level) = dissolve::max_level(body.equipment_id)? else {
        return Ok(error_response(
            "400 Bad Request",
            "equipment_not_awakenable",
        ));
    };
    let level = equipment.get("level").and_then(Value::as_i64).unwrap_or(1);
    let stack = equipment
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let Some(new_level) = level
        .checked_add(body.upgrade_count)
        .filter(|value| *value <= max_level)
    else {
        return Ok(error_response("400 Bad Request", "equipment_level_limit"));
    };
    let new_stack = if body.use_stack {
        match stack
            .checked_sub(body.upgrade_count)
            .filter(|value| *value >= 0)
        {
            Some(value) => value,
            None => {
                return Ok(error_response(
                    "400 Bad Request",
                    "not_enough_equipment_stack",
                ))
            }
        }
    } else {
        if !item_id.is_some_and(|id| crystal_matches(id, body.equipment_id)) {
            return Ok(error_response("400 Bad Request", "invalid_awaking_item"));
        }
        stack
    };
    let upgrades = vec![EquipmentUpgrade {
        equipment_id: body.equipment_id,
        level: new_level,
        stack: new_stack,
        count: body.upgrade_count,
        max_level,
    }];
    settle(
        database,
        snapshot.account_id,
        body.viewer_id,
        &mut player_data,
        upgrades,
        item_id,
        receipt,
    )
}
// //// /按所选副本或星铁钢生成单件觉醒结算 ////

// //// 将唯一装备的可用副本按主表等级上限批量觉醒 [@x380kkm 2026-09-23] ////
pub(super) fn bulk(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<BulkRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count.map_or(true, |count| count >= 0)
                && !body.equipment_ids.is_empty()
                && body.equipment_ids.len() <= 100 =>
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
    let equipment_ids = body
        .equipment_ids
        .into_iter()
        .filter(|id| *id > 0)
        .collect::<BTreeSet<_>>();
    let receipt = body.api_count.map(|count| {
        RequestReceipt::new(
            "equipment",
            "bulk-upgrade",
            body.viewer_id,
            count,
            json!({"equipment_ids":equipment_ids}),
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
    let equipment_list = require_object(root, "user_equipment_list")?;
    let mut upgrades = Vec::new();
    for equipment_id in equipment_ids {
        let Some(equipment) = equipment_list
            .get(&equipment_id.to_string())
            .and_then(Value::as_object)
        else {
            continue;
        };
        let Some(max_level) = dissolve::max_level(equipment_id)? else {
            continue;
        };
        let level = equipment.get("level").and_then(Value::as_i64).unwrap_or(1);
        let stack = equipment
            .get("stack")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let count = max_level.saturating_sub(level).min(stack);
        if count > 0 {
            upgrades.push(EquipmentUpgrade {
                equipment_id,
                level: level + count,
                stack: stack - count,
                count,
                max_level,
            });
        }
    }
    settle(
        database,
        snapshot.account_id,
        body.viewer_id,
        &mut player_data,
        upgrades,
        None,
        receipt,
    )
}
// //// /将唯一装备的可用副本按主表等级上限批量觉醒 ////

// //// 星铁钢按主表的装备星级范围选择 [@x380kkm 2026-09-23] ////
fn crystal_matches(item_id: i64, equipment_id: i64) -> bool {
    let rarity = equipment_id / 1_000_000;
    match item_id {
        12001 => (1..=4).contains(&rarity),
        12002 => rarity == 5,
        _ => false,
    }
}

fn upgrade_cost(equipment_id: i64) -> i64 {
    usize::try_from(equipment_id / 1_000_000 - 1)
        .ok()
        .and_then(|index| UPGRADE_COST_BY_RARITY.get(index).copied())
        .unwrap_or_default()
}
// //// /星铁钢按主表的装备星级范围选择 ////

// //// 扣除材料并将装备, 任务, 历史和响应凭据整体提交 [@x380kkm 2026-09-23] ////
fn settle(
    database: &mut ServiceDatabase,
    account_id: i64,
    viewer_id: i64,
    player_data: &mut Value,
    upgrades: Vec<EquipmentUpgrade>,
    item_id: Option<i64>,
    receipt: Option<RequestReceipt>,
) -> Result<HttpResponse, PersonalServiceError> {
    let mut costs = BTreeMap::new();
    let upgrade_count = upgrades.iter().try_fold(0_i64, |sum, upgrade| sum.checked_add(upgrade.count))
        .ok_or_else(|| PersonalServiceError::new("CN equipment upgrade count exceeds range"))?;
    let fully_awakened_count = upgrades
        .iter()
        .filter(|upgrade| upgrade.level == upgrade.max_level)
        .count() as i64;
    let wrightpieces = upgrades
        .iter()
        .try_fold(0_i64, |sum, upgrade| {
            upgrade_cost(upgrade.equipment_id)
                .checked_mul(upgrade.count)
                .and_then(|cost| sum.checked_add(cost))
        })
        .ok_or_else(|| PersonalServiceError::new("CN equipment upgrade cost exceeds range"))?;
    if wrightpieces > 0 {
        costs.insert(WRIGHTPIECE_ITEM_ID, wrightpieces);
    }
    if let Some(item_id) = item_id {
        costs.insert(item_id, upgrade_count);
    }
    let root = require_root(player_data)?;
    let items = require_object(root, "item_list")?;
    for (item_id, count) in &costs {
        if items
            .get(&item_id.to_string())
            .and_then(Value::as_i64)
            .unwrap_or_default()
            < *count
        {
            let error = if *item_id == WRIGHTPIECE_ITEM_ID {
                "not_enough_wrightpiece"
            } else {
                "not_enough_item"
            };
            return Ok(error_response("400 Bad Request", error));
        }
    }
    let mut response_items = Map::new();
    for (item_id, count) in costs {
        add_item(items, &mut response_items, item_id, -count)?;
    }
    let mut history = Vec::new();
    for upgrade in &upgrades {
        for (item_id, amount) in
            dissolve::ability_soul_rewards(upgrade.equipment_id, upgrade.count)?
        {
            add_item(items, &mut response_items, item_id, amount)?;
            history.push(ReceiveHistoryEntry::reward(1, Some(item_id), amount));
        }
    }
    let equipment_list = require_object(root, "user_equipment_list")?;
    let mut response_equipment = Vec::new();
    for upgrade in upgrades {
        let equipment = equipment_list
            .get_mut(&upgrade.equipment_id.to_string())
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
        equipment.insert("level".to_owned(), json!(upgrade.level));
        equipment.insert("stack".to_owned(), json!(upgrade.stack));
        response_equipment.push(serialize_equipment(upgrade.equipment_id, equipment));
    }
    let response_time = server_time(database)?;
    database
        .with_player_settlement(|database| {
            let root = require_root(player_data)?;
            let missions = crate::cn_mission::record_equipment_upgrade_action(
                root,
                database,
                account_id,
                upgrade_count,
                fully_awakened_count,
            )?;
            let mut response = json!({
                "equipment_list":response_equipment,"item_list":response_items,
                "active_mission_list":missions.active_mission_list,"mail_arrived":false,
            });
            crate::cn_mission::apply_mission_reward_response(
                &mut response,
                root,
                &missions.mission_info,
                viewer_id,
                response_time,
            )?;
            if !missions.mission_info.is_empty() {
                response["mission_info"] = json!(missions.mission_info);
            }
            let event_key = match receipt {
                Some(receipt) => receipt.record(root, &response, response_time)?,
                None => {
                    let total = database
                        .mission_counters(account_id)?
                        .get("upgrade_equipment_count")
                        .copied()
                        .unwrap_or_default();
                    format!("equipment:upgrade:{viewer_id}:total:{total}")
                }
            };
            database.save_player_snapshot_with_receive_history(
                account_id,
                &encode_player_data(player_data)?,
                &event_key,
                response_time,
                &history,
            )?;
            Ok(Some(msgpack_response_at(
                viewer_id,
                false,
                response_time,
                response,
            )?))
        })?
        .ok_or_else(|| PersonalServiceError::new("CN equipment settlement did not commit"))
}
// //// /扣除材料并将装备, 任务, 历史和响应凭据整体提交 ////
