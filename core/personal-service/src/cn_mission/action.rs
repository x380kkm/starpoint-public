// audience: internal
// # personal-service-cn-mission-action
//
// 该模块将角色与商店动作投影到普通任务和主动任务状态. 普通任务阶段来自 mission master, 主动任务阶段来自 active mission reward master.

use super::{
    build_context, compute_progress, current_stage, is_scoped_battle_mission, mission_catalog,
    scoped_battle_counter_key, MissionKey,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const ACTIVE_MISSION_REWARDS: &str = include_str!("../../assets/cn-active-mission-rewards.json");
const MANA_NODE_ACTIVE_MISSION_ID: i64 = 11_070;
const MANA_NODE_ACTIVE_MISSION_COMPLETION_ID: i64 = 11_110;
const CHARACTER_LEVEL_ACTIVE_MISSION_ID: i64 = 12_030;
const CHARACTER_LEVEL_ACTIVE_MISSION_COMPLETION_ID: i64 = 12_110;
const CHARACTER_OVER_LIMIT_ACTIVE_MISSION_ID: i64 = 13_040;
const BOND_TOKEN_ACTIVE_MISSION_ID: i64 = 14_070;
const EQUIPMENT_MAX_LEVEL_ACTIVE_MISSION_ID: i64 = 14_050;
const SHOP_PURCHASE_ACTIVE_MISSION_ID: i64 = 12_010;
const EQUIPMENT_UPGRADE_ACTIVE_MISSION_ID: i64 = 11_030;
const GENERAL_ACTIVE_MISSION_COMPLETION_ID: i64 = 14_110;
static ACTIVE_MISSION_DOCUMENT: OnceLock<Result<Value, String>> = OnceLock::new();

// //// 表示一次任务动作返回的普通任务和主动任务增量 [@x380kkm 2026-08-25] ////
pub(crate) struct MissionActionDelta {
    pub(crate) mission_info: Vec<Value>,
    pub(crate) active_mission_list: Vec<Value>,
}
// //// /表示一次任务动作返回的普通任务和主动任务增量 ////

// //// 保存战斗结算产生的任务增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_battle_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    quest_category: i64,
    is_multi: bool,
    is_accomplished: bool,
    max_skill_chain_count: Option<i64>,
    max_combo_count: Option<i64>,
    is_host: Option<bool>,
    is_mvp: Option<bool>,
) -> Result<MissionActionDelta, PersonalServiceError> {
    if !is_accomplished {
        return Ok(empty_delta());
    }

    let mut mission_info = Vec::new();
    let clear_pattern = if is_multi {
        "multi_battle_clear_count"
    } else {
        "single_battle_clear_count"
    };
    let scoped_counter =
        scoped_battle_counter_update(root, database, account_id, clear_pattern, quest_category)?;
    let mut extra_counters = BTreeMap::new();
    if let Some((key, value)) = scoped_counter {
        extra_counters.insert(key, value);
    }
    let global_progress = database
        .mission_counters(account_id)?
        .get(clear_pattern)
        .copied()
        .unwrap_or_default()
        .checked_add(1)
        .ok_or_else(|| PersonalServiceError::new("CN mission progress exceeds supported range"))?;
    mission_info.extend(record_pattern_progress_with_extra_counters(
        root,
        database,
        account_id,
        clear_pattern,
        global_progress,
        &extra_counters,
        Some(quest_category),
    )?);

    if let Some(max_skill_chain_count) = max_skill_chain_count.filter(|count| *count > 0) {
        mission_info.extend(record_pattern_progress(
            root,
            database,
            account_id,
            "max_skill_chain_achievement",
            max_skill_chain_count,
        )?);
    }
    if let Some(max_combo_count) = max_combo_count.filter(|count| *count > 0) {
        mission_info.extend(record_pattern_progress(
            root,
            database,
            account_id,
            "max_combo_achievement",
            max_combo_count,
        )?);
    }
    if is_multi && is_host == Some(true) {
        mission_info.extend(increment_pattern_progress(
            root,
            database,
            account_id,
            "multi_battle_host_count",
            1,
        )?);
    }
    if is_multi && is_host == Some(false) {
        mission_info.extend(increment_pattern_progress(
            root,
            database,
            account_id,
            "multi_battle_guest_count",
            1,
        )?);
    }
    if is_multi && is_mvp == Some(true) {
        mission_info.extend(increment_pattern_progress(
            root,
            database,
            account_id,
            "multi_battle_mvp_count",
            1,
        )?);
    }

    mission_info.sort_by_key(|mission| {
        (
            mission
                .get("mission_category_id")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            mission
                .get("mission_id")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        )
    });
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list: Vec::new(),
    })
}
// //// /保存战斗结算产生的任务增量 ////

