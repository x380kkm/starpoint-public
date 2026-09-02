// audience: internal | external
// # local-save-sync-targets
//
// 该模块管理密文存档服务器地址和登录凭据. API 响应不包含密码.

use super::*;
use crate::database::{SaveSyncStoreError, SaveSyncTarget, SaveSyncTargetInput};
use url::Host;

const TARGETS_PATH: &str = "/v1/save-sync-targets";
const PLAYER_TARGETS_PATH: &str = "/v1/player/save-sync-targets";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetRequest {
    name: String,
    scheme: String,
    host: String,
    port: u16,
    username: String,
    password: String,
}

#[derive(Serialize)]
struct TargetResponse {
    id: i64,
    name: String,
    scheme: String,
    host: String,
    port: i64,
    username: String,
    has_credentials: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct PlayerTargetResponse {
    id: i64,
    name: String,
    scheme: String,
    host: String,
    port: i64,
    has_credentials: bool,
}

pub(super) fn is_path(path: &str) -> bool {
    path == TARGETS_PATH || path.starts_with(&format!("{TARGETS_PATH}/"))
}

pub(super) fn is_player_path(path: &str) -> bool {
    path == PLAYER_TARGETS_PATH
}

pub(super) fn player_route(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.path() != PLAYER_TARGETS_PATH {
        return None;
    }
    Some(match request.method() {
        "GET" => list_player_targets(database),
        _ => Ok(method_not_allowed()),
    })
}

// //// 分派密文存档服务器配置请求 [@x380kkm 2026-07-23] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.path() == TARGETS_PATH {
        return Some(match request.method() {
            "GET" => list_targets(database),
            "POST" => create_target(request, database),
            _ => Ok(method_not_allowed()),
        });
    }
    request
        .path()
        .strip_prefix(&format!("{TARGETS_PATH}/"))
        .map(|target_id| match request.method() {
            "PUT" => update_target(request, database, target_id),
            "DELETE" => delete_target(database, target_id),
            _ => Ok(method_not_allowed()),
        })
}
// //// /分派密文存档服务器配置请求 ////

// //// 创建, 列出和修改密文存档服务器 [@x380kkm 2026-07-23] ////
fn list_targets(database: &ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    serialize_json(
        "200 OK",
        database
            .list_save_sync_targets()?
            .into_iter()
            .map(target_response)
            .collect::<Vec<_>>(),
    )
}

fn list_player_targets(database: &ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    serialize_json(
        "200 OK",
        database
            .list_save_sync_targets()?
            .into_iter()
            .map(player_target_response)
            .collect::<Vec<_>>(),
    )
}

fn create_target(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(input) = parse_target_input(request) else {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_target"));
    };
    map_target_result(database.create_save_sync_target(&input), "201 Created")
}

fn update_target(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    target_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(target_id), Some(input)) = (parse_id(target_id), parse_target_input(request)) else {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_target"));
    };
    map_target_result(
        database.update_save_sync_target(target_id, &input),
        "200 OK",
    )
}

fn delete_target(
    database: &mut ServiceDatabase,
    target_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(target_id) = parse_id(target_id) else {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_target_id"));
    };
    match database.delete_save_sync_target(target_id) {
        Ok(()) => serialize_json("200 OK", serde_json::json!({ "deleted": true })),
        Err(error) => map_target_error(error),
    }
}
// //// /创建, 列出和修改密文存档服务器 ////

fn parse_target_input(request: &HttpRequest) -> Option<SaveSyncTargetInput> {
    let body = parse_json::<TargetRequest>(request)?;
    let name = normalize_text(&body.name)?;
    let scheme = body.scheme.trim().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = canonical_host(&body.host)?;
    if scheme == "http" && !is_loopback_host(&host) {
        return None;
    }
    let username = normalize_limited_text(&body.username, 128)?;
    let password = normalize_password(&body.password)?;
    Some(SaveSyncTargetInput {
        name,
        scheme,
        host,
        port: body.port,
        username,
        password,
    })
}

fn canonical_host(value: &str) -> Option<String> {
    let host = match Host::parse(value.trim()).ok()? {
        Host::Domain(host) => host,
        Host::Ipv4(host) => host.to_string(),
        Host::Ipv6(host) => host.to_string(),
    };
    (host.len() <= 253).then_some(host)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn normalize_limited_text(value: &str, maximum: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= maximum && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn normalize_password(value: &str) -> Option<String> {
    (!value.is_empty() && value.chars().count() <= 256 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn target_response(target: SaveSyncTarget) -> TargetResponse {
    TargetResponse {
        id: target.id,
        name: target.name,
        scheme: target.scheme,
        host: target.host,
        port: target.port,
        username: target.username,
        has_credentials: !target.password.is_empty(),
        created_at: target.created_at,
        updated_at: target.updated_at,
    }
}

fn player_target_response(target: SaveSyncTarget) -> PlayerTargetResponse {
    PlayerTargetResponse {
        id: target.id,
        name: target.name,
        scheme: target.scheme,
        host: target.host,
        port: target.port,
        has_credentials: !target.password.is_empty(),
    }
}

fn map_target_result(
    result: Result<SaveSyncTarget, SaveSyncStoreError>,
    status: &'static str,
) -> Result<HttpResponse, PersonalServiceError> {
    match result {
        Ok(target) => serialize_json(status, target_response(target)),
        Err(error) => map_target_error(error),
    }
}

fn map_target_error(error: SaveSyncStoreError) -> Result<HttpResponse, PersonalServiceError> {
    match error {
        SaveSyncStoreError::NotFound => {
            Ok(json_error("404 Not Found", "save_sync_target_not_found"))
        }
        SaveSyncStoreError::NameConflict => {
            Ok(json_error("409 Conflict", "save_sync_target_name_conflict"))
        }
        SaveSyncStoreError::Storage(error) => Err(error),
    }
}
