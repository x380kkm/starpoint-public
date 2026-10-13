// audience: internal
// # activity-mission-windows
// 关联活动任务的参与和展示时间分别沿用原始阶段, 管理期别和区域期别使用同一时间投影.

use super::{
    active_windows_at, format_master_time, master_seed, matches_activity_filter,
    ordered_map::decode_ordered_map, parse_master_time, projection_manifest,
    quest_windows::QuestTimes, OrderedValue, ProjectionMaster, ProjectionWindow, OPEN_RULE_END_MS,
};
use crate::{database::ServiceDatabase, PersonalServiceError};
use std::{collections::BTreeMap, sync::OnceLock};

struct Mission {
    activity_id: String,
    fields: [Option<i64>; 4],
}

struct Catalog {
    master: &'static ProjectionMaster,
    time_index: usize,
    missions: BTreeMap<i64, Mission>,
    events: BTreeMap<String, [i64; 2]>,
}

fn catalog(name: &str) -> Result<&'static Catalog, PersonalServiceError> {
    static VALUE: OnceLock<Result<BTreeMap<&'static str, Catalog>, String>> = OnceLock::new();
    let catalogs = VALUE
        .get_or_init(|| {
            let manifest = projection_manifest().map_err(|error| error.to_string())?;
            [
                ("collect_item_event_mission", "collect_item_event"),
                ("event_mission", "raid_event"),
            ]
            .into_iter()
            .map(|(name, parent)| {
                let master = manifest
                    .masters
                    .iter()
                    .find(|master| master.name == name)
                    .ok_or_else(|| format!("missing mission projection: {name}"))?;
                let parent = manifest
                    .masters
                    .iter()
                    .find(|master| master.name == parent)
                    .ok_or_else(|| format!("missing mission parent: {parent}"))?;
                Ok((name, read_catalog(master, parent)?))
            })
            .collect()
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    catalogs
        .get(name)
        .ok_or_else(|| PersonalServiceError::new(format!("unknown mission projection: {name}")))
}

// //// 按主表关联和任务类别读取原始任务阶段 [@x380kkm 2026-09-27] ////
fn read_catalog(
    master: &'static ProjectionMaster,
    parent: &ProjectionMaster,
) -> Result<Catalog, String> {
    let mut events = BTreeMap::new();
    let mut missions = BTreeMap::new();
    let event_start = parent
        .start_index
        .ok_or("mission parent start index is missing")?;
    let event_end = parent
        .end_index
        .ok_or("mission parent end index is missing")?;
    let time_index = master.start_index.ok_or("mission start index is missing")?;
    let activity_index = master
        .activity_id_index
        .ok_or("mission activity index is missing")?;
    if let OrderedValue::Map(rows) =
        decode_ordered_map(master_seed(&parent.name).ok_or("mission parent seed is missing")?)?
    {
        for (id, value) in rows {
            if let OrderedValue::Row(row) = value {
                if let Some((start, end)) =
                    parse_master_time(&row[event_start]).zip(parse_master_time(&row[event_end]))
                {
                    events.insert(format!("{}{id}", master.activity_id_prefix), [start, end]);
                }
            }
        }
    }
    if let OrderedValue::Map(rows) =
        decode_ordered_map(master_seed(&master.name).ok_or("mission seed is missing")?)?
    {
        for (id, value) in rows {
            let OrderedValue::Row(row) = value else {
                continue;
            };
            if !matches_activity_filter(master, &row) {
                continue;
            }
            let activity_id = format!("{}{}", master.activity_id_prefix, row[activity_index]);
            if !events.contains_key(&activity_id) {
                continue;
            }
            missions.insert(
                id.parse().map_err(|_| "invalid activity mission id")?,
                Mission {
                    activity_id,
                    fields: std::array::from_fn(|index| {
                        parse_master_time(&row[time_index + index])
                    }),
                },
            );
        }
    }
    Ok(Catalog {
        master,
        time_index,
        missions,
        events,
    })
}
// //// /按主表关联和任务类别读取原始任务阶段 ////

// //// 分别投影任务参与与展示字段 [@x380kkm 2026-09-27] ////
fn fields(
    catalog: &Catalog,
    id: i64,
    mission: &Mission,
    window: Option<&ProjectionWindow>,
) -> [Option<i64>; 4] {
    let Some(window) = window else {
        return mission.fields;
    };
    if let Some(period) = window.regional_period {
        if let Some(fields) = period.master_fields(&catalog.master.name, &id.to_string()) {
            return std::array::from_fn(|index| {
                fields
                    .get(&(index + catalog.time_index))
                    .and_then(|value| parse_master_time(value))
            });
        }
        return mission.fields;
    }
    if !matches!(window.quest_times, QuestTimes::Relative) {
        return [
            Some(window.start_at_ms),
            Some(window.end_at_ms),
            Some(window.start_at_ms),
            Some(window.end_at_ms),
        ];
    }
    let Some([source_start, source_end]) = catalog.events.get(&mission.activity_id) else {
        return mission.fields;
    };
    std::array::from_fn(|index| {
        mission.fields[index].map(|original| {
            let projected = if index % 2 == 1 && original >= *source_end {
                window
                    .end_at_ms
                    .saturating_add(original.saturating_sub(*source_end))
            } else {
                window
                    .start_at_ms
                    .saturating_add(original.saturating_sub(*source_start).max(0))
            };
            projected
                .max(window.start_at_ms)
                .min(window.end_at_ms.saturating_add(window.reward_delay_ms))
        })
    })
}

pub(super) fn project(
    value: &mut OrderedValue,
    name: &str,
    windows: &BTreeMap<String, ProjectionWindow>,
) -> Result<(), PersonalServiceError> {
    let catalog = catalog(name)?;
    let OrderedValue::Map(entries) = value else {
        return Ok(());
    };
    for (id, value) in entries {
        let Some(id) = id.parse::<i64>().ok() else {
            continue;
        };
        let Some(mission) = catalog.missions.get(&id) else {
            continue;
        };
        let Some(window) = windows.get(&mission.activity_id) else {
            continue;
        };
        let OrderedValue::Row(row) = value else {
            continue;
        };
        for (index, time) in fields(catalog, id, mission, Some(window))
            .iter()
            .enumerate()
        {
            row[index + catalog.time_index] =
                time.map_or_else(|| "(None)".to_owned(), format_master_time);
        }
    }
    Ok(())
}
// //// /分别投影任务参与与展示字段 ////

// //// 读取或捕获当前任务阶段的参与时间 [@x380kkm 2026-09-27] ////
pub(crate) fn collect_mission_window(
    database: &ServiceDatabase,
    mission_id: i64,
    at_ms: i64,
) -> Result<Option<[i64; 2]>, PersonalServiceError> {
    mission_window(
        catalog("collect_item_event_mission")?,
        database,
        mission_id,
        at_ms,
    )
}

pub(crate) fn event_mission_window(
    database: &ServiceDatabase,
    mission_id: i64,
    at_ms: i64,
) -> Result<Option<[i64; 2]>, PersonalServiceError> {
    mission_window(catalog("event_mission")?, database, mission_id, at_ms)
}

fn mission_window(
    catalog: &Catalog,
    database: &ServiceDatabase,
    mission_id: i64,
    at_ms: i64,
) -> Result<Option<[i64; 2]>, PersonalServiceError> {
    let Some(mission) = catalog.missions.get(&mission_id) else {
        return Ok(None);
    };
    let windows = active_windows_at(database, at_ms)?;
    let fields = fields(
        catalog,
        mission_id,
        mission,
        windows.get(&mission.activity_id),
    );
    Ok(Some([
        fields[0].unwrap_or_default(),
        fields[1].unwrap_or(OPEN_RULE_END_MS),
    ]))
}

pub(crate) fn collect_mission_windows(
    database: &ServiceDatabase,
    at_ms: i64,
) -> Result<BTreeMap<i64, [i64; 2]>, PersonalServiceError> {
    mission_windows(catalog("collect_item_event_mission")?, database, at_ms)
}

pub(crate) fn event_mission_windows(
    database: &ServiceDatabase,
    at_ms: i64,
) -> Result<BTreeMap<i64, [i64; 2]>, PersonalServiceError> {
    mission_windows(catalog("event_mission")?, database, at_ms)
}

fn mission_windows(
    catalog: &Catalog,
    database: &ServiceDatabase,
    at_ms: i64,
) -> Result<BTreeMap<i64, [i64; 2]>, PersonalServiceError> {
    let windows = active_windows_at(database, at_ms)?;
    Ok(catalog
        .missions
        .iter()
        .filter_map(|(id, mission)| {
            let fields = fields(catalog, *id, mission, windows.get(&mission.activity_id));
            let period = [
                fields[0].unwrap_or_default(),
                fields[1].unwrap_or(OPEN_RULE_END_MS),
            ];
            (at_ms >= period[0] && at_ms <= period[1]).then_some((*id, period))
        })
        .collect())
}
// //// /读取或捕获当前任务阶段的参与时间 ////

#[cfg(test)]
mod tests;
