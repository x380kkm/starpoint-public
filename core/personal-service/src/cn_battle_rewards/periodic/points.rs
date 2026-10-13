// audience: internal
// # personal-service-cn-periodic-reward-points
// 奖励次数按游戏时间的 UTC+8 日界累积, 恢复检查点与玩家余额保存在同一快照.
// 旧存档首次建立检查点时保留余额, 时钟回退保留较晚检查点.

use super::{catalog, PeriodicPoint, PeriodicRewardCatalog, POINTS_FIELD, RECOVERY_STATE_FIELD};
use crate::cn_mission::{activity_window_status, BattleMissionWindow};
use crate::database::{evaluate_activity_schedule, ActivityWindowStatus, ServiceDatabase};
use crate::PersonalServiceError;
use serde_json::{json, Map, Value};

struct PointWindow {
    start_time: i64,
    end_time: i64,
}

impl PointWindow {
    fn contains(&self, at: i64) -> bool {
        self.start_time <= at && at <= self.end_time
    }
}

// //// 在载入玩家快照时恢复有效活动的周期点数 [@x380kkm 2026-09-28] ////
pub(crate) fn recover_periodic_reward_points(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    server_time: i64,
) -> Result<(), PersonalServiceError> {
    let catalog = catalog()?;
    for (&id, point) in &catalog.points {
        let window = point_window(point, database, server_time)?;
        recover_point(
            root,
            id,
            point,
            window.as_ref(),
            server_time,
            catalog.daily_recovery_hour,
        )?;
    }
    Ok(())
}

// //// 按捕获的入场资格恢复战斗所属奖励点数 [@x380kkm 2026-09-28] ////
pub(super) fn recover_battle_points(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    catalog: &PeriodicRewardCatalog,
    point_id: i64,
    battle: &BattleMissionWindow,
) -> Result<bool, PersonalServiceError> {
    let point = catalog
        .points
        .get(&point_id)
        .ok_or_else(|| PersonalServiceError::new("CN periodic reward point is missing"))?;
    let window = match activity_window_status(
        battle.activities.as_deref(),
        database,
        &point.activity_id,
        battle.started_at,
    )? {
        ActivityWindowStatus::Open => Some(
            point_window(point, database, battle.started_at)?.unwrap_or(PointWindow {
                start_time: battle.started_at,
                end_time: battle.started_at,
            }),
        ),
        ActivityWindowStatus::Unscheduled => natural_window(point, battle.started_at)?,
        _ => None,
    };
    let available = window
        .as_ref()
        .is_some_and(|window| window.contains(battle.started_at));
    if available {
        recover_point(
            root,
            point_id,
            point,
            window.as_ref(),
            battle.started_at,
            catalog.daily_recovery_hour,
        )?;
    }
    Ok(available)
}

// //// 读取管理规则和主表定义的周期点有效区间 [@x380kkm 2026-09-28] ////
fn point_window(
    point: &PeriodicPoint,
    database: &ServiceDatabase,
    at: i64,
) -> Result<Option<PointWindow>, PersonalServiceError> {
    if database
        .activity_temporary_open_until(&point.activity_id)?
        .is_some()
    {
        return Ok(Some(PointWindow {
            start_time: i64::MIN,
            end_time: i64::MAX,
        }));
    }
    if let Some(schedule) = database.get_activity_schedule(&point.activity_id)? {
        let evaluated = evaluate_activity_schedule(&schedule, at.saturating_mul(1_000));
        return Ok(
            (evaluated.status == ActivityWindowStatus::Open).then_some(PointWindow {
                start_time: evaluated
                    .active_start_ms
                    .map_or(i64::MIN, |value| value.div_euclid(1_000)),
                end_time: evaluated
                    .active_end_ms
                    .map_or(i64::MAX, |value| value.saturating_sub(1).div_euclid(1_000)),
            }),
        );
    }
    natural_window(point, at)
}

fn natural_window(
    point: &PeriodicPoint,
    at: i64,
) -> Result<Option<PointWindow>, PersonalServiceError> {
    let (start_time, end_time) = crate::activity_projection::regional_activity_window(
        &point.activity_id,
        at.saturating_mul(1_000),
    )?
    .map_or((point.start_time, point.end_time), |(start, end)| {
        (
            start.div_euclid(1_000),
            end.saturating_sub(1).div_euclid(1_000),
        )
    });
    let window = PointWindow {
        start_time,
        end_time,
    };
    Ok(window.contains(at).then_some(window))
}

// //// 累加有效区间内的每日恢复并前推检查点 [@x380kkm 2026-09-28] ////
fn recover_point(
    root: &mut Map<String, Value>,
    id: i64,
    point: &PeriodicPoint,
    window: Option<&PointWindow>,
    at: i64,
    recovery_hour: i64,
) -> Result<(), PersonalServiceError> {
    let stored = stored_point(root, id);
    let previous = root
        .get(RECOVERY_STATE_FIELD)
        .and_then(|states| states.get(id.to_string()))
        .and_then(|state| state.get("last_recovery_time"))
        .and_then(Value::as_i64);
    let active = window.filter(|window| window.contains(at));
    if let Some(window) = active {
        if point.recovery_cycle != 0 || point.recovery_point < 0 || point.max_point < 0 {
            return Err(PersonalServiceError::new(
                "CN periodic reward recovery is invalid",
            ));
        }
        let count = match (stored, previous) {
            (None, _) => point.recovery_point.min(point.max_point),
            (Some(count), None) => count,
            (Some(count), Some(previous)) => {
                let offset = (8 - recovery_hour) * 3_600;
                let from = previous.max(window.start_time);
                let to = at.min(window.end_time);
                let cycles = if to > from {
                    ((to.saturating_add(offset)).div_euclid(86_400)
                        - (from.saturating_add(offset)).div_euclid(86_400))
                    .max(0)
                } else {
                    0
                };
                if cycles > 0 {
                    count
                        .saturating_add(cycles.saturating_mul(point.recovery_point))
                        .min(point.max_point)
                } else {
                    count
                }
            }
        };
        store_point(root, id, count)?;
    } else if stored.is_none() && previous.is_none() {
        return Ok(());
    }
    let states = root
        .entry(RECOVERY_STATE_FIELD.to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN periodic recovery state is invalid"))?;
    states.insert(
        id.to_string(),
        json!({"last_recovery_time": previous.map_or(at, |previous| previous.max(at))}),
    );
    Ok(())
}

// //// 读取玩家持有的周期点数 [@x380kkm 2026-09-28] ////
pub(super) fn stored_point(root: &Map<String, Value>, id: i64) -> Option<i64> {
    root.get(POINTS_FIELD)?
        .as_array()?
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_i64) == Some(id))?
        .get("point")?
        .as_i64()
}

// //// 保存周期点余额并保留其他活动记录 [@x380kkm 2026-09-28] ////
pub(super) fn store_point(
    root: &mut Map<String, Value>,
    id: i64,
    point: i64,
) -> Result<(), PersonalServiceError> {
    let entries = root
        .entry(POINTS_FIELD.to_owned())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN periodic reward points are invalid"))?;
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.get("id").and_then(Value::as_i64) == Some(id))
    {
        entry
            .as_object_mut()
            .ok_or_else(|| PersonalServiceError::new("stored CN periodic reward point is invalid"))?
            .insert("point".to_owned(), json!(point));
    } else {
        entries.push(json!({"id": id, "point": point}));
    }
    Ok(())
}

#[cfg(test)]
#[path = "points_tests.rs"]
mod tests;
