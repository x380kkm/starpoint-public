// audience: internal
// # personal-service-activity-calendar-api
//
// 该模块提供受管理 token 保护的活动日历 API. 日历状态使用虚拟游戏时间计算,
// 24 小时临时开放期限使用真实墙钟并叠加到活动状态.

use crate::database::{
    evaluate_activity_schedule, is_valid_activity_id, parse_iso_timestamp, ActivityMode,
    ActivityPeriod, ActivitySchedule, ActivityScheduleStoreError, ActivityWindowStatus,
    ServiceDatabase,
};
use crate::http::{decode_path_segment, HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const ACTIVITY_CALENDAR_PATH: &str = "/v1/activities/calendar";
const ACTIVITY_CALENDAR_PREFIX: &str = "/v1/activities/calendar/";
const ACTIVITY_RESET_PATH: &str = "/v1/activities/reset";
const ACTIVITY_RULE_PREFIX: &str = "/v1/activities/";
const MAX_ACTIVITY_INTERVAL_DAYS: i64 = 3650;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateActivityScheduleRequest {
    enabled: bool,
    start_at_ms: Option<i64>,
    start_at: Option<String>,
    end_at_ms: Option<i64>,
    end_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyActivityRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateActivityModeRequest {
    mode: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateActivityWindowRequest {
    start_at_ms: Option<i64>,
    start_at: Option<String>,
    end_at_ms: Option<i64>,
    end_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateActivityPeriodRequest {
    period: String,
    interval_days: Option<i64>,
}

#[derive(Clone, Copy)]
enum ActivityRuleAction {
    Open,
    Close,
    TemporaryOpen,
    Mode,
    Window,
    Period,
}

// //// 分派活动日历管理请求 [@x380kkm 2026-08-19] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let is_calendar_request = request.path() == ACTIVITY_CALENDAR_PATH
        || request.path().starts_with(ACTIVITY_CALENDAR_PREFIX);
    let is_reset_request = request.path() == ACTIVITY_RESET_PATH;
    let activity_rule_target = parse_activity_rule_target(request.path());
    if !is_calendar_request && !is_reset_request && activity_rule_target.is_none() {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    if request.path() == ACTIVITY_CALENDAR_PATH {
        let response = match request.method() {
            "GET" => list_activity_schedules(database),
            _ => Ok(error_response(
                "405 Method Not Allowed",
                "method_not_allowed",
            )),
        };
        return Some(synchronize_projection(
            response,
            database,
            asset_root,
            override_root,
        ));
    }
    if is_reset_request {
        let response = match request.method() {
            "POST" => reset_activity_overrides(request, database),
            _ => Ok(method_not_allowed("POST")),
        };
        return Some(synchronize_projection(
            response,
            database,
            asset_root,
            override_root,
        ));
    }
    if let Some((activity_id, action)) = activity_rule_target {
        let response = manage_activity_rule(request, database, asset_root, &activity_id, action);
        return Some(synchronize_projection(
            response,
            database,
            asset_root,
            override_root,
        ));
    }
    let Some(activity_id) = request
        .path()
        .strip_prefix(ACTIVITY_CALENDAR_PREFIX)
        .and_then(decode_path_segment)
    else {
        return Some(Ok(error_response("400 Bad Request", "invalid_activity_id")));
    };
    Some(synchronize_projection(
        manage_activity_schedule(request, database, &activity_id),
        database,
        asset_root,
        override_root,
    ))
}
// //// /分派活动日历管理请求 ////

fn synchronize_projection(
    response: Result<HttpResponse, PersonalServiceError>,
    database: &ServiceDatabase,
    asset_root: &Path,
    override_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let response = response?;
    crate::activity_projection::sync(database, asset_root, override_root)?;
    Ok(response)
}

// //// 恢复包内活动时间 [@x380kkm 2026-08-24] ////
fn reset_activity_overrides(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    if parse_json_request::<EmptyActivityRequest>(request).is_none() {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_reset_request",
        ));
    }
    let (schedule_count, temporary_open_count) = database.reset_activity_overrides()?;
    json_response(
        "200 OK",
        json!({
            "reset_schedule_count": schedule_count,
            "reset_temporary_open_count": temporary_open_count,
        }),
    )
}
// //// /恢复包内活动时间 ////

fn parse_activity_rule_target(path: &str) -> Option<(String, ActivityRuleAction)> {
    let (encoded_activity_id, action) =
        path.strip_prefix(ACTIVITY_RULE_PREFIX)?.rsplit_once('/')?;
    if encoded_activity_id.is_empty() || encoded_activity_id.contains('/') {
        return None;
    }
    let action = match action {
        "open" => ActivityRuleAction::Open,
        "close" => ActivityRuleAction::Close,
        "temporary-open" => ActivityRuleAction::TemporaryOpen,
        "mode" => ActivityRuleAction::Mode,
        "window" => ActivityRuleAction::Window,
        "period" => ActivityRuleAction::Period,
        _ => return None,
    };
    let activity_id = decode_path_segment(encoded_activity_id)?;
    Some((activity_id, action))
}

// //// 管理活动规则 [@x380kkm 2026-08-19] ////
fn manage_activity_rule(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
    activity_id: &str,
    action: ActivityRuleAction,
) -> Result<HttpResponse, PersonalServiceError> {
    if !is_valid_activity_id(activity_id) {
        return Ok(error_response("400 Bad Request", "invalid_activity_id"));
    }
    match action {
        ActivityRuleAction::Open if request.method() == "POST" => {
            create_temporary_activity_open(request, database, asset_root, activity_id)
        }
        ActivityRuleAction::Close if request.method() == "POST" => {
            close_manual_activity(request, database, activity_id)
        }
        ActivityRuleAction::TemporaryOpen if request.method() == "POST" => {
            create_temporary_activity_open(request, database, asset_root, activity_id)
        }
        ActivityRuleAction::TemporaryOpen if request.method() == "DELETE" => {
            delete_temporary_activity_open(database, asset_root, activity_id)
        }
        ActivityRuleAction::Mode if request.method() == "PUT" => {
            update_activity_mode(request, database, asset_root, activity_id)
        }
        ActivityRuleAction::Window if request.method() == "PUT" => {
            update_activity_window(request, database, activity_id)
        }
        ActivityRuleAction::Period if request.method() == "PUT" => {
            update_activity_period(request, database, asset_root, activity_id)
        }
        ActivityRuleAction::Open | ActivityRuleAction::Close => Ok(method_not_allowed("POST")),
        ActivityRuleAction::TemporaryOpen => Ok(method_not_allowed("POST, DELETE")),
        ActivityRuleAction::Mode | ActivityRuleAction::Window | ActivityRuleAction::Period => {
            Ok(method_not_allowed("PUT"))
        }
    }
}

fn close_manual_activity(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    if parse_json_request::<EmptyActivityRequest>(request).is_none() {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_action_request",
        ));
    }
    let result = database.upsert_manual_activity(activity_id, false);
    serialize_store_result(database, result)
}

