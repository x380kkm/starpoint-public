// audience: internal
// # personal-service-activity-projection
//
// 该模块把活动管理开放规则投影到 CN 客户端 orderedmap. 可写差分归档,
// EntityLists 和 path 清单使用同一资源版本.

mod bundle_upgrade;
mod collect_rewards;
mod companion_windows;
mod event_currency_display;
mod event_currency_rewards;
mod event_shop;
mod mission_windows;
mod regional;
pub(crate) use event_shop::{managed_item_periods, ItemPeriods};
pub(crate) use mission_windows::{
    collect_mission_window, collect_mission_windows, event_mission_window, event_mission_windows,
};
pub(crate) use regional::{
    effective_regional_activity_period, regional_activity_period, regional_activity_period_starts,
    regional_activity_window, regional_shop_activity_alias,
};
mod ordered_map;
mod player_history;
mod projection_files;
mod quest_windows;
mod rush_rewards;
pub(crate) use quest_windows::{
    default_quest_window, is_quest_open, management_definitions, open_quest_keys,
};
mod tower_dungeon;
mod zip_archive;

#[cfg(test)]
mod box_gacha_tests;
#[cfg(test)]
mod collect_item_tests;
#[cfg(test)]
mod exchange_tests;

#[cfg(test)]
mod boss_release_tests;
#[cfg(test)]
mod character_election_tests;
#[cfg(test)]
mod upgrade_tests;

use bundle_upgrade::{append_diff, highest_version, read_bundle_manifest, upgrade_path};
use ordered_map::{decode_ordered_map, encode_ordered_map};
use projection_files::{atomic_write, atomic_write_json, master_seed};
use zip_archive::build_zip;

use crate::cn_tutorial::format_client_time;
use crate::database::{evaluate_activity_schedule, ServiceDatabase};
use crate::PersonalServiceError;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

const PROJECTION_MANIFEST: &str = include_str!("../assets/cn-activity-master-projection.json");
const PROJECTION_STATE_NAME: &str = "activity-projection.json";
const PROJECTION_ARCHIVE_PREFIX: &str = "starpoint-cn-activity-projection-";
const COMMON_DIFF_DIRECTORY: &str = "archive-common-diff";
const MILLIS_PER_HOUR: i64 = 3_600_000;
const MILLIS_PER_DAY: i64 = 86_400_000;
const MASTER_TIMEZONE_OFFSET_MS: i64 = 8 * MILLIS_PER_HOUR;
pub(crate) const AGGREGATION_DELAY_MS: i64 = 3 * MILLIS_PER_DAY;
pub(crate) const REWARD_DELAY_MS: i64 = 10 * MILLIS_PER_DAY;
const OPEN_RULE_START_MS: i64 = 946_684_800_000;
const OPEN_RULE_END_MS: i64 = 4_102_444_799_000;

#[derive(Debug, Deserialize)]
struct ProjectionManifest {
    format_version: u32,
    masters: Vec<ProjectionMaster>,
    #[serde(skip)]
    seed_signature: String,
}

#[derive(Debug, Deserialize)]
struct ProjectionMaster {
    name: String,
    activity_id_prefix: String,
    entry_path: String,
    binary_path: String,
    start_index: Option<usize>,
    end_index: Option<usize>,
    activity_id_index: Option<usize>,
    activity_id_filter: Option<(usize, String)>,
    quest_category: Option<i64>,
    #[serde(default)]
    synchronized_field_indexes: Vec<usize>,
    #[serde(default)]
    composite_schedule_indexes: Vec<usize>,
    #[serde(default)]
    composite_schedules: Vec<CompositeSchedule>,
}

