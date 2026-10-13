// audience: internal
// # personal-service-cn-box-gacha-reward
//
// 该模块把 CN 箱池抽取结果写入玩家角色、装备、道具和货币.
// 入口接收已扣费的库存, 道具收集数量覆盖本次发奖产生的入库增量.

use crate::cn_character_reward::grant_character;
use crate::cn_tutorial::require_object;
use crate::cn_reward_overflow::{grant, Conversion};
use crate::database::MailReward;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(super) struct RewardResult {
    pub(super) user_info: Value,
    pub(super) joined_character_ids: Vec<i64>,
    pub(super) character_list: Vec<Value>,
    pub(super) equipment_list: Vec<Value>,
    pub(super) item_list: Map<String, Value>,
    pub(super) acquired_items: BTreeMap<i64, i64>,
}

// //// 发放 CN 箱池抽取奖励 [@x380kkm 2026-08-22] ////
pub(super) fn apply_rewards(
    root: &mut Map<String, Value>,
    viewer_id: i64,
    rewards: &Map<String, Value>,
    drawn: &BTreeMap<i64, i64>,
    response_time: i64,
) -> Result<RewardResult, PersonalServiceError> {
    crate::cn_exp_pool::settle(root, response_time)?;
    let items_before_rewards = require_object(root, "item_list")?.clone();
    let mut joined = Vec::new();
    let mut characters = BTreeMap::new();
    let mut equipment = BTreeMap::new();
    let mut items = Map::new();
    let mut inventory_rewards = MailReward::default();
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
                add_reward_count(inventory_rewards.item_list.entry(target_id.to_string()).or_default(), count)?;
            }
            1 => {
                equipment.insert(
                    target_id,
                    crate::cn_equipment::grant_equipment(root, target_id, count, response_time)?,
                );
            }
            2 => {}
            3 => add_reward_count(&mut inventory_rewards.free_mana, count)?,
            4 => add_reward_count(&mut inventory_rewards.exp_pool, count)?,
            6 => {
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
    if inventory_rewards.free_mana > 0 || inventory_rewards.exp_pool > 0
        || !inventory_rewards.item_list.is_empty() {
        let item_ids = inventory_rewards.item_list.keys().cloned().collect::<Vec<_>>();
        grant(root, Conversion {
            created_at: response_time,
            subject: "箱式扭蛋奖励".to_owned(),
            description: "箱式扭蛋抽取所得的奖励.".to_owned(),
            notice: None,
            reward: inventory_rewards,
        })?;
        for id in item_ids {
            items.insert(id.clone(), json!(crate::cn_reward_capacity::held_count(root, "item_list", &id)));
        }
    }
    let mut acquired_items = BTreeMap::new();
    for (id, value) in require_object(root, "item_list")? {
        let before = items_before_rewards.get(id).and_then(Value::as_i64).unwrap_or_default();
        let acquired = value.as_i64().unwrap_or_default().saturating_sub(before);
        if acquired > 0 {
            let item_id = id.parse().map_err(|_| PersonalServiceError::new("CN box item id is invalid"))?;
            acquired_items.insert(item_id, acquired);
            items.insert(id.clone(), value.clone());
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
        acquired_items,
    })
}
// //// /发放 CN 箱池抽取奖励 ////

// //// 汇总同一批抽取中的库存奖励数量 [@x380kkm 2026-09-28] ////
fn add_reward_count(total: &mut i64, amount: i64) -> Result<(), PersonalServiceError> {
    *total = total.checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN box reward count exceeds range"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证同种物品支付费用后按奖励入库量返回收集数量 [@x380kkm 2026-09-26] ////
    #[test]
    fn counts_item_reward_after_currency_debit() {
        let mut root = json!({
            "user_info":{"free_mana":0,"exp_pool":0},
            "item_list":{"100000":100}
        }).as_object().unwrap().clone();
        root["item_list"]["100000"] = json!(90);
        let rewards = json!({"1":{"type":0,"id":100000,"count":1}});
        let result = apply_rewards(&mut root, 7, rewards.as_object().unwrap(),
            &BTreeMap::from([(1, 1)]), 100).unwrap();
        assert_eq!(root["item_list"]["100000"], 91);
        assert_eq!(result.item_list["100000"], 91);
        assert_eq!(result.acquired_items.get(&100000), Some(&1));
    }
    // //// /验证同种物品支付费用后按奖励入库量返回收集数量 ////

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
            "1": {"type": 6, "id": 111_001, "count": 2}
        });
        let drawn = BTreeMap::from([(1, 1)]);

        let result =
            apply_rewards(&mut root, 7, rewards.as_object().unwrap(), &drawn, 100).unwrap();

        assert_eq!(result.joined_character_ids, vec![111_001]);
        assert_eq!(result.character_list.len(), 1);
        assert_eq!(result.character_list[0]["stack"], 1);
        assert_eq!(root["user_character_list"]["111001"]["entry_count"], 1);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 1);
        assert_eq!(root["item_list"]["14003"], 3);
        assert_eq!(result.item_list["14003"], 3);
        assert_eq!(result.acquired_items.get(&14003), Some(&3));
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
            "1": {"type": 6, "id": 111001, "count": 1},
            "2": {"type": 6, "id": 111001, "count": 1},
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
        assert_eq!(result.item_list["14003"], 3);
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
                        "stack": 998
                    }
                }),
            ),
            ("item_list".to_owned(), json!({})),
        ]);
        let rewards = json!({"1": {"type": 1, "id": 5030037, "count": 3}});
        let drawn = BTreeMap::from([(1, 1)]);

        let result =
            apply_rewards(&mut root, 7, rewards.as_object().unwrap(), &drawn, 100).unwrap();
        let equipment = &result.equipment_list[0];
        assert_eq!(equipment["equipment_id"], 5_030_037);
        assert_eq!(equipment["enhancement_level"], 7);
        assert_eq!(equipment["level"], 12);
        assert_eq!(equipment["protection"], true);
        assert_eq!(equipment["stack"], 999);
        assert_eq!(root["item_list"]["100000"], 10);
        assert_eq!(root["item_list"]["5030037"], 2);
        assert_eq!(result.acquired_items.get(&100000), Some(&10));
        assert_eq!(result.acquired_items.get(&5030037), Some(&2));
    }
    // //// /验证箱池装备响应保留已有状态字段 ////
}
