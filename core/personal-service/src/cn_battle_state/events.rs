// audience: internal
// # personal-service-cn-battle-events
//
// 该模块结算 rush, raid, carnival, 极时试炼和无限演武的玩家状态与响应字段.

mod carnival;
mod raid;
mod rewards;
mod score_attack;
mod solo_time_attack;

use super::{FinishBattleInput, PreviousProgress};
use crate::cn_activity;
use crate::cn_battle_assets::{BattleFixture, BattleQuest, Reward};
use crate::cn_battle_rewards::{apply_reward_at, scale_drop_count, RewardResult};
use crate::cn_mission::MissionReward;
use crate::database::ActiveSingleQuest;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;

pub(super) struct EventFinishData {
    pub(super) raid_event: Value,
    pub(super) rush_event: Value,
    pub(super) carnival_event: Value,
    pub(super) solo_time_attack_event: Value,
    pub(super) score_attack_event: Value,
    pub(super) rewards: RewardResult,
    pub(super) granted_rewards: Vec<MissionReward>,
}

// //// 按活动规则准备开战状态 [@x380kkm 2026-09-21] ////
pub(super) fn prepare_event_battle(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    character_ids: &[i64],
) -> Result<(), PersonalServiceError> {
    if quest.category == 22 {
        carnival::prepare_carnival_battle(root, quest, character_ids)?;
    }
    Ok(())
}
// //// /按活动规则准备开战状态 ////

// //// 按活动移除档位奖励领取凭据 [@x380kkm 2026-09-21] ////
pub(crate) fn reset_event_reward_receipts(
    root: &mut Map<String, Value>,
    family: &str,
    event_id: i64,
) -> Result<usize, PersonalServiceError> {
    match family {
        "solo-time-attack" => solo_time_attack::reset_reward_receipts(root, event_id),
        "score-attack" => score_attack::reset_reward_receipts(root, event_id),
        "carnival" => carnival::reset_reward_receipts(root, event_id),
        _ => Ok(0),
    }
}
// //// /按活动移除档位奖励领取凭据 ////

// //// 从领取凭据定位所属活动 [@x380kkm 2026-09-21] ////
pub(crate) fn reward_receipt_activity_id(
    receipt_field: &str,
    reward_id: &str,
) -> Result<Option<String>, PersonalServiceError> {
    if receipt_field == "score_attack_reward_receipts" {
        return Ok(score_attack::receipt_event_id(reward_id).map(|id| format!("score-attack:{id}")));
    }
    let Ok(reward_id) = reward_id.parse::<i64>() else {
        return Ok(None);
    };
    let (family, event_id) = match receipt_field {
        "solo_time_attack_reward_receipts" => (
            "solo-time-attack",
            solo_time_attack::receipt_event_id(reward_id)?,
        ),
        "carnival_reward_receipts" => ("carnival", carnival::receipt_event_id(reward_id)?),
        _ => return Ok(None),
    };
    Ok(event_id.map(|event_id| format!("{family}:{event_id}")))
}
// //// /从领取凭据定位所属活动 ////

// //// 结算活动类别状态 [@x380kkm 2026-08-22] ////
pub(super) fn apply_event_progress(
    root: &mut Map<String, Value>,
    active_quest: &ActiveSingleQuest,
    fixture: &BattleFixture,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    previous_progress: &PreviousProgress,
    drop_multiplier: i64,
    server_time: i64,
    raid_boss: Option<&crate::database::RaidBossState>,
) -> Result<EventFinishData, PersonalServiceError> {
    let (rush_event, rewards) = if active_quest.category == 24 && input.is_accomplished {
        apply_rush_event(root, fixture, quest, input, drop_multiplier, server_time)?
    } else {
        (Value::Null, RewardResult::default())
    };
    let raid_event = if active_quest.category == 23 {
        raid::apply_raid_event(root, quest, input, raid_boss)?
    } else {
        Value::Null
    };
    let (carnival_event, mut granted_rewards) =
        if active_quest.category == 22 && input.is_accomplished {
            carnival::apply_carnival_event(root, quest, input, server_time)?
        } else {
            (Value::Null, Vec::new())
        };
    let (solo_time_attack_event, time_rewards) = if active_quest.category == 25 {
        solo_time_attack::apply_solo_time_attack_event(
            root,
            quest.quest_id,
            input,
            previous_progress,
            server_time,
        )?
    } else {
        (Value::Null, Vec::new())
    };
    granted_rewards.extend(time_rewards);
    let (score_attack_event, score_rewards) = if active_quest.category == 27 {
        let best_score = previous_progress.high_score.unwrap_or_default().max(input.score);
        score_attack::apply_score_attack_event(root, quest, input, best_score, server_time)?
    } else {
        (Value::Null, Vec::new())
    };
    granted_rewards.extend(score_rewards);
    Ok(EventFinishData {
        raid_event,
        rush_event,
        carnival_event,
        solo_time_attack_event,
        score_attack_event,
        rewards,
        granted_rewards,
    })
}
// //// /结算活动类别状态 ////

