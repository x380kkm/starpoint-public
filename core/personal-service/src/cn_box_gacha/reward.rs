// audience: internal
// # personal-service-cn-box-gacha-reward
//
// 该模块把 CN 箱池抽取结果写入玩家角色、装备、道具和货币.

use crate::cn_character_reward::grant_character;
use crate::cn_tutorial::require_object;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(super) struct RewardResult {
    pub(super) user_info: Value,
    pub(super) joined_character_ids: Vec<i64>,
    pub(super) character_list: Vec<Value>,
    pub(super) equipment_list: Vec<Value>,
    pub(super) item_list: Map<String, Value>,
}

// //// 发放 CN 箱池抽取奖励 [@x380kkm 2026-08-22] ////
pub(super) fn apply_rewards(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    rewards: &Map<String, Value>,
    drawn: &BTreeMap<i64, i64>,
    response_time: i64,
) -> Result<RewardResult, PersonalServiceError> {
    let mut joined = Vec::new();
    let mut characters = BTreeMap::new();
    let mut equipment = BTreeMap::new();
    let mut items = Map::new();
    for (reward_id, draws) in drawn {
        let reward = rewards
            .get(&reward_id.to_string())
            .and_then(Value::as_object)
            .ok_or_else(|| PersonalServiceError::new("CN box reward is invalid"))?;
        let reward_type = reward.get("type").and_then(Value::as_i64).unwrap_or(2);
        let count = reward
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            .checked_mul(*draws)
            .ok_or_else(|| PersonalServiceError::new("CN box reward exceeds range"))?;
        let target_id = reward.get("id").and_then(Value::as_i64).unwrap_or_default();
        match reward_type {
            0 => {
                let total = add_item(root, target_id, count)?;
                items.insert(target_id.to_string(), Value::from(total));
            }
            1 => {
                equipment.insert(target_id, add_equipment(root, target_id, count)?);
            }
            2 => {}
            3 => add_user_value(root, "free_mana", count)?,
            4 => add_user_value(root, "exp_pool", count)?,
            5 => {
                let mut response_character = None;
                for _ in 0..count {
                    let reward = grant_character(root, viewer_id, target_id, response_time)?;
                    if reward.joined {
                        joined.push(target_id);
                    }
                    if let Some(item) = reward.duplicate_item {
                        items.insert(item.id.to_string(), Value::from(item.total));
                    }
                    response_character = Some(reward.character);
                }
                characters.insert(
                    target_id,
                    response_character
                        .expect("positive CN box character count produces a response"),
                );
            }
            _ => return Err(PersonalServiceError::new("CN box reward type is invalid")),
        }
    }
    let user_info = root
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user info is missing"))?;
    Ok(RewardResult {
        user_info: json!({
            "free_mana": user_info.get("free_mana").and_then(Value::as_i64).unwrap_or_default(),
            "exp_pool": user_info.get("exp_pool").and_then(Value::as_i64).unwrap_or_default(),
            "exp_pooled_time": user_info.get("exp_pooled_time").cloned().unwrap_or(Value::from(response_time)),
        }),
        joined_character_ids: joined,
        character_list: characters.into_values().collect(),
        equipment_list: equipment.into_values().collect(),
        item_list: items,
    })
}
// //// /发放 CN 箱池抽取奖励 ////

fn add_user_value(
    root: &mut Map<String, Value>,
    key: &str,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let user_info = require_object(root, "user_info")?;
    let total = user_info
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN box user reward exceeds range"))?;
    user_info.insert(key.to_owned(), Value::from(total));
    Ok(())
}

