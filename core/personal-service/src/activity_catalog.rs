// audience: internal
// # personal-service-activity-catalog-api
//
// 该模块读取 CN 资产根中的可重建活动 manifest, 合并活动时间窗口, 临时租约和当前实例收藏,
// 并从独立白名单目录提供 Web 安全活动图片. manifest 和图片都保持只读.

use crate::activity_calendar::status_name;
use crate::database::{
    evaluate_activity_schedule, is_valid_activity_id, ActivityMode, ActivitySchedule,
    ActivityWindowStatus, ServiceDatabase,
};
use crate::http::{decode_path_segment, HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;

mod banner;
mod filter;
mod manifest;

use manifest::{ActivityDefinition, ActivityImageCandidate, LoadedManifest};

const ACTIVITY_CATALOG_PATH: &str = "/v1/activities/catalog";
const ACTIVITY_FAVORITE_PATH: &str = "/v1/activities/catalog/favorite";
const ACTIVITY_FAVORITE_PREFIX: &str = "/v1/activities/catalog/";
const ACTIVITY_BANNER_PREFIX: &str = "/manage/assets/activity-banners/";
const ACTIVITY_CALENDAR_START_OFFSET_MS: i64 = 60_000;
const PERMANENT_ACTIVITY_END_MS: i64 = 253_402_300_799_000;

// //// 选择首次启动使用的活动日历时间 [@x380kkm 2026-08-23] ////
pub(crate) fn initial_activity_calendar_time(asset_root: &Path) -> Option<i64> {
    let LoadedManifest::Present(manifest) = manifest::load(asset_root).ok()? else {
        return None;
    };
    manifest
        .activities
        .iter()
        .filter(|definition| !matches!(definition.kind.as_str(), "gacha" | "daily"))
        .filter_map(|definition| {
            definition
                .default_start_at_ms
                .zip(definition.default_end_at_ms)
        })
        .filter(|(start_at_ms, end_at_ms)| {
            *start_at_ms > 0 && *end_at_ms > *start_at_ms && *end_at_ms < PERMANENT_ACTIVITY_END_MS
        })
        .filter_map(|(start_at_ms, _)| start_at_ms.checked_add(ACTIVITY_CALENDAR_START_OFFSET_MS))
        .min()
}
// //// /选择首次启动使用的活动日历时间 ////

// //// 读取 manifest 默认时间窗口 [@x380kkm 2026-08-19] ////
pub(crate) fn default_activity_window_status(
    asset_root: &Path,
    activity_id: &str,
    now_ms: i64,
) -> Option<ActivityWindowStatus> {
    if crate::activity_projection::is_permanent_activity_id(activity_id) {
        return Some(ActivityWindowStatus::Open);
    }
    let (start_at_ms, end_at_ms) = default_activity_window(asset_root, activity_id)?;
    Some(if now_ms < start_at_ms {
        ActivityWindowStatus::NotStarted
    } else if now_ms >= end_at_ms {
        ActivityWindowStatus::Ended
    } else {
        ActivityWindowStatus::Open
    })
}
// //// /读取 manifest 默认时间窗口 ////

pub(crate) fn default_activity_window(asset_root: &Path, activity_id: &str) -> Option<(i64, i64)> {
    let LoadedManifest::Present(manifest) = manifest::load(asset_root).ok()? else {
        return None;
    };
    manifest
        .activities
        .iter()
        .find(|definition| definition.activity_id == activity_id)
        .and_then(|definition| {
            definition
                .default_start_at_ms
                .zip(definition.default_end_at_ms)
        })
}

pub(crate) fn contains_activity_definition(asset_root: &Path, activity_id: &str) -> bool {
    let Ok(LoadedManifest::Present(manifest)) = manifest::load(asset_root) else {
        return false;
    };
    manifest
        .activities
        .iter()
        .any(|definition| definition.activity_id == activity_id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyFavoriteRequest {
    activity_id: String,
    favorite: bool,
}

#[derive(Serialize)]
struct ActivityCatalogResponse {
    manifest_state: &'static str,
    format_version: Option<u32>,
    region: Option<String>,
    client_version: Option<String>,
    asset_version: Option<String>,
    generated_at: Option<String>,
    server_time_ms: i64,
    total: usize,
    activities: Vec<ActivityCatalogEntry>,
}

#[derive(Serialize)]
struct ActivityCatalogEntry {
    activity_id: String,
    name: String,
    kind: String,
    tags: Vec<String>,
    description: String,
    banner_key: Option<String>,
    banner_url: Option<String>,
    banner_width: Option<u32>,
    banner_height: Option<u32>,
    banner_source_type: Option<String>,
    banner_evidence: Option<String>,
    image_candidates: Vec<ActivityImageCandidateResponse>,
    default_start_at_ms: Option<i64>,
    default_end_at_ms: Option<i64>,
    mode: Option<&'static str>,
    period: Option<&'static str>,
    interval_days: Option<i64>,
    start_at_ms: Option<i64>,
    end_at_ms: Option<i64>,
    active_start_at_ms: Option<i64>,
    active_end_at_ms: Option<i64>,
    next_start_at_ms: Option<i64>,
    next_end_at_ms: Option<i64>,
    temporary_open_until_ms: Option<i64>,
    favorite: bool,
    underlying_status: &'static str,
    status: &'static str,
    is_open: bool,
    schedule: Option<ActivityScheduleResponse>,
    #[serde(skip)]
    rule: Option<ActivitySchedule>,
}

#[derive(Serialize)]
struct ActivityImageCandidateResponse {
    key: String,
    url: String,
    width: Option<u32>,
    height: Option<u32>,
    source_type: String,
    evidence: Option<String>,
}

#[derive(Serialize)]
struct ActivityScheduleResponse {
    enabled: bool,
    mode: &'static str,
    period: &'static str,
    interval_days: Option<i64>,
    start_at_ms: Option<i64>,
    end_at_ms: Option<i64>,
    active_start_at_ms: Option<i64>,
    active_end_at_ms: Option<i64>,
    next_start_at_ms: Option<i64>,
    next_end_at_ms: Option<i64>,
    created_at: String,
    updated_at: String,
}

// //// 分派活动目录和 banner 请求 [@x380kkm 2026-08-19] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if let Some(banner_key) = request.path().strip_prefix(ACTIVITY_BANNER_PREFIX) {
        return Some(Ok(banner::serve(request, asset_root, banner_key)));
    }
    let favorite_activity_id = favorite_activity_id(request.path());
    if request.path() != ACTIVITY_CATALOG_PATH
        && request.path() != ACTIVITY_FAVORITE_PATH
        && favorite_activity_id.is_none()
    {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    if request.path() == ACTIVITY_CATALOG_PATH {
        return Some(match request.method() {
            "GET" => list_activity_catalog(request, database, asset_root),
            _ => Ok(method_not_allowed("GET")),
        });
    }
    if request.path() == ACTIVITY_FAVORITE_PATH {
        return Some(match request.method() {
            "PUT" => update_legacy_activity_favorite(request, database, asset_root),
            _ => Ok(method_not_allowed("PUT")),
        });
    }
    let Some(activity_id) = favorite_activity_id else {
        return Some(Ok(error_response("404 Not Found", "activity_not_found")));
    };
    Some(match request.method() {
        "PUT" => update_activity_favorite(database, asset_root, &activity_id, true),
        "DELETE" => update_activity_favorite(database, asset_root, &activity_id, false),
        _ => Ok(method_not_allowed("PUT, DELETE")),
    })
}
// //// /分派活动目录和 banner 请求 ////

fn update_legacy_activity_favorite(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_favorite_request",
        ));
    }
    let Ok(body) = serde_json::from_slice::<LegacyFavoriteRequest>(request.body()) else {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_favorite_request",
        ));
    };
    if !is_valid_activity_id(&body.activity_id) {
        return Ok(error_response("400 Bad Request", "invalid_activity_id"));
    }
    update_activity_favorite(database, asset_root, &body.activity_id, body.favorite)
}

