// audience: internal
// # personal-service-cn-mission-reward
//
// 该模块按任务奖励类型结算玩家资产, 并从持久化状态组装客户端资产增量.
// 物品, 玛纳和 EXP 通过容量投递记录保存实际到账与邮件余量, 阶段凭据保留完整奖励定义.

use super::{mission_catalog, MissionReward};
use crate::cn_battle_assets::Reward;
use crate::cn_character_reward::record_character_encyclopedia_state;
use crate::cn_tutorial::create_character_response;
use crate::database::{MailReward, ReceiveHistoryEntry};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

// //// 保存阶段实际发放的奖励并返回领取记录 [@x380kkm 2026-09-21] ////
pub(crate) fn settle_rewards(
    root: &mut Map<String, Value>,
    receipt_field: &str,
    receipt_key: &str,
    rewards: &[MissionReward],
    recorded_rewards: &[MissionReward],
    response_time: i64,
) -> Result<Option<Vec<ReceiveHistoryEntry>>, PersonalServiceError> {
    let receipt = root.get(receipt_field).and_then(|receipts| receipts.get(receipt_key));
    if receipt.is_some_and(Value::is_object) {
        return Ok(None);
    }
    let received = receipt.and_then(Value::as_bool).unwrap_or(false);
    let remaining = rewards.iter().filter(|reward| {
        !received || !recorded_rewards.contains(reward)
    }).cloned().collect::<Vec<_>>();
    let history = apply_rewards(root, &remaining, response_time)?;
    let granted = remaining.iter().filter(|reward| !is_inventory_reward(reward)).collect::<Vec<_>>();
    root.entry(receipt_field.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN mission receipts are invalid"))?
        .insert(receipt_key.to_owned(), json!({"rewards": rewards, "granted_rewards": granted, "received_at": response_time}));
    Ok(Some(history))
}
// //// /保存阶段实际发放的奖励并返回领取记录 ////

// //// 按任务主表奖励类型发放资产和称号 [@x380kkm 2026-09-21] ////
pub(crate) fn apply_rewards(
    root: &mut Map<String, Value>,
    rewards: &[MissionReward],
    response_time: i64,
) -> Result<Vec<ReceiveHistoryEntry>, PersonalServiceError> {
    let mut history = Vec::new();
    let mut inventory = MailReward::default();
    for reward in rewards {
        if is_inventory_reward(reward) {
            add_inventory_reward(&mut inventory, reward)?;
            continue;
        }
        if reward.kind == 7 {
            let event_id = reward.pass_card_event_id.ok_or_else(|| PersonalServiceError::new("CN pass card reward event is missing"))?;
            crate::cn_pass_card::add_points_for_event(root, event_id, reward.amount)?;
            continue;
        }
        if reward.kind == 6 {
            let degree_id = reward.degree_id.ok_or_else(|| PersonalServiceError::new("CN mission degree id is missing"))?;
            let degrees = root.entry("degree_ids".to_owned()).or_insert_with(|| json!([]))
                .as_array_mut().ok_or_else(|| PersonalServiceError::new("stored CN degrees are invalid"))?;
            if !degrees.iter().any(|value| value.as_i64() == Some(degree_id)) {
                degrees.push(Value::from(degree_id));
            }
            continue;
        }
        let Some((asset, entry)) = asset_reward(reward)? else { continue; };
        crate::cn_battle_rewards::apply_reward_at(root, &asset, response_time)?;
        if let Some(character_id) = reward.character_id.filter(|_| reward.kind == 4) {
            record_character_encyclopedia_state(root, character_id)?;
        }
        history.push(entry);
    }
    if inventory.free_mana > 0 || inventory.exp_pool > 0 || !inventory.item_list.is_empty() {
        crate::cn_reward_overflow::grant(root, crate::cn_reward_overflow::Conversion {
            created_at: response_time,
            subject: "任务奖励".to_owned(),
            description: "任务完成所得的奖励.".to_owned(),
            notice: None,
            reward: inventory,
        })?;
    }
    Ok(history)
}
// //// /按任务主表奖励类型发放资产和称号 ////

fn is_inventory_reward(reward: &MissionReward) -> bool {
    matches!(reward.kind, 1 | 3 | 5)
}

// //// 合并同阶段需要按容量投递的奖励 [@x380kkm 2026-09-28] ////
fn add_inventory_reward(inventory: &mut MailReward, reward: &MissionReward) -> Result<(), PersonalServiceError> {
    if reward.amount <= 0 { return Err(PersonalServiceError::new("CN mission reward amount is invalid")); }
    let total = match reward.kind {
        1 => {
            let id = reward.item_id.ok_or_else(|| PersonalServiceError::new("CN mission item id is missing"))?;
            inventory.item_list.entry(id.to_string()).or_default()
        }
        3 => &mut inventory.free_mana,
        5 => &mut inventory.exp_pool,
        _ => return Err(PersonalServiceError::new("CN inventory reward kind is invalid")),
    };
    *total = total.checked_add(reward.amount).ok_or_else(|| PersonalServiceError::new("CN mission reward amount exceeds range"))?;
    Ok(())
}
// //// /合并同阶段需要按容量投递的奖励 ////

pub(super) fn reward_history_entries(rewards: &[MissionReward]) -> Result<Vec<ReceiveHistoryEntry>, PersonalServiceError> {
    rewards.iter().map(asset_reward).filter_map(|result| match result {
        Ok(Some((_, entry))) => Some(Ok(entry)),
        Ok(None) => None,
        Err(error) => Some(Err(error)),
    }).collect()
}

fn asset_reward(reward: &MissionReward) -> Result<Option<(Reward, ReceiveHistoryEntry)>, PersonalServiceError> {
    let (kind, id, history_kind) = match reward.kind {
        0 => (3, None, 4),
        1 => (0, reward.item_id, 1),
        2 => (1, reward.equipment_id, 6),
        3 => (4, None, 8),
        4 => (2, reward.character_id, 5),
        5 => (5, None, 9),
        6 | 7 => return Ok(None),
        _ => return Err(PersonalServiceError::new("CN mission reward kind is unsupported")),
    };
    Ok(Some((Reward { kind, id, count: Some(reward.amount), rarity: None },
        ReceiveHistoryEntry::reward(history_kind, id, reward.amount))))
}

// //// 将任务奖励对应的当前资产合并到客户端响应 [@x380kkm 2026-09-21] ////
pub(crate) fn apply_mission_reward_response(
    response: &mut Value,
    root: &mut Map<String, Value>,
    mission_info: &[Value],
    viewer_id: i64,
    response_time: i64,
) -> Result<(), PersonalServiceError> {
    let catalog = mission_catalog()?;
    let mut rewards = Vec::new();
    for info in mission_info {
        let category = info.get("mission_category_id").and_then(Value::as_i64).unwrap_or_default();
        let mission_id = info.get("mission_id").and_then(Value::as_i64);
        let reward_id = info.get("mission_reward_id").and_then(Value::as_i64);
        if let Some(stage) = catalog.categories.get(&category)
            .and_then(|missions| missions.iter().find(|mission| Some(mission.id) == mission_id))
            .and_then(|mission| mission.stages.iter().find(|stage| Some(stage.reward_id) == reward_id)) {
            rewards.extend(stage.rewards.iter().cloned());
        }
    }
    apply_reward_response(response, root, &rewards, viewer_id, response_time)?;
    super::awake::apply_response(response, root, response_time)
}
// //// /将任务奖励对应的当前资产合并到客户端响应 ////

// //// 按奖励标识返回玩家当前持有量 [@x380kkm 2026-09-21] ////
pub(crate) fn apply_reward_response(
    response: &mut Value,
    root: &mut Map<String, Value>,
    rewards: &[MissionReward],
    viewer_id: i64,
    response_time: i64,
) -> Result<(), PersonalServiceError> {
    crate::cn_reward_overflow::apply_response(response, root)?;
    let response = response.as_object_mut().ok_or_else(|| PersonalServiceError::new("CN mission response is not an object"))?;
    for reward in rewards {
        match reward.kind {
            0 | 3 | 5 => {
                let field = match reward.kind { 0 => "free_vmoney", 3 => "free_mana", _ => "exp_pool" };
                if let Some(value) = root.get("user_info").and_then(|user| user.get(field)) {
                    response_object(response, "user_info")?.insert(field.to_owned(), value.clone());
                }
                if reward.kind == 5 {
                    if let Some(time) = root.get("user_info").and_then(|user| user.get("exp_pooled_time")) {
                        response_object(response, "user_info")?.insert("exp_pooled_time".to_owned(), time.clone());
                    }
                }
            }
            1 => {
                if let Some(item_id) = reward.item_id {
                    let key = item_id.to_string();
                    if let Some(value) = root.get("item_list").and_then(|items| items.get(&key)) {
                        response_object(response, "item_list")?.insert(key, value.clone());
                    }
                }
            }
            2 => {
                if let Some(equipment_id) = reward.equipment_id {
                    if let Some(stored) = root.get("user_equipment_list").and_then(|items| items.get(equipment_id.to_string())) {
                        let mut value = stored.clone();
                        value["equipment_id"] = Value::from(equipment_id);
                        merge_response_entry(response, "equipment_list", "equipment_id", value)?;
                    }
                }
            }
            4 => {
                if let Some(character_id) = reward.character_id {
                    if let Some(stored) = root.get("user_character_list").and_then(|characters| characters.get(character_id.to_string())) {
                        let mut value = create_character_response(viewer_id, character_id, stored, response_time);
                        for field in ["evolution_level", "over_limit_step", "protection", "stack"] {
                            if let Some(stored_value) = stored.get(field) { value[field] = stored_value.clone(); }
                        }
                        merge_response_entry(response, "character_list", "character_id", value)?;
                        if let Some(items) = root.get("item_list").and_then(Value::as_object) {
                            response_object(response, "item_list")?.extend(items.clone());
                        }
                        if let Some(encyclopedia) = root.get("encyclopedia_list").and_then(Value::as_object) {
                            response_object(response, "encyclopedia_info")?.extend(encyclopedia.clone());
                        }
                    }
                }
            }
            6 => {
                if let Some(degree_id) = reward.degree_id {
                    merge_response_entry(response, "degree_list", "degree_id", json!({"viewer_id":viewer_id,"degree_id":degree_id}))?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
// //// /按奖励标识返回玩家当前持有量 ////

fn response_object<'a>(response: &'a mut Map<String, Value>, field: &str) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    response.entry(field.to_owned()).or_insert_with(|| json!({})).as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("CN mission response object is invalid"))
}

// //// 按资产编号合并客户端列表 [@x380kkm 2026-09-21] ////
fn merge_response_entry(
    response: &mut Map<String, Value>,
    field: &str,
    id_field: &str,
    value: Value,
) -> Result<(), PersonalServiceError> {
    let entries = response.entry(field.to_owned()).or_insert_with(|| json!([]))
        .as_array_mut().ok_or_else(|| PersonalServiceError::new("CN mission response list is invalid"))?;
    if let Some(stored) = entries.iter_mut().find(|entry| entry.get(id_field) == value.get(id_field)) {
        *stored = value;
    } else {
        entries.push(value);
    }
    Ok(())
}
// //// /按资产编号合并客户端列表 ////
