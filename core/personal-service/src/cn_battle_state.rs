// audience: internal
// # personal-service-cn-battle-state
//
// 该模块将 CN 单机战斗开始, 继续和结算转换为持久化玩家快照.

mod events;

use self::events::apply_event_progress;
use crate::cn_battle_assets::{rank_degree_and_stamina, BattleFixture, BattleQuest};
use crate::cn_battle_rewards::{
    apply_character_exp, apply_reward_at, apply_score_attack_border_reward, apply_score_rewards,
    RewardResult,
};
use crate::cn_stamina::{battle_stamina_cost, current_stamina};
use crate::database::ActiveSingleQuest;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const EXPERT_SINGLE_EVENT_CATEGORY: i64 = 21;
const SCORE_ATTACK_EVENT_CATEGORY: i64 = 27;
const EVENT_CHALLENGE_POINT_MAP: &str =
    include_str!("../../../assets/event_challenge_point_map.json");
static EVENT_CHALLENGE_POINTS: OnceLock<Result<BTreeMap<String, i64>, String>> = OnceLock::new();

pub(crate) struct FinishBattleInput<'a> {
    pub(crate) elapsed_time_ms: i64,
    pub(crate) score: i64,
    pub(crate) add_mana: i64,
    pub(crate) is_accomplished: bool,
    pub(crate) character_ids: &'a [i64],
    pub(crate) main_character_ids: &'a [Option<i64>],
    pub(crate) unison_character_ids: &'a [Option<i64>],
    pub(crate) equipment_ids: &'a [Option<i64>],
    pub(crate) ability_soul_ids: &'a [Option<i64>],
    pub(crate) is_multi: bool,
    pub(crate) power_flip_count: Option<i64>,
    pub(crate) dash_count: Option<i64>,
    pub(crate) skill_count: Option<i64>,
    pub(crate) max_skill_chain_count: Option<i64>,
    pub(crate) max_combo_count: Option<i64>,
    pub(crate) is_host: Option<bool>,
    pub(crate) is_mvp: Option<bool>,
}

pub(crate) struct BattleMutation {
    pub(crate) snapshot: String,
    pub(crate) response: Value,
}

pub(crate) struct StartBattleMutation {
    pub(crate) snapshot: String,
    pub(crate) stamina: i64,
    pub(crate) stamina_heal_time: i64,
}

pub(crate) enum StartBattleFailure {
    InsufficientEntryItem,
    InsufficientStamina,
}

#[derive(Default)]
struct PreviousProgress {
    best_elapsed_time_ms: Option<i64>,
    clear_rank: Option<i64>,
    high_score: Option<i64>,
}

// //// 扣除单机战斗入场资源并更新队伍槽位 [@x380kkm 2026-08-22] ////
pub(crate) fn prepare_battle_start(
    serialized: &str,
    quest: &BattleQuest,
    party_id: i64,
    response_time: i64,
) -> Result<Result<StartBattleMutation, StartBattleFailure>, PersonalServiceError> {
    let mut player_data = decode_snapshot(serialized)?;
    let root = require_root(&mut player_data)?;
    if !deduct_entry_item(root, quest)? {
        return Ok(Err(StartBattleFailure::InsufficientEntryItem));
    }
    let stamina_cost = battle_stamina_cost(
        quest.category,
        quest.quest_id,
        quest.stamina_cost,
        response_time,
    )?;
    let Some(stamina) = deduct_stamina(root, stamina_cost, response_time)? else {
        return Ok(Err(StartBattleFailure::InsufficientStamina));
    };
    if !quest.has_fixed_party {
        set_user_info_value(root, "party_slot", party_id)?;
    }
    Ok(Ok(StartBattleMutation {
        snapshot: encode_snapshot(&player_data)?,
        stamina,
        stamina_heal_time: response_time,
    }))
}
// //// /扣除单机战斗入场资源并更新队伍槽位 ////

fn deduct_entry_item(
    root: &mut Map<String, Value>,
    quest: &BattleQuest,
) -> Result<bool, PersonalServiceError> {
    let Some(item_id) = quest.entry_item_id.filter(|item_id| *item_id > 0) else {
        return Ok(true);
    };
    let count = quest.entry_item_count.max(0);
    let items = require_object(root, "item_list")?;
    let current = items
        .get(&item_id.to_string())
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if current < count {
        return Ok(false);
    }
    items.insert(item_id.to_string(), Value::from(current - count));
    Ok(true)
}

