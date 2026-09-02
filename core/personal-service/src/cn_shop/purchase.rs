// audience: internal
// # personal-service-cn-shop-purchase
//
// 该模块计算 CN 商店费用并把商品奖励写入玩家快照.

use super::catalog::{SHOP_TYPE_EQUIPMENT_ENHANCEMENT, SHOP_TYPE_STAR_GRAIN};
use crate::cn_character_reward::{grant_character, grant_character_without_duplicate_item};
use crate::cn_tutorial::require_object;
use crate::database::ReceiveHistoryEntry;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(super) struct ShopCosts {
    user_info: BTreeMap<String, i64>,
    items: BTreeMap<String, i64>,
}

pub(super) struct ShopRewards {
    pub(super) user_info: Value,
    pub(super) joined_character_id_list: Vec<i64>,
    pub(super) character_list: Vec<Value>,
    pub(super) equipment_list: Vec<Value>,
    pub(super) item_list: Map<String, Value>,
    pub(super) history_entries: Vec<ReceiveHistoryEntry>,
}

// //// 合并批量购买产生的 CN 商店奖励 [@x380kkm 2026-08-24] ////
impl ShopRewards {
    pub(super) fn merge(&mut self, mut other: Self) {
        self.user_info = other.user_info;
        for character_id in other.joined_character_id_list {
            if !self.joined_character_id_list.contains(&character_id) {
                self.joined_character_id_list.push(character_id);
            }
        }
        merge_response_list(
            &mut self.character_list,
            other.character_list,
            "character_id",
        );
        merge_response_list(
            &mut self.equipment_list,
            other.equipment_list,
            "equipment_id",
        );
        self.item_list.extend(other.item_list);
        self.history_entries.append(&mut other.history_entries);
    }
}

fn merge_response_list(target: &mut Vec<Value>, source: Vec<Value>, id_field: &str) {
    for response in source {
        let Some(id) = response.get(id_field).and_then(Value::as_i64) else {
            target.push(response);
            continue;
        };
        match target
            .iter_mut()
            .find(|current| current.get(id_field).and_then(Value::as_i64) == Some(id))
        {
            Some(current) => *current = response,
            None => target.push(response),
        }
    }
}
// //// /合并批量购买产生的 CN 商店奖励 ////

// //// 计算购买后的 CN 商店费用余额 [@x380kkm 2026-08-22] ////
pub(super) fn shop_costs(
    root: &Map<String, Value>,
    item: &Map<String, Value>,
    purchase_count: i64,
) -> Result<ShopCosts, PersonalServiceError> {
    let user_info = root
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user info is missing"))?;
    let item_list = root
        .get("item_list")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN item list is missing"))?;
    let mut user_info_costs = BTreeMap::new();
    if let Some(user_cost) = item.get("userCost").and_then(Value::as_object) {
        let cost_type = user_cost
            .get("type")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN shop user cost type is invalid"))?;
        let amount = user_cost
            .get("amount")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN shop user cost is invalid"))?
            .checked_mul(purchase_count)
            .ok_or_else(|| PersonalServiceError::new("CN shop user cost exceeds range"))?;
        let field = match cost_type {
            0 => "free_vmoney",
            1 => "free_mana",
            2 => "bond_token",
            _ => {
                return Err(PersonalServiceError::new(
                    "CN shop user cost type is invalid",
                ))
            }
        };
        let remaining = user_info
            .get(field)
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .checked_sub(amount)
            .filter(|value| *value >= 0)
            .ok_or_else(|| PersonalServiceError::new("CN shop user balance is insufficient"))?;
        user_info_costs.insert(field.to_owned(), remaining);
    }
    let mut required_items = BTreeMap::<String, i64>::new();
    for cost in item
        .get("costs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let item_id = cost
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN shop item cost id is invalid"))?;
        let amount = cost
            .get("amount")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN shop item cost is invalid"))?
            .checked_mul(purchase_count)
            .ok_or_else(|| PersonalServiceError::new("CN shop item cost exceeds range"))?;
        let required = required_items.entry(item_id.to_string()).or_default();
        *required = required
            .checked_add(amount)
            .ok_or_else(|| PersonalServiceError::new("CN shop item cost exceeds range"))?;
    }
    let mut remaining_items = BTreeMap::new();
    for (item_id, required) in required_items {
        let remaining = item_list
            .get(&item_id)
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .checked_sub(required)
            .filter(|value| *value >= 0)
            .ok_or_else(|| PersonalServiceError::new("CN shop item balance is insufficient"))?;
        remaining_items.insert(item_id, remaining);
    }
    Ok(ShopCosts {
        user_info: user_info_costs,
        items: remaining_items,
    })
}
// //// /计算购买后的 CN 商店费用余额 ////