// //// 计算并保存关卡类别内的战斗次数 [@x380kkm 2026-08-29] ////
fn scoped_battle_counter_update(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    pattern: &str,
    quest_category: i64,
) -> Result<Option<(String, i64)>, PersonalServiceError> {
    if quest_category <= 0 {
        return Ok(None);
    }
    let catalog = mission_catalog()?;
    let has_matching_mission = catalog
        .pattern_index
        .get(pattern)
        .into_iter()
        .flatten()
        .filter_map(|key| {
            catalog
                .categories
                .get(&key.category)
                .and_then(|missions| missions.iter().find(|mission| mission.id == key.mission_id))
        })
        .any(|mission| {
            is_scoped_battle_mission(mission) && mission.quest_categories.contains(&quest_category)
        });
    if !has_matching_mission {
        return Ok(None);
    }
    let counter_key = scoped_battle_counter_key(pattern, quest_category);
    let counters = database.mission_counters(account_id)?;
    let progress = match counters.get(&counter_key).copied() {
        Some(progress) => progress.checked_add(1).ok_or_else(|| {
            PersonalServiceError::new("CN scoped battle progress exceeds supported range")
        })?,
        None => battle_progress_for_category(root, pattern, quest_category)?.max(1),
    };
    Ok(Some((counter_key, progress)))
}

fn battle_progress_for_category(
    root: &Map<String, Value>,
    pattern: &str,
    quest_category: i64,
) -> Result<i64, PersonalServiceError> {
    let Some(progress) = root
        .get("quest_progress")
        .and_then(Value::as_object)
        .and_then(|categories| categories.get(&quest_category.to_string()))
    else {
        return Ok(0);
    };
    let progress = progress
        .as_array()
        .ok_or_else(|| PersonalServiceError::new("stored CN quest progress list is invalid"))?;
    let count = if pattern == "multi_battle_clear_count" {
        progress.iter().fold(0_i64, |total, entry| {
            if entry.get("finished").and_then(Value::as_bool) != Some(true) {
                return total;
            }
            total.saturating_add(
                entry
                    .get("multi_clear_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(1)
                    .max(0),
            )
        })
    } else {
        i64::try_from(
            progress
                .iter()
                .filter(|entry| entry.get("finished").and_then(Value::as_bool) == Some(true))
                .count(),
        )
        .unwrap_or(i64::MAX)
    };
    Ok(count)
}
// //// /计算并保存关卡类别内的战斗次数 ////

fn empty_delta() -> MissionActionDelta {
    MissionActionDelta {
        mission_info: Vec::new(),
        active_mission_list: Vec::new(),
    }
}

// //// 保存装备升级产生的普通任务和主动任务增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_equipment_upgrade_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    upgrade_count: i64,
    max_level_equipment_count: i64,
) -> Result<MissionActionDelta, PersonalServiceError> {
    if upgrade_count <= 0 && max_level_equipment_count <= 0 {
        return Ok(empty_delta());
    }
    let mut mission_info = Vec::new();
    if upgrade_count > 0 {
        mission_info.extend(increment_pattern_progress(
            root,
            database,
            account_id,
            "upgrade_equipment_count",
            upgrade_count,
        )?);
    }
    if max_level_equipment_count > 0 {
        mission_info.extend(record_pattern_progress(
            root,
            database,
            account_id,
            "level_max_equipment_count",
            max_level_equipment_count,
        )?);
    }
    let active_mission_list = record_active_mission_actions(
        root,
        &[
            (
                EQUIPMENT_MAX_LEVEL_ACTIVE_MISSION_ID,
                max_level_equipment_count,
            ),
            (EQUIPMENT_UPGRADE_ACTIVE_MISSION_ID, upgrade_count),
        ],
        Some(GENERAL_ACTIVE_MISSION_COMPLETION_ID),
    )?;
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list,
    })
}
// //// /保存装备升级产生的普通任务和主动任务增量 ////

