// audience: internal
// # personal-service-cn-payment-rebate-rewards
// 返利使用 GiftType 的资产编号. 客户端余额和数量保持有符号 32 位整数范围.

use crate::cn_battle_assets::Reward;
use crate::cn_battle_rewards::apply_reward_at;
use crate::cn_tutorial::{create_character_response, require_object};
use crate::database::ReceiveHistoryEntry;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct RebateReward {
    #[serde(rename = "type")]
    pub(super) kind: i64,
    pub(super) type_id: Option<i64>,
    pub(super) number: i64,
}

impl RebateReward {
    pub(super) fn is_valid(&self) -> bool {
        if !(1..=i32::MAX as i64).contains(&self.number) {
            return false;
        }
        match self.kind {
            1 | 5 | 6 | 13 | 14 => self
                .type_id
                .is_some_and(|id| (1..=i32::MAX as i64).contains(&id)),
            3 | 4 | 7..=12 | 15 => self.type_id.is_none(),
            _ => false,
        }
    }
}

#[derive(Default)]
pub(super) struct GrantedRewards {
    pub(super) history: Vec<ReceiveHistoryEntry>,
    items: BTreeSet<String>,
    characters: BTreeSet<i64>,
    equipment: BTreeSet<i64>,
    degrees: BTreeSet<i64>,
    balances: BTreeSet<&'static str>,
    challenges: BTreeSet<i64>,
}

// //// 按礼包资产类型授予返利并记录实发数量 [@x380kkm 2026-09-23] ////
pub(super) fn grant(
    root: &mut Map<String, Value>,
    rewards: &[RebateReward],
    viewer_id: i64,
    now: i64,
) -> Result<GrantedRewards, PersonalServiceError> {
    let mut granted = GrantedRewards::default();
    for reward in rewards {
        let mut number = reward.number;
        let id = reward.type_id.unwrap_or_default();
        match reward.kind {
            1 => granted.apply_asset(root, 0, Some(id), number, now)?,
            3 => granted.add_balance(root, "vmoney", number)?,
            4 => granted.add_balance(root, "free_vmoney", number)?,
            5 => {
                for _ in 0..number {
                    let character =
                        crate::cn_character_reward::grant_character(root, viewer_id, id, now)?;
                    if let Some(item) = character.duplicate_item {
                        granted.items.insert(item.id.to_string());
                        granted.history.push(ReceiveHistoryEntry::reward(
                            1,
                            Some(item.id),
                            item.count,
                        ));
                    }
                }
                crate::cn_character_reward::record_character_encyclopedia_state(root, id)?;
                granted.characters.insert(id);
            }
            6 => {
                granted.apply_asset(root, 1, Some(id), number, now)?;
                granted.equipment.insert(id);
            }
            7 => granted.add_balance(root, "star_crumb", number)?,
            8 => {
                granted.apply_asset(root, 4, None, number, now)?;
                granted.balances.insert("free_mana");
            }
            9 => {
                crate::cn_exp_pool::settle(root, now)?;
                granted.apply_asset(root, 5, None, number, now)?;
                granted.balances.extend(["exp_pool", "exp_pooled_time"]);
            }
            10 => granted.add_balance(root, "bond_token", number)?,
            11 => granted.add_balance(root, "boss_boost_point", number)?,
            12 => granted.add_balance(root, "boost_point", number)?,
            13 => {
                let degrees = root
                    .entry("degree_ids".to_owned())
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .ok_or_else(|| PersonalServiceError::new("stored CN degrees are invalid"))?;
                number = if degrees.iter().any(|value| value.as_i64() == Some(id)) {
                    0
                } else {
                    degrees.push(Value::from(id));
                    1
                };
                granted.degrees.insert(id);
            }
            14 => {
                add_challenge_points(root, id, number)?;
                granted.challenges.insert(id);
            }
            15 => {
                if crate::cn_rank_reward::grant_rank_points(root, number, now)? {
                    granted.balances.extend(["stamina", "stamina_heal_time"]);
                }
                granted.balances.insert("rank_point");
            }
            _ => {
                return Err(PersonalServiceError::new(
                    "CN rebate reward type is unsupported",
                ))
            }
        }
        if number > 0 {
            granted.history.push(ReceiveHistoryEntry::reward(
                reward.kind,
                reward.type_id,
                number,
            ));
        }
    }
    for entry in &mut granted.history {
        entry.subject = Some("返利奖励".to_owned());
        entry.description = Some("返利领取".to_owned());
    }
    Ok(granted)
}
// //// /按礼包资产类型授予返利并记录实发数量 ////

impl GrantedRewards {
    fn apply_asset(
        &mut self,
        root: &mut Map<String, Value>,
        kind: i64,
        id: Option<i64>,
        count: i64,
        now: i64,
    ) -> Result<(), PersonalServiceError> {
        let result = apply_reward_at(
            root,
            &Reward {
                kind,
                id,
                count: Some(count),
                rarity: None,
            },
            now,
        )?;
        self.items.extend(result.items.into_keys());
        Ok(())
    }