// //// 管理 24 小时临时开放租约 [@x380kkm 2026-08-24] ////
fn create_temporary_activity_open(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    if parse_json_request::<EmptyActivityRequest>(request).is_none() {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_temporary_activity_open_request",
        ));
    }
    if database.get_activity_schedule(activity_id)?.is_none()
        && !crate::activity_catalog::contains_activity_definition(asset_root, activity_id)
    {
        return Ok(error_response("404 Not Found", "activity_not_found"));
    }
    match database.create_activity_temporary_open_lease(activity_id) {
        Ok(expires_at_ms) => temporary_activity_response(
            database,
            asset_root,
            activity_id,
            Some(expires_at_ms),
            false,
        ),
        Err(ActivityScheduleStoreError::Invalid) => {
            Ok(error_response("400 Bad Request", "invalid_activity_id"))
        }
        Err(ActivityScheduleStoreError::NotFound) => Ok(error_response(
            "404 Not Found",
            "activity_temporary_open_lease_not_found",
        )),
        Err(ActivityScheduleStoreError::Storage(error)) => Err(error),
    }
}

fn delete_temporary_activity_open(
    database: &mut ServiceDatabase,
    asset_root: &Path,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let deleted = database.delete_activity_temporary_open_lease(activity_id)?;
    temporary_activity_response(database, asset_root, activity_id, None, deleted)
}

