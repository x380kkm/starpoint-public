// audience: internal
// # regional-activity-periods
// 区域期别复用 CN 活动身份和奖励, 原始子记录时间同时约束客户端和服务端.

use super::{
    active_windows_at, activity_reward_tail_ms, projection_window, quest_windows::QuestTimes,
    OrderedValue, ProjectionWindow,
};
use crate::database::ServiceDatabase;
use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(super) const CATALOG: &str = include_str!("../../assets/cn-regional-activity-periods.json");

#[derive(Debug, Deserialize)]
pub(crate) struct RegionalPeriod {
    pub(crate) start_at_ms: i64,
    pub(crate) end_at_ms: i64,
    pub(crate) aggregation_end_at_ms: i64,
    pub(crate) reward_end_at_ms: i64,
    masters: BTreeMap<String, BTreeMap<String, BTreeMap<usize, String>>>,
    pub(super) quests: BTreeMap<String, [i64; 2]>,
    pub(super) items: BTreeMap<i64, [i64; 2]>,
    #[serde(default)]
    canonical_activity_id: Option<String>,
    #[serde(default)]
    canonical_items: BTreeMap<i64, [i64; 2]>,
}

impl RegionalPeriod {
    pub(super) fn master_fields(
        &self,
        master: &str,
        row: &str,
    ) -> Option<&BTreeMap<usize, String>> {
        self.masters.get(master)?.get(row)
    }
}

fn catalog() -> Result<&'static BTreeMap<String, RegionalPeriod>, PersonalServiceError> {
    static VALUE: OnceLock<Result<BTreeMap<String, RegionalPeriod>, String>> = OnceLock::new();
    VALUE
        .get_or_init(|| {
            serde_json::from_str(CATALOG)
                .map_err(|error| format!("invalid regional activity periods: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 按完整商品集合关联区域活动与本地商店身份 [@x380kkm 2026-09-29] ////
pub(crate) fn regional_shop_activity_alias(
    activity_id: &str,
) -> Result<Option<String>, PersonalServiceError> {
    for (regional_id, period) in catalog()? {
        if let Some(canonical_id) = &period.canonical_activity_id {
            if regional_id == activity_id {
                return Ok(Some(canonical_id.clone()));
            }
            if canonical_id == activity_id {
                return Ok(Some(regional_id.clone()));
            }
        }
    }
    Ok(None)
}

pub(super) fn canonical_item_periods() -> Result<BTreeMap<i64, [i64; 2]>, PersonalServiceError> {
    let mut items = BTreeMap::new();
    for period in catalog()?.values() {
        for (id, window) in &period.canonical_items {
            if items
                .insert(*id, *window)
                .is_some_and(|previous| previous != *window)
            {
                return Err(PersonalServiceError::new(format!(
                    "conflicting canonical shop period: {id}"
                )));
            }
        }
    }
    Ok(items)
}
// //// /按完整商品集合关联区域活动与本地商店身份 ////

// //// 选择虚拟日期所在的原始区域期别 [@x380kkm 2026-09-27] ////
pub(crate) fn regional_activity_window(
    activity_id: &str,
    now_ms: i64,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    Ok(regional_activity_period(activity_id, now_ms)?
        .map(|period| (period.start_at_ms, period.end_at_ms)))
}

pub(crate) fn regional_activity_period(
    activity_id: &str,
    now_ms: i64,
) -> Result<Option<&'static RegionalPeriod>, PersonalServiceError> {
    Ok(catalog()?
        .get(activity_id)
        .filter(|period| now_ms >= period.start_at_ms && now_ms <= period.reward_end_at_ms))
}

pub(crate) fn effective_regional_activity_period(
    database: &ServiceDatabase,
    activity_id: &str,
    now_ms: i64,
) -> Result<Option<&'static RegionalPeriod>, PersonalServiceError> {
    if database.get_activity_schedule(activity_id)?.is_some()
        || database
            .activity_temporary_reward_window(activity_id, activity_reward_tail_ms(activity_id)?)?
            .is_some()
    {
        return Ok(None);
    }
    regional_activity_period(activity_id, now_ms)
}

pub(crate) fn regional_activity_period_starts(
    database: &ServiceDatabase,
    now_ms: i64,
) -> Result<BTreeMap<String, i64>, PersonalServiceError> {
    Ok(active_windows_at(database, now_ms)?
        .into_iter()
        .filter_map(|(id, window)| {
            window
                .regional_period
                .map(|period| (id, period.start_at_ms))
        })
        .collect())
}

pub(super) fn apply_periods(
    windows: &mut BTreeMap<String, ProjectionWindow>,
    configured: &BTreeSet<String>,
    now_ms: i64,
) -> Result<(), PersonalServiceError> {
    for (activity_id, period) in catalog()? {
        if configured.contains(activity_id)
            || now_ms < period.start_at_ms
            || now_ms > period.reward_end_at_ms
        {
            continue;
        }
        let mut window = projection_window(period.start_at_ms, period.end_at_ms, i64::MAX);
        window.reward_delay_ms = period.reward_end_at_ms.saturating_sub(period.end_at_ms);
        window.quest_times = QuestTimes::Relative;
        window.regional_period = Some(period);
        windows.insert(activity_id.clone(), window);
    }
    Ok(())
}
// //// /选择虚拟日期所在的原始区域期别 ////

// //// 以原始字段时间投影活动根记录与任务阶段 [@x380kkm 2026-09-27] ////
pub(super) fn project_master(value: &mut OrderedValue, name: &str, period: &RegionalPeriod) {
    let Some(rows) = period.masters.get(name) else {
        return;
    };
    for (path, fields) in rows {
        let Some(row) = row_at_path(value, path) else {
            continue;
        };
        for (index, time) in fields {
            if let Some(value) = row.get_mut(*index) {
                *value = time.clone();
            }
        }
    }
}

fn row_at_path<'a>(value: &'a mut OrderedValue, path: &str) -> Option<&'a mut Vec<String>> {
    let mut current = value;
    for key in path.split('/') {
        let OrderedValue::Map(entries) = current else {
            return None;
        };
        current = &mut entries.iter_mut().find(|(id, _)| id == key)?.1;
    }
    match current {
        OrderedValue::Row(row) => Some(row),
        OrderedValue::Map(_) => None,
    }
}
// //// /以原始字段时间投影活动根记录与任务阶段 ////

#[cfg(test)]
mod tests;