fn favorite_activity_id(path: &str) -> Option<String> {
    let encoded = path
        .strip_prefix(ACTIVITY_FAVORITE_PREFIX)?
        .strip_suffix("/favorite")?;
    if encoded.is_empty() || encoded.contains('/') {
        return None;
    }
    decode_path_segment(encoded).filter(|activity_id| is_valid_activity_id(activity_id))
}

// //// 合并活动元数据、时间窗口和收藏 [@x380kkm 2026-08-19] ////
fn list_activity_catalog(
    request: &HttpRequest,
    database: &ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(filter) = filter::parse(request) else {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_catalog_filter",
        ));
    };
    let manifest = match manifest::load(asset_root) {
        Ok(manifest) => manifest,
        Err(()) => {
            return Ok(error_response(
                "422 Unprocessable Entity",
                "invalid_activity_catalog_manifest",
            ))
        }
    };
    let schedules = database.list_activity_schedules()?;
    let schedule_by_id = schedules
        .iter()
        .map(|schedule| (schedule.activity_id.as_str(), schedule))
        .collect::<HashMap<_, _>>();
    let temporary_open_leases = database.list_active_activity_temporary_open_leases()?;
    let temporary_open_until_by_id = temporary_open_leases
        .iter()
        .map(|(activity_id, expires_at_ms)| (activity_id.as_str(), *expires_at_ms))
        .collect::<HashMap<_, _>>();
    let favorites = database.list_activity_favorites()?;
    let now_ms = database.current_server_time_millis()?;

    let (
        manifest_state,
        format_version,
        region,
        client_version,
        asset_version,
        generated_at,
        definitions,
    ) = match manifest {
        LoadedManifest::Missing => ("missing", None, None, None, None, None, Vec::new()),
        LoadedManifest::Present(manifest) => (
            "loaded",
            Some(manifest.format_version),
            manifest.region,
            manifest.client_version,
            manifest.asset_version,
            manifest.generated_at,
            manifest.activities,
        ),
    };
    let mut known_ids = HashSet::with_capacity(definitions.len());
    let mut entries = definitions
        .into_iter()
        .map(|definition| {
            known_ids.insert(definition.activity_id.clone());
            let temporary_open_until_ms = temporary_open_until_by_id
                .get(definition.activity_id.as_str())
                .copied();
            catalog_entry(definition, None, false, now_ms, temporary_open_until_ms)
        })
        .collect::<Vec<_>>();

    for entry in &mut entries {
        let schedule = schedule_by_id.get(entry.activity_id.as_str()).copied();
        apply_schedule_and_favorite(
            entry,
            schedule,
            favorites.contains(&entry.activity_id),
            now_ms,
            temporary_open_until_by_id
                .get(entry.activity_id.as_str())
                .copied(),
        );
    }
    for schedule in &schedules {
        if known_ids.contains(&schedule.activity_id) {
            continue;
        }
        let kind = schedule
            .activity_id
            .split_once(':')
            .map(|(kind, _)| kind)
            .unwrap_or("other")
            .to_owned();
        let definition = ActivityDefinition {
            activity_id: schedule.activity_id.clone(),
            name: schedule.activity_id.clone(),
            kind,
            tags: Vec::new(),
            description: String::new(),
            banner_key: None,
            banner_width: None,
            banner_height: None,
            image_candidates: Vec::new(),
            default_start_at_ms: None,
            default_end_at_ms: None,
        };
        entries.push(catalog_entry(
            definition,
            Some(schedule),
            favorites.contains(&schedule.activity_id),
            now_ms,
            temporary_open_until_by_id
                .get(schedule.activity_id.as_str())
                .copied(),
        ));
    }
    entries.retain(|entry| filter::matches(entry, &filter));
    entries.sort_by(|left, right| {
        right
            .favorite
            .cmp(&left.favorite)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.activity_id.cmp(&right.activity_id))
    });

    json_response(
        "200 OK",
        &ActivityCatalogResponse {
            manifest_state,
            format_version,
            region,
            client_version,
            asset_version,
            generated_at,
            server_time_ms: now_ms,
            total: entries.len(),
            activities: entries,
        },
    )
}
// //// /合并活动元数据、时间窗口和收藏 ////