// //// 按无限挑战最高层数选择战斗成绩归属 [@x380kkm 2026-09-27] ////
pub(super) fn compare_battle_record_round(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
) -> Result<Ordering, PersonalServiceError> {
    if quest.category != 24 || quest.rush_event_round != Some(0) {
        return Ok(Ordering::Equal);
    }
    let Some(event_id) = quest.rush_event_id else {
        return Ok(Ordering::Equal);
    };
    let event = cn_activity::activate_battle_event_state(root, "rush", event_id)?;
    let (next_round, maximum_round) = endless_rounds(event);
    Ok(next_round.cmp(&maximum_round.unwrap_or_default()))
}

fn endless_rounds(event: &Map<String, Value>) -> (i64, Option<i64>) {
    let next = event
        .get("endless_battle_next_round")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .max(1);
    let maximum = event
        .get("endless_battle_max_round")
        .and_then(Value::as_i64)
        .filter(|round| *round > 0);
    (next, maximum)
}
// //// /按无限挑战最高层数选择战斗成绩归属 ////

fn apply_rush_event(
    root: &mut Map<String, Value>,
    fixture: &BattleFixture,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    drop_multiplier: i64,
    server_time: i64,
) -> Result<(Value, RewardResult), PersonalServiceError> {
    let (Some(event_id), Some(folder_id), Some(configured_round)) = (
        quest.rush_event_id,
        quest.rush_event_folder_id,
        quest.rush_event_round,
    ) else {
        return Ok((Value::Null, RewardResult::default()));
    };
    let party = played_party(root, input);
    let main_evolution_levels = evolution_levels(root, input.main_character_ids);

    if configured_round == 0 {
        let (
            next_round,
            old_max_round,
            old_best_time,
            new_max_round,
            new_best_time,
            folder_parties,
            endless_parties,
        ) = {
            let event = cn_activity::activate_battle_event_state(root, "rush", event_id)?;
            let (next_round, old_max_round) = endless_rounds(event);
            let old_best_time = old_max_round.and_then(|_| {
                event
                    .get("endless_battle_max_round_time")
                    .and_then(Value::as_i64)
            });
            let is_record = old_max_round.map_or(true, |maximum| next_round > maximum)
                || (old_max_round == Some(next_round)
                    && old_best_time.map_or(true, |best| input.elapsed_time_ms <= best));
            if is_record {
                event.insert(
                    "endless_battle_max_round".to_owned(),
                    Value::from(next_round),
                );
                event.insert(
                    "endless_battle_max_round_time".to_owned(),
                    Value::from(input.elapsed_time_ms),
                );
                event.insert(
                    "endless_battle_max_round_character_ids".to_owned(),
                    Value::Array(
                        input
                            .main_character_ids
                            .iter()
                            .map(|character_id| character_id
                                .filter(|id| crate::cn_character_presentation::is_valid_character(*id))
                                .map_or(Value::Null, Value::from))
                            .collect(),
                    ),
                );
                event.insert(
                    "endless_battle_max_round_character_evolution_img_lvls".to_owned(),
                    main_evolution_levels,
                );
            }
            event.insert(
                "endless_battle_next_round".to_owned(),
                Value::from(next_round.checked_add(1).ok_or_else(|| {
                    PersonalServiceError::new("CN rush event round exceeds the supported range")
                })?),
            );
            get_or_create_object(event, "endless_played_party_list")?
                .insert(next_round.to_string(), party);
            (
                next_round,
                old_max_round,
                old_best_time,
                event
                    .get("endless_battle_max_round")
                    .and_then(Value::as_i64),
                event
                    .get("endless_battle_max_round_time")
                    .and_then(Value::as_i64),
                event
                    .get("folder_played_party_list")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                event
                    .get("endless_played_party_list")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            )
        };
        let high_score = super::read_previous_progress(root, quest.category, quest.quest_id)?
            .and_then(|progress| progress.high_score);
        return Ok((
            json!({
                "rush_battle_reward_list": [],
                "rush_battle_played_party_list": folder_parties,
                "endless_battle_played_party_list": endless_parties,
                "is_out_of_period": false,
                "endless_battle_next_round": next_round + 1,
                "endless_battle_max_round": new_max_round,
                "high_score": high_score,
                "best_elapsed_time_ms": new_best_time,
                "old_endless_battle_max_round": old_max_round,
                "old_best_elapsed_time_ms": old_best_time,
            }),
            RewardResult::default(),
        ));
    }

    let final_round = rush_folder_max_round(fixture, event_id, folder_id);
    let is_folder_final = final_round.is_some_and(|round| configured_round >= round);
    let (folder_parties, endless_parties, endless_next_round, endless_max_round) = {
        let event = cn_activity::activate_battle_event_state(root, "rush", event_id)?;
        if is_folder_final {
            let cleared = event
                .entry("cleared_folder_id_list".to_owned())
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| {
                    PersonalServiceError::new("stored CN rush cleared folder list is invalid")
                })?;
            if !cleared
                .iter()
                .any(|value| value.as_i64() == Some(folder_id))
            {
                cleared.push(Value::from(folder_id));
            }
            event.insert("active_folder_id".to_owned(), Value::Null);
            event.insert(
                "folder_played_party_list".to_owned(),
                Value::Object(Map::new()),
            );
        } else {
            get_or_create_object(event, "folder_played_party_list")?
                .insert(quest.quest_id.to_string(), party);
        }
        (
            event
                .get("folder_played_party_list")
                .cloned()
                .unwrap_or_else(|| json!({})),
            event
                .get("endless_played_party_list")
                .cloned()
                .unwrap_or_else(|| json!({})),
            endless_rounds(event).0,
            endless_rounds(event).1,
        )
    };
    let (rush_battle_reward_list, rewards) = if is_folder_final {
        apply_rush_folder_rewards(root, event_id, folder_id, drop_multiplier, server_time)?
    } else {
        (Vec::new(), RewardResult::default())
    };
    Ok((
        json!({
            "rush_battle_reward_list": rush_battle_reward_list,
            "rush_battle_played_party_list": folder_parties,
            "endless_battle_played_party_list": endless_parties,
            "is_out_of_period": false,
            "endless_battle_next_round": endless_next_round,
            "endless_battle_max_round": endless_max_round,
            "high_score": null,
            "best_elapsed_time_ms": null,
            "old_endless_battle_max_round": null,
            "old_best_elapsed_time_ms": null,
        }),
        rewards,
    ))
}

