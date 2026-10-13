// audience: internal
// # personal-service-cn-shop-enhancement
// 强化商品数量对应装备强化等级增量, 每个商品只覆盖其所属阶段.

use super::catalog::{shop_catalog, SHOP_TYPE_EQUIPMENT_ENHANCEMENT};
use crate::PersonalServiceError;
use serde_json::json;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

// //// 按当前强化等级选择每组装备的可购买阶段 [@x380kkm 2026-08-24] ////
pub(super) fn equipment_enhancement_sale_entries<'a>(
    root: &Map<String, Value>,
    groups: BTreeMap<(i64, i64), BTreeMap<i64, &'a Map<String, Value>>>,
) -> Vec<Value> {
    let equipment_list = root.get("user_equipment_list").and_then(Value::as_object);
    let mut sales = Vec::with_capacity(groups.len());

    for items in groups.into_values() {
        let mut items = items.into_iter().collect::<Vec<_>>();
        items.sort_by_key(|(shop_item_id, item)| {
            (
                item.get("stage")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                *shop_item_id,
            )
        });
        let Some((_, first_item)) = items.first() else {
            continue;
        };
        let Some(equipment_id) = first_item.get("equipmentId").and_then(Value::as_i64) else {
            continue;
        };
        let enhancement_level = equipment_list
            .and_then(|equipment_list| equipment_list.get(&equipment_id.to_string()))
            .and_then(Value::as_object)
            .map(|equipment| {
                equipment
                    .get("enhancement_level")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            })
            .unwrap_or(-1);
        let target_index = if enhancement_level < 0 {
            0
        } else {
            items
                .iter()
                .position(|(_, item)| {
                    item.get("enhancementMaxLevel")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                        > enhancement_level
                })
                .unwrap_or(items.len() - 1)
        };
        let (shop_item_id, target_item) = items[target_index];
        let target_level = target_item
            .get("enhancementMaxLevel")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let total_purchase_num = enhancement_level.max(0);
        let stock_quantity = if enhancement_level < 0 {
            target_level
        } else {
            target_level.saturating_sub(enhancement_level).max(0)
        };
        let max_level = items
            .iter()
            .filter_map(|(_, item)| item.get("enhancementMaxLevel").and_then(Value::as_i64))
            .max()
            .unwrap_or_default();
        let other_group_items =
            equipment_enhancement_other_group_items(&items, target_index, enhancement_level.max(0));
        sales.push(json!({
            "shop_item_id": shop_item_id,
            "stock_quantity": stock_quantity,
            "today_purchase_num": 0,
            "this_month_purchase_num": null,
            "total_purchase_num": total_purchase_num,
            "discount_id": null,
            "discount_rate": null,
            "discounted_price": null,
            "group_info": {
                "group_total_stock_quantity": max_level.saturating_sub(total_purchase_num).max(0),
                "group_total_purchase_num": total_purchase_num,
                "multi_stage": items.len() > 1,
                "other_group_items": other_group_items,
            },
            "shop_type": SHOP_TYPE_EQUIPMENT_ENHANCEMENT,
        }));
    }

    sales
}
// //// /按当前强化等级选择每组装备的可购买阶段 ////

// //// 返回装备强化组中其余阶段的可购买状态 [@x380kkm 2026-08-29] ////
fn equipment_enhancement_other_group_items(
    items: &[(i64, &Map<String, Value>)],
    target_index: usize,
    enhancement_level: i64,
) -> Vec<Value> {
    let mut previous_max_level = 0;
    items
        .iter()
        .enumerate()
        .filter_map(|(index, (shop_item_id, item))| {
            let max_level = item
                .get("enhancementMaxLevel")
                .and_then(Value::as_i64)
                .unwrap_or(previous_max_level)
                .max(previous_max_level);
            let stage_capacity = max_level.saturating_sub(previous_max_level);
            let stage_purchase_num = enhancement_level
                .saturating_sub(previous_max_level)
                .clamp(0, stage_capacity);
            previous_max_level = max_level;
            (index != target_index).then(|| {
                json!({
                    "shop_item_id": shop_item_id,
                    "stock_quantity": stage_capacity.saturating_sub(stage_purchase_num),
                    "today_purchase_num": 0,
                    "this_month_purchase_num": null,
                    "total_purchase_num": stage_purchase_num,
                    "discount_id": null,
                    "discount_rate": null,
                    "discounted_price": null,
                })
            })
        })
        .collect()
}
// //// /返回装备强化组中其余阶段的可购买状态 ////

// //// 检查装备归属、觉醒等级和强化阶段容量 [@x380kkm 2026-09-21] ////
pub(super) fn purchase_error(
    root: &Map<String, Value>,
    item: &Map<String, Value>,
    count: i64,
) -> Result<Option<&'static str>, PersonalServiceError> {
    let equipment_id = item
        .get("equipmentId")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN enhancement equipment id is invalid"))?;
    let Some(equipment) = root
        .get("user_equipment_list")
        .and_then(Value::as_object)
        .and_then(|items| items.get(&equipment_id.to_string()))
        .and_then(Value::as_object)
    else {
        return Ok(Some("equipment_not_owned"));
    };
    let required_level = item
        .get("requireAwakeningLevel")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN enhancement awakening level is invalid"))?;
    if equipment.get("level").and_then(Value::as_i64).unwrap_or(1) < required_level {
        return Ok(Some("equipment_level_shortage"));
    }
    let max_level = item
        .get("enhancementMaxLevel")
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new("CN enhancement level is invalid"))?;
    let group_id = item.get("groupId");
    let stage = item
        .get("stage")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let previous_max_level = shop_catalog(SHOP_TYPE_EQUIPMENT_ENHANCEMENT)?
        .into_iter()
        .flat_map(|items| items.values())
        .filter(|entry| {
            entry.get("groupId") == group_id
                && entry.get("shopCategoryId") == item.get("shopCategoryId")
                && entry
                    .get("stage")
                    .and_then(Value::as_i64)
                    .is_some_and(|value| value < stage)
        })
        .filter_map(|entry| entry.get("enhancementMaxLevel").and_then(Value::as_i64))
        .max()
        .unwrap_or_default();
    let current = equipment
        .get("enhancement_level")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    Ok(if current < previous_max_level || current >= max_level {
        Some("equipment_enhancement_stage_mismatch")
    } else if count > max_level - current {
        Some("shop_stock_exceeded")
    } else {
        None
    })
}
// //// /检查装备归属、觉醒等级和强化阶段容量 ////