pub(super) fn apply_shop_costs(
    root: &mut Map<String, Value>,
    costs: &ShopCosts,
) -> Result<(), PersonalServiceError> {
    let user_info = require_object(root, "user_info")?;
    for (field, value) in &costs.user_info {
        user_info.insert(field.clone(), Value::from(*value));
    }
    let item_list = require_object(root, "item_list")?;
    for (item_id, value) in &costs.items {
        item_list.insert(item_id.clone(), Value::from(*value));
    }
    Ok(())
}

// //// 发放 CN 商店商品奖励 [@x380kkm 2026-08-22] ////
pub(super) fn apply_shop_rewards(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    shop_type: i64,
    item: &Map<String, Value>,
    purchase_count: i64,
    response_time: i64,
    costs: &ShopCosts,
) -> Result<ShopRewards, PersonalServiceError> {
    let mut joined_character_id_list = Vec::new();
    let mut character_list = BTreeMap::new();
    let mut equipment_list = BTreeMap::new();
    let mut response_items = costs
        .items
        .iter()
        .map(|(item_id, value)| (item_id.clone(), Value::from(*value)))
        .collect::<Map<_, _>>();
    let mut history_entries = Vec::new();

    if shop_type == SHOP_TYPE_EQUIPMENT_ENHANCEMENT {
        let equipment_id = item
            .get("equipmentId")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN enhancement equipment id is invalid"))?;
        let target_level = item
            .get("enhancementMaxLevel")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN enhancement level is invalid"))?;
        let equipment = require_object(root, "user_equipment_list")?
            .get_mut(&equipment_id.to_string())
            .and_then(Value::as_object_mut)
            .ok_or_else(|| PersonalServiceError::new("CN enhancement equipment is not owned"))?;
        let enhancement_level = equipment
            .get("enhancement_level")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(target_level);
        equipment.insert(
            "enhancement_level".to_owned(),
            Value::from(enhancement_level),
        );
        equipment_list.insert(
            equipment_id,
            serialize_shop_equipment(equipment_id, equipment),
        );
    }

    for reward in item
        .get("rewards")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let reward_type = reward
            .get("type")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("CN shop reward type is invalid"))?;
        let count = reward
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            .checked_mul(purchase_count)
            .ok_or_else(|| PersonalServiceError::new("CN shop reward count exceeds range"))?;
        match reward_type {
            0 => {
                let item_id = reward
                    .get("id")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| PersonalServiceError::new("CN shop reward id is invalid"))?;
                let total = add_shop_item(root, item_id, count)?;
                response_items.insert(item_id.to_string(), Value::from(total));
                history_entries.push(ReceiveHistoryEntry::reward(1, Some(item_id), count));
            }
            1 => {
                add_shop_user_value(root, "exp_pool", count)?;
                history_entries.push(ReceiveHistoryEntry::reward(9, None, count));
            }
            2 => {
                add_shop_user_value(root, "free_mana", count)?;
                history_entries.push(ReceiveHistoryEntry::reward(8, None, count));
            }
            3 => {
                let character_id = reward
                    .get("id")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| PersonalServiceError::new("CN shop character id is invalid"))?;
                for _ in 0..count {
                    let reward = if shop_type == SHOP_TYPE_STAR_GRAIN {
                        grant_character(root, viewer_id, character_id, response_time)?
                    } else {
                        grant_character_without_duplicate_item(
                            root,
                            viewer_id,
                            character_id,
                            response_time,
                        )?
                    };
                    if reward.joined {
                        joined_character_id_list.push(character_id);
                    }
                    if let Some(item) = reward.duplicate_item {
                        response_items.insert(item.id.to_string(), Value::from(item.total));
                    }
                    character_list.insert(character_id, reward.character);
                }
                history_entries.push(ReceiveHistoryEntry::reward(5, Some(character_id), count));
            }
            4 => {
                let equipment_id = reward
                    .get("id")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| PersonalServiceError::new("CN shop equipment id is invalid"))?;
                equipment_list.insert(equipment_id, add_shop_equipment(root, equipment_id, count)?);
                history_entries.push(ReceiveHistoryEntry::reward(6, Some(equipment_id), count));
            }
            _ => return Err(PersonalServiceError::new("CN shop reward type is invalid")),
        }
    }
    let user_info = root
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user info is missing"))?;
    Ok(ShopRewards {
        user_info: json!({
            "free_vmoney": user_info.get("free_vmoney").and_then(Value::as_i64).unwrap_or_default(),
            "free_mana": user_info.get("free_mana").and_then(Value::as_i64).unwrap_or_default(),
            "bond_token": user_info.get("bond_token").and_then(Value::as_i64).unwrap_or_default(),
            "exp_pool": user_info.get("exp_pool").and_then(Value::as_i64).unwrap_or_default(),
            "exp_pooled_time": user_info.get("exp_pooled_time").and_then(Value::as_i64).unwrap_or(response_time),
        }),
        joined_character_id_list,
        character_list: character_list.into_values().collect(),
        equipment_list: equipment_list.into_values().collect(),
        item_list: response_items,
        history_entries,
    })
}
// //// /发放 CN 商店商品奖励 ////