#[derive(Debug, Deserialize)]
struct CompositeSchedule {
    index: usize,
    separator: String,
    #[serde(default)]
    components: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProjectionState {
    signature: Option<String>,
    base_signature: Option<String>,
    target_version: Option<String>,
    archive_name: Option<String>,
    #[serde(default)]
    touched_master_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct ProjectionWindow {
    start_at_ms: i64,
    end_at_ms: i64,
    expires_at_ms: i64,
    reward_delay_ms: i64,
    tower_quest_ids: Option<[i64; 10]>,
    quest_times: quest_windows::QuestTimes,
    regional_period: Option<&'static regional::RegionalPeriod>,
    follows_main: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OrderedValue {
    Map(Vec<(String, OrderedValue)>),
    Row(Vec<String>),
}

#[derive(Debug, Clone, Copy)]
enum TimeKind {
    Start,
    End,
    Aggregation,
    Reward,
}

static MANIFEST: OnceLock<Result<ProjectionManifest, String>> = OnceLock::new();

// //// 读取并校验活动 master 投影清单 [@x380kkm 2026-08-29] ////
fn projection_manifest() -> Result<&'static ProjectionManifest, PersonalServiceError> {
    MANIFEST
        .get_or_init(|| {
            let mut manifest = serde_json::from_str::<ProjectionManifest>(PROJECTION_MANIFEST)
                .map_err(|error| format!("failed to decode CN activity projection: {error}"))?;
            manifest
                .masters
                .push(quest_windows::boss_master().map_err(|error| error.to_string())?);
            if manifest.format_version != 1 || manifest.masters.is_empty() {
                return Err("CN activity projection has an invalid format".to_owned());
            }
            let mut digest = Sha256::new();
            for master in &manifest.masters {
                let seed = master_seed(&master.name).ok_or_else(|| {
                    format!("CN activity projection seed is missing: {}", master.name)
                })?;
                digest.update(master.name.as_bytes());
                digest.update([0]);
                digest.update(seed);
            }
            manifest.seed_signature = URL_SAFE_NO_PAD.encode(digest.finalize());
            Ok(manifest)
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}
// //// /读取并校验活动 master 投影清单 ////

// //// 同步 CN 活动时间差分 [@x380kkm 2026-08-29] ////
pub(crate) fn sync(
    database: &ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
) -> Result<(), PersonalServiceError> {
    if !asset_root.join("path").is_file() || !has_entity_manifest(asset_root) {
        return Ok(());
    }
    let manifest = projection_manifest()?;
    let mut state = read_state(override_root)?;
    let mut windows = active_windows(database)?;
    windows.retain(|activity_id, _| {
        manifest
            .masters
            .iter()
            .any(|master| master_has_schedule(master) && matches_master(master, activity_id))
    });
    let mut touched = state
        .touched_master_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    touched.insert(quest_windows::boss_master()?.name);
    touched.insert(event_currency_rewards::QUEST_MASTER_NAME.to_owned());
    touched.extend(event_currency_display::master_names()?.cloned());
    for activity_id in windows.keys() {
        for master in &manifest.masters {
            if master_has_schedule(master) && matches_master(master, activity_id) {
                touched.insert(master.name.clone());
            }
        }
    }
    let bundle = read_bundle_manifest(asset_root)?;
    let bundle_changed = state.base_signature.as_deref() != Some(bundle.signature.as_str());
    let signature = projection_signature(&windows, &touched, &bundle.signature, manifest);
    if state.signature.as_deref() == Some(signature.as_str())
        && override_root.join("path").is_file()
        && state.archive_name.as_deref().is_some_and(|name| {
            override_root
                .join(COMMON_DIFF_DIRECTORY)
                .join(name)
                .is_file()
        })
    {
        return Ok(());
    }

    let mut projected_entries = vec![
        rush_rewards::projected_entry()?,
        collect_rewards::projected_entry()?,
    ];
    projected_entries.extend(event_currency_rewards::projected_entries()?);
    for master in &manifest.masters {
        if !touched.contains(&master.name) || !master_has_schedule(master) {
            continue;
        }
        let seed = master_seed(&master.name).ok_or_else(|| {
            PersonalServiceError::new(format!(
                "CN activity projection seed is missing: {}",
                master.name
            ))
        })?;
        if !master
            .binary_path
            .ends_with(&format!("{}.orderedmap", master.name))
        {
            return Err(PersonalServiceError::new(
                "CN activity projection binary path is inconsistent",
            ));
        }
        let mut value = decode_ordered_map(seed).map_err(PersonalServiceError::new)?;
        if master.name == "event_item_shop" {
            event_shop::project(&mut value, &windows)?;
        }
        for (activity_id, window) in &windows {
            if let Some(period) = window.regional_period {
                regional::project_master(&mut value, &master.name, period);
                continue;
            }
            if activity_id.starts_with("quest:") {
                continue;
            }
            let Some(activity_key) = activity_suffix(master, activity_id) else {
                continue;
            };
            project_master_value(&mut value, master, activity_key, window);
        }
        quest_windows::project_quests(&mut value, master, &windows)?;
        event_currency_display::project(&mut value, &master.name)?;
        if master.name == event_currency_rewards::QUEST_MASTER_NAME {
            event_currency_rewards::project_quests(&mut value)?;
        }
        if matches!(
            master.name.as_str(),
            "collect_item_event_mission" | "event_mission"
        ) {
            mission_windows::project(&mut value, &master.name, &windows)?;
        }
        projected_entries.push((
            master.entry_path.clone(),
            encode_ordered_map(&value).map_err(PersonalServiceError::new)?,
        ));
    }

    let upgraded = upgrade_path(&bundle, override_root, bundle_changed)?;
    let mut path_manifest = upgraded.manifest;
    let original_version = highest_version(&path_manifest)?;
    let target_version = increment_version(&original_version)?;
    let archive_name =
        format!("{PROJECTION_ARCHIVE_PREFIX}{original_version}-{target_version}-overlay.zip");
    let archive_data = build_zip(&projected_entries).map_err(PersonalServiceError::new)?;
    let archive_relative = format!("{COMMON_DIFF_DIRECTORY}/{archive_name}");
    append_path_diff(
        &mut path_manifest,
        &original_version,
        &target_version,
        &archive_relative,
        &archive_data,
    )?;

    let archive_path = override_root.join(&archive_relative);
    atomic_write(&archive_path, &archive_data)?;
    synchronize_entity_manifests(
        asset_root,
        override_root,
        &target_version,
        &projected_entries,
        bundle_changed,
        upgraded
            .repair_version
            .as_deref()
            .unwrap_or(&target_version),
    )?;
    atomic_write_json(&override_root.join("path"), &path_manifest)?;

    state.signature = Some(signature);
    state.base_signature = Some(bundle.signature);
    state.target_version = Some(target_version);
    state.archive_name = Some(archive_name);
    state.touched_master_names = touched.into_iter().collect();
    atomic_write_json(&override_root.join(PROJECTION_STATE_NAME), &state)
}
// //// /同步 CN 活动时间差分 ////

// //// 判断 CDN 根是否包含可更新的实体清单 [@x380kkm 2026-08-29] ////
fn has_entity_manifest(asset_root: &Path) -> bool {
    ["entities", "EntityLists"].iter().any(|directory| {
        let root = asset_root.join(directory);
        root.join("PathFile.csv").is_file()
            || root.join("10939-ios_medium.csv").is_file()
            || root.join("10939-android_medium.csv").is_file()
    })
}
// //// /判断 CDN 根是否包含可更新的实体清单 ////

// //// 读取当前开放规则对应的虚拟活动窗口 [@x380kkm 2026-08-29] ////
fn active_windows(
    database: &ServiceDatabase,
) -> Result<BTreeMap<String, ProjectionWindow>, PersonalServiceError> {
    Ok(activity_windows(database)?.0)
}

pub(crate) fn hidden_daily_ids(
    database: &ServiceDatabase,
) -> Result<BTreeSet<String>, PersonalServiceError> {
    Ok(activity_windows(database)?.1)
}

fn activity_windows(
    database: &ServiceDatabase,
) -> Result<(BTreeMap<String, ProjectionWindow>, BTreeSet<String>), PersonalServiceError> {
    let virtual_now = database.current_server_time_millis()?;
    activity_windows_at(database, virtual_now)
}

fn active_windows_at(
    database: &ServiceDatabase,
    virtual_now: i64,
) -> Result<BTreeMap<String, ProjectionWindow>, PersonalServiceError> {
    Ok(activity_windows_at(database, virtual_now)?.0)
}

// //// 读取收集任务活动跟随主活动得到的窗口 [@x380kkm 2026-10-05] ////
pub(crate) fn companion_activity_windows(
    database: &ServiceDatabase,
    virtual_now: i64,
) -> Result<BTreeMap<String, (i64, i64)>, PersonalServiceError> {
    Ok(active_windows_at(database, virtual_now)?
        .into_iter()
        .filter(|(_, window)| window.follows_main)
        .map(|(activity_id, window)| (activity_id, (window.start_at_ms, window.end_at_ms)))
        .collect())
}

pub(crate) fn companion_activity_window(
    database: &ServiceDatabase,
    activity_id: &str,
    virtual_now: i64,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    if !companion_windows::is_companion(activity_id)? {
        return Ok(None);
    }
    Ok(companion_activity_windows(database, virtual_now)?.remove(activity_id))
}
// //// /读取收集任务活动跟随主活动得到的窗口 ////

fn activity_windows_at(
    database: &ServiceDatabase,
    virtual_now: i64,
) -> Result<(BTreeMap<String, ProjectionWindow>, BTreeSet<String>), PersonalServiceError> {
    let mut windows = BTreeMap::new();
    let schedules = database.list_activity_schedules()?;
    let mut configured_activity_ids = schedules
        .iter()
        .map(|schedule| schedule.activity_id.clone())
        .collect::<BTreeSet<_>>();
    for (activity_id, opened_at_ms, expires_at_ms) in
        database.list_activity_temporary_open_records()?
    {
        let reward_tail_ms = activity_reward_tail_ms(&activity_id)?;
        if database
            .activity_temporary_reward_window(&activity_id, reward_tail_ms)?
            .is_none()
        {
            continue;
        }
        configured_activity_ids.insert(activity_id.clone());
        let (start_at_ms, end_at_ms) =
            database.project_wall_time_window(opened_at_ms, expires_at_ms)?;
        let mut window = projection_window(start_at_ms, end_at_ms, expires_at_ms);
        window.quest_times = quest_windows::QuestTimes::Relative;
        windows.insert(activity_id, window);
    }
    let mut disabled_activity_ids = BTreeSet::new();
    for schedule in &schedules {
        if windows.contains_key(&schedule.activity_id) {
            continue;
        }
        let mut visible_schedule = schedule.clone();
        let reward_tail_ms = activity_reward_tail_ms(&schedule.activity_id)?;
        visible_schedule.end_at_ms = visible_schedule.end_at_ms.saturating_add(reward_tail_ms);
        let evaluation = evaluate_activity_schedule(&visible_schedule, virtual_now);
        let window = if !schedule.enabled {
            disabled_activity_ids.insert(schedule.activity_id.clone());
            quest_windows::closed_window()
        } else if matches!(
            schedule.mode,
            crate::database::ActivityMode::Always | crate::database::ActivityMode::Manual
        ) {
            projection_window(OPEN_RULE_START_MS, OPEN_RULE_END_MS, i64::MAX)
        } else {
            let (start_at_ms, end_at_ms) = evaluation
                .active_start_ms
                .zip(evaluation.active_end_ms)
                .or_else(|| evaluation.next_start_ms.zip(evaluation.next_end_ms))
                .map(|(start, end)| (start, end.saturating_sub(reward_tail_ms)))
                .unwrap_or((schedule.start_at_ms, schedule.end_at_ms));
            let mut window = projection_window(start_at_ms, end_at_ms, i64::MAX);
            window.quest_times = quest_windows::QuestTimes::Relative;
            window
        };
        windows.insert(schedule.activity_id.clone(), window);
    }
    companion_windows::apply(&mut windows, &mut configured_activity_ids, &disabled_activity_ids)?;
    regional::apply_periods(&mut windows, &configured_activity_ids, virtual_now)?;
    let hidden_daily_ids =
        quest_windows::apply_daily_coverage(&mut windows, &configured_activity_ids, virtual_now)?;
    quest_windows::apply_permanent_challenge(&mut windows);
    for (activity_id, window) in &mut windows {
        if let Some(delay) = exchange_reward_period_millis(activity_id)? {
            window.reward_delay_ms = delay;
        }
        if let Some(event_id) = activity_id
            .strip_prefix("tower-dungeon:")
            .and_then(|id| id.parse().ok())
        {
            window.tower_quest_ids =
                crate::cn_tower_dungeon::quest_ids_for_time(event_id, virtual_now)?;
        }
    }
    Ok((windows, hidden_daily_ids))
}
// //// /读取当前开放规则对应的虚拟活动窗口 ////

// //// 读取兑换和通行证主表的领奖期长度 [@x380kkm 2026-09-25] ////
fn exchange_reward_period_millis(activity_id: &str) -> Result<Option<i64>, PersonalServiceError> {
    if let Some(delay) = crate::cn_optional_exchange::reward_period_millis(activity_id)? {
        return Ok(Some(delay));
    }
    if let Some(delay) = crate::cn_multi_special_exchange::reward_period_millis(activity_id)? {
        return Ok(Some(delay));
    }
    crate::cn_pass_card::reward_period_millis(activity_id)
}

fn activity_reward_tail_ms(activity_id: &str) -> Result<i64, PersonalServiceError> {
    Ok(
        if let Some(delay) = exchange_reward_period_millis(activity_id)? {
            delay
        } else if activity_id.starts_with("ranking:")
            || activity_id.starts_with("active-mission:")
            || crate::cn_shop::has_activity_shop(activity_id)?
        {
            REWARD_DELAY_MS
        } else {
            0
        },
    )
}

fn projection_window(start_at_ms: i64, end_at_ms: i64, expires_at_ms: i64) -> ProjectionWindow {
    ProjectionWindow {
        start_at_ms: round_millis_to_second(start_at_ms),
        end_at_ms: round_millis_to_second(end_at_ms),
        expires_at_ms: round_millis_to_second(expires_at_ms),
        reward_delay_ms: REWARD_DELAY_MS,
        tower_quest_ids: None,
        quest_times: quest_windows::QuestTimes::Open,
        regional_period: None,
        follows_main: false,
    }
}

fn round_millis_to_second(value: i64) -> i64 {
    value.saturating_div(1_000).saturating_mul(1_000)
}

fn master_has_schedule(master: &ProjectionMaster) -> bool {
    !master.synchronized_field_indexes.is_empty()
        && (master.start_index.is_some()
            || master.end_index.is_some()
            || !master.composite_schedule_indexes.is_empty())
}

fn matches_master(master: &ProjectionMaster, activity_id: &str) -> bool {
    quest_windows::matches_master(master, activity_id)
        || event_shop::matches_master(master, activity_id)
}

fn activity_suffix<'a>(master: &ProjectionMaster, activity_id: &'a str) -> Option<&'a str> {
    activity_id.strip_prefix(&master.activity_id_prefix)
}

// //// 投影目标活动及关联行 [@x380kkm 2026-09-21] ////
fn project_master_value(
    value: &mut OrderedValue,
    master: &ProjectionMaster,
    activity_key: &str,
    window: &ProjectionWindow,
) -> bool {
    if master.name == "tower_dungeon_event_quest" {
        return window.tower_quest_ids.as_ref().is_some_and(|quest_ids| {
            tower_dungeon::project_quests(value, master, activity_key, window, quest_ids)
        });
    }
    if let Some(index) = master.activity_id_index {
        let OrderedValue::Map(entries) = value else {
            return false;
        };
        let mut changed = false;
        for (_, value) in entries {
            if let OrderedValue::Row(row) = value {
                if row
                    .get(index)
                    .is_some_and(|event_id| event_id == activity_key)
                    && matches_activity_filter(master, row)
                {
                    changed |= mutate_row(row, master, window);
                }
            }
        }
        return changed;
    }
    let target = match value {
        OrderedValue::Map(entries) => entries
            .iter_mut()
            .find(|(key, _)| key == activity_key)
            .map(|(_, value)| value),
        OrderedValue::Row(_) => None,
    };
    target.is_some_and(|value| mutate_rows(value, master, window))
}
// //// /投影目标活动及关联行 ////

fn matches_activity_filter(master: &ProjectionMaster, row: &[String]) -> bool {
    master
        .activity_id_filter
        .as_ref()
        .map_or(true, |(index, value)| row.get(*index) == Some(value))
}

fn mutate_rows(
    value: &mut OrderedValue,
    master: &ProjectionMaster,
    window: &ProjectionWindow,
) -> bool {
    match value {
        OrderedValue::Row(row) => mutate_row(row, master, window),
        OrderedValue::Map(entries) => {
            let mut changed = false;
            for (_, value) in entries {
                changed |= mutate_rows(value, master, window);
            }
            changed
        }
    }
}

fn mutate_row(row: &mut [String], master: &ProjectionMaster, window: &ProjectionWindow) -> bool {
    if master.name == "player_history" {
        return player_history::project_period(row, window);
    }
    let mut changed = false;
    if master.name == "daily_week_event" {
        for day in &mut row[8..15] {
            changed |= day != "true";
            *day = "true".to_owned();
        }
    }
    for &index in &master.synchronized_field_indexes {
        if index >= row.len() {
            continue;
        }
        if master.composite_schedule_indexes.contains(&index) {
            let Some(schedule) = master
                .composite_schedules
                .iter()
                .find(|schedule| schedule.index == index)
            else {
                continue;
            };
            changed |= rewrite_composite(&mut row[index], schedule, window);
            continue;
        }
        let Some(kind) = scalar_time_kind(master, index) else {
            continue;
        };
        let replacement = projected_time(kind, window);
        if row[index] != replacement {
            row[index] = replacement;
            changed = true;
        }
    }
    changed
}

fn rewrite_composite(
    value: &mut String,
    schedule: &CompositeSchedule,
    window: &ProjectionWindow,
) -> bool {
    if schedule.components.is_empty() {
        return false;
    }
    let mut parts = value
        .split(schedule.separator.as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if parts.len() < 2 || parts.iter().any(|part| !is_time_placeholder(part)) {
        return false;
    }
    parts.resize(
        parts.len().max(schedule.components.len()),
        "(None)".to_owned(),
    );
    for (index, part) in parts.iter_mut().enumerate() {
        let kind = match index {
            0 => TimeKind::Start,
            1 => TimeKind::End,
            2 => TimeKind::Aggregation,
            _ => TimeKind::Reward,
        };
        *part = projected_time(kind, window);
    }
    let replacement = parts.join(schedule.separator.as_str());
    if *value == replacement {
        false
    } else {
        *value = replacement;
        true
    }
}

fn scalar_time_kind(master: &ProjectionMaster, index: usize) -> Option<TimeKind> {
    match master.name.as_str() {
        "collect_item_event_mission" => match index {
            27 | 29 => Some(TimeKind::Start),
            28 | 30 => Some(TimeKind::End),
            _ => None,
        },
        "event_mission" => match index {
            25 | 27 => Some(TimeKind::Start),
            26 | 28 => Some(TimeKind::End),
            _ => None,
        },
        "ranking_event" => match index {
            11 => Some(TimeKind::Aggregation),
            12 => Some(TimeKind::Reward),
            18 => Some(TimeKind::Start),
            19 | 20 => Some(TimeKind::End),
            _ => None,
        },
        "active_mission_event" => match index {
            14 => Some(TimeKind::Start),
            16 => Some(TimeKind::End),
            17 => Some(TimeKind::Aggregation),
            15 => Some(TimeKind::Reward),
            _ => None,
        },
        "rush_event" => match index {
            15 => Some(TimeKind::Start),
            16 => Some(TimeKind::End),
            17 => Some(TimeKind::Reward),
            _ => None,
        },
        "raid_event" => match index {
            19 | 24 => Some(TimeKind::Reward),
            22 => Some(TimeKind::Start),
            23 => Some(TimeKind::End),
            _ => None,
        },
        "multi_special_exchange_campaign" => match index {
            3 => Some(TimeKind::Start),
            4 => Some(TimeKind::End),
            5 => Some(TimeKind::Reward),
            _ => None,
        },
        "special_exchange_campaign" | "treasure_vault_exchange_campaign" => match index {
            4 | 6 => Some(TimeKind::Start),
            5 => Some(TimeKind::End),
            7 => Some(TimeKind::Reward),
            _ => None,
        },
        _ if master.start_index == Some(index) => Some(TimeKind::Start),
        _ if master.end_index == Some(index) => Some(TimeKind::End),
        _ if master.end_index.is_some_and(|end_index| index > end_index) => Some(TimeKind::Reward),
        _ => None,
    }
}

fn projected_time(kind: TimeKind, window: &ProjectionWindow) -> String {
    let timestamp = match kind {
        TimeKind::Start => window.start_at_ms,
        TimeKind::End => window.end_at_ms,
        TimeKind::Aggregation => window.end_at_ms.saturating_add(AGGREGATION_DELAY_MS),
        TimeKind::Reward => window.end_at_ms.saturating_add(window.reward_delay_ms),
    };
    format_master_time(timestamp)
}

fn format_master_time(timestamp: i64) -> String {
    format_client_time(
        timestamp
            .saturating_add(MASTER_TIMEZONE_OFFSET_MS)
            .saturating_div(1_000),
    )
}

fn parse_master_time(value: &str) -> Option<i64> {
    crate::database::parse_iso_timestamp(&format!("{}Z", value.replace(' ', "T")))
        .map(|timestamp| timestamp.saturating_sub(MASTER_TIMEZONE_OFFSET_MS))
}

fn is_time_placeholder(value: &str) -> bool {
    value.is_empty()
        || value == "(None)"
        || (value.len() == 19
            && value.bytes().enumerate().all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                10 => byte == b' ',
                13 | 16 => byte == b':',
                _ => byte.is_ascii_digit(),
            }))
}

fn projection_signature(
    windows: &BTreeMap<String, ProjectionWindow>,
    touched: &BTreeSet<String>,
    bundle_signature: &str,
    manifest: &ProjectionManifest,
) -> String {
    let mut digest = Sha256::new();
    digest.update(PROJECTION_MANIFEST.as_bytes());
    digest.update(quest_windows::CATALOG.as_bytes());
    digest.update(regional::CATALOG.as_bytes());
    digest.update(rush_rewards::CATALOG.as_bytes());
    digest.update(collect_rewards::CATALOG.as_bytes());
    digest.update(event_currency_rewards::CATALOG.as_bytes());
    digest.update(event_currency_display::CATALOG.as_bytes());
    digest.update(bundle_signature.as_bytes());
    digest.update(manifest.seed_signature.as_bytes());
    for value in [
        MASTER_TIMEZONE_OFFSET_MS,
        AGGREGATION_DELAY_MS,
        REWARD_DELAY_MS,
    ] {
        digest.update(value.to_le_bytes());
    }
    for master in &manifest.masters {
        digest.update(master.name.as_bytes());
        for &index in &master.synchronized_field_indexes {
            digest.update(format!("{index}:{:?};", scalar_time_kind(master, index)).as_bytes());
        }
    }
    for name in touched {
        digest.update(name.as_bytes());
        digest.update([0]);
    }
    for (activity_id, window) in windows {
        digest.update(activity_id.as_bytes());
        digest.update(window.start_at_ms.saturating_div(1_000).to_le_bytes());
        digest.update(window.end_at_ms.saturating_div(1_000).to_le_bytes());
        digest.update(window.expires_at_ms.saturating_div(1_000).to_le_bytes());
        digest.update(window.reward_delay_ms.to_le_bytes());
        digest.update(format!("{:?}", window.quest_times).as_bytes());
        if let Some(quest_ids) = window.tower_quest_ids {
            for quest_id in quest_ids {
                digest.update(quest_id.to_le_bytes());
            }
        }
    }
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

fn read_state(override_root: &Path) -> Result<ProjectionState, PersonalServiceError> {
    let path = override_root.join(PROJECTION_STATE_NAME);
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to decode CN activity projection state: {error}"
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ProjectionState::default())
        }
        Err(error) => Err(PersonalServiceError::new(format!(
            "failed to read CN activity projection state: {error}"
        ))),
    }
}

