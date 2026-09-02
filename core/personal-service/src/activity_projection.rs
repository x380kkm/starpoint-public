// audience: internal
// # personal-service-activity-projection
//
// 该模块把活动管理开放规则投影到 CN 客户端 orderedmap. iOS 可写差分归档,
// EntityLists 和 path 清单使用同一资源版本.

mod ordered_map;
mod projection_files;
mod zip_archive;

use ordered_map::{decode_ordered_map, encode_ordered_map};
use projection_files::{atomic_write, atomic_write_json, master_seed};
use zip_archive::build_zip;

use crate::cn_tutorial::format_client_time;
use crate::database::{evaluate_activity_schedule, ActivityWindowStatus, ServiceDatabase};
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
const MASTER_TIMEZONE_OFFSET_MS: i64 = 9 * MILLIS_PER_HOUR;
const WINDOW_PADDING_MS: i64 = 60_000;
const AGGREGATION_DELAY_MS: i64 = 3 * MILLIS_PER_DAY;
const REWARD_DELAY_MS: i64 = 10 * MILLIS_PER_DAY;
const OPEN_RULE_START_MS: i64 = 946_684_800_000;
const OPEN_RULE_END_MS: i64 = 4_102_444_799_000;
const PERMANENT_ACTIVITY_MASTERS: [(&str, &str); 3] = [
    ("daily_week_event", "daily-week:"),
    ("daily_exp_mana_event", "daily-exp-mana:"),
    ("challenge_dungeon_event", "challenge-dungeon:"),
];

#[derive(Debug, Deserialize)]
struct ProjectionManifest {
    format_version: u32,
    masters: Vec<ProjectionMaster>,
}

#[derive(Debug, Deserialize)]
struct ProjectionMaster {
    name: String,
    activity_id_prefix: String,
    entry_path: String,
    binary_path: String,
    start_index: Option<usize>,
    end_index: Option<usize>,
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
            let manifest = serde_json::from_str::<ProjectionManifest>(PROJECTION_MANIFEST)
                .map_err(|error| format!("failed to decode CN activity projection: {error}"))?;
            if manifest.format_version != 1 || manifest.masters.is_empty() {
                return Err("CN activity projection has an invalid format".to_owned());
            }
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
        manifest.masters.iter().any(|master| {
            master_has_schedule(master) && activity_suffix(master, activity_id).is_some()
        })
    });
    let mut touched = state
        .touched_master_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for activity_id in windows.keys() {
        for master in &manifest.masters {
            if master_has_schedule(master) && activity_suffix(master, activity_id).is_some() {
                touched.insert(master.name.clone());
            }
        }
    }
    if touched.is_empty() {
        return Ok(());
    }

    let signature = projection_signature(&windows, &touched);
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

    let mut projected_entries = Vec::new();
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
        for (activity_id, window) in &windows {
            let Some(activity_key) = activity_suffix(master, activity_id) else {
                continue;
            };
            project_master_value(&mut value, master, activity_key, window);
        }
        projected_entries.push((
            master.entry_path.clone(),
            encode_ordered_map(&value).map_err(PersonalServiceError::new)?,
        ));
    }

    let mut path_manifest = read_active_path_manifest(asset_root, override_root)?;
    let original_version = path_manifest["info"]["target_asset_version"]
        .as_str()
        .and_then(|value| parse_version(value).map(|_| value.to_owned()))
        .ok_or_else(|| PersonalServiceError::new("CN asset path target version is invalid"))?;
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
    atomic_write_json(&override_root.join("path"), &path_manifest)?;
    synchronize_entity_manifests(
        asset_root,
        override_root,
        &target_version,
        &projected_entries,
    )?;

    state.signature = Some(signature);
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
    let wall_now = database.current_wall_time_millis()?;
    let virtual_now = database.current_server_time_millis()?;
    let rate = database.virtual_time_state()?.rate.max(0.000_001);
    let mut windows = BTreeMap::new();
    let schedules = database.list_activity_schedules()?;
    let mut configured_activity_ids = schedules
        .iter()
        .map(|schedule| schedule.activity_id.clone())
        .collect::<BTreeSet<_>>();
    for (activity_id, _) in database.list_active_activity_temporary_open_leases()? {
        let Some((opened_at_ms, expires_at_ms)) =
            database.activity_temporary_open_window(&activity_id)?
        else {
            continue;
        };
        configured_activity_ids.insert(activity_id.clone());
        let elapsed_virtual = scale_millis(wall_now.saturating_sub(opened_at_ms), rate);
        let remaining_virtual = scale_millis(expires_at_ms.saturating_sub(wall_now), rate);
        let start_at_ms = virtual_now
            .saturating_sub(elapsed_virtual)
            .saturating_sub(WINDOW_PADDING_MS);
        let end_at_ms = virtual_now
            .saturating_add(remaining_virtual)
            .max(start_at_ms.saturating_add(MILLIS_PER_HOUR));
        windows.insert(
            activity_id,
            projection_window(start_at_ms, end_at_ms, expires_at_ms),
        );
    }
    for schedule in &schedules {
        if windows.contains_key(&schedule.activity_id) {
            continue;
        }
        let evaluation = evaluate_activity_schedule(schedule, virtual_now);
        if evaluation.status != ActivityWindowStatus::Open {
            continue;
        }
        let (start_at_ms, end_at_ms) = match (evaluation.active_start_ms, evaluation.active_end_ms)
        {
            (Some(start_at_ms), Some(end_at_ms)) => (start_at_ms, end_at_ms),
            _ => (OPEN_RULE_START_MS, OPEN_RULE_END_MS),
        };
        windows.insert(
            schedule.activity_id.clone(),
            projection_window(start_at_ms, end_at_ms, i64::MAX),
        );
    }
    for activity_id in permanent_activity_ids()? {
        if configured_activity_ids.contains(activity_id.as_str()) {
            continue;
        }
        windows
            .entry(activity_id)
            .or_insert_with(|| projection_window(OPEN_RULE_START_MS, OPEN_RULE_END_MS, i64::MAX));
    }
    Ok(windows)
}
// //// /读取当前开放规则对应的虚拟活动窗口 ////

