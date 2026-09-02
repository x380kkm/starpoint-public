// audience: internal
// # personal-service-cn-battle-rewards
//
// 该模块按 CN 奖励类型修改玩家物品, 角色, 装备, 货币和经验.

use crate::cn_battle_assets::{BattleFixture, BattleQuest, Reward};
use crate::cn_character_reward::grant_character;
use crate::cn_expod::{character_exp_cap, character_level_from_experience};
use crate::cn_tutorial::format_client_time;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct RewardResult {
    pub(crate) mana: i64,
    pub(crate) vmoney: i64,
    pub(crate) exp_pool: i64,
    pub(crate) character_list: Vec<Value>,
    pub(crate) joined_character_ids: Vec<i64>,
    pub(crate) equipment_list: Vec<Value>,
    pub(crate) items: BTreeMap<String, i64>,
}

pub(crate) struct ScoreRewardResult {
    pub(crate) rewards: RewardResult,
    pub(crate) drop_score_reward_ids: Vec<Value>,
    pub(crate) drop_rare_reward_ids: Vec<Value>,
}

pub(crate) struct CharacterExpResult {
    pub(crate) add_exp_list: Vec<Value>,
    pub(crate) character_list: Vec<Value>,
    pub(crate) bond_token_status_list: Map<String, Value>,
    pub(crate) maximum_level: i64,
}

impl RewardResult {
    pub(crate) fn merge(&mut self, other: Self) {
        self.mana += other.mana;
        self.vmoney += other.vmoney;
        self.exp_pool += other.exp_pool;
        self.character_list.extend(other.character_list);
        self.joined_character_ids.extend(other.joined_character_ids);
        self.equipment_list.extend(other.equipment_list);
        for (item_id, count) in other.items {
            self.items.insert(item_id, count);
        }
    }
}

// //// 发放普通奖励和分数掉落 [@x380kkm 2026-07-22] ////
pub(crate) fn apply_reward(
    root: &mut Map<String, Value>,
    reward: &Reward,
) -> Result<RewardResult, PersonalServiceError> {
    apply_reward_at(root, reward, 0)
}

pub(crate) fn apply_reward_at(
    root: &mut Map<String, Value>,
    reward: &Reward,
    server_time: i64,
) -> Result<RewardResult, PersonalServiceError> {
    let mut result = RewardResult::default();
    match reward.kind {
        0 => {
            let item_id = require_reward_id(reward)?;
            let updated = add_item(root, item_id, require_reward_count(reward)?)?;
            result.items.insert(item_id.to_string(), updated);
        }
        1 => {
            let equipment_id = require_reward_id(reward)?;
            let equipment_list = require_object(root, "user_equipment_list")?;
            let equipment_key = equipment_id.to_string();
            let amount = require_reward_count(reward)?;
            let was_owned = equipment_list.contains_key(&equipment_key);
            if !was_owned {
                equipment_list.insert(
                    equipment_key.clone(),
                    json!({
                        "enhancement_level": 0,
                        "level": 1,
                        "protection": false,
                        "stack": amount - 1,
                    }),
                );
            }
            let equipment = equipment_list
                .get_mut(&equipment_key)
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    PersonalServiceError::new("stored CN equipment data is not an object")
                })?;
            if was_owned {
                let stack = equipment
                    .get("stack")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        PersonalServiceError::new("stored CN equipment stack is missing")
                    })?;
                let updated_stack = stack.checked_add(amount).ok_or_else(|| {
                    PersonalServiceError::new("CN equipment stack exceeds the supported range")
                })?;
                equipment.insert("stack".to_owned(), Value::from(updated_stack));
            }
            result.equipment_list.push(json!({
                "equipment_id": equipment_id,
                "protection": equipment.get("protection").and_then(Value::as_bool).unwrap_or(false),
                "level": equipment.get("level").and_then(Value::as_i64).unwrap_or(1),
                "enhancement_level": equipment.get("enhancement_level").and_then(Value::as_i64).unwrap_or(0),
                "stack": equipment.get("stack").and_then(Value::as_i64).unwrap_or_default(),
            }));
        }
        2 => {
            let character_id = require_reward_id(reward)?;
            let reward = grant_character(root, 0, character_id, server_time)?;
            if reward.joined {
                result.joined_character_ids.push(character_id);
            }
            if let Some(item) = reward.duplicate_item {
                result.items.insert(item.id.to_string(), item.total);
            }
            result.character_list.push(reward.character);
        }
        3 => {
            let count = require_reward_count(reward)?;
            add_user_info_value(root, "free_vmoney", count)?;
            result.vmoney = count;
        }
        4 => {
            let count = require_reward_count(reward)?;
            add_user_info_value(root, "free_mana", count)?;
            result.mana = count;
        }
        5 => {
            let count = require_reward_count(reward)?;
            add_user_info_value(root, "exp_pool", count)?;
            result.exp_pool = count;
        }
        6 | 7 => {
            let item_id = require_reward_id(reward)?;
            let updated = add_item(root, item_id, require_reward_count(reward)?)?;
            result.items.insert(item_id.to_string(), updated);
        }
        _ => {
            return Err(PersonalServiceError::new(format!(
                "unsupported CN battle reward kind: {}",
                reward.kind
            )));
        }
    }
    Ok(result)
}