fn deduct_stamina(
    root: &mut Map<String, Value>,
    stamina_cost: i64,
    response_time: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    let stamina = current_stamina(root, response_time)?;
    let stamina_cost = stamina_cost.max(0);
    if stamina < stamina_cost {
        return Ok(None);
    }
    let remaining = stamina - stamina_cost;
    if stamina_cost > 0 {
        set_user_info_value(root, "stamina", remaining)?;
        set_user_info_value(root, "stamina_heal_time", response_time)?;
        let total_stamina_used = root
            .get("user_info")
            .and_then(Value::as_object)
            .and_then(|user_info| user_info.get("total_stamina_used"))
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .checked_add(stamina_cost)
            .ok_or_else(|| {
                PersonalServiceError::new("CN total stamina use exceeds the supported range")
            })?;
        set_user_info_value(root, "total_stamina_used", total_stamina_used)?;
    }
    Ok(Some(remaining))
}

// //// 扣除单机战斗继续费用 [@x380kkm 2026-07-22] ////
pub(crate) fn continue_battle(
    serialized: &str,
) -> Result<Option<BattleMutation>, PersonalServiceError> {
    const CONTINUE_VMONEY_COST: i64 = 50;

    let mut player_data = decode_snapshot(serialized)?;
    let root = require_root(&mut player_data)?;
    let free_vmoney = get_user_info_value(root, "free_vmoney")?;
    let vmoney = get_user_info_value(root, "vmoney")?;
    let new_free_vmoney = free_vmoney - CONTINUE_VMONEY_COST;
    let new_vmoney = if new_free_vmoney < 0 {
        vmoney - CONTINUE_VMONEY_COST
    } else {
        vmoney
    };
    if new_free_vmoney < 0 && new_vmoney < 0 {
        return Ok(None);
    }
    let stored_free_vmoney = if new_free_vmoney < 0 {
        free_vmoney
    } else {
        new_free_vmoney
    };
    set_user_info_value(root, "free_vmoney", stored_free_vmoney)?;
    set_user_info_value(root, "vmoney", new_vmoney)?;
    Ok(Some(BattleMutation {
        snapshot: encode_snapshot(&player_data)?,
        response: json!({
            "user_info": {
                "free_vmoney": stored_free_vmoney,
                "vmoney": new_vmoney,
            },
            "mail_arrived": false,
        }),
    }))
}
// //// /扣除单机战斗继续费用 ////