// //// 合并活动图片, 时间窗口和收藏 [@x380kkm 2026-08-19] ////
fn catalog_entry(
    definition: ActivityDefinition,
    schedule: Option<&ActivitySchedule>,
    favorite: bool,
    now_ms: i64,
    temporary_open_until_ms: Option<i64>,
) -> ActivityCatalogEntry {
    let mut image_candidates = definition
        .image_candidates
        .into_iter()
        .map(image_candidate_response)
        .collect::<Vec<_>>();
    if let Some(key) = definition.banner_key {
        if !image_candidates
            .iter()
            .any(|candidate| candidate.key == key)
        {
            image_candidates.insert(
                0,
                image_candidate_response(ActivityImageCandidate {
                    key,
                    width: definition.banner_width,
                    height: definition.banner_height,
                    source_type: "activity_banner".to_owned(),
                    evidence: Some("legacy_manifest".to_owned()),
                }),
            );
        }
    }
    let banner_key = image_candidates
        .first()
        .map(|candidate| candidate.key.clone());
    let banner_url = image_candidates
        .first()
        .map(|candidate| candidate.url.clone());
    let banner_width = image_candidates
        .first()
        .and_then(|candidate| candidate.width);
    let banner_height = image_candidates
        .first()
        .and_then(|candidate| candidate.height);
    let banner_source_type = image_candidates
        .first()
        .map(|candidate| candidate.source_type.clone());
    let banner_evidence = image_candidates
        .first()
        .and_then(|candidate| candidate.evidence.clone());
    let mut entry = ActivityCatalogEntry {
        activity_id: definition.activity_id,
        name: definition.name,
        kind: definition.kind,
        tags: definition.tags,
        description: definition.description,
        banner_key,
        banner_url,
        banner_width,
        banner_height,
        banner_source_type,
        banner_evidence,
        image_candidates,
        default_start_at_ms: definition.default_start_at_ms,
        default_end_at_ms: definition.default_end_at_ms,
        mode: None,
        period: None,
        interval_days: None,
        start_at_ms: None,
        end_at_ms: None,
        active_start_at_ms: None,
        active_end_at_ms: None,
        next_start_at_ms: None,
        next_end_at_ms: None,
        temporary_open_until_ms,
        favorite,
        underlying_status: "unscheduled",
        status: "unscheduled",
        is_open: true,
        schedule: None,
        rule: None,
    };
    apply_schedule_and_favorite(
        &mut entry,
        schedule,
        favorite,
        now_ms,
        temporary_open_until_ms,
    );
    entry
}