pub(crate) fn apply_drop_reward_at(
    root: &mut Map<String, Value>,
    reward: &Reward,
    drop_multiplier: i64,
    server_time: i64,
) -> Result<RewardResult, PersonalServiceError> {
    if !is_item_drop_kind(reward.kind) {
        return apply_reward_at(root, reward, server_time);
    }
    let mut scaled_reward = reward.clone();
    scaled_reward.count = Some(scale_drop_count(
        require_reward_count(reward)?,
        drop_multiplier,
    )?);
    apply_reward_at(root, &scaled_reward, server_time)
}

fn scaled_item_drop_amount(
    reward_kind: i64,
    count: i64,
    drop_multiplier: i64,
) -> Result<i64, PersonalServiceError> {
    if !is_item_drop_kind(reward_kind) {
        return Ok(count);
    }
    scale_drop_count(count, drop_multiplier)
}

pub(crate) fn apply_score_rewards(
    root: &mut Map<String, Value>,
    fixture: &BattleFixture,
    quest: &BattleQuest,
    boost_point_used: bool,
    drop_multiplier: i64,
    server_time: i64,
) -> Result<ScoreRewardResult, PersonalServiceError> {
    let mut rewards = RewardResult::default();
    let mut drop_score_reward_ids = Vec::new();
    let mut drop_rare_reward_ids = Vec::new();
    let Some(group_id) = quest
        .score_reward_group_id
        .or(quest.score_attack_reward_group_id)
    else {
        return Ok(ScoreRewardResult {
            rewards,
            drop_score_reward_ids,
            drop_rare_reward_ids,
        });
    };
    for (reward_index, score_reward) in quest.score_rewards.iter().enumerate() {
        match score_reward.kind {
            0 => {
                let reward_kind = score_reward
                    .reward_type
                    .ok_or_else(|| PersonalServiceError::new("CN score reward kind is missing"))?;
                let boost_multiplier = if boost_point_used { 2 } else { 1 };
                let base_amount = score_reward
                    .count
                    .unwrap_or_default()
                    .checked_mul(boost_multiplier)
                    .ok_or_else(|| {
                        PersonalServiceError::new(
                            "CN score reward amount exceeds the supported range",
                        )
                    })?;
                let amount = scaled_item_drop_amount(reward_kind, base_amount, drop_multiplier)?;
                let result = apply_reward_at(
                    root,
                    &Reward {
                        kind: reward_kind,
                        id: score_reward.id,
                        count: Some(amount),
                        rarity: None,
                    },
                    server_time,
                )?;
                rewards.merge(result);
                drop_score_reward_ids.push(json!({
                    "group_id": group_id,
                    "index": score_reward.position.unwrap_or(reward_index as i64 + 1),
                    "number": amount,
                }));
            }
            1 => {
                let rarity = score_reward.rarity.ok_or_else(|| {
                    PersonalServiceError::new("CN rare score reward probability is missing")
                })?;
                let roll = random_below(100)? as f64 / 100.0;
                if rarity < roll {
                    continue;
                }
                let rare_group_id = score_reward.id.ok_or_else(|| {
                    PersonalServiceError::new("CN rare score reward group ID is missing")
                })?;
                let rare_group = fixture
                    .rare_reward_groups
                    .get(&rare_group_id.to_string())
                    .ok_or_else(|| {
                        PersonalServiceError::new("CN rare score reward group is missing")
                    })?;
                if rare_group.is_empty() {
                    return Err(PersonalServiceError::new(
                        "CN rare score reward group is empty",
                    ));
                }
                let rare_index = random_below(rare_group.len() as u32)? as usize;
                let rare_reward = &rare_group[rare_index];
                let result = apply_drop_reward_at(root, rare_reward, drop_multiplier, server_time)?;
                rewards.merge(result);
                drop_rare_reward_ids.push(json!({
                    "group_id": rare_group_id,
                    "index": rare_index + 1,
                    "number": if rare_reward.kind == 2 {
                        1
                    } else {
                        scaled_item_drop_amount(
                            rare_reward.kind,
                            require_reward_count(rare_reward)?,
                            drop_multiplier,
                        )?
                    },
                }));
            }
            kind => {
                return Err(PersonalServiceError::new(format!(
                    "unsupported CN score reward kind: {kind}"
                )));
            }
        }
    }
    Ok(ScoreRewardResult {
        rewards,
        drop_score_reward_ids,
        drop_rare_reward_ids,
    })
}