fn append_path_diff(
    manifest: &mut Value,
    original_version: &str,
    target_version: &str,
    archive_relative: &str,
    archive_data: &[u8],
) -> Result<(), PersonalServiceError> {
    append_diff(
        manifest,
        original_version,
        target_version,
        vec![json!({
                "location": archive_relative,
                "size": archive_data.len(),
                "sha256": STANDARD.encode(Sha256::digest(archive_data)),
        })],
    )
}

// //// 同步投影资源的 EntityLists 行 [@x380kkm 2026-08-29] ////
fn synchronize_entity_manifests(
    asset_root: &Path,
    override_root: &Path,
    target_version: &str,
    entries: &[(String, Vec<u8>)],
    bundle_changed: bool,
    repair_version: &str,
) -> Result<(), PersonalServiceError> {
    let directory_name = if asset_root.join("entities").is_dir() {
        "entities"
    } else {
        "EntityLists"
    };
    let destination = override_root.join(directory_name);
    fs::create_dir_all(&destination).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to create CN activity EntityLists directory: {error}"
        ))
    })?;
    for name in [
        "PathFile.csv",
        "10939-ios_medium.csv",
        "10939-android_medium.csv",
    ] {
        let override_path = destination.join(name);
        let bundle_path = asset_root.join(directory_name).join(name);
        let source_path = if (bundle_changed && bundle_path.is_file()) || !override_path.is_file() {
            bundle_path
        } else {
            override_path.clone()
        };
        if !source_path.is_file() {
            continue;
        }
        let mut source = fs::read_to_string(&source_path).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to read CN activity EntityLists manifest: {error}"
            ))
        })?;
        if bundle_changed && override_path.is_file() {
            let previous = fs::read_to_string(&override_path).map_err(|error| {
                PersonalServiceError::new(format!(
                    "failed to read CN override EntityLists: {error}"
                ))
            })?;
            source = bundle_upgrade::merge_entity_rows(&source, &previous, repair_version);
        }
        let rendered = render_entity_manifest(&source, target_version, entries)?;
        atomic_write(&override_path, rendered.as_bytes())?;
    }
    Ok(())
}
// //// /同步投影资源的 EntityLists 行 ////

