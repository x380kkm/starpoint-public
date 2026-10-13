// audience: internal
// # personal-service-cn-active-mission-progress
//
// 主动任务按 CN 主表定义匹配动作, 前置阶段和活动时窗. 动作增量和阶段状态随玩家快照保存.

mod battle;
mod state;
#[cfg(test)]
mod tests;

use super::battle_time::{activity_window_status, BattleMissionWindow};
use super::conditions::battle_fixture;
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(crate) use battle::{
    record_active_battle, record_active_quest_challenge, record_active_quest_clear,
};
pub(crate) use state::refresh_active_missions;

const CATALOG: &str = include_str!("../../assets/cn-active-mission-incentives.json");
const REWARDS: &str = include_str!("../../assets/cn-active-mission-rewards.json");
static DEFINITIONS: OnceLock<Result<Catalog, String>> = OnceLock::new();
static STAGES: OnceLock<Result<BTreeMap<i64, BTreeMap<i64, Vec<Vec<Value>>>>, String>> =
    OnceLock::new();

#[derive(Deserialize)]
struct Catalog {
    missions: BTreeMap<i64, Mission>,
    events: BTreeMap<i64, Event>,
}

#[derive(Deserialize)]
struct Event {
    start_time: Option<i64>,
    playable_end_time: Option<i64>,
}

#[derive(Deserialize)]
struct Mission {
    event_id: i64,
    pattern: String,
    battle_kind: Option<i64>,
    statistics_kind: Option<i64>,
    quest_kind: Option<i64>,
    quest_parts: [Vec<i64>; 3],
    quest_categories: Vec<i64>,
    quest_ids: Vec<i64>,
    quest_rank: Option<i64>,
    character_id: Option<i64>,
    campaign_id: Option<i64>,
    main_character_ids: [Option<i64>; 3],
    unison_character_ids: [Option<i64>; 3],
    party_equipment_ids: [Option<i64>; 3],
    mission_ids: Vec<i64>,
    need_mission_id: Option<i64>,
    need_mission_stage: Option<i64>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    character_element: Option<i64>,
    equipment_element: Option<i64>,
    action_effects: Vec<String>,
    effects_ignore_characters: Vec<String>,
}

