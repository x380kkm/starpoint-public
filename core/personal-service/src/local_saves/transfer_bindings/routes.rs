// audience: external
// # transfer-binding-routes
//
// 该模块管理明确的跨实例槽位绑定并公开冲突解决操作.
// 响应不返回远端槽 token、服务器地址或本地管理凭据.

use super::client::{self, TransferEndpoint};
use super::*;
use crate::database::{CreateTransferBindingInput, ServerProfileMode, UpdateTransferBindingInput};
use serde::Deserialize;
use serde_json::json;

mod support;

use support::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBindingRequest {
    target_profile_id: i64,
    target_instance_kind: String,
    target_instance_id: String,
    target_slot_id: i64,
    target_token: String,
    upload_mode: String,
    pull_mode: String,
    conflict_policy: String,
    #[serde(default = "default_transfer_interval_seconds")]
    interval_seconds: i64,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateBindingRequest {
    upload_mode: String,
    pull_mode: String,
    conflict_policy: String,
    interval_seconds: i64,
    enabled: bool,
    target_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveConflictRequest {
    resolution: Option<String>,
}

// //// 分派传输绑定管理请求 [@x380kkm 2026-08-03] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let prefix = format!("{LOCAL_SAVES_PATH}/");
    let segments = request
        .path()
        .strip_prefix(&prefix)?
        .split('/')
        .collect::<Vec<_>>();
    let response = match (request.method(), segments.as_slice()) {
        ("GET", [slot_id, "transfer-bindings"]) => list_bindings(database, slot_id),
        ("POST", [slot_id, "transfer-bindings"]) => create_binding(request, database, slot_id),
        ("GET", [slot_id, "transfer-bindings", binding_id]) => {
            get_binding(database, slot_id, binding_id)
        }
        ("PUT", [slot_id, "transfer-bindings", binding_id]) => {
            update_binding(request, database, slot_id, binding_id)
        }
        ("DELETE", [slot_id, "transfer-bindings", binding_id]) => {
            delete_binding(database, slot_id, binding_id)
        }
        ("POST", [slot_id, "transfer-bindings", binding_id, "sync"]) => {
            synchronize_binding(database, slot_id, binding_id)
        }
        ("GET", [slot_id, "transfer-bindings", binding_id, "conflicts"]) => {
            list_conflicts(database, slot_id, binding_id)
        }
        (
            "POST",
            [slot_id, "transfer-bindings", binding_id, "conflicts", conflict_id, "resolve"],
        ) => resolve_conflict(request, database, slot_id, binding_id, conflict_id),
        (_, [_, "transfer-bindings"])
        | (_, [_, "transfer-bindings", _])
        | (_, [_, "transfer-bindings", _, "sync"])
        | (_, [_, "transfer-bindings", _, "conflicts"])
        | (_, [_, "transfer-bindings", _, "conflicts", _, "resolve"]) => Ok(method_not_allowed()),
        _ => return None,
    };
    Some(response.map(|response| response.with_header("Cache-Control", "no-store")))
}
// //// /分派传输绑定管理请求 ////

// //// 创建并验证目标槽绑定 [@x380kkm 2026-08-03] ////
fn create_binding(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    source_slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(source_slot_id), Some(body)) = (
        parse_id(source_slot_id),
        parse_json::<CreateBindingRequest>(request),
    ) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding"));
    };
    let Some(instance_kind) = parse_instance_kind(&body.target_instance_kind) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding"));
    };
    let Some(modes) = parse_modes(
        &body.upload_mode,
        &body.pull_mode,
        &body.conflict_policy,
        body.interval_seconds,
    ) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding"));
    };
    if body.target_profile_id <= 0
        || body.target_slot_id <= 0
        || !is_valid_instance_id(&body.target_instance_id)
        || !is_valid_slot_token(&body.target_token)
    {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding"));
    }
    let Some(profile) = database.get_server_profile(body.target_profile_id)? else {
        return Ok(json_error("404 Not Found", "server_profile_not_found"));
    };
    if profile.mode != ServerProfileMode::Remote {
        return Ok(json_error(
            "400 Bad Request",
            "transfer_target_profile_is_local",
        ));
    }
    let endpoint = TransferEndpoint {
        instance_kind,
        instance_id: body.target_instance_id.clone(),
        shell_id: None,
        slot_id: body.target_slot_id,
        token: body.target_token.clone(),
        scheme: profile.scheme.expect("remote server profile scheme"),
        host: profile.host.expect("remote server profile host"),
        port: profile.port.expect("remote server profile port"),
    };
    let target = match client::download(&endpoint) {
        Ok(target) => target,
        Err(error) => {
            return map_transfer_binding_operation_error(map_transfer_binding_client_error(error));
        }
    };
    let Some(source) = create_local_transfer_save(database, source_slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    let input = CreateTransferBindingInput {
        source_slot_id,
        target_profile_id: body.target_profile_id,
        target_instance_kind: instance_kind,
        target_instance_id: body.target_instance_id,
        target_shell_id: target.shell_id,
        target_slot_id: body.target_slot_id,
        target_token: body.target_token,
        upload_mode: modes.upload,
        pull_mode: modes.pull,
        conflict_policy: modes.conflict,
        interval_seconds: modes.interval_seconds,
        enabled: body.enabled,
        observed_source_etag: source.etag,
        observed_target_etag: target.etag,
    };
    match database.create_transfer_binding(&input) {
        Ok(binding) => serialize_json("201 Created", binding_response(&binding)),
        Err(error) => map_transfer_binding_store_error(error),
    }
}
// //// /创建并验证目标槽绑定 ////

