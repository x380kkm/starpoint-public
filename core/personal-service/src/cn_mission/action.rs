// audience: internal
// # personal-service-cn-mission-action
//
// 该模块将角色与商店动作投影到普通任务和主动任务状态. 普通任务阶段来自 mission master, 主动任务阶段来自 active mission reward master.

use super::{
    build_context, compute_progress, is_scoped_battle_mission, mission_catalog,
    mission_scope, settle_mission_stage, mission_reward_event_key, MissionDefinition, MissionKey,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

// //// 表示一次任务动作返回的普通任务和主动任务增量 [@x380kkm 2026-08-25] ////
pub(crate) struct MissionActionDelta {
    pub(crate) mission_info: Vec<Value>,
    pub(crate) active_mission_list: Vec<Value>,
}
// //// /表示一次任务动作返回的普通任务和主动任务增量 ////


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
    let active_mission_list = record_active_action(root, database, &[("upgrade_equipment_count", upgrade_count)])?;
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
    let active_mission_list = record_active_action(root, database, &[("over_limit_total_count", over_limit_count)])?;
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
    let active_mission_list = super::active::refresh_active_missions(root, database)?;
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
    let active_mission_list = record_active_action(root, database, &[("episode_clear_count", episode_clear_count)])?;
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
    let active_mission_list = record_active_action(root, database, &[("total_obtained_bond_token_count", 1)])?;
    Ok(MissionActionDelta {
        mission_info,
        active_mission_list,
    })
}
// //// /保存领取信赖之证产生的普通任务和主动任务增量 ////

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
    let active_mission_list = record_active_action(root, database, &[("total_released_mana_node_count", learned_node_count)])?;
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
pub(super) fn record_pattern_progress(
    root: &mut Map<String, Value>,
    database: &mut ServiceDatabase,
    account_id: i64,
    pattern: &str,
    progress: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let collections = super::plan_collect_item_missions(database)?;
    record_pattern_progress_with_scope(root, database, account_id, pattern, progress, &collections, mission_scope)
}

// //// 按当前战斗资格结算体力消耗任务 [@x380kkm 2026-09-24] ////
pub(super) fn record_battle_stamina_progress(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64, progress: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let now = database.current_server_time_millis()?.div_euclid(1000);
    let window = super::battle_time::battle_mission_window(root, database, account_id, now)?;
    let collections = super::collect::plan_battle_collect_item_missions(root, database, account_id)?;
    record_pattern_progress_with_scope(root, database, account_id, "used_stamina_count", progress, &collections, |key, mission, database| {
        super::window::mission_scope_for_battle(key, mission, database, &window, now)
    })
}

pub(super) fn record_pattern_progress_with_scope(
    root: &mut Map<String, Value>, database: &mut ServiceDatabase, account_id: i64,
    pattern: &str, progress: i64,
    collections: &super::collect::CollectItemMissionPlan,
    scope_for: impl Fn(MissionKey, &MissionDefinition, &ServiceDatabase) -> Result<Option<super::window::MissionScope>, PersonalServiceError>,
) -> Result<Vec<Value>, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let items_before = super::capture_collect_item_counts(root)?;
    let stored_progress = database.mission_progress(account_id)?;
    let mut counters = database.mission_counters(account_id)?;
    let action_progress = progress;
    let previous_counter = counters.get(pattern).copied().unwrap_or_default();
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
    let mut receipts = Vec::new();
    let response_time = database.current_server_time_millis()?.div_euclid(1000);

    for key in mission_keys {
        let Some(mission) = catalog
            .categories
            .get(&key.category)
            .and_then(|missions| missions.iter().find(|mission| mission.id == key.mission_id))
        else {
            continue;
        };
        let Some(scope) = scope_for(key, mission, database)? else { continue; };
        if is_scoped_battle_mission(mission) || !super::dependency_received_in_scope(root, key, mission, database, &scope)? {
            continue;
        }
        let database_progress = stored_progress
            .get(&(key.category, key.mission_id))
            .copied()
            .unwrap_or_default();
        let computed_progress = if scope.is_lifetime {
            compute_progress(key.category, mission, &context, &counters, database_progress, catalog)
        } else {
            super::window::advance_progress(root, &scope, pattern, progress.saturating_sub(previous_counter), action_progress)?
        };
        mission_updates.insert((key.category, key.mission_id), computed_progress);
        let mut completed_stages = Vec::new();
        for stage in mission.stages.iter().filter(|stage| stage.target.is_some_and(|target| computed_progress >= target)) {
            let receipt = scope.receipt(root, stage.stage);
            if settle_mission_stage(root, key, stage, &receipt, response_time)?.is_some() {
                receipts.push(receipt);
                completed_stages.push((stage.stage, stage.reward_id));
            }
        }
        if !completed_stages.is_empty() {
            stage_updates.push((key, completed_stages));
        }
    }

    let mut mission_info = Vec::new();
    for (key, stages) in stage_updates {
        mission_info.extend(stages.into_iter().map(|(_, reward_id)| {
            json!({
                "mission_category_id": key.category,
                "mission_id": key.mission_id,
                "mission_reward_id": reward_id,
            })
        }));
    }
    mission_info.extend(super::collect::record_collected_item_changes(root, collections, &items_before, response_time)?);

    let counter_update = BTreeMap::from([(pattern.to_owned(), progress)]);
    database.set_mission_progress(account_id, &counter_update, &mission_updates)?;
    if !receipts.is_empty() {
        database.save_mission_rewards_with_receive_history(account_id, &serde_json::to_string(root).map_err(|error| PersonalServiceError::new(format!("failed to encode mission state: {error}")))?, &mission_updates, &mission_reward_event_key(&receipts), response_time, &[])?;
    }
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

// //// /按普通任务 master 保存动作进度和已完成阶段 ////

// //// 合并主动任务动作增量和当前状态变化 [@x380kkm 2026-09-21] ////
fn record_active_action(
    root: &mut Map<String, Value>, database: &ServiceDatabase, counters: &[(&str, i64)],
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut updates = super::active::record_active_counters(root, database, counters)?;
    updates.extend(super::active::refresh_active_missions(root, database)?);
    Ok(updates)
}
// //// /合并主动任务动作增量和当前状态变化 ////
