// audience: internal
// # personal-service-cn-equipment-dissolve-validation
// 保护约束覆盖本体和副本. 装配与强化约束仅限制整件出售.

use super::{EquipmentDefinition, Sale, SellItem};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

// //// 校验出售资格并读取副本数和魂珠剩余容量 [@x380kkm 2026-09-23] ////
fn equipment<'a>(
    root: &'a Map<String, Value>,
    definitions: &BTreeMap<i64, EquipmentDefinition>,
    equipment_id: i64,
) -> Result<(&'a Map<String, Value>, i64, i64), &'static str> {
    if equipment_id <= 0 {
        return Err("invalid_equipment_id");
    }
    let owned = root
        .get("user_equipment_list")
        .and_then(|list| list.get(equipment_id.to_string()))
        .and_then(Value::as_object)
        .ok_or("equipment_not_owned")?;
    let definition = definitions
        .get(&equipment_id)
        .ok_or("equipment_not_sellable")?;
    if !definition.sellable {
        return Err("equipment_not_sellable");
    }
    if owned
        .get("protection")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("equipment_protected");
    }
    let stack = owned
        .get("stack")
        .and_then(Value::as_i64)
        .ok_or("invalid_equipment_stack")?;
    if stack < 0 {
        return Err("invalid_equipment_stack");
    }
    let capacity = if definition.generate_ability_soul {
        let current = root
            .get("item_list")
            .and_then(|items| items.get(definition.ability_soul_id.to_string()))
            .and_then(Value::as_i64)
            .unwrap_or_default();
        definition
            .ability_soul_max_count
            .saturating_sub(current)
            .max(0)
    } else {
        i64::MAX
    };
    if capacity == 0 {
        return Err("ability_soul_max");
    }
    Ok((owned, stack, capacity))
}
// //// /校验出售资格并读取副本数和魂珠剩余容量 ////

// //// 按全部编队引用校验最后一件装备出售 [@x380kkm 2026-09-23] ////
pub(super) fn whole(
    root: &Map<String, Value>,
    definitions: &BTreeMap<i64, EquipmentDefinition>,
    items: &[SellItem],
) -> Result<Vec<Sale>, &'static str> {
    let mut ids = BTreeSet::new();
    let mut sales = Vec::new();
    for item in items {
        if !ids.insert(item.equipment_id) {
            continue;
        }
        if item.number.is_some_and(|count| count != 1) {
            return Err("invalid_equipment_sell_count");
        }
        let (owned, stack, _) = equipment(root, definitions, item.equipment_id)?;
        if stack > 0 {
            return Err("equipment_has_stack");
        }
        if is_equipped(root, item.equipment_id) {
            return Err("equipment_is_equipped");
        }
        if owned
            .get("enhancement_level")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            != 0
        {
            return Err("equipment_is_enhanced");
        }
        sales.push(Sale {
            equipment_id: item.equipment_id,
            remaining_stack: None,
            count: 1,
        });
    }
    Ok(sales)
}

fn is_equipped(root: &Map<String, Value>, equipment_id: i64) -> bool {
    let normal = ["user_party_group_list", "profile_party_group_list"]
        .into_iter()
        .filter_map(|field| root.get(field).and_then(Value::as_object))
        .flat_map(|groups| groups.values())
        .filter_map(|group| group.get("list").and_then(Value::as_object))
        .flat_map(|parties| parties.values())
        .filter_map(|party| party.get("equipment_ids").and_then(Value::as_array))
        .flatten()
        .any(|id| id.as_i64() == Some(equipment_id));
    let activity = root
        .get("cn_activity_state")
        .and_then(|state| state.get("event_families"))
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|families| families.values())
        .filter_map(Value::as_object)
        .flat_map(|events| events.values())
        .filter_map(|event| event.get("party_group_list").and_then(Value::as_array))
        .flatten()
        .filter_map(|group| group.get("party_list").and_then(Value::as_array))
        .flatten()
        .filter_map(|party| party.get("equipment_ids").and_then(Value::as_array))
        .flatten()
        .any(|id| id.as_i64() == Some(equipment_id));
    normal || activity
}
// //// /按全部编队引用校验最后一件装备出售 ////

// //// 汇总副本分解数量并校验库存与魂珠容量 [@x380kkm 2026-09-23] ////
pub(super) fn stack(
    root: &Map<String, Value>,
    definitions: &BTreeMap<i64, EquipmentDefinition>,
    items: &[SellItem],
) -> Result<Vec<Sale>, &'static str> {
    let mut counts = BTreeMap::<i64, i64>::new();
    for item in items {
        let count = item
            .number
            .filter(|count| *count > 0)
            .ok_or("invalid_equipment_sell_count")?;
        let total = counts.entry(item.equipment_id).or_default();
        *total = total
            .checked_add(count)
            .ok_or("equipment_sell_count_exceeds_range")?;
    }
    counts
        .into_iter()
        .map(|(equipment_id, count)| {
            let (_, stack, capacity) = equipment(root, definitions, equipment_id)?;
            if count > stack {
                return Err("not_enough_equipment_stack");
            }
            if count > capacity {
                return Err("ability_soul_max");
            }
            Ok(Sale {
                equipment_id,
                remaining_stack: Some(stack - count),
                count,
            })
        })
        .collect()
}
// //// /汇总副本分解数量并校验库存与魂珠容量 ////

// //// 从批量选择中取得可分解的装备副本 [@x380kkm 2026-09-23] ////
pub(super) fn bulk(
    root: &Map<String, Value>,
    definitions: &BTreeMap<i64, EquipmentDefinition>,
    ids: &[i64],
) -> Result<Vec<Sale>, &'static str> {
    Ok(ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|equipment_id| {
            let (_, stack, _) = equipment(root, definitions, equipment_id).ok()?;
            (stack > 0).then_some(Sale {
                equipment_id,
                remaining_stack: Some(0),
                count: stack,
            })
        })
        .collect())
}
// //// /从批量选择中取得可分解的装备副本 ////