// //// 结算单机战斗并持久化奖励 [@x380kkm 2026-07-22] ////
pub(crate) fn finish_battle(
    database: &mut crate::database::ServiceDatabase,
    account_id: i64,
    serialized: &str,
    active_quest: &ActiveSingleQuest,
    quest: &BattleQuest,
    fixture: &BattleFixture,
    input: &FinishBattleInput<'_>,
    drop_multiplier: i64,
    server_time: i64,
) -> Result<BattleMutation, PersonalServiceError> {
    let mut player_data = decode_snapshot(serialized)?;
    let root = require_root(&mut player_data)?;
    let previous_progress =
        read_previous_progress(root, active_quest.category, active_quest.quest_id)?;
    let quest_previously_completed = previous_progress.is_some();
    let previous_progress = previous_progress.unwrap_or_default();
    let clear_rank = calculate_clear_rank(quest, input.elapsed_time_ms);
    let quest_accomplished = if active_quest.category == SCORE_ATTACK_EVENT_CATEGORY {
        quest
            .score_attack_border_rewards
            .iter()
            .min_by_key(|border| border.score)
            .map_or(input.is_accomplished, |border| input.score >= border.score)
    } else {
        input.is_accomplished
    };

    let initial_free_mana = get_user_info_value(root, "free_mana")?;
    let initial_exp_pool = get_user_info_value(root, "exp_pool")?;
    let initial_rank_point = get_user_info_value(root, "rank_point")?;
    let initial_boost_point = get_user_info_value(root, "boost_point")?;
    let initial_boss_boost_point = get_user_info_value(root, "boss_boost_point")?;
    let mut stamina = get_user_info_value(root, "stamina")?;
    let mut stamina_heal_time = get_user_info_value(root, "stamina_heal_time")?;
    let exp_pooled_time = get_user_info_value(root, "exp_pooled_time")?;

    let new_mana = checked_sum(&[initial_free_mana, quest.mana_reward, input.add_mana])?;
    let new_exp_pool = checked_sum(&[initial_exp_pool, quest.pool_exp_reward])?;
    let new_rank_point = checked_sum(&[initial_rank_point, quest.rank_point_reward])?;
    let new_boost_point = initial_boost_point - i64::from(active_quest.use_boost_point);
    let new_boss_boost_point =
        initial_boss_boost_point - i64::from(active_quest.use_boss_boost_point);
    set_user_info_value(root, "free_mana", new_mana)?;
    set_user_info_value(root, "exp_pool", new_exp_pool)?;
    set_user_info_value(root, "rank_point", new_rank_point)?;
    set_user_info_value(root, "boost_point", new_boost_point)?;
    set_user_info_value(root, "boss_boost_point", new_boss_boost_point)?;
    // //// 段位提升时增加新段位体力上限 [@x380kkm 2026-08-23] ////
    let (previous_degree, _, _) = rank_degree_and_stamina(initial_rank_point)?;
    let (new_degree, new_max_stamina, _) = rank_degree_and_stamina(new_rank_point)?;
    if new_degree > previous_degree {
        stamina = stamina.checked_add(new_max_stamina).ok_or_else(|| {
            PersonalServiceError::new("CN rank-up stamina exceeds the supported range")
        })?;
        stamina_heal_time = server_time;
        set_user_info_value(root, "stamina", stamina)?;
        set_user_info_value(root, "stamina_heal_time", stamina_heal_time)?;
    }
    // //// /段位提升时增加新段位体力上限 ////

    let clear_reward = if !quest_previously_completed {
        match &quest.clear_reward {
            Some(reward) => apply_reward_at(root, reward, server_time)?,
            None => RewardResult::default(),
        }
    } else {
        RewardResult::default()
    };
    let s_plus_reward = if clear_rank == 5 && previous_progress.clear_rank != Some(5) {
        match &quest.s_plus_reward {
            Some(reward) => apply_reward_at(root, reward, server_time)?,
            None => RewardResult::default(),
        }
    } else {
        RewardResult::default()
    };

    if quest_accomplished {
        update_quest_progress(
            root,
            active_quest.category,
            active_quest.quest_id,
            input,
            clear_rank,
            &previous_progress,
        )?;
    }
    update_battle_progress(root, fixture, input)?;
    update_action_totals(root, input)?;
    let mut mission_delta = crate::cn_mission::record_battle_action(
        root,
        database,
        account_id,
        active_quest.category,
        input.is_multi,
        quest_accomplished,
        input.max_skill_chain_count,
        input.max_combo_count,
        input.is_host,
        input.is_mvp,
    )?;
    let boost_point_used = (active_quest.use_boost_point && new_boost_point >= 0)
        || (active_quest.use_boss_boost_point && new_boss_boost_point >= 0);
    let mut score_rewards = apply_score_rewards(
        root,
        fixture,
        quest,
        boost_point_used,
        drop_multiplier,
        server_time,
    )?;
    if active_quest.category == SCORE_ATTACK_EVENT_CATEGORY {
        score_rewards
            .rewards
            .merge(apply_score_attack_border_reward(
                root,
                quest,
                input.score,
                drop_multiplier,
                server_time,
            )?);
    }
    let character_exp = apply_character_exp(
        root,
        fixture,
        input.character_ids,
        quest.character_exp_reward,
        server_time,
        quest.has_fixed_party,
    )?;
    if character_exp.maximum_level > 0 {
        let character_level_delta = crate::cn_mission::record_character_level_action(
            root,
            database,
            account_id,
            character_exp.maximum_level,
        )?;
        mission_delta
            .mission_info
            .extend(character_level_delta.mission_info);
        mission_delta
            .active_mission_list
            .extend(character_level_delta.active_mission_list);
    }
    let mut event_data = apply_event_progress(
        root,
        active_quest,
        fixture,
        quest,
        input,
        drop_multiplier,
        server_time,
    )?;
    consume_expert_challenge_point(root, active_quest.category, quest.event_id)?;

    let final_free_mana = get_user_info_value(root, "free_mana")?;
    let final_exp_pool = get_user_info_value(root, "exp_pool")?;
    let final_free_vmoney = get_user_info_value(root, "free_vmoney")?;
    let degree_id = get_user_info_value(root, "degree_id")?;
    let daily_challenge_points = if active_quest.category == EXPERT_SINGLE_EVENT_CATEGORY {
        root.get("user_daily_challenge_point_list")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()))
    } else {
        Value::Array(Vec::new())
    };
    let mut character_list = character_exp.character_list;
    let drop_score_reward_ids = score_rewards.drop_score_reward_ids;
    let drop_rare_reward_ids = score_rewards.drop_rare_reward_ids;
    let mut reward_result = clear_reward;
    reward_result.merge(s_plus_reward);
    reward_result.merge(score_rewards.rewards);
    reward_result.merge(std::mem::take(&mut event_data.rewards));
    character_list.extend(reward_result.character_list);
    if let Some(entry_item_id) = quest.entry_item_id.filter(|item_id| *item_id > 0) {
        let current = root
            .get("item_list")
            .and_then(Value::as_object)
            .and_then(|items| items.get(&entry_item_id.to_string()))
            .and_then(Value::as_i64)
            .unwrap_or_default();
        reward_result
            .items
            .entry(entry_item_id.to_string())
            .or_insert(current);
    }
    let snapshot = encode_snapshot(&player_data)?;
    let response = json!({
        "user_info": {
            "free_mana": final_free_mana,
            "exp_pool": final_exp_pool,
            "exp_pooled_time": exp_pooled_time,
            "free_vmoney": final_free_vmoney,
            "rank_point": new_rank_point,
            "degree_id": degree_id,
            "stamina": stamina,
            "stamina_heal_time": stamina_heal_time,
            "boost_point": new_boost_point,
            "boss_boost_point": new_boss_boost_point,
        },
        "add_exp_list": character_exp.add_exp_list,
        "character_list": character_list,
        "bond_token_status_list": character_exp.bond_token_status_list,
        "rewards": {
            "overflow_pool_exp": 0,
            "converted_pool_exp": 0,
            "reward_pool_exp": quest.pool_exp_reward,
            "reward_mana": quest.mana_reward,
            "field_mana": input.add_mana,
        },
        "old_high_score": previous_progress.high_score.unwrap_or_default(),
        "joined_character_id_list": reward_result.joined_character_ids,
        "before_rank_point": initial_rank_point,
        "clear_rank": clear_rank,
        "drop_score_reward_ids": drop_score_reward_ids,
        "drop_rare_reward_ids": drop_rare_reward_ids,
        "drop_additional_reward_ids": [],
        "drop_periodic_reward_ids": [],
        "equipment_list": reward_result.equipment_list,
        "category_id": active_quest.category,
        "start_time": server_time,
        "is_multi": "single",
        "quest_name": quest.name,
        "item_list": reward_result.items,
        "rush_event": event_data.rush_event,
        "carnival_event": event_data.carnival_event,
        "user_daily_challenge_point_list": daily_challenge_points,
        "presigned_quest_category": [],
        "mission_info": mission_delta.mission_info,
        "active_mission_list": mission_delta.active_mission_list,
        "mail_arrived": false,
    });
    Ok(BattleMutation { snapshot, response })
}
// //// /结算单机战斗并持久化奖励 ////