fn render_entity_manifest(
    source: &str,
    target_version: &str,
    entries: &[(String, Vec<u8>)],
) -> Result<String, PersonalServiceError> {
    let replacements = entries
        .iter()
        .map(|(entry_path, bytes)| {
            (
                entry_path.as_str(),
                format!(
                    "{entry_path},{target_version},{},{},common",
                    bytes.len(),
                    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes)),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for line in source.lines() {
        let entry_path = line.split(',').next().unwrap_or_default();
        if let Some(replacement) = replacements.get(entry_path) {
            if !seen.insert(entry_path) {
                return Err(PersonalServiceError::new(
                    "CN activity EntityLists contains a duplicate entry",
                ));
            }
            lines.push(replacement.clone());
        } else {
            lines.push(line.to_owned());
        }
    }
    for (entry_path, replacement) in replacements {
        if !seen.contains(entry_path) {
            lines.push(replacement);
        }
    }
    Ok(format!("{}{newline}", lines.join(newline)))
}

fn parse_version(value: &str) -> Option<[u64; 3]> {
    let parts = value
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]])
}

fn increment_version(value: &str) -> Result<String, PersonalServiceError> {
    let [major, minor, patch] = parse_version(value)
        .ok_or_else(|| PersonalServiceError::new("CN asset version is invalid"))?;
    let patch = patch
        .checked_add(1)
        .ok_or_else(|| PersonalServiceError::new("CN asset version exceeds supported range"))?;
    Ok(format!("{major}.{minor}.{patch}"))
}