// //// 读取, 更新和删除绑定 [@x380kkm 2026-08-03] ////
fn list_bindings(
    database: &ServiceDatabase,
    source_slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(source_slot_id) = parse_id(source_slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(bindings) = database.list_transfer_bindings(source_slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    serialize_json(
        "200 OK",
        bindings.iter().map(binding_response).collect::<Vec<_>>(),
    )
}

fn get_binding(
    database: &ServiceDatabase,
    source_slot_id: &str,
    binding_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(source_slot_id), Some(binding_id)) =
        (parse_id(source_slot_id), parse_object_id(binding_id))
    else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding_id"));
    };
    let Some(binding) = database.get_transfer_binding(binding_id)? else {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    };
    if binding.source_slot_id != source_slot_id {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    }
    serialize_json("200 OK", binding_response(&binding))
}

fn update_binding(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    source_slot_id: &str,
    binding_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(source_slot_id), Some(binding_id), Some(body)) = (
        parse_id(source_slot_id),
        parse_object_id(binding_id),
        parse_json::<UpdateBindingRequest>(request),
    ) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding"));
    };
    let Some(modes) = parse_modes(
        &body.upload_mode,
        &body.pull_mode,
        &body.conflict_policy,
        body.interval_seconds,
    ) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding"));
    };
    if body
        .target_token
        .as_deref()
        .is_some_and(|token| !is_valid_slot_token(token))
    {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding"));
    }
    let Some(binding) = database.get_transfer_binding(binding_id)? else {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    };
    if binding.source_slot_id != source_slot_id {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    }
    if let Some(token) = body.target_token.as_deref() {
        let mut endpoint = TransferEndpoint::from(&binding);
        endpoint.token = token.to_owned();
        if let Err(error) = client::download(&endpoint) {
            return map_transfer_binding_operation_error(map_transfer_binding_client_error(error));
        }
    }
    let input = UpdateTransferBindingInput {
        upload_mode: modes.upload,
        pull_mode: modes.pull,
        conflict_policy: modes.conflict,
        interval_seconds: modes.interval_seconds,
        enabled: body.enabled,
        target_token: body.target_token,
    };
    match database.update_transfer_binding(source_slot_id, binding_id, &input) {
        Ok(binding) => serialize_json("200 OK", binding_response(&binding)),
        Err(error) => map_transfer_binding_store_error(error),
    }
}

fn delete_binding(
    database: &mut ServiceDatabase,
    source_slot_id: &str,
    binding_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(source_slot_id), Some(binding_id)) =
        (parse_id(source_slot_id), parse_object_id(binding_id))
    else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding_id"));
    };
    match database.delete_transfer_binding(source_slot_id, binding_id) {
        Ok(()) => serialize_json("200 OK", json!({ "deleted": true })),
        Err(error) => map_transfer_binding_store_error(error),
    }
}
// //// /读取, 更新和删除绑定 ////

// //// 手动同步并管理冲突 [@x380kkm 2026-08-03] ////
fn synchronize_binding(
    database: &mut ServiceDatabase,
    source_slot_id: &str,
    binding_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(source_slot_id), Some(binding_id)) =
        (parse_id(source_slot_id), parse_object_id(binding_id))
    else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding_id"));
    };
    let Some(binding) = database.get_transfer_binding(binding_id)? else {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    };
    if binding.source_slot_id != source_slot_id {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    }
    match synchronize_transfer_binding(database, binding_id, TransferSyncTrigger::Manual) {
        Ok(TransferSyncOutcome::Synchronized { action, binding }) => serialize_json(
            "200 OK",
            json!({
                "synchronized": true,
                "action": action.as_str(),
                "binding": binding_response(&binding),
            }),
        ),
        Ok(TransferSyncOutcome::Conflict(conflict)) => serialize_json(
            "409 Conflict",
            json!({
                "error": "transfer_conflict",
                "conflict": conflict_response(&conflict),
            }),
        ),
        Err(error) => map_transfer_binding_operation_error(error),
    }
}

fn list_conflicts(
    database: &ServiceDatabase,
    source_slot_id: &str,
    binding_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(source_slot_id), Some(binding_id)) =
        (parse_id(source_slot_id), parse_object_id(binding_id))
    else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_binding_id"));
    };
    let Some(binding) = database.get_transfer_binding(binding_id)? else {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    };
    if binding.source_slot_id != source_slot_id {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    }
    let conflicts = database
        .list_transfer_conflicts(binding_id)?
        .unwrap_or_default();
    serialize_json(
        "200 OK",
        conflicts.iter().map(conflict_response).collect::<Vec<_>>(),
    )
}

fn resolve_conflict(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    source_slot_id: &str,
    binding_id: &str,
    conflict_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(source_slot_id), Some(binding_id), Some(conflict_id), Some(body)) = (
        parse_id(source_slot_id),
        parse_object_id(binding_id),
        parse_object_id(conflict_id),
        parse_json::<ResolveConflictRequest>(request),
    ) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_conflict"));
    };
    let Some(binding) = database.get_transfer_binding(binding_id)? else {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    };
    if binding.source_slot_id != source_slot_id {
        return Ok(json_error("404 Not Found", "transfer_binding_not_found"));
    }
    let Some(resolution) = parse_resolution(body.resolution.as_deref(), binding.conflict_policy)
    else {
        return Ok(json_error(
            "400 Bad Request",
            "transfer_resolution_required",
        ));
    };
    match resolve_open_transfer_conflict(database, binding_id, conflict_id, resolution) {
        Ok(resolved) => serialize_json(
            "200 OK",
            json!({
                "resolved": true,
                "conflict": conflict_response(&resolved.conflict),
                "binding": binding_response(&resolved.binding),
            }),
        ),
        Err(error) => map_transfer_binding_operation_error(error),
    }
}
// //// /手动同步并管理冲突 ////