// //// 从活动 master seed 枚举长期活动 key [@x380kkm 2026-08-29] ////
fn permanent_activity_ids() -> Result<BTreeSet<String>, PersonalServiceError> {
    let mut activity_ids = BTreeSet::new();
    for (master_name, prefix) in PERMANENT_ACTIVITY_MASTERS {
        let seed = master_seed(master_name).ok_or_else(|| {
            PersonalServiceError::new(format!(
                "CN permanent activity seed is missing: {master_name}"
            ))
        })?;
        let value = decode_ordered_map(seed).map_err(PersonalServiceError::new)?;
        let OrderedValue::Map(entries) = value else {
            return Err(PersonalServiceError::new(format!(
                "CN permanent activity seed is not a map: {master_name}"
            )));
        };
        for (activity_key, _) in entries {
            activity_ids.insert(format!("{prefix}{activity_key}"));
        }
    }
    Ok(activity_ids)
}
// //// /从活动 master seed 枚举长期活动 key ////

// //// 判断活动编号是否属于长期开放活动 [@x380kkm 2026-08-30] ////
pub(crate) fn is_permanent_activity_id(activity_id: &str) -> bool {
    PERMANENT_ACTIVITY_MASTERS.iter().any(|(_, prefix)| {
        activity_id.strip_prefix(prefix).is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
    })
}
// //// /判断活动编号是否属于长期开放活动 ////

fn projection_window(start_at_ms: i64, end_at_ms: i64, expires_at_ms: i64) -> ProjectionWindow {
    ProjectionWindow {
        start_at_ms: round_millis_to_second(start_at_ms),
        end_at_ms: round_millis_to_second(end_at_ms),
        expires_at_ms: round_millis_to_second(expires_at_ms),
    }
}

fn round_millis_to_second(value: i64) -> i64 {
    value.saturating_div(1_000).saturating_mul(1_000)
}

fn scale_millis(value: i64, rate: f64) -> i64 {
    let scaled = value.max(0) as f64 * rate;
    if !scaled.is_finite() || scaled >= i64::MAX as f64 {
        i64::MAX
    } else {
        scaled.round() as i64
    }
}

fn master_has_schedule(master: &ProjectionMaster) -> bool {
    !master.synchronized_field_indexes.is_empty()
        && (master.start_index.is_some()
            || master.end_index.is_some()
            || !master.composite_schedule_indexes.is_empty())
}

fn activity_suffix<'a>(master: &ProjectionMaster, activity_id: &'a str) -> Option<&'a str> {
    activity_id.strip_prefix(&master.activity_id_prefix)
}