fn temporary_activity_response(
    database: &ServiceDatabase,
    asset_root: &Path,
    activity_id: &str,
    temporary_open_until_ms: Option<i64>,
    deleted: bool,
) -> Result<HttpResponse, PersonalServiceError> {
    let now_ms = database.current_server_time_millis()?;
    let underlying_status = database
        .get_activity_schedule(activity_id)?
        .map(|schedule| evaluate_activity_schedule(&schedule, now_ms).status)
        .or_else(|| {
            crate::activity_catalog::default_activity_window_status(asset_root, activity_id, now_ms)
        })
        .unwrap_or(ActivityWindowStatus::Unscheduled);
    let status = if temporary_open_until_ms.is_some() {
        ActivityWindowStatus::Open
    } else {
        underlying_status
    };
    json_response(
        "200 OK",
        json!({
            "activity_id": activity_id,
            "temporary_open_until_ms": temporary_open_until_ms,
            "underlying_status": status_name(underlying_status),
            "status": status_name(status),
            "is_open": matches!(status, ActivityWindowStatus::Unscheduled | ActivityWindowStatus::Open),
            "deleted": deleted,
            "server_time_ms": now_ms,
        }),
    )
}
// //// /管理 24 小时临时开放租约 ////

fn update_activity_mode(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json_request::<UpdateActivityModeRequest>(request) else {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_mode_request",
        ));
    };
    let Some(mode) = ActivityMode::parse(&body.mode) else {
        return Ok(error_response("400 Bad Request", "invalid_activity_mode"));
    };
    let existing = database.get_activity_schedule(activity_id)?;
    let result = match mode {
        ActivityMode::Manual => {
            let now_ms = database.current_server_time_millis()?;
            let enabled = existing
                .as_ref()
                .map(|schedule| {
                    evaluate_activity_schedule(schedule, now_ms).status
                        == ActivityWindowStatus::Open
                })
                .unwrap_or(true);
            database.upsert_manual_activity(activity_id, enabled)
        }
        ActivityMode::Always => database.upsert_always_activity(activity_id),
        ActivityMode::Window => {
            let Some((start_at_ms, end_at_ms)) =
                configured_window(database, activity_id, asset_root)?
            else {
                return Ok(error_response("409 Conflict", "activity_window_required"));
            };
            database.upsert_activity_rule(
                activity_id,
                true,
                ActivityMode::Window,
                ActivityPeriod::Once,
                None,
                start_at_ms,
                end_at_ms,
            )
        }
        ActivityMode::Periodic => {
            let Some((start_at_ms, end_at_ms)) =
                configured_window(database, activity_id, asset_root)?
            else {
                return Ok(error_response("409 Conflict", "activity_window_required"));
            };
            let period = if existing
                .as_ref()
                .map_or(true, |schedule| schedule.period == ActivityPeriod::Once)
            {
                ActivityPeriod::Daily
            } else {
                existing
                    .as_ref()
                    .expect("existing schedule is present")
                    .period
            };
            let interval_days = (period == ActivityPeriod::IntervalDays).then_some(
                existing
                    .as_ref()
                    .and_then(|schedule| schedule.interval_days)
                    .unwrap_or(1),
            );
            database.upsert_activity_rule(
                activity_id,
                true,
                ActivityMode::Periodic,
                period,
                interval_days,
                start_at_ms,
                end_at_ms,
            )
        }
    };
    serialize_store_result(database, result)
}

fn update_activity_window(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json_request::<UpdateActivityWindowRequest>(request) else {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_window_request",
        ));
    };
    let Some(start_at_ms) = request_timestamp(body.start_at_ms, body.start_at.as_deref()) else {
        return Ok(error_response("400 Bad Request", "invalid_activity_window"));
    };
    let Some(end_at_ms) = request_timestamp(body.end_at_ms, body.end_at.as_deref()) else {
        return Ok(error_response("400 Bad Request", "invalid_activity_window"));
    };
    let result = database.upsert_activity_rule(
        activity_id,
        true,
        ActivityMode::Window,
        ActivityPeriod::Once,
        None,
        start_at_ms,
        end_at_ms,
    );
    serialize_store_result(database, result)
}