// //// 保存角色突破产生的普通任务和主动任务增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_character_over_limit_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    over_limit_count: i64,
) -> Result<MissionActionDelta, PersonalServiceError> {
    if over_limit_count <= 0 {
        return Ok(empty_delta());
    }
    let mission_info = increment_pattern_progress(
        root,
        database,
        account_id,
        "over_limit_total_count",
        over_limit_count,
    )?;
    let active_mission_list = record_active_mission_actions(
        root,
        &[(CHARACTER_OVER_LIMIT_ACTIVE_MISSION_ID, over_limit_count)],
        None,
    )?;
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list,
    })
}
// //// /保存角色突破产生的普通任务和主动任务增量 ////

// //// 保存角色等级变化产生的普通任务和主动任务增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_character_level_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    maximum_level: i64,
) -> Result<MissionActionDelta, PersonalServiceError> {
    let maximum_level = maximum_level.max(0);
    let mission_info = record_pattern_progress(
        root,
        database,
        account_id,
        "character_level_achievement",
        maximum_level,
    )?;
    let active_mission_list = record_active_mission_absolute(
        root,
        CHARACTER_LEVEL_ACTIVE_MISSION_ID,
        CHARACTER_LEVEL_ACTIVE_MISSION_COMPLETION_ID,
        maximum_level,
    )?;
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list,
    })
}
// //// /保存角色等级变化产生的普通任务和主动任务增量 ////

// //// 保存剧情结算产生的普通任务和主动任务增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_story_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    episode_clear_count: i64,
) -> Result<MissionActionDelta, PersonalServiceError> {
    if episode_clear_count <= 0 {
        return Ok(empty_delta());
    }
    let mission_info = increment_pattern_progress(
        root,
        database,
        account_id,
        "episode_clear_count",
        episode_clear_count,
    )?;
    let mut active_mission_list = Vec::new();
    if active_mission_progress(root, 11_010) < 1 {
        active_mission_list.extend(record_active_mission_action(root, 11_010, 11_010, 1)?);
    }
    active_mission_list.extend(record_active_mission_action(root, 11_110, 11_110, 1)?);
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list,
    })
}
// //// /保存剧情结算产生的普通任务和主动任务增量 ////

// //// 保存领取信赖之证产生的普通任务和主动任务增量 [@x380kkm 2026-08-25] ////
pub(crate) fn record_bond_token_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    claimed: bool,
) -> Result<MissionActionDelta, PersonalServiceError> {
    if !claimed {
        return Ok(MissionActionDelta {
            mission_info: Vec::new(),
            active_mission_list: Vec::new(),
        });
    }
    let mission_info = increment_pattern_progress(
        root,
        database,
        account_id,
        "total_obtained_bond_token_count",
        1,
    )?;
    let active_mission_list = record_active_mission_action(
        root,
        BOND_TOKEN_ACTIVE_MISSION_ID,
        GENERAL_ACTIVE_MISSION_COMPLETION_ID,
        1,
    )?;
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list,
    })
}
// //// /保存领取信赖之证产生的普通任务和主动任务增量 ////

// //// 保存商店购买产生的主动任务增量 [@x380kkm 2026-08-25] ////
pub(crate) fn record_shop_purchase_active_missions(
    root: &mut Map<String, Value>,
    purchased_count: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    if purchased_count <= 0 {
        return Ok(Vec::new());
    }
    record_active_mission_action(
        root,
        SHOP_PURCHASE_ACTIVE_MISSION_ID,
        GENERAL_ACTIVE_MISSION_COMPLETION_ID,
        purchased_count,
    )
}
// //// /保存商店购买产生的主动任务增量 ////

