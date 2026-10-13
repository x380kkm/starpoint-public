// audience: internal
// # personal-service-cn-mission-window
//
// 活动资格与周期进度分别计算. 战斗沿用入场资格, 日周进度使用结算周期, 终身计数独立保存.

use super::{mission_catalog, MissionDefinition, MissionKey};
use super::battle_time::{activity_window_status, ActivityWindows, BattleMissionWindow};
use crate::database::{ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub(super) struct MissionScope {
    pub(super) key: String,
    pub(super) is_lifetime: bool,
    pub(super) evaluated_at: i64,
    progress_at: i64,
    activities: Option<Arc<ActivityWindows>>,
    activity_id: Option<String>,
}

impl MissionScope {
    pub(super) fn related(
        &self, key: MissionKey, mission: &MissionDefinition, database: &ServiceDatabase,
    ) -> Result<Option<Self>, PersonalServiceError> {
        mission_scope_in_window(key, mission, database, self.evaluated_at, self.progress_at, self.activities.clone())
    }

    // //// 复用当前任务期别的领取凭据并为重置后的任务生成新轮次身份 [@x380kkm 2026-09-28] ////
    pub(super) fn receipt(&self, root: &Map<String, Value>, stage: i64) -> String {
        let round = self.activity_id.as_deref().map(|activity_id| {
            crate::activity_history::participation_round(root, activity_id)
        }).unwrap_or_default();
        let current = if round > 0 {
            format!("{}:round:{round}:{stage}", self.key)
        } else {
            format!("{}:{stage}", self.key)
        };
        let Some(receipts) = root.get("mission_stage_receipts").and_then(Value::as_object) else {
            return current;
        };
        if round == 0 || receipts.contains_key(&current) {
            return current;
        }
        let original = format!("{}:{stage}", self.key);
        if receipts.contains_key(&original) {
            return original;
        }
        let prefix = format!("{}:round:", self.key);
        receipts.keys().find(|key| {
            key.strip_prefix(&prefix).and_then(|tail| tail.split_once(':'))
                .is_some_and(|(round, recorded_stage)| round.parse::<i64>().is_ok()
                    && recorded_stage.parse::<i64>() == Ok(stage))
        }).cloned().unwrap_or(current)
    }
}

// //// 根据活动时窗和任务周期确定进度范围 [@x380kkm 2026-09-21] ////
pub(super) fn mission_scope(
    key: MissionKey,
    mission: &MissionDefinition,
    database: &ServiceDatabase,
) -> Result<Option<MissionScope>, PersonalServiceError> {
    let now = database.current_server_time_millis()?.div_euclid(1000);
    mission_scope_at(key, mission, database, now)
}

pub(super) fn mission_scope_for_battle(
    key: MissionKey,
    mission: &MissionDefinition,
    database: &ServiceDatabase,
    window: &BattleMissionWindow,
    completed_at: i64,
) -> Result<Option<MissionScope>, PersonalServiceError> {
    if matches!(key.category, 3 | 4 | 6 | 7 | 8) {
        mission_scope_in_window(key, mission, database, window.started_at, completed_at, window.activities.clone())
    } else {
        mission_scope_at(key, mission, database, completed_at)
    }
}

pub(super) fn mission_activity_id(category: i64, mission: &MissionDefinition) -> Option<String> {
    if category == 3 {
        if mission.quest_kind == Some(16) && mission.quest_parts[0].len() == 1 {
            return Some(format!("raid:{}", mission.quest_parts[0][0]));
        }
        if mission.pattern.as_deref() == Some("character_election_vote_count") {
            return mission.event_id.map(|id| format!("character-election:{id}"));
        }
    }
    mission.event_id.and_then(|event_id| match category {
        4 => Some(format!("collect-item:{event_id}")),
        6..=8 => Some(format!("pass-card:{event_id}")),
        _ => None,
    })
}

pub(super) fn mission_scope_at(
    key: MissionKey, mission: &MissionDefinition, database: &ServiceDatabase, now: i64,
) -> Result<Option<MissionScope>, PersonalServiceError> {
    mission_scope_in_window(key, mission, database, now, now, None)
}

fn mission_scope_in_window(
    key: MissionKey, mission: &MissionDefinition, database: &ServiceDatabase, now: i64, progress_at: i64,
    activities: Option<Arc<ActivityWindows>>,
) -> Result<Option<MissionScope>, PersonalServiceError> {
    let activity_id = mission_activity_id(key.category, mission);
    let managed = match &activity_id {
        Some(activity_id) => activity_window_status(activities.as_deref(), database, activity_id, now)?,
        None => ActivityWindowStatus::Unscheduled,
    };
    let phased_mission = key.category == 4
        || key.category == 3 && activity_id.as_ref().is_some_and(|id| id.starts_with("raid:"));
    let within_mission_window = if phased_mission {
        activity_mission_is_open(key, mission, database, now, managed, activities.as_deref())?
    } else {
        mission.start_time.map_or(true, |start| now >= start)
            && mission.end_time.map_or(true, |end| now <= end)
    };
    let available = match managed {
        ActivityWindowStatus::Unscheduled => within_mission_window,
        ActivityWindowStatus::Open => !phased_mission || within_mission_window,
        _ => false,
    };
    if !available { return Ok(None); }
    let base = format!("{}:{}", key.category, key.mission_id);
    let regional_start = if let Some(activity_id) = activity_id.as_deref().filter(|_| phased_mission) {
        match activities.as_ref() {
            Some(captured) => captured.event_periods.as_ref().and_then(|periods| periods.get(activity_id)).copied(),
            None => crate::activity_projection::effective_regional_activity_period(database, activity_id, now.saturating_mul(1_000))?
                .map(|period| period.start_at_ms),
        }
    } else { None };
    let scope = match key.category {
        2 | 6 => format!("{base}:day:{}", game_day(progress_at)),
        7 | 10 => format!("{base}:week:{}", (game_day(progress_at) + 3).div_euclid(7)),
        3 | 4 | 8 => match regional_start {
            Some(start) => format!("{base}:event:regional:{start}"),
            None => format!("{base}:event:{}", mission.start_time.unwrap_or_default()),
        },
        _ => base,
    };
    Ok(Some(MissionScope { key: scope, is_lifetime: !matches!(key.category, 2 | 3 | 4 | 6 | 7 | 8 | 10), evaluated_at: now, progress_at, activities, activity_id }))
}
// //// /根据活动时窗和任务周期确定进度范围 ////

// //// 读取当前活动任务阶段或入场时保存的阶段资格 [@x380kkm 2026-09-27] ////
fn activity_mission_is_open(
    key: MissionKey, mission: &MissionDefinition, database: &ServiceDatabase, now: i64,
    managed: ActivityWindowStatus, captured: Option<&ActivityWindows>,
) -> Result<bool, PersonalServiceError> {
    let now_ms = now.saturating_mul(1_000);
    let original_open = mission.start_time.map_or(true, |start| now >= start)
        && mission.end_time.map_or(true, |end| now <= end);
    if let Some(captured) = captured {
        let periods = if key.category == 4 { &captured.collect_missions } else { &captured.event_missions };
        return Ok(match periods {
            Some(periods) => periods.get(&key.mission_id)
                .is_some_and(|[start, end]| now_ms >= *start && now_ms <= *end),
            None => managed == ActivityWindowStatus::Open || original_open,
        });
    }
    let period = if key.category == 4 {
        crate::activity_projection::collect_mission_window(database, key.mission_id, now_ms)?
    } else {
        crate::activity_projection::event_mission_window(database, key.mission_id, now_ms)?
    };
    Ok(period.map_or(original_open, |[start, end]| now_ms >= start && now_ms <= end))
}
// //// /读取当前活动任务阶段或入场时保存的阶段资格 ////

pub(super) fn game_day(now: i64) -> i64 {
    (now + 3 * 3600).div_euclid(86_400)
}

// //// 读取当期任务进度和依赖任务完成数 [@x380kkm 2026-09-21] ////
pub(super) fn scoped_progress(
    root: &Map<String, Value>,
    key: MissionKey,
    mission: &MissionDefinition,
    scope: &MissionScope,
    database: &ServiceDatabase,
) -> Result<i64, PersonalServiceError> {
    if mission.pattern.as_deref() == Some("target_mission_clear") {
        let catalog = mission_catalog()?;
        let mut completed = 0;
        for id in &mission.mission_ids {
            let Some(target) = catalog.categories.get(&key.category)
                .and_then(|missions| missions.iter().find(|target| target.id == *id)) else { continue; };
            let target_key = MissionKey { category: key.category, mission_id: *id };
            let Some(target_scope) = scope.related(target_key, target, database)? else { continue; };
            let progress = stored_progress(root, &target_scope.key);
            completed += i64::from(target.stages.last().and_then(|stage| stage.target).is_some_and(|threshold| progress >= threshold));
        }
        return Ok(completed);
    }
    Ok(stored_progress(root, &scope.key))
}
// //// /读取当期任务进度和依赖任务完成数 ////

fn stored_progress(root: &Map<String, Value>, key: &str) -> i64 {
    root.get("mission_window_progress").and_then(|progress| progress.get(key))
        .and_then(Value::as_i64).unwrap_or_default()
}

// //// 按动作增量更新当期进度并保留历史周期 [@x380kkm 2026-09-21] ////
pub(super) fn advance_progress(
    root: &mut Map<String, Value>, scope: &MissionScope, pattern: &str, delta: i64, absolute: i64,
) -> Result<i64, PersonalServiceError> {
    let previous = stored_progress(root, &scope.key);
    let progress = if pattern.starts_with("max_") || pattern.ends_with("_achievement") {
        previous.max(absolute)
    } else {
        previous.checked_add(delta.max(0)).ok_or_else(|| PersonalServiceError::new("CN mission window progress exceeds range"))?
    };
    root.entry("mission_window_progress".to_owned()).or_insert_with(|| json!({}))
        .as_object_mut().ok_or_else(|| PersonalServiceError::new("stored CN mission window progress is invalid"))?
        .insert(scope.key.clone(), Value::from(progress));
    Ok(progress)
}
// //// /按动作增量更新当期进度并保留历史周期 ////

// //// 将当前登录日期计入每周任务 [@x380kkm 2026-09-21] ////
pub(super) fn record_login_day(root: &mut Map<String, Value>, now: i64) -> Result<(), PersonalServiceError> {
    let day = game_day(now);
    let week = (day + 3).div_euclid(7);
    let dates = root.entry("mission_login_days".to_owned()).or_insert_with(|| json!([]))
        .as_array_mut().ok_or_else(|| PersonalServiceError::new("stored CN mission login days are invalid"))?;
    if !dates.iter().any(|value| value.as_i64() == Some(day)) { dates.push(Value::from(day)); }
    let login_days = dates.iter().filter_map(Value::as_i64).filter(|day| (day + 3).div_euclid(7) == week).count() as i64;
    let catalog = mission_catalog()?;
    for mission in catalog.categories.get(&10).into_iter().flatten().filter(|mission| mission.pattern.as_deref() == Some("total_login_days")) {
        root.entry("mission_window_progress".to_owned()).or_insert_with(|| json!({}))
            .as_object_mut().ok_or_else(|| PersonalServiceError::new("stored CN mission window progress is invalid"))?
            .insert(format!("10:{}:week:{week}", mission.id), Value::from(login_days));
    }
    Ok(())
}
// //// /将当前登录日期计入每周任务 ////

// //// 将登录日期计入当前修行活动任务 [@x380kkm 2026-09-22] ////
pub(crate) fn record_pass_card_login_days(root: &mut Map<String, Value>, database: &ServiceDatabase, now: i64) -> Result<(), PersonalServiceError> {
    let day = game_day(now);
    let catalog = mission_catalog()?;
    for mission in catalog.categories.get(&8).into_iter().flatten().filter(|mission| mission.pattern.as_deref() == Some("total_login_days")) {
        let Some(scope) = mission_scope(MissionKey { category: 8, mission_id: mission.id }, mission, database)? else { continue; };
        let Some(event_id) = mission.event_id else { continue; };
        let count = crate::cn_pass_card::record_login_day_for_event(root, event_id, day)?;
        root.entry("mission_window_progress".to_owned()).or_insert_with(|| json!({}))
            .as_object_mut().ok_or_else(|| PersonalServiceError::new("stored CN mission window progress is invalid"))?
            .insert(scope.key, Value::from(count));
    }
    Ok(())
}
// //// /将登录日期计入当前修行活动任务 ////
