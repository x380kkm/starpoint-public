// audience: internal
// # personal-service-cn-mission-battle-time
//
// 当前局保存入场时的活动开放资格. 缺少资格快照的存档从单人或多人入场收据读取时间.
// 没有自己开放规则的收集任务活动按跟随主活动得到的窗口判定资格.

use crate::database::{ActivityWindowStatus, MultiplayerBattleIdentity, ServiceDatabase};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const SNAPSHOT_KEY: &str = "battle_mission_window";

pub(crate) struct ActivityWindows {
    states: BTreeMap<String, Option<bool>>,
    pub(crate) unlocked_additional_events: Option<BTreeSet<i64>>,
    pub(super) collect_missions: Option<BTreeMap<i64, [i64; 2]>>,
    pub(super) event_missions: Option<BTreeMap<i64, [i64; 2]>>,
    pub(super) event_periods: Option<BTreeMap<String, i64>>,
}

#[derive(Deserialize, Serialize)]
struct CapturedWindow {
    play_id: String,
    started_at: i64,
    activities: BTreeMap<String, Option<bool>>,
    unlocked_additional_events: Option<BTreeSet<i64>>,
    collect_missions: Option<BTreeMap<i64, [i64; 2]>>,
    event_missions: Option<BTreeMap<i64, [i64; 2]>>,
    event_periods: Option<BTreeMap<String, i64>>,
}

pub(crate) struct BattleMissionWindow {
    pub(crate) started_at: i64,
    pub(crate) activities: Option<Arc<ActivityWindows>>,
}

// //// 保存当前局入场时的限时任务活动资格 [@x380kkm 2026-09-24] ////
pub(crate) fn capture_battle_mission_window(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    play_id: &str,
    started_at: i64,
) -> Result<(), PersonalServiceError> {
    let catalog = super::mission_catalog()?;
    let mut activity_ids = BTreeSet::new();
    for (category, missions) in &catalog.categories {
        for mission in missions {
            if let Some(id) = super::window::mission_activity_id(*category, mission) {
                activity_ids.insert(id);
            }
        }
    }
    activity_ids.extend(super::active::battle_activity_ids()?);
    activity_ids.extend(crate::cn_battle_rewards::additional_activity_ids()?);
    activity_ids.extend(crate::cn_battle_rewards::periodic_activity_ids()?);
    let started_at_ms = started_at.saturating_mul(1_000);
    let companions = crate::activity_projection::companion_activity_windows(database, started_at_ms)?;
    let activities = activity_ids
        .into_iter()
        .map(|id| {
            let status = with_companion_window(
                database.activity_window_status(&id, started_at_ms)?,
                companions.get(&id).copied(),
                started_at_ms,
            );
            let available = match status {
                ActivityWindowStatus::Unscheduled => None,
                ActivityWindowStatus::Open => Some(true),
                _ => Some(false),
            };
            Ok((id, available))
        })
        .collect::<Result<_, PersonalServiceError>>()?;
    let value = serde_json::to_value(CapturedWindow {
        play_id: play_id.to_owned(),
        started_at,
        activities,
        unlocked_additional_events: Some(crate::cn_battle_rewards::unlocked_additional_event_ids(root)?),
        collect_missions: Some(crate::activity_projection::collect_mission_windows(database, started_at.saturating_mul(1_000))?),
        event_missions: Some(crate::activity_projection::event_mission_windows(database, started_at.saturating_mul(1_000))?),
        event_periods: Some(crate::activity_projection::regional_activity_period_starts(database, started_at.saturating_mul(1_000))?),
    })
    .map_err(|error| {
        PersonalServiceError::new(format!("failed to encode battle mission window: {error}"))
    })?;
    root.insert(SNAPSHOT_KEY.to_owned(), value);
    Ok(())
}
// //// /保存当前局入场时的限时任务活动资格 ////

// //// 按当前战斗身份读取已保存的活动资格 [@x380kkm 2026-09-24] ////
pub(crate) fn battle_mission_window(
    root: &Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    completed_at: i64,
) -> Result<BattleMissionWindow, PersonalServiceError> {
    if let Some(active) = database.get_active_single_quest(account_id)? {
        if let Some(value) = root.get(SNAPSHOT_KEY).filter(|value| {
            value.get("play_id").and_then(Value::as_str) == Some(active.play_id.as_str())
        }) {
            let captured: CapturedWindow =
                serde_json::from_value(value.clone()).map_err(|error| {
                    PersonalServiceError::new(format!("invalid battle mission window: {error}"))
                })?;
            return Ok(BattleMissionWindow {
                started_at: captured.started_at,
                activities: Some(Arc::new(ActivityWindows {
                    states: captured.activities,
                    unlocked_additional_events: captured.unlocked_additional_events,
                    collect_missions: captured.collect_missions,
                    event_missions: captured.event_missions,
                    event_periods: captured.event_periods,
                })),
            });
        }
    }
    Ok(BattleMissionWindow {
        started_at: battle_started_at(database, account_id)?.unwrap_or(completed_at),
        activities: None,
    })
}
// //// /按当前战斗身份读取已保存的活动资格 ////