fn update_activity_period(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json_request::<UpdateActivityPeriodRequest>(request) else {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_period_request",
        ));
    };
    let Some(period) = ActivityPeriod::parse(&body.period) else {
        return Ok(error_response("400 Bad Request", "invalid_activity_period"));
    };
    if body
        .interval_days
        .is_some_and(|days| !(1..=MAX_ACTIVITY_INTERVAL_DAYS).contains(&days))
        || (period == ActivityPeriod::IntervalDays && body.interval_days.is_none())
    {
        return Ok(error_response("400 Bad Request", "invalid_activity_period"));
    }
    let Some((start_at_ms, end_at_ms)) = configured_window(database, activity_id, asset_root)?
    else {
        return Ok(error_response("409 Conflict", "activity_window_required"));
    };
    let interval_days = if period == ActivityPeriod::IntervalDays {
        body.interval_days
    } else {
        None
    };
    let mode = if period == ActivityPeriod::Once {
        ActivityMode::Window
    } else {
        ActivityMode::Periodic
    };
    let result = database.upsert_activity_rule(
        activity_id,
        true,
        mode,
        period,
        interval_days,
        start_at_ms,
        end_at_ms,
    );
    serialize_store_result(database, result)
}

fn parse_json_request<T: for<'de> Deserialize<'de>>(request: &HttpRequest) -> Option<T> {
    request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
        .then(|| serde_json::from_slice(request.body()).ok())
        .flatten()
}

fn has_real_window(schedule: &ActivitySchedule) -> bool {
    matches!(schedule.mode, ActivityMode::Window | ActivityMode::Periodic)
        && schedule.start_at_ms < schedule.end_at_ms
        && schedule.end_at_ms != i64::MAX
}

fn configured_window(
    database: &ServiceDatabase,
    activity_id: &str,
    asset_root: &Path,
) -> Result<Option<(i64, i64)>, PersonalServiceError> {
    if let Some(schedule) = database.get_activity_schedule(activity_id)? {
        if has_real_window(&schedule) {
            return Ok(Some((schedule.start_at_ms, schedule.end_at_ms)));
        }
    }
    Ok(crate::activity_catalog::default_activity_window(
        asset_root,
        activity_id,
    ))
}

fn serialize_store_result(
    database: &ServiceDatabase,
    result: Result<ActivitySchedule, ActivityScheduleStoreError>,
) -> Result<HttpResponse, PersonalServiceError> {
    match result {
        Ok(schedule) => serialize_activity_schedule(database, &schedule),
        Err(ActivityScheduleStoreError::Invalid) => {
            Ok(error_response("400 Bad Request", "invalid_activity_rule"))
        }
        Err(ActivityScheduleStoreError::NotFound) => Ok(error_response(
            "404 Not Found",
            "activity_schedule_not_found",
        )),
        Err(ActivityScheduleStoreError::Storage(error)) => Err(error),
    }
}
// //// /管理活动规则 ////

// //// 管理单个活动时间窗口 [@x380kkm 2026-08-19] ////
fn manage_activity_schedule(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    if !is_valid_activity_id(activity_id) {
        return Ok(error_response("400 Bad Request", "invalid_activity_id"));
    }
    match request.method() {
        "GET" => match database.get_activity_schedule(activity_id)? {
            Some(schedule) => serialize_activity_schedule(database, &schedule),
            None => Ok(error_response(
                "404 Not Found",
                "activity_schedule_not_found",
            )),
        },
        "PUT" => update_activity_schedule(request, database, activity_id),
        "DELETE" => {
            if database.delete_activity_schedule(activity_id)? {
                json_response("200 OK", json!({ "deleted": true }))
            } else {
                Ok(error_response(
                    "404 Not Found",
                    "activity_schedule_not_found",
                ))
            }
        }
        _ => Ok(error_response(
            "405 Method Not Allowed",
            "method_not_allowed",
        )),
    }
}

fn list_activity_schedules(
    database: &ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let schedules = database.list_activity_schedules()?;
    let now_ms = database.current_server_time_millis()?;
    let entries = schedules
        .iter()
        .map(|schedule| activity_schedule_value(database, schedule, now_ms))
        .collect::<Result<Vec<_>, _>>()?;
    json_response(
        "200 OK",
        json!({
            "server_time_ms": now_ms,
            "schedules": entries,
        }),
    )
}