fn add_shop_user_value(
    root: &mut Map<String, Value>,
    field: &str,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let user_info = require_object(root, "user_info")?;
    let total = user_info
        .get(field)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN shop user reward exceeds range"))?;
    user_info.insert(field.to_owned(), Value::from(total));
    Ok(())
}

fn add_shop_item(
    root: &mut Map<String, Value>,
    item_id: i64,
    amount: i64,
) -> Result<i64, PersonalServiceError> {
    let item_list = require_object(root, "item_list")?;
    let key = item_id.to_string();
    let total = item_list
        .get(&key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN shop item reward exceeds range"))?;
    item_list.insert(key, Value::from(total));
    Ok(total)
}

fn add_shop_equipment(
    root: &mut Map<String, Value>,
    equipment_id: i64,
    count: i64,
) -> Result<Value, PersonalServiceError> {
    let equipment_list = require_object(root, "user_equipment_list")?;
    let key = equipment_id.to_string();
    let was_owned = equipment_list.contains_key(&key);
    let equipment = equipment_list
        .entry(key)
        .or_insert_with(|| {
            json!({
                "enhancement_level": 0,
                "level": 1,
                "protection": false,
                "stack": 0,
            })
        })
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN equipment data is invalid"))?;
    let extra_stack = if was_owned {
        count
    } else {
        count.saturating_sub(1)
    };
    let stack = equipment
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(extra_stack)
        .ok_or_else(|| PersonalServiceError::new("CN equipment stack exceeds range"))?;
    equipment.insert("stack".to_owned(), Value::from(stack));
    Ok(serialize_shop_equipment(equipment_id, equipment))
}

fn serialize_shop_equipment(equipment_id: i64, equipment: &Map<String, Value>) -> Value {
    json!({
        "equipment_id": equipment_id,
        "protection": equipment.get("protection").and_then(Value::as_bool).unwrap_or(false),
        "level": equipment.get("level").and_then(Value::as_i64).unwrap_or(1),
        "enhancement_level": equipment.get("enhancement_level").and_then(Value::as_i64).unwrap_or_default(),
        "stack": equipment.get("stack").and_then(Value::as_i64).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证商店批量奖励只返回角色和装备的最终状态 [@x380kkm 2026-08-28] ////
    #[test]
    fn merges_latest_character_and_equipment_rewards() {
        let mut root = Map::from_iter([
            (
                "user_info".to_owned(),
                json!({
                    "free_vmoney": 0,
                    "free_mana": 0,
                    "bond_token": 0,
                    "exp_pool": 0,
                }),
            ),
            ("user_character_list".to_owned(), json!({})),
            ("user_equipment_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);
        let costs = ShopCosts {
            user_info: BTreeMap::new(),
            items: BTreeMap::new(),
        };
        let first_item = json!({
            "rewards": [
                {"type": 3, "id": 111001, "count": 1},
                {"type": 4, "id": 5030037, "count": 1}
            ]
        });
        let second_item = json!({
            "rewards": [
                {"type": 3, "id": 111001, "count": 1},
                {"type": 4, "id": 5030037, "count": 2}
            ]
        });

        let mut result = apply_shop_rewards(
            &mut root,
            7,
            9,
            first_item.as_object().unwrap(),
            1,
            100,
            &costs,
        )
        .unwrap();
        result.merge(
            apply_shop_rewards(
                &mut root,
                7,
                9,
                second_item.as_object().unwrap(),
                1,
                200,
                &costs,
            )
            .unwrap(),
        );

        assert_eq!(result.joined_character_id_list, vec![111001]);
        assert_eq!(result.character_list.len(), 1);
        assert_eq!(result.character_list[0]["stack"], 1);
        assert_eq!(result.equipment_list.len(), 1);
        assert_eq!(result.equipment_list[0]["stack"], 2);
        assert_eq!(result.item_list["14003"], 1);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 1);
        assert_eq!(root["user_equipment_list"]["5030037"]["stack"], 2);
        assert!(root.get("encyclopedia_list").is_none());
    }
    // //// /验证商店批量奖励只返回角色和装备的最终状态 ////

    // //// 验证普通商店重复角色不产生星之粒素材 [@x380kkm 2026-08-28] ////
    #[test]
    fn grants_duplicate_character_items_only_in_star_grain_shop() {
        let mut root = Map::from_iter([
            (
                "user_info".to_owned(),
                json!({
                    "free_vmoney": 0,
                    "free_mana": 0,
                    "bond_token": 0,
                    "exp_pool": 0,
                }),
            ),
            ("user_character_list".to_owned(), json!({})),
            ("user_equipment_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);
        let costs = ShopCosts {
            user_info: BTreeMap::new(),
            items: BTreeMap::new(),
        };
        let item = json!({"rewards": [{"type": 3, "id": 111001, "count": 1}]});

        apply_shop_rewards(
            &mut root,
            7,
            SHOP_TYPE_STAR_GRAIN,
            item.as_object().unwrap(),
            1,
            100,
            &costs,
        )
        .unwrap();
        let duplicate = apply_shop_rewards(
            &mut root,
            7,
            super::super::catalog::SHOP_TYPE_EVENT,
            item.as_object().unwrap(),
            1,
            200,
            &costs,
        )
        .unwrap();

        assert_eq!(root["user_character_list"]["111001"]["stack"], 1);
        assert!(root["item_list"].as_object().is_some_and(Map::is_empty));
        assert!(duplicate.item_list.is_empty());

        let star_grain_duplicate = apply_shop_rewards(
            &mut root,
            7,
            SHOP_TYPE_STAR_GRAIN,
            item.as_object().unwrap(),
            1,
            300,
            &costs,
        )
        .unwrap();
        assert_eq!(root["user_character_list"]["111001"]["stack"], 2);
        assert_eq!(root["item_list"]["14003"], 1);
        assert_eq!(star_grain_duplicate.item_list["14003"], 1);
    }
    // //// /验证普通商店重复角色不产生星之粒素材 ////
}
