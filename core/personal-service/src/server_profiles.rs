// audience: internal
// # personal-service-server-profiles
//
// 该模块提供受管理 token 保护的设备级服务器配置 API. 远端配置保存连接地址, 不保存玩家快照.
// 验证切换先检查目标 /healthz, 探测失败时保留当前配置.

use crate::database::{
    RemoteServerProfileInput, ServerProfile, ServerProfileMode, ServerProfileState,
    ServerProfileStoreError, ServiceDatabase,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

mod probe;

const SERVER_PROFILES_PATH: &str = "/v1/server-profiles";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveRemoteServerProfileRequest {
    name: String,
    scheme: String,
    host: String,
    port: u16,
}

#[derive(Serialize)]
struct ServerProfileResponse {
    id: i64,
    name: String,
    mode: &'static str,
    scheme: Option<String>,
    host: Option<String>,
    port: Option<i64>,
    is_builtin: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct ServerProfileStateResponse {
    active_profile_id: i64,
    profiles: Vec<ServerProfileResponse>,
}

#[derive(Serialize)]
struct VerifiedActivationResponse {
    activated: bool,
    probe: probe::ServerProfileProbe,
    state: ServerProfileStateResponse,
}

#[derive(Serialize)]
struct ProbeFailureResponse {
    error: &'static str,
    probe: probe::ServerProfileProbe,
}

// //// 分派受保护的服务器配置请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let path = request.path();
    let profile_prefix = format!("{SERVER_PROFILES_PATH}/");
    if path != SERVER_PROFILES_PATH && !path.starts_with(&profile_prefix) {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    Some(route_authorized(request, database, &profile_prefix))
}

fn route_authorized(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    profile_prefix: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    if request.path() == SERVER_PROFILES_PATH {
        return match request.method() {
            "GET" => list_profiles(database),
            "POST" => create_profile(request, database),
            _ => Ok(method_not_allowed()),
        };
    }

    let suffix = request
        .path()
        .strip_prefix(profile_prefix)
        .unwrap_or_default();
    let segments = suffix.split('/').collect::<Vec<_>>();
    match (request.method(), segments.as_slice()) {
        ("PUT", [profile_id]) => update_profile(request, database, profile_id),
        ("DELETE", [profile_id]) => delete_profile(database, profile_id),
        ("POST", [profile_id, "activate"]) => activate_profile(database, profile_id),
        ("POST", [profile_id, "probe"]) => probe_profile(database, profile_id),
        ("POST", [profile_id, "activate-verified"]) => {
            activate_verified_profile(database, profile_id)
        }
        (_, [_]) | (_, [_, "activate" | "probe" | "activate-verified"]) => Ok(method_not_allowed()),
        _ => Ok(json_error(
            "404 Not Found",
            "server_profile_route_not_found",
        )),
    }
}
// //// /分派受保护的服务器配置请求 ////

// //// 读写和切换服务器配置 [@x380kkm 2026-07-24] ////
fn list_profiles(database: &ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    profile_state_response("200 OK", database.list_server_profiles()?)
}

fn create_profile(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(input) = parse_remote_profile(request) else {
        return Ok(json_error("400 Bad Request", "invalid_server_profile"));
    };
    match database.create_server_profile(&input) {
        Ok(profile) => serialize_json("201 Created", profile_response(profile)),
        Err(error) => map_store_error(error),
    }
}

fn update_profile(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    profile_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(profile_id), Some(input)) =
        (parse_profile_id(profile_id), parse_remote_profile(request))
    else {
        return Ok(json_error("400 Bad Request", "invalid_server_profile"));
    };
    match database.update_server_profile(profile_id, &input) {
        Ok(profile) => serialize_json("200 OK", profile_response(profile)),
        Err(error) => map_store_error(error),
    }
}

fn delete_profile(
    database: &mut ServiceDatabase,
    profile_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(profile_id) = parse_profile_id(profile_id) else {
        return Ok(json_error("400 Bad Request", "invalid_server_profile_id"));
    };
    match database.delete_server_profile(profile_id) {
        Ok(()) => profile_state_response("200 OK", database.list_server_profiles()?),
        Err(error) => map_store_error(error),
    }
}

fn activate_profile(
    database: &mut ServiceDatabase,
    profile_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(profile_id) = parse_profile_id(profile_id) else {
        return Ok(json_error("400 Bad Request", "invalid_server_profile_id"));
    };
    match database.activate_server_profile(profile_id) {
        Ok(()) => profile_state_response("200 OK", database.list_server_profiles()?),
        Err(error) => map_store_error(error),
    }
}

fn probe_profile(
    database: &ServiceDatabase,
    profile_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(profile_id) = parse_profile_id(profile_id) else {
        return Ok(json_error("400 Bad Request", "invalid_server_profile_id"));
    };
    let Some(profile) = find_profile(database, profile_id)? else {
        return Ok(json_error("404 Not Found", "server_profile_not_found"));
    };
    serialize_json("200 OK", probe::probe_server_profile(&profile))
}

fn activate_verified_profile(
    database: &mut ServiceDatabase,
    profile_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(profile_id) = parse_profile_id(profile_id) else {
        return Ok(json_error("400 Bad Request", "invalid_server_profile_id"));
    };
    let Some(profile) = find_profile(database, profile_id)? else {
        return Ok(json_error("404 Not Found", "server_profile_not_found"));
    };
    let probe = probe::probe_server_profile(&profile);
    if !probe.reachable || !probe.compatible {
        let error = if probe.reachable {
            "server_profile_incompatible"
        } else {
            "server_profile_unreachable"
        };
        return serialize_json("409 Conflict", ProbeFailureResponse { error, probe });
    }
    if let Err(error) = database.activate_server_profile(profile_id) {
        return map_store_error(error);
    }
    serialize_json(
        "200 OK",
        VerifiedActivationResponse {
            activated: true,
            probe,
            state: state_response(database.list_server_profiles()?),
        },
    )
}

fn find_profile(
    database: &ServiceDatabase,
    profile_id: i64,
) -> Result<Option<ServerProfile>, PersonalServiceError> {
    Ok(database
        .list_server_profiles()?
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id))
}
// //// /读写和切换服务器配置 ////