fn update_activity_schedule(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    activity_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match serde_json::from_slice::<UpdateActivityScheduleRequest>(request.body()) {
        Ok(body) => body,
        Err(_) => {
            return Ok(error_response(
                "400 Bad Request",
                "invalid_activity_schedule",
            ))
        }
    };
    let Some(start_at_ms) = request_timestamp(body.start_at_ms, body.start_at.as_deref()) else {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_schedule",
        ));
    };
    let Some(end_at_ms) = request_timestamp(body.end_at_ms, body.end_at.as_deref()) else {
        return Ok(error_response(
            "400 Bad Request",
            "invalid_activity_schedule",
        ));
    };
    match database.upsert_activity_schedule(activity_id, body.enabled, start_at_ms, end_at_ms) {
        Ok(schedule) => serialize_activity_schedule(database, &schedule),
        Err(ActivityScheduleStoreError::Invalid) => Ok(error_response(
            "400 Bad Request",
            "invalid_activity_schedule",
        )),
        Err(ActivityScheduleStoreError::NotFound) => Ok(error_response(
            "404 Not Found",
            "activity_schedule_not_found",
        )),
        Err(ActivityScheduleStoreError::Storage(error)) => Err(error),
    }
}

fn request_timestamp(unix_time_ms: Option<i64>, iso: Option<&str>) -> Option<i64> {
    match (unix_time_ms, iso) {
        (Some(value), None) if value >= 0 => Some(value),
        (None, Some(value)) => parse_iso_timestamp(value),
        _ => None,
    }
}

fn serialize_activity_schedule(
    database: &ServiceDatabase,
    schedule: &ActivitySchedule,
) -> Result<HttpResponse, PersonalServiceError> {
    let now_ms = database.current_server_time_millis()?;
    json_response(
        "200 OK",
        activity_schedule_value(database, schedule, now_ms)?,
    )
}

fn activity_schedule_value(
    database: &ServiceDatabase,
    schedule: &ActivitySchedule,
    now_ms: i64,
) -> Result<Value, PersonalServiceError> {
    let evaluation = evaluate_activity_schedule(schedule, now_ms);
    let temporary_open_until_ms = database.activity_temporary_open_until(&schedule.activity_id)?;
    let status = if temporary_open_until_ms.is_some() {
        ActivityWindowStatus::Open
    } else {
        evaluation.status
    };
    Ok(json!({
        "activity_id": schedule.activity_id,
        "enabled": schedule.enabled,
        "mode": schedule.mode.as_str(),
        "period": schedule.period.as_str(),
        "interval_days": schedule.interval_days,
        "start_at_ms": schedule.start_at_ms,
        "end_at_ms": schedule.end_at_ms,
        "active_start_at_ms": evaluation.active_start_ms,
        "active_end_at_ms": evaluation.active_end_ms,
        "next_start_at_ms": evaluation.next_start_ms,
        "next_end_at_ms": evaluation.next_end_ms,
        "temporary_open_until_ms": temporary_open_until_ms,
        "underlying_status": status_name(evaluation.status),
        "status": status_name(status),
        "is_open": status == ActivityWindowStatus::Open,
        "server_time_ms": now_ms,
        "created_at": schedule.created_at,
        "updated_at": schedule.updated_at,
    }))
}

pub(crate) fn status_name(status: ActivityWindowStatus) -> &'static str {
    match status {
        ActivityWindowStatus::Unscheduled => "unscheduled",
        ActivityWindowStatus::Disabled => "disabled",
        ActivityWindowStatus::NotStarted => "not_started",
        ActivityWindowStatus::Open => "open",
        ActivityWindowStatus::Ended => "ended",
    }
}
// //// /管理单个活动时间窗口 ////

fn json_response(status: &'static str, value: Value) -> Result<HttpResponse, PersonalServiceError> {
    let body = serde_json::to_string(&value).map_err(|error| {
        PersonalServiceError::new(format!(
            "failed to encode activity calendar response: {error}"
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