fn add_item(
    root: &mut Map<String, Value>,
    item_id: i64,
    amount: i64,
) -> Result<i64, PersonalServiceError> {
    let items = require_object(root, "item_list")?;
    let key = item_id.to_string();
    let total = items
        .get(&key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN box item reward exceeds range"))?;
    items.insert(key, Value::from(total));
    Ok(total)
}

fn add_equipment(
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
        .ok_or_else(|| PersonalServiceError::new("stored CN equipment is invalid"))?;
    let extra = if was_owned {
        count
    } else {
        count.saturating_sub(1)
    };
    let stack = equipment
        .get("stack")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(extra)
        .ok_or_else(|| PersonalServiceError::new("CN box equipment stack exceeds range"))?;
    equipment.insert("stack".to_owned(), Value::from(stack));
    let protection = equipment
        .get("protection")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let level = equipment.get("level").and_then(Value::as_i64).unwrap_or(1);
    let enhancement_level = equipment
        .get("enhancement_level")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    Ok(json!({
        "equipment_id": equipment_id,
        "protection": protection,
        "level": level,
        "enhancement_level": enhancement_level,
        "stack": stack,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证箱池角色复用统一的重复奖励语义 [@x380kkm 2026-08-23] ////
    #[test]
    fn grants_duplicate_character_item_from_box_rewards() {
        let mut root = Map::from_iter([
            (
                "user_info".to_owned(),
                json!({"free_mana": 0, "exp_pool": 0}),
            ),
            ("user_character_list".to_owned(), json!({})),
            ("user_equipment_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);
        let rewards = json!({
            "1": {"type": 5, "id": 111_001, "count": 2}
        });
        let drawn = BTreeMap::from([(1, 1)]);

        let result =
            apply_rewards(&mut root, 7, rewards.as_object().unwrap(), &drawn, 100).unwrap();

        assert_eq!(result.joined_character_ids, vec![111_001]);
        assert_eq!(result.character_list.len(), 1);
        assert_eq!(result.character_list[0]["stack"], 1);
        assert_eq!(root["user_character_list"]["111001"]["entry_count"], 1);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 1);
        assert_eq!(root["item_list"]["14003"], 1);
        assert_eq!(result.item_list["14003"], 1);
        assert!(root.get("encyclopedia_list").is_none());
    }
    // //// /验证箱池角色复用统一的重复奖励语义 ////

    // //// 验证箱池按角色和装备编号返回最终状态 [@x380kkm 2026-08-28] ////
    #[test]
    fn merges_rewards_that_share_character_and_equipment_ids() {
        let mut root = Map::from_iter([
            (
                "user_info".to_owned(),
                json!({"free_mana": 0, "exp_pool": 0}),
            ),
            ("user_character_list".to_owned(), json!({})),
            ("user_equipment_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);
        let rewards = json!({
            "1": {"type": 5, "id": 111001, "count": 1},
            "2": {"type": 5, "id": 111001, "count": 1},
            "3": {"type": 1, "id": 5030037, "count": 1},
            "4": {"type": 1, "id": 5030037, "count": 2}
        });
        let drawn = BTreeMap::from([(1, 1), (2, 1), (3, 1), (4, 1)]);

        let result =
            apply_rewards(&mut root, 7, rewards.as_object().unwrap(), &drawn, 100).unwrap();

        assert_eq!(result.joined_character_ids, vec![111001]);
        assert_eq!(result.character_list.len(), 1);
        assert_eq!(result.character_list[0]["stack"], 1);
        assert_eq!(result.equipment_list.len(), 1);
        assert_eq!(result.equipment_list[0]["stack"], 2);
        assert_eq!(result.item_list["14003"], 1);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 1);
        assert_eq!(root["user_equipment_list"]["5030037"]["stack"], 2);
    }
    // //// /验证箱池按角色和装备编号返回最终状态 ////

    // //// 验证箱池装备响应保留已有状态字段 [@x380kkm 2026-08-28] ////
    #[test]
    fn returns_existing_equipment_state_after_box_reward() {
        let mut root = Map::from_iter([
            (
                "user_info".to_owned(),
                json!({"free_mana": 0, "exp_pool": 0}),
            ),
            ("user_character_list".to_owned(), json!({})),
            (
                "user_equipment_list".to_owned(),
                json!({
                    "5030037": {
                        "enhancement_level": 7,
                        "level": 12,
                        "protection": true,
                        "stack": 3
                    }
                }),
            ),
            ("item_list".to_owned(), json!({})),
        ]);
        let rewards = json!({"1": {"type": 1, "id": 5030037, "count": 1}});
        let drawn = BTreeMap::from([(1, 1)]);

        let result =
            apply_rewards(&mut root, 7, rewards.as_object().unwrap(), &drawn, 100).unwrap();
        let equipment = &result.equipment_list[0];
        assert_eq!(equipment["equipment_id"], 5_030_037);
        assert_eq!(equipment["enhancement_level"], 7);
        assert_eq!(equipment["level"], 12);
        assert_eq!(equipment["protection"], true);
        assert_eq!(equipment["stack"], 4);
    }
    // //// /验证箱池装备响应保留已有状态字段 ////
}