// //// 投影目标活动及关联关卡行 [@x380kkm 2026-08-29] ////
fn project_master_value(
    value: &mut OrderedValue,
    master: &ProjectionMaster,
    activity_key: &str,
    window: &ProjectionWindow,
) -> bool {
    let target = match value {
        OrderedValue::Map(entries) => entries
            .iter_mut()
            .find(|(key, _)| key == activity_key)
            .map(|(_, value)| value),
        OrderedValue::Row(_) => None,
    };
    target.is_some_and(|value| mutate_rows(value, master, window))
}
// //// /投影目标活动及关联关卡行 ////

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
    let mut changed = false;
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
        "ranking_event" => match index {
            11 => Some(TimeKind::Aggregation),
            12 => Some(TimeKind::Reward),
            18 => Some(TimeKind::Start),
            19 | 20 => Some(TimeKind::End),
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
        TimeKind::Reward => window.end_at_ms.saturating_add(REWARD_DELAY_MS),
    };
    format_client_time(
        timestamp
            .saturating_add(MASTER_TIMEZONE_OFFSET_MS)
            .saturating_div(1_000),
    )
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
) -> String {
    let mut digest = Sha256::new();
    digest.update(PROJECTION_MANIFEST.as_bytes());
    for name in touched {
        digest.update(name.as_bytes());
        digest.update([0]);
    }
    for (activity_id, window) in windows {
        digest.update(activity_id.as_bytes());
        digest.update(window.start_at_ms.saturating_div(1_000).to_le_bytes());
        digest.update(window.end_at_ms.saturating_div(1_000).to_le_bytes());
        digest.update(window.expires_at_ms.saturating_div(1_000).to_le_bytes());
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

fn read_active_path_manifest(
    asset_root: &Path,
    override_root: &Path,
) -> Result<Value, PersonalServiceError> {
    let path = if override_root.join("path").is_file() {
        override_root.join("path")
    } else {
        asset_root.join("path")
    };
    let bytes = fs::read(path).map_err(|error| {
        PersonalServiceError::new(format!("failed to read CN asset path manifest: {error}"))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode CN asset path manifest: {error}"))
    })
}

fn append_path_diff(
    manifest: &mut Value,
    original_version: &str,
    target_version: &str,
    archive_relative: &str,
    archive_data: &[u8],
) -> Result<(), PersonalServiceError> {
    let info = manifest
        .get_mut("info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new("CN asset path info is invalid"))?;
    info.insert(
        "target_asset_version".to_owned(),
        Value::String(target_version.to_owned()),
    );
    info.insert(
        "eventual_target_asset_version".to_owned(),
        Value::String(target_version.to_owned()),
    );
    let diffs = manifest
        .get_mut("diff")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PersonalServiceError::new("CN asset path diff is invalid"))?;
    diffs.push(json!({
        "version": target_version,
        "original_version": original_version,
        "archive": [{
            "location": archive_relative,
            "size": archive_data.len(),
            "sha256": STANDARD.encode(Sha256::digest(archive_data)),
        }],
    }));
    Ok(())
}

// //// 同步投影资源的 EntityLists 行 [@x380kkm 2026-08-29] ////
fn synchronize_entity_manifests(
    asset_root: &Path,
    override_root: &Path,
    target_version: &str,
    entries: &[(String, Vec<u8>)],
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
        let source_path = if override_path.is_file() {
            override_path.clone()
        } else {
            asset_root.join(directory_name).join(name)
        };
        if !source_path.is_file() {
            continue;
        }
        let source = fs::read_to_string(&source_path).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to read CN activity EntityLists manifest: {error}"
            ))
        })?;
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
        active_windows, master_seed, permanent_activity_ids, project_master_value, projected_time,
        projection_manifest, projection_window, OrderedValue, TimeKind, OPEN_RULE_END_MS,
        OPEN_RULE_START_MS,
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

    fn collect_rows<'a>(value: &'a OrderedValue, rows: &mut Vec<&'a Vec<String>>) {
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

    #[test]
    fn enumerates_all_permanent_activity_keys_from_seeds() {
        let activity_ids = permanent_activity_ids().expect("permanent activity seeds decode");

        assert_eq!(activity_ids.len(), 22);
        for activity_key in 1..=19 {
            assert!(activity_ids.contains(&format!("daily-week:{activity_key}")));
        }
        assert!(activity_ids.contains("daily-exp-mana:1"));
        assert!(activity_ids.contains("challenge-dungeon:1"));
        assert!(activity_ids.contains("challenge-dungeon:2"));
    }

    #[test]
    fn projects_permanent_daily_and_challenge_windows() {
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
    fn projects_every_permanent_parent_and_quest_row() {
        let activity_ids = permanent_activity_ids().expect("permanent activity seeds decode");
        let window = projection_window(OPEN_RULE_START_MS, OPEN_RULE_END_MS, i64::MAX);
        for (master_name, activity_prefix, expected_key_count, expected_row_count, fields) in [
            (
                "daily_week_event",
                "daily-week:",
                19,
                19,
                &[
                    (16, TimeKind::Start),
                    (17, TimeKind::End),
                    (18, TimeKind::Reward),
                ][..],
            ),
            (
                "daily_week_event_quest",
                "daily-week:",
                19,
                114,
                &[(4, TimeKind::Start), (5, TimeKind::End)][..],
            ),
            (
                "daily_exp_mana_event",
                "daily-exp-mana:",
                1,
                1,
                &[(9, TimeKind::Start), (10, TimeKind::End)][..],
            ),
            (
                "daily_exp_mana_event_quest",
                "daily-exp-mana:",
                1,
                6,
                &[(5, TimeKind::Start), (6, TimeKind::End)][..],
            ),
            (
                "challenge_dungeon_event",
                "challenge-dungeon:",
                2,
                2,
                &[(13, TimeKind::Start), (14, TimeKind::End)][..],
            ),
            (
                "challenge_dungeon_event_quest",
                "challenge-dungeon:",
                2,
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
            let activity_keys = activity_ids
                .iter()
                .filter_map(|activity_id| activity_id.strip_prefix(activity_prefix))
                .collect::<Vec<_>>();
            assert_eq!(activity_keys.len(), expected_key_count);
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
    fn keeps_permanent_windows_consistent_with_schedule_overrides() {
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

        assert!(windows.contains_key("daily-week:19"));
        assert!(windows.contains_key("daily-exp-mana:1"));
        assert!(!windows.contains_key("daily-week:1"));
        assert!(!windows.contains_key("challenge-dungeon:1"));
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