fn consume_expert_challenge_point(
    root: &mut Map<String, Value>,
    category: i64,
    event_id: Option<i64>,
) -> Result<(), PersonalServiceError> {
    if category != EXPERT_SINGLE_EVENT_CATEGORY {
        return Ok(());
    }
    let Some(event_id) = event_id else {
        return Ok(());
    };
    let challenge_points = EVENT_CHALLENGE_POINTS
        .get_or_init(|| {
            serde_json::from_str(EVENT_CHALLENGE_POINT_MAP)
                .map_err(|error| format!("invalid CN challenge point map: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let Some(challenge_point_id) = challenge_points.get(&format!("expert_{event_id}")) else {
        return Ok(());
    };
    let entries = root
        .get_mut("user_daily_challenge_point_list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("stored CN daily challenge points are missing"))?;
    let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.get("id").and_then(Value::as_i64) == Some(*challenge_point_id))
    else {
        return Ok(());
    };
    let entry = entry
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN daily challenge point is invalid"))?;
    let point = entry
        .get("point")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    entry.insert(
        "point".to_owned(),
        Value::from(point.saturating_sub(1).max(0)),
    );
    Ok(())
}

fn update_battle_progress(
    root: &mut Map<String, Value>,
    fixture: &BattleFixture,
    input: &FinishBattleInput<'_>,
) -> Result<(), PersonalServiceError> {
    let leader_id = input
        .main_character_ids
        .first()
        .and_then(|character_id| *character_id)
        .filter(|character_id| *character_id > 0);
    if let Some(leader_id) = leader_id {
        increment_character_clear_count(root, "character_leader_clear_counts", leader_id)?;
        if input.is_multi {
            increment_character_clear_count(
                root,
                "character_leader_multi_clear_counts",
                leader_id,
            )?;
        }
        if input.power_flip_count.is_some_and(|count| count > 0) {
            increment_character_clear_count_by(
                root,
                "character_leader_power_flip_counts",
                leader_id,
                input.power_flip_count.unwrap_or_default(),
            )?;
        }
    }
    for character_id in input.character_ids {
        increment_character_clear_count(root, "character_clear_counts", *character_id)?;
        if input.is_multi {
            increment_character_clear_count(root, "character_multi_clear_counts", *character_id)?;
        }
    }
    for (index, character_id) in input.character_ids.iter().enumerate() {
        for other_character_id in &input.character_ids[index + 1..] {
            let key = ordered_pair_key(*character_id, *other_character_id);
            increment_named_count(root, "party_member_co_clear_counts", &key, 1)?;
        }
    }
    let races = input
        .character_ids
        .iter()
        .filter_map(|character_id| fixture.characters.get(&character_id.to_string()))
        .flat_map(|character| character.races.iter().cloned())
        .filter(|race| !race.is_empty())
        .collect::<BTreeSet<_>>();
    if !races.is_empty() {
        increment_named_count(
            root,
            "party_race_clear_counts",
            &races.into_iter().collect::<Vec<_>>().join("+"),
            1,
        )?;
    }
    Ok(())
}

fn increment_character_clear_count(
    root: &mut Map<String, Value>,
    field: &str,
    character_id: i64,
) -> Result<(), PersonalServiceError> {
    increment_character_clear_count_by(root, field, character_id, 1)
}

fn increment_character_clear_count_by(
    root: &mut Map<String, Value>,
    field: &str,
    character_id: i64,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    increment_named_count(root, field, &character_id.to_string(), amount)
}

fn increment_named_count(
    root: &mut Map<String, Value>,
    field: &str,
    key: &str,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let counts = root
        .entry(field.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN character clear counts are invalid"))?;
    let count = counts
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or_default()
        .checked_add(amount)
        .ok_or_else(|| PersonalServiceError::new("CN character clear count exceeds range"))?;
    counts.insert(key.to_owned(), Value::from(count));
    Ok(())
}

fn ordered_pair_key(left: i64, right: i64) -> String {
    if left < right {
        format!("{left}_{right}")
    } else {
        format!("{right}_{left}")
    }
}

fn update_action_totals(
    root: &mut Map<String, Value>,
    input: &FinishBattleInput<'_>,
) -> Result<(), PersonalServiceError> {
    for (key, amount) in [
        ("total_powerflips", input.power_flip_count),
        ("total_dashes", input.dash_count),
        ("total_skills", input.skill_count),
    ] {
        let Some(amount) = amount else {
            continue;
        };
        if amount <= 0 {
            continue;
        }
        let current = root
            .get("user_info")
            .and_then(Value::as_object)
            .and_then(|user_info| user_info.get(key))
            .and_then(Value::as_i64)
            .unwrap_or_default();
        set_user_info_value(
            root,
            key,
            current.checked_add(amount).ok_or_else(|| {
                PersonalServiceError::new("CN battle action count exceeds the supported range")
            })?,
        )?;
    }
    Ok(())
}

// //// 读取和更新玩家任务进度 [@x380kkm 2026-07-22] ////
fn read_previous_progress(
    root: &Map<String, Value>,
    category: i64,
    quest_id: i64,
) -> Result<Option<PreviousProgress>, PersonalServiceError> {
    let quest_progress = root
        .get("quest_progress")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress is missing"))?;
    let Some(progress_list) = quest_progress
        .get(&category.to_string())
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    let Some(progress) = progress_list
        .iter()
        .find(|progress| progress.get("quest_id").and_then(Value::as_i64) == Some(quest_id))
    else {
        return Ok(None);
    };
    Ok(Some(PreviousProgress {
        best_elapsed_time_ms: progress.get("best_elapsed_time_ms").and_then(Value::as_i64),
        clear_rank: progress.get("clear_rank").and_then(Value::as_i64),
        high_score: progress.get("high_score").and_then(Value::as_i64),
    }))
}

fn update_quest_progress(
    root: &mut Map<String, Value>,
    category: i64,
    quest_id: i64,
    input: &FinishBattleInput<'_>,
    clear_rank: i64,
    previous: &PreviousProgress,
) -> Result<(), PersonalServiceError> {
    let quest_progress = require_object(root, "quest_progress")?;
    let progress_list = quest_progress
        .entry(category.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress list is invalid"))?;
    let leader_character_id = input
        .main_character_ids
        .first()
        .and_then(|character_id| *character_id)
        .filter(|character_id| *character_id > 0);
    if let Some(progress) = progress_list
        .iter_mut()
        .find(|progress| progress.get("quest_id").and_then(Value::as_i64) == Some(quest_id))
    {
        let progress = progress.as_object_mut().ok_or_else(|| {
            PersonalServiceError::new("stored CN quest progress entry is invalid")
        })?;
        progress.insert("finished".to_owned(), Value::Bool(true));
        progress.insert(
            "best_elapsed_time_ms".to_owned(),
            Value::from(
                previous
                    .best_elapsed_time_ms
                    .map_or(input.elapsed_time_ms, |time| {
                        time.min(input.elapsed_time_ms)
                    }),
            ),
        );
        progress.insert(
            "clear_rank".to_owned(),
            Value::from(
                previous
                    .clear_rank
                    .map_or(clear_rank, |rank| rank.max(clear_rank)),
            ),
        );
        progress.insert(
            "high_score".to_owned(),
            Value::from(
                previous
                    .high_score
                    .map_or(input.score, |score| score.max(input.score)),
            ),
        );
        progress.insert(
            "leader_character_id".to_owned(),
            leader_character_id.map_or(Value::Null, Value::from),
        );
    } else {
        progress_list.push(json!({
            "quest_id": quest_id,
            "finished": true,
            "best_elapsed_time_ms": input.elapsed_time_ms,
            "clear_rank": clear_rank,
            "high_score": input.score,
            "leader_character_id": leader_character_id,
        }));
    }
    Ok(())
}
// //// /读取和更新玩家任务进度 ////

// //// 校验并编码单机战斗快照 [@x380kkm 2026-07-22] ////
fn calculate_clear_rank(quest: &BattleQuest, elapsed_time_ms: i64) -> i64 {
    if quest.b_rank_time <= 0 {
        5
    } else if quest.s_plus_rank_time >= elapsed_time_ms {
        5
    } else if quest.s_rank_time >= elapsed_time_ms {
        4
    } else if quest.a_rank_time >= elapsed_time_ms {
        3
    } else if quest.b_rank_time >= elapsed_time_ms {
        2
    } else {
        1
    }
}

fn decode_snapshot(serialized: &str) -> Result<Value, PersonalServiceError> {
    serde_json::from_str(serialized).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode CN battle snapshot: {error}"))
    })
}

fn encode_snapshot(player_data: &Value) -> Result<String, PersonalServiceError> {
    serde_json::to_string(player_data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode CN battle snapshot: {error}"))
    })
}

fn require_root(player_data: &mut Value) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN battle snapshot is not an object"))
}

fn require_object<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} data is missing")))
}

