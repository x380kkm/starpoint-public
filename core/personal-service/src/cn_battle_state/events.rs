// audience: internal
// # personal-service-cn-battle-events
//
// 该模块结算 rush, raid 和 carnival 的玩家状态与响应字段.

use super::FinishBattleInput;
use crate::cn_activity;
use crate::cn_battle_assets::{BattleFixture, BattleQuest, Reward};
use crate::cn_battle_rewards::{apply_reward_at, scale_drop_count, RewardResult};
use crate::database::ActiveSingleQuest;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

pub(super) struct EventFinishData {
    pub(super) rush_event: Value,
    pub(super) carnival_event: Value,
    pub(super) rewards: RewardResult,
}

// //// 结算活动类别状态 [@x380kkm 2026-08-22] ////
pub(super) fn apply_event_progress(
    root: &mut Map<String, Value>,
    active_quest: &ActiveSingleQuest,
    fixture: &BattleFixture,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
    drop_multiplier: i64,
    server_time: i64,
) -> Result<EventFinishData, PersonalServiceError> {
    let (rush_event, rewards) = if active_quest.category == 24 {
        apply_rush_event(root, fixture, quest, input, drop_multiplier, server_time)?
    } else {
        (Value::Null, RewardResult::default())
    };
    if active_quest.category == 23 {
        record_raid_party(root, quest, input)?;
    }
    let carnival_event = if active_quest.category == 22 && input.is_accomplished {
        apply_carnival_event(root, quest, input)?
    } else {
        Value::Null
    };
    Ok(EventFinishData {
        rush_event,
        carnival_event,
        rewards,
    })
}
// //// /结算活动类别状态 ////

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
            let next_round = event
                .get("endless_battle_next_round")
                .and_then(Value::as_i64)
                .unwrap_or(1)
                .max(1);
            let old_max_round = event
                .get("endless_battle_max_round")
                .and_then(Value::as_i64);
            let old_best_time = event
                .get("endless_battle_max_round_time")
                .and_then(Value::as_i64);
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
                            .map(|character_id| character_id.map_or(Value::Null, Value::from))
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
        return Ok((
            json!({
                "rush_battle_reward_list": [],
                "rush_battle_played_party_list": folder_parties,
                "endless_battle_played_party_list": endless_parties,
                "is_out_of_period": false,
                "endless_battle_next_round": next_round + 1,
                "endless_battle_max_round": new_max_round,
                "high_score": input.elapsed_time_ms,
                "best_elapsed_time_ms": new_best_time,
                "old_endless_battle_max_round": old_max_round,
                "old_best_elapsed_time_ms": old_best_time,
            }),
            RewardResult::default(),
        ));
    }

    let final_round = rush_folder_max_round(fixture, event_id, folder_id);
    let is_folder_final = final_round.is_some_and(|round| configured_round >= round);
    let (folder_parties, endless_parties) = {
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
            "endless_battle_next_round": null,
            "endless_battle_max_round": null,
            "high_score": null,
            "best_elapsed_time_ms": null,
            "old_endless_battle_max_round": null,
            "old_best_elapsed_time_ms": null,
        }),
        rewards,
    ))
}

fn record_raid_party(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
) -> Result<(), PersonalServiceError> {
    let Some(event_id) = quest.raid_event_id.or(quest.event_id) else {
        return Ok(());
    };
    let party = played_party(root, input);
    let event = cn_activity::activate_battle_event_state(root, "raid", event_id)?;
    get_or_create_object(event, "folder_played_party_list")?
        .insert(quest.quest_id.to_string(), party);
    Ok(())
}

fn apply_carnival_event(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
    input: &FinishBattleInput<'_>,
) -> Result<Value, PersonalServiceError> {
    let (Some(event_id), Some(folder_id), Some(difficulty), Some(time_limit)) = (
        quest.carnival_event_id,
        quest.carnival_folder_id,
        quest.carnival_difficulty_score,
        quest.carnival_time_limit_ms,
    ) else {
        return Ok(Value::Null);
    };
    let difficulty_bonus = difficulty.checked_mul(100).ok_or_else(|| {
        PersonalServiceError::new("CN carnival difficulty score exceeds the supported range")
    })?;
    let time_bonus = time_limit.saturating_sub(input.elapsed_time_ms).max(0);
    let total_score = difficulty_bonus.checked_add(time_bonus).ok_or_else(|| {
        PersonalServiceError::new("CN carnival score exceeds the supported range")
    })?;
    let event = cn_activity::activate_battle_event_state(root, "carnival", event_id)?;
    let records = event
        .entry("carnival_records".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN carnival records are invalid"))?;
    let existing_index = records
        .iter()
        .position(|record| record.get("folder_id").and_then(Value::as_i64) == Some(folder_id));
    let previous_best_score = existing_index
        .and_then(|index| records.get(index))
        .and_then(|record| record.get("best_score"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let record = json!({
        "folder_id": folder_id,
        "best_score": previous_best_score.max(total_score),
        "previous_score": total_score,
        "previous_character_ids": input.main_character_ids,
        "previous_unison_character_ids": input.unison_character_ids,
    });
    if let Some(index) = existing_index {
        records[index] = record;
    } else {
        records.push(record);
    }
    Ok(json!({
        "is_record_valid": true,
        "leader_character_id": input.main_character_ids.first().copied().flatten().unwrap_or_default(),
        "new_degree_ids": [],
        "previous_total_best_score": 0,
        "reward_ids": [],
        "score": {
            "difficulty_bonus": difficulty_bonus,
            "time_bonus": time_bonus,
        },
    }))
}

// //// 构造活动结算队伍和奖励 [@x380kkm 2026-08-25] ////
fn played_party(root: &Map<String, Value>, input: &FinishBattleInput<'_>) -> Value {
    json!({
        "character_id_1": slot_id(input.main_character_ids, 0),
        "character_id_2": slot_id(input.main_character_ids, 1),
        "character_id_3": slot_id(input.main_character_ids, 2),
        "unison_character_id_1": slot_id(input.unison_character_ids, 0),
        "unison_character_id_2": slot_id(input.unison_character_ids, 1),
        "unison_character_id_3": slot_id(input.unison_character_ids, 2),
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
    root.get("user_character_list")
        .and_then(Value::as_object)
        .and_then(|characters| characters.get(&character_id?.to_string()))
        .and_then(|character| character.get("evolution_level"))
        .and_then(Value::as_i64)
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