fn catalog() -> Result<&'static Catalog, PersonalServiceError> {
    DEFINITIONS
        .get_or_init(|| serde_json::from_str(CATALOG).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 枚举战斗入场需要保存的主动任务活动 [@x380kkm 2026-09-24] ////
pub(super) fn battle_activity_ids() -> Result<Vec<String>, PersonalServiceError> {
    Ok(catalog()?
        .events
        .keys()
        .map(|id| format!("active-mission:{id}"))
        .collect())
}

fn stage_targets(mission_id: i64) -> Result<Vec<(i64, i64)>, PersonalServiceError> {
    let stages = STAGES
        .get_or_init(|| serde_json::from_str(REWARDS).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    stages
        .get(&mission_id)
        .into_iter()
        .flatten()
        .map(|(stage, rows)| {
            let target = rows
                .first()
                .and_then(|row| row.get(3))
                .and_then(|value| {
                    value
                        .as_i64()
                        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
                })
                .ok_or_else(|| PersonalServiceError::new("CN active mission target is missing"))?;
            Ok((*stage, target))
        })
        .collect()
}

fn stage_time_limit(mission_id: i64) -> Result<Option<i64>, PersonalServiceError> {
    let stages = STAGES
        .get_or_init(|| serde_json::from_str(REWARDS).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    Ok(stages
        .get(&mission_id)
        .and_then(|stages| stages.values().next())
        .and_then(|rows| rows.first())
        .and_then(|row| row.get(4))
        .and_then(|value| value.as_str())
        .and_then(|value| value.parse::<i64>().ok()))
}

fn progress(root: &Map<String, Value>, mission_id: i64) -> i64 {
    root.get("all_active_mission_list")
        .and_then(|missions| missions.get(mission_id.to_string()))
        .and_then(|mission| mission.get("progress"))
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

fn stage_received(root: &Map<String, Value>, mission_id: i64, stage: i64) -> bool {
    if root
        .get("active_mission_receipts")
        .and_then(|receipts| receipts.get(format!("{mission_id}:{stage}")))
        .is_some()
    {
        return true;
    }
    let stages = root
        .get("all_active_mission_list")
        .and_then(|missions| missions.get(mission_id.to_string()))
        .and_then(|mission| mission.get("stages"));
    stages
        .and_then(|stages| stages.get(stage.to_string()))
        .and_then(Value::as_bool)
        .or_else(|| {
            stages
                .and_then(Value::as_array)
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry.get("stage").and_then(Value::as_i64) == Some(stage))
                })
                .and_then(|entry| entry.get("received"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

fn is_available(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    mission: &Mission,
) -> Result<bool, PersonalServiceError> {
    let window = BattleMissionWindow {
        started_at: database.current_server_time_millis()?.div_euclid(1000),
        activities: None,
    };
    is_available_in_window(root, database, mission, &window)
}

// //// 按指定活动资格与任务前置阶段判定可参与任务 [@x380kkm 2026-09-24] ////
fn is_available_in_window(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    mission: &Mission,
    window: &BattleMissionWindow,
) -> Result<bool, PersonalServiceError> {
    if let Some(required) = mission.need_mission_id {
        let stage = mission.need_mission_stage.unwrap_or(1);
        if !stage_received(root, required, stage) {
            return Ok(false);
        }
    }
    let now = window.started_at;
    match activity_window_status(
        window.activities.as_deref(),
        database,
        &format!("active-mission:{}", mission.event_id),
        now,
    )? {
        ActivityWindowStatus::Open => Ok(true),
        ActivityWindowStatus::Unscheduled => {
            let event = catalog()?.events.get(&mission.event_id);
            Ok(mission.start_time.map_or(true, |start| now >= start)
                && mission.end_time.map_or(true, |end| now <= end)
                && event
                    .and_then(|event| event.start_time)
                    .map_or(true, |start| now >= start)
                && event
                    .and_then(|event| event.playable_end_time)
                    .map_or(true, |end| now <= end))
        }
        _ => Ok(false),
    }
}
// //// /按指定活动资格与任务前置阶段判定可参与任务 ////

// //// 保存主表任务进度并返回新完成阶段 [@x380kkm 2026-09-21] ////
fn advance(
    root: &mut Map<String, Value>,
    mission_id: i64,
    value: i64,
) -> Result<Option<Value>, PersonalServiceError> {
    let previous = progress(root, mission_id);
    let value = value.max(previous).max(0);
    if value == 0 {
        return Ok(None);
    }
    let targets = stage_targets(mission_id)?;
    let missions = root
        .entry("all_active_mission_list".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN active missions are invalid"))?;
    let mission = missions
        .entry(mission_id.to_string())
        .or_insert_with(|| json!({"progress":0,"stages":{}}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN active mission is invalid"))?;
    let stages = mission
        .entry("stages".to_owned())
        .or_insert_with(|| json!({}));
    if let Some(entries) = stages.as_array() {
        *stages = Value::Object(
            entries
                .iter()
                .filter_map(|entry| {
                    Some((
                        entry.get("stage")?.as_i64()?.to_string(),
                        entry.get("received")?.clone(),
                    ))
                })
                .collect(),
        );
    }
    let stages = stages
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN active mission stages are invalid"))?;
    let mut changed_stages = Vec::new();
    for (stage, target) in targets {
        if value >= target && !stages.contains_key(&stage.to_string()) {
            stages.insert(stage.to_string(), json!(false));
            changed_stages.push(json!({"stage":stage,"received":false}));
        }
    }
    mission.insert("progress".to_owned(), json!(value));
    if value == previous && changed_stages.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        json!({"mission_id":mission_id,"progress_value":value,"stages":changed_stages}),
    ))
}
// //// /保存主表任务进度并返回新完成阶段 ////

fn complete_dependencies(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    updates: &mut Vec<Value>,
) -> Result<(), PersonalServiceError> {
    complete_dependencies_with(root, updates, |root, mission| {
        is_available(root, database, mission)
    })
}

// //// 使用同一活动资格更新关联任务完成数 [@x380kkm 2026-09-24] ////
fn complete_dependencies_with(
    root: &mut Map<String, Value>,
    updates: &mut Vec<Value>,
    available: impl Fn(&Map<String, Value>, &Mission) -> Result<bool, PersonalServiceError>,
) -> Result<(), PersonalServiceError> {
    for (id, mission) in &catalog()?.missions {
        if mission.pattern != "target_mission_clear" || !available(root, mission)? {
            continue;
        }
        let mut completed = 0;
        for target in &mission.mission_ids {
            completed += i64::from(stage_targets(*target)?.last().is_some_and(
                |(stage, threshold)| {
                    progress(root, *target) >= *threshold || stage_received(root, *target, *stage)
                },
            ));
        }
        if let Some(update) = advance(root, *id, completed)? {
            updates.push(update);
        }
    }
    Ok(())
}
// //// /使用同一活动资格更新关联任务完成数 ////

// //// 按真实动作增量更新当前生效的主动任务 [@x380kkm 2026-09-21] ////
pub(crate) fn record_active_counters(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    counters: &[(&str, i64)],
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut updates = Vec::new();
    for (id, mission) in &catalog()?.missions {
        let delta = counters
            .iter()
            .filter(|(pattern, _)| *pattern == mission.pattern)
            .try_fold(0_i64, |total, (_, delta)| {
                total.checked_add((*delta).max(0))
            })
            .ok_or_else(|| PersonalServiceError::new("CN active mission action exceeds range"))?;
        if delta == 0 || !is_available(root, database, mission)? {
            continue;
        }
        let value = progress(root, *id)
            .checked_add(delta)
            .ok_or_else(|| PersonalServiceError::new("CN active mission progress exceeds range"))?;
        if let Some(update) = advance(root, *id, value)? {
            updates.push(update);
        }
    }
    complete_dependencies(root, database, &mut updates)?;
    Ok(updates)
}
// //// /按真实动作增量更新当前生效的主动任务 ////

// //// 将指定活动抽取和兑换次数计入对应主表任务 [@x380kkm 2026-09-21] ////
pub(crate) fn record_active_campaign_action(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    pattern: &str,
    campaign_id: i64,
    count: i64,
) -> Result<Vec<Value>, PersonalServiceError> {
    let mut updates = Vec::new();
    for (id, mission) in &catalog()?.missions {
        if mission.pattern != pattern
            || mission.campaign_id != Some(campaign_id)
            || count <= 0
            || !is_available(root, database, mission)?
        {
            continue;
        }
        let value = progress(root, *id)
            .checked_add(count)
            .ok_or_else(|| PersonalServiceError::new("CN active mission progress exceeds range"))?;
        if let Some(update) = advance(root, *id, value)? {
            updates.push(update);
        }
    }
    complete_dependencies(root, database, &mut updates)?;
    Ok(updates)
}
// //// /将指定活动抽取和兑换次数计入对应主表任务 ////