// //// 构造活动结算队伍和奖励 [@x380kkm 2026-08-25] ////
fn played_party(root: &Map<String, Value>, input: &FinishBattleInput<'_>) -> Value {
    json!({
        "character_id_1": character_slot_id(input.main_character_ids, 0),
        "character_id_2": character_slot_id(input.main_character_ids, 1),
        "character_id_3": character_slot_id(input.main_character_ids, 2),
        "unison_character_id_1": character_slot_id(input.unison_character_ids, 0),
        "unison_character_id_2": character_slot_id(input.unison_character_ids, 1),
        "unison_character_id_3": character_slot_id(input.unison_character_ids, 2),
        "equipment_id_1": slot_id(input.equipment_ids, 0),
        "equipment_id_2": slot_id(input.equipment_ids, 1),
        "equipment_id_3": slot_id(input.equipment_ids, 2),
        "ability_soul_id_1": slot_id(input.ability_soul_ids, 0),
        "ability_soul_id_2": slot_id(input.ability_soul_ids, 1),
        "ability_soul_id_3": slot_id(input.ability_soul_ids, 2),
        "evolution_img_level_1": character_evolution_level(root, slot_id(input.main_character_ids, 0)),
        "evolution_img_level_2": character_evolution_level(root, slot_id(input.main_character_ids, 1)),
        "evolution_img_level_3": character_evolution_level(root, slot_id(input.main_character_ids, 2)),
        "unison_evolution_img_level_1": character_evolution_level(root, slot_id(input.unison_character_ids, 0)),
        "unison_evolution_img_level_2": character_evolution_level(root, slot_id(input.unison_character_ids, 1)),
        "unison_evolution_img_level_3": character_evolution_level(root, slot_id(input.unison_character_ids, 2)),
    })
}