// //// 保存 Mana node 学习产生的普通任务和主动任务增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_mana_node_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    unlocked_node_count: i64,
    learned_node_count: i64,
) -> Result<MissionActionDelta, PersonalServiceError> {
    if learned_node_count <= 0 {
        return Ok(MissionActionDelta {
            mission_info: Vec::new(),
            active_mission_list: Vec::new(),
        });
    }
    let mission_info = record_pattern_progress(
        root,
        database,
        account_id,
        "total_released_mana_node_count",
        unlocked_node_count,
    )?;
    let active_mission_list = record_active_mission_action(
        root,
        MANA_NODE_ACTIVE_MISSION_ID,
        MANA_NODE_ACTIVE_MISSION_COMPLETION_ID,
        learned_node_count,
    )?;
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list,
    })
}
// //// /保存 Mana node 学习产生的普通任务和主动任务增量 ////

// //// 保存第二张 Mana board 开启产生的普通任务增量 [@x380kkm 2026-08-28] ////
pub(crate) fn record_mana_board_open_action(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    increment_pattern_progress(root, database, account_id, "mana_board_2nd_open_count", 1)
}
// //// /保存第二张 Mana board 开启产生的普通任务增量 ////

// //// 按普通任务 master 保存动作进度和已完成阶段 [@x380kkm 2026-08-25] ////
fn record_pattern_progress(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    pattern: &str,
    progress: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    record_pattern_progress_with_extra_counters(
        root,
        database,
        account_id,
        pattern,
        progress,
        &BTreeMap::new(),
        None,
    )
}

fn record_pattern_progress_with_extra_counters(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    pattern: &str,
    progress: i64,
    extra_counters: &BTreeMap<String, i64>,
    scope_category: Option<i64>,
) -> Result<Vec<Value>, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let stored_progress = database.mission_progress(account_id)?;
    let mut counters = database.mission_counters(account_id)?;
    counters.extend(extra_counters.clone());
    let progress = progress
        .max(counters.get(pattern).copied().unwrap_or_default())
        .max(0);
    counters.insert(pattern.to_owned(), progress);
    let context = build_context(root, catalog, &counters)?;
    let mission_keys = catalog
        .pattern_index
        .get(pattern)
        .cloned()
        .unwrap_or_default();
    let mut mission_updates = BTreeMap::new();
    let mut stage_updates = Vec::new();

    for key in mission_keys {
        let Some(mission) = catalog
            .categories
            .get(&key.category)
            .and_then(|missions| missions.iter().find(|mission| mission.id == key.mission_id))
        else {
            continue;
        };
        let scoped = is_scoped_battle_mission(mission);
        let include = match scope_category {
            Some(quest_category) => !scoped || mission.quest_categories.contains(&quest_category),
            None => !scoped,
        };
        if !include {
            continue;
        }
        let database_progress = stored_progress
            .get(&(key.category, key.mission_id))
            .copied()
            .unwrap_or_default();
        let computed_progress = compute_progress(
            key.category,
            mission,
            &context,
            &counters,
            database_progress,
            catalog,
        );
        mission_updates.insert((key.category, key.mission_id), computed_progress);
        let completed_stages = if key.category == 1 {
            let current_stage_number = current_stage(mission, computed_progress);
            let previous_stage = cleared_regular_stage(root, key);
            mission
                .stages
                .iter()
                .take_while(|stage| stage.stage <= current_stage_number)
                .filter(|stage| {
                    stage.stage > previous_stage
                        && stage
                            .target
                            .is_some_and(|target| computed_progress >= target)
                })
                .map(|stage| (stage.stage, stage.reward_id))
                .collect::<Vec<_>>()
        } else {
            mission
                .stages
                .iter()
                .filter(|stage| {
                    stage.target.is_some_and(|target| {
                        database_progress < target && computed_progress >= target
                    })
                })
                .map(|stage| (stage.stage, stage.reward_id))
                .collect::<Vec<_>>()
        };
        if !completed_stages.is_empty() {
            stage_updates.push((key, completed_stages));
        }
    }

    let mut mission_info = Vec::new();
    for (key, stages) in stage_updates {
        let latest_stage = stages.last().map(|(stage, _)| *stage).unwrap_or_default();
        if key.category == 1 {
            set_cleared_regular_stage(root, key, latest_stage)?;
        }
        mission_info.extend(stages.into_iter().map(|(_, reward_id)| {
            json!({
                "mission_category_id": key.category,
                "mission_id": key.mission_id,
                "mission_reward_id": reward_id,
            })
        }));
    }

    let mut counter_update = BTreeMap::from([(pattern.to_owned(), progress)]);
    counter_update.extend(extra_counters.clone());
    database.set_mission_progress(account_id, &counter_update, &mission_updates)?;
    Ok(mission_info)
}