pub(crate) fn apply_score_attack_border_reward(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    score: i64,
    drop_multiplier: i64,
    server_time: i64,
) -> Result<RewardResult, PersonalServiceError> {
    let Some(border) = quest
        .score_attack_border_rewards
        .iter()
        .filter(|border| score >= border.score)
        .max_by_key(|border| border.score)
    else {
        return Ok(RewardResult::default());
    };
    if border.coin_item_id <= 0 || border.coin_count <= 0 {
        return Err(PersonalServiceError::new(
            "CN score attack border reward is invalid",
        ));
    }
    apply_drop_reward_at(
        root,
        &Reward {
            kind: 0,
            id: Some(border.coin_item_id),
            count: Some(border.coin_count),
            rarity: None,
        },
        drop_multiplier,
        server_time,
    )
}

fn is_item_drop_kind(reward_kind: i64) -> bool {
    matches!(reward_kind, 0 | 6 | 7)
}

pub(crate) fn scale_drop_count(
    count: i64,
    drop_multiplier: i64,
) -> Result<i64, PersonalServiceError> {
    count
        .max(0)
        .checked_mul(drop_multiplier.max(1))
        .ok_or_else(|| {
            PersonalServiceError::new("CN battle drop reward amount exceeds the supported range")
        })
}
// //// /发放普通奖励和分数掉落 ////