fn evolution_levels(root: &Map<String, Value>, character_ids: &[Option<i64>]) -> Value {
    Value::Array(
        (0..3)
            .map(|index| {
                character_evolution_level(root, slot_id(character_ids, index))
                    .map_or(Value::Null, Value::from)
            })
            .collect(),
    )
}

fn character_evolution_level(root: &Map<String, Value>, character_id: Option<i64>) -> Option<i64> {
    let character_id = character_id.filter(|id| crate::cn_character_presentation::is_valid_character(*id))?;
    root.get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id.to_string()))
        .and_then(|character| character.get("evolution_level"))
        .and_then(Value::as_i64)
}

// //// 将空槽和失效角色引用表示为客户端空成员 [@x380kkm 2026-09-28] ////
fn character_slot_id(ids: &[Option<i64>], index: usize) -> Option<i64> {
    slot_id(ids, index).filter(|id| crate::cn_character_presentation::is_valid_character(*id))
}

fn slot_id(ids: &[Option<i64>], index: usize) -> Option<i64> {
    ids.get(index).copied().flatten()
}

fn rush_folder_max_round(fixture: &BattleFixture, event_id: i64, folder_id: i64) -> Option<i64> {
    fixture
        .quests
        .values()
        .filter(|quest| {
            quest.rush_event_id == Some(event_id) && quest.rush_event_folder_id == Some(folder_id)
        })
        .filter_map(|quest| quest.rush_event_round)
        .filter(|round| *round > 0)
        .max()
}

fn apply_rush_folder_rewards(
    root: &mut Map<String, Value>,
    event_id: i64,
    folder_id: i64,
    drop_multiplier: i64,
    server_time: i64,
) -> Result<(Vec<Value>, RewardResult), PersonalServiceError> {
    let configured =
        cn_activity::rush_battle_folder_rewards(event_id, folder_id)?.unwrap_or_default();
    let mut response = Vec::with_capacity(configured.len());
    let mut rewards = RewardResult::default();
    for (item_id, count) in configured {
        let scaled_count = scale_drop_count(count, drop_multiplier)?;
        rewards.merge(apply_reward_at(
            root,
            &Reward {
                kind: 0,
                id: Some(item_id),
                count: Some(scaled_count),
                rarity: None,
            },
            server_time,
        )?);
        response.push(json!({
            "kind": 1,
            "kind_id": item_id,
            "number": scaled_count,
        }));
    }
    Ok((response, rewards))
}
// //// /构造活动结算队伍和奖励 ////

fn get_or_create_object<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} data is invalid")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 重置活动凭据时保留其它活动凭据和已获得资产 [@x380kkm 2026-09-21] ////
    #[test]
    fn resets_only_selected_activity_receipts() {
        let catalog: Value =
            serde_json::from_str(include_str!("../../assets/cn-carnival-rewards.json")).unwrap();
        let first = catalog["events"]["2"][0]["id"].as_i64().unwrap();
        let second = catalog["events"]["3"][0]["id"].as_i64().unwrap();
        let mut root = json!({"item_list": {"90010": 200}, "degree_ids": [61000],
            "carnival_reward_receipts": {first.to_string(): {"received_at": 1}, second.to_string(): {"received_at": 1}},
        }).as_object().unwrap().clone();
        assert_eq!(
            reward_receipt_activity_id("carnival_reward_receipts", &first.to_string())
                .unwrap()
                .as_deref(),
            Some("carnival:2")
        );
        assert_eq!(
            reset_event_reward_receipts(&mut root, "carnival", 2).unwrap(),
            1
        );
        assert!(root["carnival_reward_receipts"]
            .get(first.to_string())
            .is_none());
        assert!(root["carnival_reward_receipts"]
            .get(second.to_string())
            .is_some());
        assert_eq!(root["item_list"]["90010"], 200);
        assert_eq!(root["degree_ids"], json!([61000]));
    }
}