    fn add_balance(
        &mut self,
        root: &mut Map<String, Value>,
        field: &'static str,
        amount: i64,
    ) -> Result<(), PersonalServiceError> {
        let user = require_object(root, "user_info")?;
        add_count(user, field, amount)?;
        self.balances.insert(field);
        Ok(())
    }

    // //// 核对本次发放涉及的客户端整数边界 [@x380kkm 2026-09-23] ////
    pub(super) fn fits_client(&self, root: &Map<String, Value>) -> bool {
        let in_range = |value: Option<&Value>| {
            value
                .and_then(Value::as_i64)
                .is_some_and(|value| (0..=i32::MAX as i64).contains(&value))
        };
        self.balances
            .iter()
            .all(|key| in_range(root.get("user_info").and_then(|user| user.get(*key))))
            && self
                .items
                .iter()
                .all(|id| in_range(root.get("item_list").and_then(|items| items.get(id))))
            && self.equipment.iter().all(|id| {
                in_range(
                    root.get("user_equipment_list")
                        .and_then(|items| items.get(id.to_string()))
                        .and_then(|item| item.get("stack")),
                )
            })
            && self.characters.iter().all(|id| {
                in_range(
                    root.get("user_character_list")
                        .and_then(|items| items.get(id.to_string()))
                        .and_then(|item| item.get("stack")),
                )
            })
            && self.challenges.iter().all(|id| {
                in_range(
                    root.get("user_daily_challenge_point_list")
                        .and_then(Value::as_array)
                        .and_then(|items| {
                            items
                                .iter()
                                .find(|item| item.get("id").and_then(Value::as_i64) == Some(*id))
                        })
                        .and_then(|item| item.get("point")),
                )
            })
    }

    // //// 将返利后的实际资产投影到公共响应 [@x380kkm 2026-09-23] ////
    pub(super) fn project(
        &self,
        response: &mut Value,
        root: &Map<String, Value>,
        viewer_id: i64,
        now: i64,
    ) {
        let mut user = Map::new();
        for field in &self.balances {
            if let Some(value) = root.get("user_info").and_then(|user| user.get(*field)) {
                user.insert((*field).to_owned(), value.clone());
            }
        }
        if !user.is_empty() {
            response["user_info"] = Value::Object(user);
        }
        if !self.items.is_empty() {
            response["item_list"] = Value::Object(
                self.items
                    .iter()
                    .filter_map(|id| {
                        root.get("item_list")
                            .and_then(|items| items.get(id))
                            .map(|value| (id.clone(), value.clone()))
                    })
                    .collect(),
            );
        }
        if !self.characters.is_empty() {
            response["character_list"] = Value::Array(
                self.characters
                    .iter()
                    .filter_map(|id| {
                        let stored = root.get("user_character_list")?.get(id.to_string())?;
                        let mut value = create_character_response(viewer_id, *id, stored, now);
                        for field in ["evolution_level", "over_limit_step", "protection", "stack"] {
                            if let Some(stored_value) = stored.get(field) {
                                value[field] = stored_value.clone();
                            }
                        }
                        Some(value)
                    })
                    .collect(),
            );
            if let Some(value) = root.get("encyclopedia_list") {
                response["encyclopedia_info"] = value.clone();
            }
        }
        if !self.equipment.is_empty() {
            response["equipment_list"] = Value::Array(
                self.equipment
                    .iter()
                    .filter_map(|id| {
                        let mut value = root
                            .get("user_equipment_list")?
                            .get(id.to_string())?
                            .clone();
                        value["equipment_id"] = Value::from(*id);
                        Some(value)
                    })
                    .collect(),
            );
        }
        if !self.degrees.is_empty() {
            response["degree_list"] = json!(self
                .degrees
                .iter()
                .map(|id| json!({"viewer_id": viewer_id, "degree_id": id}))
                .collect::<Vec<_>>());
        }
        if !self.challenges.is_empty() {
            let mut challenges = root
                .get("user_daily_challenge_point_list")
                .cloned()
                .unwrap_or_else(|| json!([]));
            if let Some(challenges) = challenges.as_array_mut() {
                for challenge in challenges {
                    if !challenge.get("campaign_list").is_some_and(Value::is_array) {
                        challenge["campaign_list"] = json!([]);
                    }
                }
            }
            response["user_daily_challenge_point_list"] = challenges;
        }
    }
}

fn add_count(
    object: &mut Map<String, Value>,
    key: &str,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let before = object.get(key).and_then(Value::as_i64).unwrap_or_default();
    let after = before
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN rebate amount exceeds the supported range"))?;
    object.insert(key.to_owned(), Value::from(after));
    Ok(())
}

// //// 增加指定每日挑战的现有可用次数 [@x380kkm 2026-09-23] ////
fn add_challenge_points(
    root: &mut Map<String, Value>,
    id: i64,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let list = root
        .entry("user_daily_challenge_point_list".to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN challenge points are invalid"))?;
    if !list
        .iter()
        .any(|entry| entry.get("id").and_then(Value::as_i64) == Some(id))
    {
        list.push(json!({"id": id, "point": 0, "campaign_list": []}));
    }
    let entry = list
        .iter_mut()
        .find(|entry| entry.get("id").and_then(Value::as_i64) == Some(id))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN challenge point is invalid"))?;
    add_count(entry, "point", amount)
}