fn image_candidate_response(candidate: ActivityImageCandidate) -> ActivityImageCandidateResponse {
    ActivityImageCandidateResponse {
        url: format!("{ACTIVITY_BANNER_PREFIX}{}", candidate.key),
        key: candidate.key,
        width: candidate.width,
        height: candidate.height,
        source_type: candidate.source_type,
        evidence: candidate.evidence,
    }
}

fn apply_schedule_and_favorite(
    entry: &mut ActivityCatalogEntry,
    schedule: Option<&ActivitySchedule>,
    favorite: bool,
    now_ms: i64,
    temporary_open_until_ms: Option<i64>,
) {
    entry.favorite = favorite;
    entry.temporary_open_until_ms = temporary_open_until_ms;
    let Some(schedule) = schedule else {
        if crate::activity_projection::is_permanent_activity_id(&entry.activity_id) {
            entry.mode = Some("always");
            entry.period = None;
            entry.schedule = None;
            entry.rule = None;
            apply_temporary_open_status(
                entry,
                ActivityWindowStatus::Open,
                temporary_open_until_ms,
            );
            return;
        }
        let Some((start_at_ms, end_at_ms)) = entry.default_start_at_ms.zip(entry.default_end_at_ms)
        else {
            entry.schedule = None;
            entry.rule = None;
            apply_temporary_open_status(
                entry,
                ActivityWindowStatus::Unscheduled,
                temporary_open_until_ms,
            );
            return;
        };
        entry.mode = Some("window");
        entry.period = Some("once");
        entry.start_at_ms = Some(start_at_ms);
        entry.end_at_ms = Some(end_at_ms);
        let underlying_status = if now_ms < start_at_ms {
            entry.next_start_at_ms = Some(start_at_ms);
            entry.next_end_at_ms = Some(end_at_ms);
            ActivityWindowStatus::NotStarted
        } else if now_ms >= end_at_ms {
            ActivityWindowStatus::Ended
        } else {
            entry.active_start_at_ms = Some(start_at_ms);
            entry.active_end_at_ms = Some(end_at_ms);
            ActivityWindowStatus::Open
        };
        entry.schedule = None;
        entry.rule = None;
        apply_temporary_open_status(entry, underlying_status, temporary_open_until_ms);
        return;
    };
    let evaluation = evaluate_activity_schedule(schedule, now_ms);
    let has_window = matches!(schedule.mode, ActivityMode::Window | ActivityMode::Periodic);
    entry.mode = Some(schedule.mode.as_str());
    entry.period = Some(schedule.period.as_str());
    entry.interval_days = schedule.interval_days;
    entry.start_at_ms = has_window.then_some(schedule.start_at_ms);
    entry.end_at_ms = has_window.then_some(schedule.end_at_ms);
    entry.active_start_at_ms = evaluation.active_start_ms;
    entry.active_end_at_ms = evaluation.active_end_ms;
    entry.next_start_at_ms = evaluation.next_start_ms;
    entry.next_end_at_ms = evaluation.next_end_ms;
    entry.schedule = Some(ActivityScheduleResponse {
        enabled: schedule.enabled,
        mode: schedule.mode.as_str(),
        period: schedule.period.as_str(),
        interval_days: schedule.interval_days,
        start_at_ms: has_window.then_some(schedule.start_at_ms),
        end_at_ms: has_window.then_some(schedule.end_at_ms),
        active_start_at_ms: evaluation.active_start_ms,
        active_end_at_ms: evaluation.active_end_ms,
        next_start_at_ms: evaluation.next_start_ms,
        next_end_at_ms: evaluation.next_end_ms,
        created_at: schedule.created_at.clone(),
        updated_at: schedule.updated_at.clone(),
    });
    entry.rule = Some(schedule.clone());
    apply_temporary_open_status(entry, evaluation.status, temporary_open_until_ms);
}