fn increment_pattern_progress(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    pattern: &str,
    progress_delta: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    if progress_delta <= 0 {
        return Ok(Vec::new());
    }
    let current_progress = database
        .mission_counters(account_id)?
        .get(pattern)
        .copied()
        .unwrap_or_default();
    let progress = current_progress
        .checked_add(progress_delta)
        .ok_or_else(|| PersonalServiceError::new("CN mission progress exceeds supported range"))?;
    record_pattern_progress(root, database, account_id, pattern, progress)
}

fn cleared_regular_stage(root: &Map<String, Value>, key: MissionKey) -> i64 {
    root.get("cleared_regular_mission_list")
        .and_then(Value::as_object)
        .and_then(|missions| missions.get(&key.mission_id.to_string()))
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

fn set_cleared_regular_stage(
    root: &mut Map<String, Value>,
    key: MissionKey,
    stage: i64,
) -> Result<(), PersonalServiceError> {
    let missions = object_or_insert(root, "cleared_regular_mission_list")?;
    missions.insert(key.mission_id.to_string(), Value::from(stage));
    Ok(())
}
// //// /按普通任务 master 保存动作进度和已完成阶段 ////

// //// 按主动任务 master 保存进度和阶段状态 [@x380kkm 2026-08-25] ////
fn record_active_mission_action(
    root: &mut Map<String, Value>,
    mission_id: i64,
    completion_mission_id: i64,
    progress_delta: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let primary = advance_active_mission(root, mission_id, progress_delta)?;
    let completed_stage_count = i64::try_from(primary.completed_stage_count).map_err(|_| {
        PersonalServiceError::new("CN active mission stage count exceeds supported range")
    })?;
    let mut responses = vec![primary.response];
    if completed_stage_count > 0 && mission_id != completion_mission_id {
        responses.push(
            advance_active_mission(root, completion_mission_id, completed_stage_count)?.response,
        );
    }
    Ok(responses)
}

fn record_active_mission_actions(
    root: &mut Map<String, Value>,
    actions: &[(i64, i64)],
    completion_mission_id: Option<i64>,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut responses = Vec::new();
    let mut completed_stage_count = 0_i64;
    for (mission_id, progress_delta) in actions {
        if *progress_delta <= 0 {
            continue;
        }
        let advanced = advance_active_mission(root, *mission_id, *progress_delta)?;
        completed_stage_count = completed_stage_count
            .checked_add(i64::try_from(advanced.completed_stage_count).map_err(|_| {
                PersonalServiceError::new("CN active mission stage count exceeds supported range")
            })?)
            .ok_or_else(|| {
                PersonalServiceError::new("CN active mission stage count exceeds supported range")
            })?;
        responses.push(advanced.response);
    }
    if let Some(completion_mission_id) = completion_mission_id.filter(|_| completed_stage_count > 0)
    {
        responses.push(
            advance_active_mission(root, completion_mission_id, completed_stage_count)?.response,
        );
    }
    Ok(responses)
}

fn record_active_mission_absolute(
    root: &mut Map<String, Value>,
    mission_id: i64,
    completion_mission_id: i64,
    progress: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let previous_progress = active_mission_progress(root, mission_id);
    let progress_delta = progress.max(0).saturating_sub(previous_progress.max(0));
    record_active_mission_action(root, mission_id, completion_mission_id, progress_delta)
}

fn active_mission_progress(root: &Map<String, Value>, mission_id: i64) -> i64 {
    root.get("all_active_mission_list")
        .and_then(Value::as_object)
        .and_then(|missions| missions.get(&mission_id.to_string()))
        .and_then(Value::as_object)
        .and_then(|mission| mission.get("progress"))
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

struct ActiveMissionAdvance {
    response: Value,
    completed_stage_count: usize,
}

fn advance_active_mission(
    root: &mut Map<String, Value>,
    mission_id: i64,
    progress_delta: i64,
) -> Result<ActiveMissionAdvance, PersonalServiceError> {
    let targets = active_mission_stage_targets(mission_id)?;
    let missions = object_or_insert(root, "all_active_mission_list")?;
    let mission = missions
        .entry(mission_id.to_string())
        .or_insert_with(|| json!({"progress": 0, "stages": {}}));
    let mission = mission
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN active mission entry is invalid"))?;
    let previous_progress = mission
        .get("progress")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let progress = previous_progress
        .checked_add(progress_delta)
        .ok_or_else(|| {
            PersonalServiceError::new("CN active mission progress exceeds supported range")
        })?;
    let stages = stages_object(mission)?;
    let mut response_stages = Vec::new();
    for (stage, target) in targets {
        let stage_key = stage.to_string();
        if previous_progress < target && progress >= target && !stages.contains_key(&stage_key) {
            stages.insert(stage_key, Value::Bool(false));
            response_stages.push(json!({"stage": stage, "received": false}));
        }
    }
    mission.insert("progress".to_owned(), Value::from(progress));
    Ok(ActiveMissionAdvance {
        completed_stage_count: response_stages.len(),
        response: json!({
            "mission_id": mission_id,
            "progress_value": progress,
            "stages": response_stages,
        }),
    })
}

fn stages_object(
    mission: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, PersonalServiceError> {
    let stages = mission
        .entry("stages".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if stages.as_array().is_some_and(Vec::is_empty) {
        *stages = Value::Object(Map::new());
    }
    stages
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN active mission stages are invalid"))
}

fn object_or_insert<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    let value = root
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if value.is_null() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} data is invalid")))
}

fn active_mission_stage_targets(mission_id: i64) -> Result<Vec<(i64, i64)>, PersonalServiceError> {
    let document = ACTIVE_MISSION_DOCUMENT
        .get_or_init(|| {
            serde_json::from_str::<Value>(ACTIVE_MISSION_REWARDS)
                .map_err(|error| format!("failed to decode CN active mission master: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let mission = document
        .get(mission_id.to_string())
        .and_then(Value::as_object)
        .ok_or_else(|| {
            PersonalServiceError::new(format!(
                "CN active mission {mission_id} is missing from the master"
            ))
        })?;
    let mut targets = mission
        .iter()
        .map(|(stage, rows)| {
            let stage = stage.parse::<i64>().map_err(|error| {
                PersonalServiceError::new(format!(
                    "CN active mission {mission_id} stage is invalid: {error}"
                ))
            })?;
            let target = rows
                .as_array()
                .and_then(|rows| rows.first())
                .and_then(Value::as_array)
                .and_then(|row| row.get(3))
                .and_then(value_i64)
                .ok_or_else(|| {
                    PersonalServiceError::new(format!(
                        "CN active mission {mission_id}:{stage} target is missing"
                    ))
                })?;
            Ok((stage, target))
        })
        .collect::<Result<Vec<_>, PersonalServiceError>>()?;
    targets.sort_by_key(|(stage, _)| *stage);
    Ok(targets)
}

fn value_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}
// //// /按主动任务 master 保存进度和阶段状态 ////