// //// 验证远端服务器地址 [@x380kkm 2026-07-23] ////
fn parse_remote_profile(request: &HttpRequest) -> Option<RemoteServerProfileInput> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return None;
    }
    let body = serde_json::from_slice::<SaveRemoteServerProfileRequest>(request.body()).ok()?;
    let name = body.name.trim();
    let scheme = body.scheme.trim().to_ascii_lowercase();
    let host = body.host.trim().to_ascii_lowercase();
    if name.is_empty()
        || name.chars().count() > 64
        || name.chars().any(char::is_control)
        || !matches!(scheme.as_str(), "http" | "https")
        || body.port == 0
        || !is_valid_host(&host)
    {
        return None;
    }
    Some(RemoteServerProfileInput {
        name: name.to_owned(),
        scheme,
        host,
        port: body.port,
    })
}

fn is_valid_host(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}
fn parse_profile_id(value: &str) -> Option<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|profile_id| *profile_id > 0)
}
// //// /验证远端服务器地址 ////

// //// 生成服务器配置 JSON 响应 [@x380kkm 2026-07-24] ////
fn profile_response(profile: ServerProfile) -> ServerProfileResponse {
    ServerProfileResponse {
        id: profile.id,
        name: profile.name,
        mode: match profile.mode {
            ServerProfileMode::Local => "local",
            ServerProfileMode::Remote => "remote",
        },
        scheme: profile.scheme,
        host: profile.host,
        port: profile.port,
        is_builtin: profile.is_builtin,
        created_at: profile.created_at,
        updated_at: profile.updated_at,
    }
}

fn profile_state_response(
    status: &'static str,
    state: ServerProfileState,
) -> Result<HttpResponse, PersonalServiceError> {
    serialize_json(status, state_response(state))
}

fn state_response(state: ServerProfileState) -> ServerProfileStateResponse {
    ServerProfileStateResponse {
        active_profile_id: state.active_profile_id,
        profiles: state.profiles.into_iter().map(profile_response).collect(),
    }
}

fn serialize_json<T: Serialize>(
    status: &'static str,
    value: T,
) -> Result<HttpResponse, PersonalServiceError> {
    serde_json::to_string(&value)
        .map(|body| HttpResponse::json(status, body))
        .map_err(|error| {
            PersonalServiceError::new(format!("failed to encode server profile response: {error}"))
        })
}

fn map_store_error(error: ServerProfileStoreError) -> Result<HttpResponse, PersonalServiceError> {
    match error {
        ServerProfileStoreError::NotFound => {
            Ok(json_error("404 Not Found", "server_profile_not_found"))
        }
        ServerProfileStoreError::NameConflict => {
            Ok(json_error("409 Conflict", "server_profile_name_conflict"))
        }
        ServerProfileStoreError::ActiveProfile => {
            Ok(json_error("409 Conflict", "active_server_profile"))
        }
        ServerProfileStoreError::BuiltinProfile => {
            Ok(json_error("409 Conflict", "builtin_server_profile"))
        }
        ServerProfileStoreError::Storage(error) => Err(error),
    }
}

fn method_not_allowed() -> HttpResponse {
    json_error("405 Method Not Allowed", "method_not_allowed")
}

fn json_error(status: &'static str, error: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{error}\"}}"))
}
// //// /生成服务器配置 JSON 响应 ////