fn apply_temporary_open_status(
    entry: &mut ActivityCatalogEntry,
    underlying_status: ActivityWindowStatus,
    temporary_open_until_ms: Option<i64>,
) {
    let status = if temporary_open_until_ms.is_some() {
        ActivityWindowStatus::Open
    } else {
        underlying_status
    };
    entry.underlying_status = status_name(underlying_status);
    entry.status = status_name(status);
    entry.is_open = matches!(
        status,
        ActivityWindowStatus::Unscheduled | ActivityWindowStatus::Open
    );
}

// //// /合并活动图片, 时间窗口和收藏 ////

// //// 持久化当前实例的活动收藏 [@x380kkm 2026-08-19] ////
fn update_activity_favorite(
    database: &mut ServiceDatabase,
    asset_root: &Path,
    activity_id: &str,
    favorite: bool,
) -> Result<HttpResponse, PersonalServiceError> {
    let manifest = match manifest::load(asset_root) {
        Ok(manifest) => manifest,
        Err(()) => {
            return Ok(error_response(
                "422 Unprocessable Entity",
                "invalid_activity_catalog_manifest",
            ))
        }
    };
    let manifest_contains = match manifest {
        LoadedManifest::Missing => false,
        LoadedManifest::Present(manifest) => manifest
            .activities
            .iter()
            .any(|activity| activity.activity_id == activity_id),
    };
    if !manifest_contains && database.get_activity_schedule(activity_id)?.is_none() {
        return Ok(error_response("404 Not Found", "activity_not_found"));
    }
    if !database.set_activity_favorite(activity_id, favorite)? {
        return Ok(error_response("400 Bad Request", "invalid_activity_id"));
    }
    json_response(
        "200 OK",
        &json!({
            "activity_id": activity_id,
            "favorite": favorite,
        }),
    )
}
// //// /持久化当前实例的活动收藏 ////

fn json_response<T: Serialize>(
    status: &'static str,
    value: &T,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = serde_json::to_string(value).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode activity catalog response: {error}"
        ))
    })?;
    Ok(HttpResponse::json(status, body))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

fn method_not_allowed(allow: &'static str) -> HttpResponse {
    error_response("405 Method Not Allowed", "method_not_allowed").with_header("Allow", allow)
}
