// audience: internal | external
// # personal-service-virtual-time-api
//
// 该模块提供受管理 token 保护的本地虚拟时间 API. 设置只影响个人服务当前设备.

use crate::database::{parse_iso_timestamp, ServiceDatabase, VirtualTimeState};
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};

const VIRTUAL_TIME_PATH: &str = "/v1/time";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VirtualTimeRequest {
    enabled: bool,
    unix_time_ms: Option<i64>,
    iso: Option<String>,
    rate: f64,
}

#[derive(Serialize)]
struct VirtualTimeResponse {
    enabled: bool,
    unix_time_ms: i64,
    iso: String,
    rate: f64,
}

// //// 分派受保护的虚拟时间请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.path() != VIRTUAL_TIME_PATH {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    Some(match request.method() {
        "GET" => get_time(database),
        "PUT" => set_time(request, database),
        _ => Ok(json_error("405 Method Not Allowed", "method_not_allowed")),
    })
}
// //// /分派受保护的虚拟时间请求 ////

// //// 读取当前虚拟时间状态 [@x380kkm 2026-07-24] ////
fn get_time(database: &ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    serialize_response("200 OK", database.virtual_time_state()?)
}
// //// /读取当前虚拟时间状态 ////

// //// 验证并保存虚拟时间设置 [@x380kkm 2026-07-24] ////
fn set_time(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return Ok(json_error("400 Bad Request", "invalid_virtual_time"));
    }
    let Ok(body) = serde_json::from_slice::<VirtualTimeRequest>(request.body()) else {
        return Ok(json_error("400 Bad Request", "invalid_virtual_time"));
    };
    if body.unix_time_ms.is_some() && body.iso.is_some() {
        return Ok(json_error("400 Bad Request", "invalid_virtual_time"));
    }
    let unix_time_ms = match (body.unix_time_ms, body.iso.as_deref()) {
        (Some(value), None) => Some(value),
        (None, Some(value)) => parse_iso_timestamp(value),
        (None, None) => Some(database.virtual_time_state()?.unix_time_ms),
        (Some(_), Some(_)) => None,
    };
    let Some(unix_time_ms) = unix_time_ms else {
        return Ok(json_error("400 Bad Request", "invalid_virtual_time"));
    };
    if unix_time_ms < 0 || !body.rate.is_finite() || !(0.0 < body.rate && body.rate <= 1000.0) {
        return Ok(json_error("400 Bad Request", "invalid_virtual_time"));
    }
    database.set_virtual_time(body.enabled, unix_time_ms, body.rate)?;
    serialize_response("200 OK", database.virtual_time_state()?)
}
// //// /验证并保存虚拟时间设置 ////

fn serialize_response(
    status: &'static str,
    state: VirtualTimeState,
) -> Result<HttpResponse, PersonalServiceError> {
    serde_json::to_string(&VirtualTimeResponse {
        enabled: state.enabled,
        unix_time_ms: state.unix_time_ms,
        iso: state.iso,
        rate: state.rate,
    })
    .map(|body| HttpResponse::json(status, body))
    .map_err(|error| {
        PersonalServiceError::new(format!("failed to encode virtual time response: {error}"))
    })
}

fn json_error(status: &'static str, error: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{error}\"}}"))
}