#[cfg(test)]
mod tests {
    use super::{
        active_windows, master_seed, project_master_value, projected_time, projection_manifest,
        projection_window, OrderedValue, TimeKind, OPEN_RULE_END_MS, OPEN_RULE_START_MS,
    };
    use crate::database::ServiceDatabase;
    use tempfile::TempDir;

    fn target_row<'a>(value: &'a OrderedValue, key: &str) -> Option<&'a Vec<String>> {
        let OrderedValue::Map(entries) = value else {
            return None;
        };
        let target = entries
            .iter()
            .find(|(entry_key, _)| entry_key == key)
            .map(|(_, value)| value)?;
        match target {
            OrderedValue::Row(row) => Some(row),
            OrderedValue::Map(entries) => entries.iter().find_map(|(_, value)| match value {
                OrderedValue::Row(row) => Some(row),
                OrderedValue::Map(_) => None,
            }),
        }
    }

    pub(super) fn collect_rows<'a>(value: &'a OrderedValue, rows: &mut Vec<&'a Vec<String>>) {
        match value {
            OrderedValue::Row(row) => rows.push(row),
            OrderedValue::Map(entries) => {
                for (_, value) in entries {
                    collect_rows(value, rows);
                }
            }
        }
    }

    fn rows_for_key<'a>(value: &'a OrderedValue, key: &str) -> Vec<&'a Vec<String>> {
        let OrderedValue::Map(entries) = value else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        if let Some((_, value)) = entries.iter().find(|(entry_key, _)| entry_key == key) {
            collect_rows(value, &mut rows);
        }
        rows
    }

    // //// 崩坏域基础关常驻且后续关卡按原始日期开放 [@x380kkm 2026-09-29] ////
    #[test]
    fn keeps_base_challenge_permanent_and_preserves_later_release_dates() {
        let root = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root.path()).unwrap();
        database
            .set_virtual_time(true, 1_575_259_260_000, 0.0001)
            .unwrap();
        for quest_id in 1001..=1019 {
            assert!(super::is_quest_open(&database, 13, quest_id).unwrap());
        }
        assert!(!super::is_quest_open(&database, 13, 1020).unwrap());
        assert!(!super::is_quest_open(&database, 13, 1040).unwrap());
        assert!(!super::is_quest_open(&database, 13, 2001).unwrap());
        database
            .set_virtual_time(true, 1_588_219_200_000, 0.0001)
            .unwrap();
        assert!(super::is_quest_open(&database, 13, 1001).unwrap());
        assert!(!super::is_quest_open(&database, 13, 1040).unwrap());
        database
            .set_virtual_time(true, 1_671_681_600_000, 0.0001)
            .unwrap();
        assert!(super::is_quest_open(&database, 13, 1020).unwrap());
        database
            .set_virtual_time(true, 1_672_286_400_000, 0.0001)
            .unwrap();
        assert!(!super::is_quest_open(&database, 13, 1020).unwrap());
        database
            .set_virtual_time(true, 1_698_292_800_000, 0.0001)
            .unwrap();
        assert!(super::is_quest_open(&database, 13, 1040).unwrap());
        database
            .set_virtual_time(true, 1_790_640_000_000, 0.0001)
            .unwrap();
        for quest_id in 1001..=1019 {
            assert!(super::is_quest_open(&database, 13, quest_id).unwrap());
        }
        assert!(super::is_quest_open(&database, 13, 1040).unwrap());
        assert!(!super::is_quest_open(&database, 13, 1020).unwrap());
    }

    #[test]
    fn projects_explicit_daily_and_challenge_windows() {
        let window = projection_window(OPEN_RULE_START_MS, OPEN_RULE_END_MS, i64::MAX);
        for (master_name, activity_key, expected_fields) in [
            (
                "daily_week_event",
                "19",
                &[
                    (16, TimeKind::Start),
                    (17, TimeKind::End),
                    (18, TimeKind::Reward),
                ][..],
            ),
            (
                "daily_exp_mana_event",
                "1",
                &[(9, TimeKind::Start), (10, TimeKind::End)][..],
            ),
            (
                "challenge_dungeon_event",
                "1",
                &[(13, TimeKind::Start), (14, TimeKind::End)][..],
            ),
            (
                "challenge_dungeon_event",
                "2",
                &[(13, TimeKind::Start), (14, TimeKind::End)][..],
            ),
        ] {
            let master = projection_manifest()
                .expect("projection manifest loads")
                .masters
                .iter()
                .find(|master| master.name == master_name)
                .expect("permanent activity master exists");
            let seed = master_seed(master_name).expect("permanent activity seed exists");
            let mut value = super::ordered_map::decode_ordered_map(seed)
                .expect("permanent activity seed decodes");
            assert!(project_master_value(
                &mut value,
                master,
                activity_key,
                &window
            ));
            let row = target_row(&value, activity_key).expect("permanent activity row exists");
            for (index, kind) in expected_fields {
                assert_eq!(row[*index], projected_time(*kind, &window));
            }
        }
    }

    #[test]
    fn preserves_historical_composite_schedules_without_field_meanings() {
        let master = projection_manifest()
            .expect("projection manifest loads")
            .masters
            .iter()
            .find(|master| master.name == "challenge_dungeon_event")
            .expect("challenge dungeon master exists");
        let seed =
            master_seed("challenge_dungeon_event").expect("challenge dungeon master seed exists");
        let mut value = super::ordered_map::decode_ordered_map(seed)
            .expect("challenge dungeon master seed decodes");
        let original_schedule =
            target_row(&value, "1").expect("challenge dungeon row exists")[2].clone();
        let window = projection_window(OPEN_RULE_START_MS, OPEN_RULE_END_MS, i64::MAX);

        assert!(project_master_value(&mut value, master, "1", &window));
        let projected = target_row(&value, "1").expect("projected challenge dungeon row exists");

        assert_eq!(projected[2], original_schedule);
        assert_eq!(projected[13], projected_time(TimeKind::Start, &window));
        assert_eq!(projected[14], projected_time(TimeKind::End, &window));
    }

    #[test]
    fn rewrites_declared_scalar_times_even_when_seed_has_historical_dates() {
        let master = projection_manifest()
            .expect("projection manifest loads")
            .masters
            .iter()
            .find(|master| master.name == "daily_week_event")
            .expect("daily week master exists");
        let seed = master_seed("daily_week_event").expect("daily week seed exists");
        let mut value =
            super::ordered_map::decode_ordered_map(seed).expect("daily week master decodes");
        let window = projection_window(1_700_000_000_000, 1_700_086_400_000, i64::MAX);

        assert!(project_master_value(&mut value, master, "1", &window));
        let row = target_row(&value, "1").expect("daily week row exists");
        assert_eq!(row[16], projected_time(TimeKind::Start, &window));
        assert_eq!(row[17], projected_time(TimeKind::End, &window));
        assert_eq!(row[18], projected_time(TimeKind::Reward, &window));
    }

    #[test]
    fn projects_managed_challenge_parent_and_quest_rows() {
        let window = projection_window(OPEN_RULE_START_MS, OPEN_RULE_END_MS, i64::MAX);
        for (master_name, expected_row_count, fields) in [
            (
                "challenge_dungeon_event",
                2,
                &[(13, TimeKind::Start), (14, TimeKind::End)][..],
            ),
            (
                "challenge_dungeon_event_quest",
                46,
                &[(5, TimeKind::Start), (6, TimeKind::End)][..],
            ),
        ] {
            let master = projection_manifest()
                .expect("projection manifest loads")
                .masters
                .iter()
                .find(|master| master.name == master_name)
                .expect("permanent activity master exists");
            let seed = master_seed(master_name).expect("permanent activity seed exists");
            let mut value = super::ordered_map::decode_ordered_map(seed)
                .expect("permanent activity seed decodes");
            let activity_keys = ["1", "2"];
            for activity_key in &activity_keys {
                assert!(project_master_value(
                    &mut value,
                    master,
                    activity_key,
                    &window
                ));
            }

            let rows = activity_keys
                .iter()
                .flat_map(|activity_key| rows_for_key(&value, activity_key))
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), expected_row_count);
            for row in rows {
                for (index, kind) in fields {
                    assert_eq!(row[*index], projected_time(*kind, &window));
                }
            }
        }
    }

    #[test]
    fn keeps_default_coverage_consistent_with_schedule_overrides() {
        let root = TempDir::new().expect("temporary service directory is created");
        let mut database = ServiceDatabase::open(root.path()).expect("service database opens");
        database
            .set_virtual_time(true, 1_575_259_260_000, 1.0)
            .expect("virtual time is set");
        database
            .upsert_activity_schedule("daily-week:1", true, 1_000, 2_000)
            .expect("ended daily schedule is stored");
        database
            .upsert_activity_schedule(
                "challenge-dungeon:1",
                false,
                1_575_259_000_000,
                1_575_260_000_000,
            )
            .expect("disabled challenge schedule is stored");
        database
            .create_activity_temporary_open_lease("challenge-dungeon:2")
            .expect("temporary challenge open lease is stored");

        let windows = active_windows(&database).expect("activity windows load");

        assert!(!windows.contains_key("daily-week:19"));
        assert!(!windows.contains_key("daily-exp-mana:1"));
        assert!(windows["daily-week:1"].end_at_ms <= 2_000);
        assert!(windows["challenge-dungeon:1"].end_at_ms <= 1_000);
        assert!(!super::quest_windows::is_quest_open(&database, 6, 17001).unwrap());
        assert!(super::quest_windows::is_quest_open(&database, 6, 13001).unwrap());
        let temporary_window = windows
            .get("challenge-dungeon:2")
            .expect("temporary challenge open lease projects");
        assert!(temporary_window.expires_at_ms < i64::MAX);

        let now_ms = database
            .current_server_time_millis()
            .expect("virtual server time loads");
        let daily_status = database
            .activity_window_status("daily-week:1", now_ms)
            .expect("ended daily status loads");
        assert_eq!(daily_status, crate::database::ActivityWindowStatus::Ended);
        let challenge_status = database
            .activity_window_status("challenge-dungeon:1", now_ms)
            .expect("disabled challenge status loads");
        assert_eq!(
            challenge_status,
            crate::database::ActivityWindowStatus::Disabled
        );
        let temporary_status = database
            .activity_window_status("challenge-dungeon:2", now_ms)
            .expect("temporary challenge status loads");
        assert_eq!(
            temporary_status,
            crate::database::ActivityWindowStatus::Open
        );
    }
}