pub(crate) fn activity_window_status(
    activities: Option<&ActivityWindows>,
    database: &ServiceDatabase,
    activity_id: &str,
    at: i64,
) -> Result<ActivityWindowStatus, PersonalServiceError> {
    if let Some(available) = activities.and_then(|activities| activities.states.get(activity_id)) {
        return Ok(match available {
            None => ActivityWindowStatus::Unscheduled,
            Some(true) => ActivityWindowStatus::Open,
            Some(false) => ActivityWindowStatus::Disabled,
        });
    }
    let at_ms = at.saturating_mul(1_000);
    let status = database.activity_window_status(activity_id, at_ms)?;
    let companion = if status == ActivityWindowStatus::Unscheduled {
        crate::activity_projection::companion_activity_window(database, activity_id, at_ms)?
    } else {
        None
    };
    Ok(with_companion_window(status, companion, at_ms))
}

// //// 未排期的收集任务活动按跟随主活动的窗口判定 [@x380kkm 2026-10-05] ////
fn with_companion_window(
    status: ActivityWindowStatus,
    companion: Option<(i64, i64)>,
    at_ms: i64,
) -> ActivityWindowStatus {
    match (status, companion) {
        (ActivityWindowStatus::Unscheduled, Some((start_at_ms, _))) if at_ms < start_at_ms => ActivityWindowStatus::NotStarted,
        (ActivityWindowStatus::Unscheduled, Some((_, end_at_ms))) if at_ms > end_at_ms => ActivityWindowStatus::Ended,
        (ActivityWindowStatus::Unscheduled, Some(_)) => ActivityWindowStatus::Open,
        _ => status,
    }
}
// //// /未排期的收集任务活动按跟随主活动的窗口判定 ////

// //// 读取当前局的单人或多人入场时间 [@x380kkm 2026-09-23] ////
fn battle_started_at(
    database: &ServiceDatabase,
    account_id: i64,
) -> Result<Option<i64>, PersonalServiceError> {
    let Some(active) = database.get_active_single_quest(account_id)? else {
        return Ok(None);
    };
    if let Some(receipt) = database.single_battle_start_receipt(account_id, &active.play_id)? {
        let response: Value = serde_json::from_str(&receipt).map_err(|error| {
            PersonalServiceError::new(format!("invalid single battle start receipt: {error}"))
        })?;
        return Ok(response.get("start_time").and_then(Value::as_i64));
    }
    let Some(room_number) = database.multiplayer_battle_room(
        account_id,
        &active.play_id,
        active.category,
        active.quest_id,
    )?
    else {
        return Ok(None);
    };
    Ok(database
        .multiplayer_start_receipt(MultiplayerBattleIdentity {
            account_id,
            room_number: &room_number,
            play_id: &active.play_id,
            category_id: active.category,
            quest_id: active.quest_id,
            api_count: None,
        })?
        .map(|receipt| receipt.response_time))
}
// //// /读取当前局的单人或多人入场时间 ////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cn_mission::{window, MissionKey};
    use crate::database::parse_iso_timestamp;
    use tempfile::TempDir;

    // //// 验证收集任务分阶段开放与入场资格跨期保存 [@x380kkm 2026-09-27] ////
    #[test]
    fn captures_original_mission_phase_across_event_closure() {
        let root = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root.path()).unwrap();
        let early = parse_iso_timestamp("2020-09-16T04:00:00Z").unwrap() / 1000;
        let started = parse_iso_timestamp("2020-09-24T04:00:00Z").unwrap() / 1000;
        let completed = parse_iso_timestamp("2020-10-03T04:00:00Z").unwrap() / 1000;
        let catalog = crate::cn_mission::mission_catalog().unwrap();
        let mission = catalog.categories[&4].iter().find(|mission| mission.id == 1769).unwrap();
        let key = MissionKey { category: 4, mission_id: 1769 };
        database.set_virtual_time(true, early * 1000, 1.0).unwrap();
        assert!(window::mission_scope_at(key, mission, &database, early).unwrap().is_none());
        database.set_virtual_time(true, started * 1000, 1.0).unwrap();
        assert!(window::mission_scope_at(key, mission, &database, started).unwrap().is_some());
        let mut player = Map::new();
        capture_battle_mission_window(&mut player, &database, "saga-stage", started).unwrap();
        let captured: CapturedWindow = serde_json::from_value(player[SNAPSHOT_KEY].clone()).unwrap();
        let battle = BattleMissionWindow {
            started_at: captured.started_at,
            activities: Some(Arc::new(ActivityWindows {
                states: captured.activities,
                unlocked_additional_events: captured.unlocked_additional_events,
                collect_missions: captured.collect_missions,
                event_missions: captured.event_missions,
                event_periods: captured.event_periods,
            })),
        };
        database.set_virtual_time(true, completed * 1000, 1.0).unwrap();
        database.upsert_manual_activity("collect-item:10003", false).unwrap();
        assert!(window::mission_scope_at(key, mission, &database, completed).unwrap().is_none());
        assert!(window::mission_scope_for_battle(key, mission, &database, &battle, completed).unwrap().is_some());
        player[SNAPSHOT_KEY].as_object_mut().unwrap().remove("collect_missions");
        let legacy: CapturedWindow = serde_json::from_value(player[SNAPSHOT_KEY].clone()).unwrap();
        assert_eq!(legacy.started_at, started);
        assert!(legacy.collect_missions.is_none());
        let legacy_battle = BattleMissionWindow {
            started_at: legacy.started_at,
            activities: Some(Arc::new(ActivityWindows {
                states: BTreeMap::from([("collect-item:10003".to_owned(), Some(true))]),
                unlocked_additional_events: None,
                collect_missions: None,
                event_missions: None,
                event_periods: None,
            })),
        };
        assert!(window::mission_scope_for_battle(key, mission, &database, &legacy_battle, completed).unwrap().is_some());
    }
    // //// /验证收集任务分阶段开放与入场资格跨期保存 ////
}