// //// 发放参战角色经验 [@x380kkm 2026-07-22] ////
pub(crate) fn apply_character_exp(
    root: &mut Map<String, Value>,
    fixture: &BattleFixture,
    character_ids: &[i64],
    exp_amount: i64,
    server_time: i64,
    ignore_update: bool,
) -> Result<CharacterExpResult, PersonalServiceError> {
    let mut add_exp_list = Vec::new();
    let mut character_list = Vec::new();
    let mut bond_token_status_list = Map::new();
    let mut pooled_exp = 0_i64;
    let mut maximum_level = 0_i64;
    let characters = require_object(root, "user_character_list")?;
    for character_id in character_ids {
        let Some(character) = characters
            .get_mut(&character_id.to_string())
            .and_then(Value::as_object_mut)
        else {
            add_exp_list.push(json!({
                "character_id": character_id,
                "add_exp": 0,
                "after_exp": 379988,
                "add_exp_pool": 0,
            }));
            bond_token_status_list.insert(
                character_id.to_string(),
                json!({ "before": [], "after": [] }),
            );
            continue;
        };
        if ignore_update {
            add_exp_list.push(json!({
                "character_id": character_id,
                "add_exp": 0,
                "after_exp": 379988,
                "add_exp_pool": 0,
            }));
            bond_token_status_list.insert(
                character_id.to_string(),
                json!({ "before": [], "after": [] }),
            );
            continue;
        }
        let rarity = fixture
            .characters
            .get(&character_id.to_string())
            .map(|asset| asset.rarity)
            .ok_or_else(|| PersonalServiceError::new("CN character battle data is missing"))?;
        let over_limit_step = character
            .get("over_limit_step")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN character limit is missing"))?;
        let exp_cap = character_exp_cap(rarity, over_limit_step)
            .ok_or_else(|| PersonalServiceError::new("CN character rarity is invalid"))?;
        let current_exp = character
            .get("exp")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN character EXP is missing"))?;
        let uncapped_exp = current_exp.checked_add(exp_amount).ok_or_else(|| {
            PersonalServiceError::new("CN character EXP exceeds the supported range")
        })?;
        let overflow_exp = uncapped_exp.saturating_sub(exp_cap).max(0);
        let after_exp = uncapped_exp.min(exp_cap);
        let character_level =
            character_level_from_experience(rarity, after_exp, over_limit_step)
                .ok_or_else(|| PersonalServiceError::new("CN character progression is invalid"))?;
        maximum_level = maximum_level.max(character_level);
        pooled_exp = pooled_exp.checked_add(overflow_exp).ok_or_else(|| {
            PersonalServiceError::new("CN pooled EXP exceeds the supported range")
        })?;
        character.insert("exp".to_owned(), Value::from(after_exp));
        if server_time > 0 {
            character.insert("update_time".to_owned(), Value::from(server_time));
        }
        let join_time = character
            .get("join_time")
            .and_then(Value::as_i64)
            .ok_or_else(|| PersonalServiceError::new("stored CN character join time is missing"))?;
        let update_time = character
            .get("update_time")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                PersonalServiceError::new("stored CN character update time is missing")
            })?;
        let join_time = format_client_time(join_time);
        let update_time = format_client_time(update_time);
        add_exp_list.push(json!({
            "character_id": character_id,
            "add_exp": exp_amount - overflow_exp,
            "after_exp": after_exp,
            "add_exp_pool": overflow_exp,
        }));
        character_list.push(json!({
            "character_id": character_id,
            "exp": after_exp,
            "create_time": join_time.clone(),
            "update_time": update_time,
            "join_time": join_time,
            "exp_total": after_exp,
        }));
        let bond_tokens = character
            .get("bond_token_list")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        bond_token_status_list.insert(
            character_id.to_string(),
            json!({ "before": bond_tokens, "after": bond_tokens }),
        );
    }
    add_user_info_value(root, "exp_pool", pooled_exp)?;
    Ok(CharacterExpResult {
        add_exp_list,
        character_list,
        bond_token_status_list,
        maximum_level,
    })
}
// //// /发放参战角色经验 ////

// //// 校验奖励数据并计算掉落边界 [@x380kkm 2026-07-22] ////
fn add_item(
    root: &mut Map<String, Value>,
    item_id: i64,
    amount: i64,
) -> Result<i64, PersonalServiceError> {
    let item_list = require_object(root, "item_list")?;
    let item_key = item_id.to_string();
    let current = item_list
        .get(&item_key)
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let updated = current.checked_add(amount).ok_or_else(|| {
        PersonalServiceError::new("CN battle item count exceeds the supported range")
    })?;
    item_list.insert(item_key, Value::from(updated));
    Ok(updated)
}

fn require_reward_id(reward: &Reward) -> Result<i64, PersonalServiceError> {
    reward
        .id
        .ok_or_else(|| PersonalServiceError::new("CN battle reward ID is missing"))
}

fn require_reward_count(reward: &Reward) -> Result<i64, PersonalServiceError> {
    reward
        .count
        .filter(|count| *count > 0)
        .ok_or_else(|| PersonalServiceError::new("CN battle reward count is missing"))
}

fn require_object<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} data is missing")))
}

fn add_user_info_value(
    root: &mut Map<String, Value>,
    key: &str,
    amount: i64,
) -> Result<i64, PersonalServiceError> {
    let user_info = require_object(root, "user_info")?;
    let current = user_info
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} value is missing")))?;
    let updated = current.checked_add(amount).ok_or_else(|| {
        PersonalServiceError::new(format!("stored CN {key} exceeds the supported range"))
    })?;
    user_info.insert(key.to_owned(), Value::from(updated));
    Ok(updated)
}

fn random_below(upper_bound: u32) -> Result<u32, PersonalServiceError> {
    if upper_bound <= 1 {
        return Ok(0);
    }
    let accepted_range = u32::MAX - (u32::MAX % upper_bound);
    loop {
        let mut bytes = [0_u8; 4];
        getrandom::getrandom(&mut bytes).map_err(|error| {
            PersonalServiceError::new(format!("failed to generate CN battle reward: {error}"))
        })?;
        let value = u32::from_le_bytes(bytes);
        if value < accepted_range {
            return Ok(value % upper_bound);
        }
    }
}