fn get_user_info_value(root: &Map<String, Value>, key: &str) -> Result<i64, PersonalServiceError> {
    root.get("user_info")
        .and_then(Value::as_object)
        .and_then(|user_info| user_info.get(key))
        .and_then(Value::as_i64)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} value is missing")))
}

fn set_user_info_value(
    root: &mut Map<String, Value>,
    key: &str,
    value: i64,
) -> Result<(), PersonalServiceError> {
    require_object(root, "user_info")?.insert(key.to_owned(), Value::from(value));
    Ok(())
}

fn checked_sum(values: &[i64]) -> Result<i64, PersonalServiceError> {
    values.iter().try_fold(0_i64, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| PersonalServiceError::new("CN battle value exceeds the supported range"))
    })
}
// //// /校验并编码单机战斗快照 ////

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证单机战斗开始同时扣除入场物品和体力 [@x380kkm 2026-08-22] ////
    #[test]
    fn prepares_entry_costs_atomically() {
        let quest = BattleQuest {
            category: 1,
            quest_id: 1,
            name: String::new(),
            clear_reward_id: None,
            clear_reward: None,
            s_plus_reward_id: None,
            s_plus_reward: None,
            score_reward_group_id: None,
            score_attack_reward_group_id: None,
            score_rewards: Vec::new(),
            score_attack_border_rewards: Vec::new(),
            b_rank_time: 0,
            a_rank_time: 0,
            s_rank_time: 0,
            s_plus_rank_time: 0,
            rank_point_reward: 0,
            character_exp_reward: 0,
            mana_reward: 0,
            pool_exp_reward: 0,
            element: None,
            event_id: None,
            folder_id: None,
            fixed_party_id: None,
            has_fixed_party: false,
            linked_quest_id: None,
            rush_event_id: None,
            rush_event_folder_id: None,
            rush_event_round: None,
            raid_event_id: None,
            carnival_event_id: None,
            carnival_folder_id: None,
            carnival_difficulty_score: None,
            carnival_time_limit_ms: None,
            entry_item_id: Some(40_000),
            entry_item_count: 2,
            stamina_cost: 5,
        };
        let snapshot = json!({
            "user_info": {
                "stamina": 10,
                "stamina_heal_time": 1,
                "rank_point": 0,
                "party_slot": 1,
            },
            "item_list": {"40000": 3},
        })
        .to_string();
        let result =
            prepare_battle_start(&snapshot, &quest, 2, 100).expect("battle start is evaluated");
        let Ok(prepared) = result else {
            panic!("battle entry resources are sufficient");
        };
        let stored: Value =
            serde_json::from_str(&prepared.snapshot).expect("battle snapshot is decoded");
        assert_eq!(stored["item_list"]["40000"], 1);
        assert_eq!(stored["user_info"]["stamina"], 5);
        assert_eq!(stored["user_info"]["stamina_heal_time"], 100);
        assert_eq!(stored["user_info"]["party_slot"], 2);
        assert_eq!(prepared.stamina, 5);
        assert_eq!(prepared.stamina_heal_time, 100);
    }
    // //// /验证单机战斗开始同时扣除入场物品和体力 ////

    // //// 验证专家活动结算扣除每日挑战点 [@x380kkm 2026-08-23] ////
    #[test]
    fn consumes_expert_daily_challenge_point() {
        let mut root = Map::from_iter([(
            "user_daily_challenge_point_list".to_owned(),
            json!([
                {"id": 1, "point": 2, "campaign_list": []},
                {"id": 251, "point": 3, "campaign_list": []},
            ]),
        )]);
        consume_expert_challenge_point(&mut root, 21, Some(1))
            .expect("expert challenge point is consumed");
        assert_eq!(root["user_daily_challenge_point_list"][0]["point"], 1);
        assert_eq!(root["user_daily_challenge_point_list"][1]["point"], 3);
    }
    // //// /验证专家活动结算扣除每日挑战点 ////
}