// //// /校验奖励数据并计算掉落边界 ////

#[cfg(test)]
mod tests {
    use super::*;

    fn reward(kind: i64, id: Option<i64>, count: i64) -> Reward {
        Reward {
            kind,
            id,
            count: Some(count),
            rarity: None,
        }
    }

    // //// 验证全部战斗奖励类型和重复角色奖励 [@x380kkm 2026-08-23] ////
    #[test]
    fn grants_every_reward_kind_and_counts_duplicate_characters() {
        let mut root = Map::from_iter([
            (
                "user_info".to_owned(),
                json!({"free_vmoney": 10, "free_mana": 20, "exp_pool": 30}),
            ),
            ("item_list".to_owned(), json!({})),
            ("user_equipment_list".to_owned(), json!({})),
            ("user_character_list".to_owned(), json!({})),
        ]);
        let item = apply_reward_at(&mut root, &reward(0, Some(10), 2), 100).unwrap();
        let equipment = apply_reward_at(&mut root, &reward(1, Some(20), 2), 100).unwrap();
        let first_character =
            apply_reward_at(&mut root, &reward(2, Some(111_001), 1), 100).unwrap();
        let duplicate_character =
            apply_reward_at(&mut root, &reward(2, Some(111_001), 1), 200).unwrap();
        apply_reward_at(&mut root, &reward(3, None, 5), 100).unwrap();
        apply_reward_at(&mut root, &reward(4, None, 7), 100).unwrap();
        apply_reward_at(&mut root, &reward(5, None, 9), 100).unwrap();
        let element = apply_reward_at(&mut root, &reward(6, Some(40), 3), 100).unwrap();
        let aether = apply_reward_at(&mut root, &reward(7, Some(41), 4), 100).unwrap();

        assert_eq!(root["item_list"]["10"], 2);
        assert_eq!(root["item_list"]["40"], 3);
        assert_eq!(root["item_list"]["41"], 4);
        assert_eq!(item.items["10"], 2);
        assert_eq!(element.items["40"], 3);
        assert_eq!(aether.items["41"], 4);
        assert_eq!(root["user_equipment_list"]["20"]["stack"], 1);
        assert_eq!(equipment.equipment_list[0]["stack"], 1);
        assert_eq!(root["user_character_list"]["111001"]["entry_count"], 1);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 1);
        assert_eq!(root["item_list"]["14003"], 1);
        assert_eq!(duplicate_character.items["14003"], 1);
        assert_eq!(first_character.joined_character_ids, vec![111_001]);
        assert!(duplicate_character.joined_character_ids.is_empty());
        assert_eq!(root["user_info"]["free_vmoney"], 15);
        assert_eq!(root["user_info"]["free_mana"], 27);
        assert_eq!(root["user_info"]["exp_pool"], 39);
    }
    // //// /验证全部战斗奖励类型和重复角色奖励 ////

    // //// 验证角色奖励数量字段不会重复发放角色 [@x380kkm 2026-08-28] ////
    #[test]
    fn treats_character_reward_as_one_grant() {
        let mut root = Map::from_iter([
            ("user_character_list".to_owned(), json!({})),
            ("item_list".to_owned(), json!({})),
        ]);

        let result = apply_reward_at(&mut root, &reward(2, Some(111_001), 131_182), 100).unwrap();

        assert_eq!(result.character_list.len(), 1);
        assert_eq!(result.joined_character_ids, vec![111_001]);
        assert_eq!(root["user_character_list"]["111001"]["entry_count"], 1);
        assert_eq!(root["user_character_list"]["111001"]["stack"], 0);
        assert_eq!(root["item_list"], json!({}));
    }
    // //// /验证角色奖励数量字段不会重复发放角色 ////

    // //// 验证合并战斗奖励时保留道具最终持有量 [@x380kkm 2026-08-28] ////
    #[test]
    fn keeps_latest_item_total_when_merging_rewards() {
        let mut root = Map::from_iter([("item_list".to_owned(), json!({"10": 5}))]);
        let mut rewards = apply_reward_at(&mut root, &reward(0, Some(10), 2), 100).unwrap();
        rewards.merge(apply_reward_at(&mut root, &reward(0, Some(10), 3), 100).unwrap());

        assert_eq!(root["item_list"]["10"], 10);
        assert_eq!(rewards.items["10"], 10);
    }
    // //// /验证合并战斗奖励时保留道具最终持有量 ////

    // //// 验证掉落倍率只作用于战斗道具数量 [@x380kkm 2026-08-26] ////
    #[test]
    fn scales_item_drops_without_scaling_other_reward_types() {
        let mut root = Map::from_iter([
            (
                "user_info".to_owned(),
                json!({"free_vmoney": 10, "free_mana": 20, "exp_pool": 30}),
            ),
            ("item_list".to_owned(), json!({})),
            ("user_equipment_list".to_owned(), json!({})),
            ("user_character_list".to_owned(), json!({})),
        ]);

        let item = apply_drop_reward_at(&mut root, &reward(0, Some(10), 2), 3, 100).unwrap();
        let element = apply_drop_reward_at(&mut root, &reward(6, Some(40), 2), 3, 100).unwrap();
        let aether = apply_drop_reward_at(&mut root, &reward(7, Some(41), 2), 3, 100).unwrap();
        let equipment = apply_drop_reward_at(&mut root, &reward(1, Some(20), 2), 3, 100).unwrap();
        apply_drop_reward_at(&mut root, &reward(3, None, 5), 3, 100).unwrap();
        apply_drop_reward_at(&mut root, &reward(4, None, 7), 3, 100).unwrap();
        apply_drop_reward_at(&mut root, &reward(5, None, 9), 3, 100).unwrap();

        assert_eq!(root["item_list"]["10"], 6);
        assert_eq!(root["item_list"]["40"], 6);
        assert_eq!(root["item_list"]["41"], 6);
        assert_eq!(item.items["10"], 6);
        assert_eq!(element.items["40"], 6);
        assert_eq!(aether.items["41"], 6);
        assert_eq!(root["user_equipment_list"]["20"]["stack"], 1);
        assert_eq!(equipment.equipment_list[0]["stack"], 1);
        assert_eq!(root["user_info"]["free_vmoney"], 15);
        assert_eq!(root["user_info"]["free_mana"], 27);
        assert_eq!(root["user_info"]["exp_pool"], 39);
    }
    // //// /验证掉落倍率只作用于战斗道具数量 ////

    // //// 验证角色溢出经验进入经验池 [@x380kkm 2026-08-22] ////
    #[test]
    fn moves_character_exp_overflow_into_exp_pool() {
        let fixture = crate::cn_battle_assets::load_battle_fixture().unwrap();
        let mut root = Map::from_iter([
            ("user_info".to_owned(), json!({"exp_pool": 5})),
            (
                "user_character_list".to_owned(),
                json!({
                    "1": {
                        "exp": 76270,
                        "over_limit_step": 0,
                        "join_time": 100,
                        "update_time": 100,
                        "bond_token_list": []
                    }
                }),
            ),
        ]);
        let result = apply_character_exp(&mut root, &fixture, &[1], 10, 200, false).unwrap();
        assert_eq!(root["user_character_list"]["1"]["exp"], 76272);
        assert_eq!(root["user_character_list"]["1"]["update_time"], 200);
        assert_eq!(root["user_info"]["exp_pool"], 13);
        assert_eq!(result.add_exp_list[0]["add_exp"], 2);
        assert_eq!(result.add_exp_list[0]["add_exp_pool"], 8);
        assert_eq!(result.maximum_level, 70);
        assert_eq!(
            result.character_list[0]["update_time"],
            format_client_time(200)
        );
    }
    // //// /验证角色溢出经验进入经验池 ////

    // //// 验证经验响应为每个参战角色提供羁绊状态 [@x380kkm 2026-08-28] ////
    #[test]
    fn includes_bond_status_for_missing_and_ignored_characters() {
        let fixture = crate::cn_battle_assets::load_battle_fixture().unwrap();
        let mut root = Map::from_iter([
            ("user_info".to_owned(), json!({"exp_pool": 0})),
            ("user_character_list".to_owned(), json!({"1": {}})),
        ]);

        let result =
            apply_character_exp(&mut root, &fixture, &[999_999, 1], 100, 200, true).unwrap();

        assert_eq!(
            result.bond_token_status_list["999999"],
            json!({"before": [], "after": []})
        );
        assert_eq!(
            result.bond_token_status_list["1"],
            json!({"before": [], "after": []})
        );
    }
    // //// /验证经验响应为每个参战角色提供羁绊状态 ////
}
