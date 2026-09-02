// audience: external
// # local-save-transfer
// 此模块用壳 token 管理本地账号壳的槽 token.
// 此模块用槽 token 导入和导出一个可移植本地存档.
// 公开传输路由不返回管理 token、登录会话或好友数据.

use super::*;
use crate::portable_save;
use serde::Deserialize;
use serde_json::Value;

const TRANSFER_PREFIX: &str = "/v1/transfer/v1/";
const INSTANCE_ID_HEADER: &str = "X-Starpoint-Instance-Id";
const SHELL_ID_HEADER: &str = "X-Starpoint-Shell-Id";
const SLOT_ID_HEADER: &str = "X-Starpoint-Slot-Id";

pub(super) struct LocalTransferSave {
    pub(super) package: portable_save::StarpointSavePackage,
    pub(super) revision_id: String,
    pub(super) etag: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShellTokenRequest {
    expires_at: Option<String>,
    device_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SlotTokenRequest {
    slot_id: Option<i64>,
    permission: String,
    expires_at: Option<String>,
    device_name: Option<String>,
}

fn parse_token_id(value: &str) -> Option<&str> {
    (value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    .then_some(value)
}

fn parse_permission(value: &str) -> Option<LocalTransferPermission> {
    match value {
        "upload" => Some(LocalTransferPermission::Upload),
        "download" => Some(LocalTransferPermission::Download),
        "both" => Some(LocalTransferPermission::Both),
        _ => None,
    }
}

fn bearer_token(request: &HttpRequest) -> Option<&str> {
    request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
}

fn transfer_unauthorized() -> HttpResponse {
    HttpResponse::json(
        "401 Unauthorized",
        "{\"error\":\"transfer_token_required\"}".to_owned(),
    )
    .with_header("WWW-Authenticate", "Bearer")
}

fn transfer_metadata_response(metadata: LocalTransferTokenMetadata) -> Value {
    transfer_token_response(metadata)
}

fn issue_response(
    kind: &str,
    issued: LocalIssuedTransferToken,
) -> Result<HttpResponse, PersonalServiceError> {
    serialize_json(
        "201 Created",
        serde_json::json!({
            "token": issued.token,
            "tokenType": kind,
            "instanceId": issued.instance_id,
            "metadata": transfer_metadata_response(issued.metadata),
        }),
    )
}

fn shell_for_request(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Result<Option<LocalTransferTokenMetadata>, PersonalServiceError> {
    let Some(token) = bearer_token(request) else {
        return Ok(None);
    };
    database.resolve_local_shell_transfer_token(token)
}

fn slot_for_request(
    request: &HttpRequest,
    database: &ServiceDatabase,
    slot_id: i64,
    permission: LocalTransferPermission,
) -> Result<Option<LocalTransferTokenMetadata>, PersonalServiceError> {
    let Some(token) = bearer_token(request) else {
        return Ok(None);
    };
    database.resolve_local_slot_transfer_token(token, slot_id, permission)
}

pub(super) fn create_local_transfer_save(
    database: &mut ServiceDatabase,
    slot_id: i64,
) -> Result<Option<LocalTransferSave>, PersonalServiceError> {
    let Some(export) = database.export_local_save(slot_id)? else {
        return Ok(None);
    };
    let data = serde_json::from_str(&export.data_json).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode transfer export: {error}"))
    })?;
    let package = portable_save::create_package(
        data,
        export.slot.updated_at.clone(),
        portable_save::PortableSaveSource {
            instance_kind: "local".to_owned(),
            slot_id: Some(export.slot.id.to_string()),
            slot_name: Some(export.slot.name),
            revision_id: Some(export.revision_id.clone()),
        },
    )?;
    let etag = package.payload_sha256.clone();
    Ok(Some(LocalTransferSave {
        package,
        revision_id: export.revision_id,
        etag,
    }))
}

fn export_transfer_save(
    database: &mut ServiceDatabase,
    slot_id: i64,
    shell_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(save) = create_local_transfer_save(database, slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    let response = serialize_json("200 OK", save.package)?
        .with_header_value("ETag", format!("\"{}\"", save.etag))
        .with_header("Cache-Control", "no-store");
    transfer_identity_response(response, database, shell_id, slot_id)
}

fn normalize_etag(value: &str) -> Option<&str> {
    let value = value.trim().strip_prefix("W/").unwrap_or(value.trim());
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    (value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    .then_some(value)
}

fn revision_conflict(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: i64,
) -> Result<Option<HttpResponse>, PersonalServiceError> {
    let current_revision =
        match database.ensure_local_save_revision(slot_id, "Before transfer upload") {
            Ok(revision) => revision,
            Err(error) => return map_store_error(error).map(Some),
        };
    let Some(current) = create_local_transfer_save(database, slot_id)? else {
        return Ok(Some(json_error("404 Not Found", "local_save_not_found")));
    };
    let Some(raw_etag) = request.header("if-match") else {
        return Ok(None);
    };
    let Some(expected_etag) = normalize_etag(raw_etag) else {
        return Ok(Some(json_error(
            "400 Bad Request",
            "invalid_save_revision_etag",
        )));
    };
    if expected_etag == current.etag {
        return Ok(None);
    }
    serialize_json(
        "409 Conflict",
        serde_json::json!({
            "error": "save_revision_conflict",
            "currentRevisionId": current_revision.id,
            "currentEtag": current.etag,
        }),
    )
    .map(Some)
}

// //// 分派受管理 token 保护的本地 transfer token 接口 [@x380kkm 2026-07-27] ////
pub(super) fn authorized_route(
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
        ("POST", [slot_id, "transfer-tokens", "shell"]) => {
            issue_shell_token(request, database, slot_id)
        }
        ("GET", [slot_id, "transfer-tokens", "shell"]) => list_shell_tokens(database, slot_id),
        ("DELETE", [slot_id, "transfer-tokens", "shell", token_id]) => {
            revoke_shell_token(database, slot_id, token_id)
        }
        ("POST", [slot_id, "transfer-tokens", "slot"]) => {
            issue_slot_token(request, database, slot_id)
        }
        ("GET", [slot_id, "transfer-tokens", "slot"]) => list_slot_tokens(database, slot_id),
        ("DELETE", [slot_id, "transfer-tokens", "slot", token_id]) => {
            revoke_slot_token(database, slot_id, token_id)
        }
        (_, [_, "transfer-tokens", "shell"])
        | (_, [_, "transfer-tokens", "shell", _])
        | (_, [_, "transfer-tokens", "slot"])
        | (_, [_, "transfer-tokens", "slot", _]) => Ok(method_not_allowed()),
        _ => return None,
    };
    Some(response.map(|response| response.with_header("Cache-Control", "no-store")))
}
// //// /分派受管理 token 保护的本地 transfer token 接口 ////

fn issue_shell_token(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<ShellTokenRequest>(request))
    else {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_transfer_shell_token",
        ));
    };
    match database.issue_local_shell_transfer_token(
        slot_id,
        body.expires_at.as_deref(),
        body.device_name.as_deref(),
    ) {
        Ok(Some(issued)) => issue_response("shell", issued),
        Ok(None) => Ok(json_error("404 Not Found", "local_save_not_found")),
        Err(error) => map_store_error(error),
    }
}

fn list_shell_tokens(
    database: &ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    match database.list_local_shell_transfer_tokens(slot_id) {
        Ok(Some(tokens)) => serialize_json(
            "200 OK",
            serde_json::json!({
                "instanceId": database.local_transfer_instance_id()?,
                "tokens": tokens.into_iter().map(transfer_metadata_response).collect::<Vec<_>>(),
            }),
        ),
        Ok(None) => Ok(json_error("404 Not Found", "local_save_not_found")),
        Err(error) => map_store_error(error),
    }
}

fn revoke_shell_token(
    database: &mut ServiceDatabase,
    slot_id: &str,
    token_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(token_id)) = (parse_id(slot_id), parse_token_id(token_id)) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_token_id"));
    };
    match database.revoke_local_shell_transfer_token(slot_id, token_id) {
        Ok(true) => serialize_json("200 OK", serde_json::json!({ "revoked": true })),
        Ok(false) => Ok(json_error("404 Not Found", "transfer_token_not_found")),
        Err(error) => map_store_error(error),
    }
}

fn issue_slot_token(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<SlotTokenRequest>(request))
    else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_slot_token"));
    };
    let Some(permission) = parse_permission(&body.permission) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_slot_token"));
    };
    match database.issue_local_slot_transfer_token(
        slot_id,
        permission,
        body.expires_at.as_deref(),
        body.device_name.as_deref(),
    ) {
        Ok(Some(issued)) => issue_response("slot", issued),
        Ok(None) => Ok(json_error("404 Not Found", "local_save_not_found")),
        Err(error) => map_store_error(error),
    }
}

fn list_slot_tokens(
    database: &ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    match database.list_local_slot_transfer_tokens(slot_id) {
        Ok(Some(tokens)) => serialize_json(
            "200 OK",
            serde_json::json!({
                "instanceId": database.local_transfer_instance_id()?,
                "tokens": tokens.into_iter().map(transfer_metadata_response).collect::<Vec<_>>(),
            }),
        ),
        Ok(None) => Ok(json_error("404 Not Found", "local_save_not_found")),
        Err(error) => map_store_error(error),
    }
}

fn revoke_slot_token(
    database: &mut ServiceDatabase,
    slot_id: &str,
    token_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(token_id)) = (parse_id(slot_id), parse_token_id(token_id)) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_token_id"));
    };
    match database.revoke_local_slot_transfer_token(slot_id, token_id) {
        Ok(true) => serialize_json("200 OK", serde_json::json!({ "revoked": true })),
        Ok(false) => Ok(json_error("404 Not Found", "transfer_token_not_found")),
        Err(error) => map_store_error(error),
    }
}

// //// 分派 transfer token 保护的公开存档接口 [@x380kkm 2026-07-27] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let segments = request
        .path()
        .strip_prefix(TRANSFER_PREFIX)?
        .split('/')
        .collect::<Vec<_>>();
    let response = match (request.method(), segments.as_slice()) {
        ("GET", ["shell", "slots"]) => list_shell_slots(request, database),
        ("POST", ["shell", "slot-tokens"]) => issue_shell_slot_token(request, database),
        ("DELETE", ["shell", "slots", slot_id, "tokens", token_id]) => {
            revoke_shell_slot_token(request, database, slot_id, token_id)
        }
        ("GET", ["slots", slot_id]) => download_slot(request, database, slot_id),
        ("PUT", ["slots", slot_id]) => upload_slot(request, database, slot_id),
        (_, ["shell", "slots"])
        | (_, ["shell", "slot-tokens"])
        | (_, ["shell", "slots", _, "tokens", _])
        | (_, ["slots", _]) => Ok(method_not_allowed()),
        _ => Ok(json_error("404 Not Found", "transfer_route_not_found")),
    };
    Some(response.map(|response| response.with_header("Cache-Control", "no-store")))
}
// //// /分派 transfer token 保护的公开存档接口 ////

fn list_shell_slots(
    request: &HttpRequest,
    database: &ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(shell) = shell_for_request(request, database)? else {
        return Ok(transfer_unauthorized());
    };
    let slots = database
        .list_owned_local_saves(shell.account_id)?
        .into_iter()
        .map(slot_response)
        .collect::<Vec<_>>();
    serialize_json(
        "200 OK",
        serde_json::json!({
            "instanceId": database.local_transfer_instance_id()?,
            "slots": slots,
        }),
    )
}

fn issue_shell_slot_token(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(shell) = shell_for_request(request, database)? else {
        return Ok(transfer_unauthorized());
    };
    let Some(body) = parse_json::<SlotTokenRequest>(request) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_slot_token"));
    };
    let Some(permission) = parse_permission(&body.permission) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_slot_token"));
    };
    let Some(slot_id) = body.slot_id.filter(|slot_id| *slot_id > 0) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_slot_token"));
    };
    if !database.local_save_is_owned_by_account(shell.account_id, slot_id)? {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    }
    match database.issue_local_slot_transfer_token(
        slot_id,
        permission,
        body.expires_at.as_deref(),
        body.device_name.as_deref(),
    ) {
        Ok(Some(issued)) => issue_response("slot", issued),
        Ok(None) => Ok(json_error("404 Not Found", "local_save_not_found")),
        Err(error) => map_store_error(error),
    }
}

fn revoke_shell_slot_token(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
    token_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(shell) = shell_for_request(request, database)? else {
        return Ok(transfer_unauthorized());
    };
    let (Some(slot_id), Some(token_id)) = (parse_id(slot_id), parse_token_id(token_id)) else {
        return Ok(json_error("400 Bad Request", "invalid_transfer_token_id"));
    };
    if !database.local_save_is_owned_by_account(shell.account_id, slot_id)? {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    }
    match database.revoke_local_slot_transfer_token(slot_id, token_id) {
        Ok(true) => serialize_json("200 OK", serde_json::json!({ "revoked": true })),
        Ok(false) => Ok(json_error("404 Not Found", "transfer_token_not_found")),
        Err(error) => map_store_error(error),
    }
}

fn download_slot(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(token) = slot_for_request(
        request,
        database,
        slot_id,
        LocalTransferPermission::Download,
    )?
    else {
        return Ok(transfer_unauthorized());
    };
    if !database.local_save_is_owned_by_account(token.account_id, slot_id)? {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    }
    export_transfer_save(database, slot_id, token.account_id)
}

fn upload_slot(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(token) =
        slot_for_request(request, database, slot_id, LocalTransferPermission::Upload)?
    else {
        return Ok(transfer_unauthorized());
    };
    if !database.local_save_is_owned_by_account(token.account_id, slot_id)? {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    }
    if database.local_save_has_active_single_quest(slot_id)? {
        return Ok(json_error("409 Conflict", "local_save_import_blocked"));
    }
    if let Some(response) = revision_conflict(request, database, slot_id)? {
        return Ok(response);
    }
    let Some(value) = parse_json::<Value>(request) else {
        return Ok(json_error("400 Bad Request", "invalid_save_package"));
    };
    let Some(package) = portable_save::parse_package(value) else {
        return Ok(json_error("400 Bad Request", "invalid_save_package"));
    };
    let portable_etag = package.payload_sha256.clone();
    let data_json = serde_json::to_string(&package.data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode transfer upload: {error}"))
    })?;
    match database.replace_local_save_data(slot_id, &data_json) {
        Ok(revision) => {
            let response = serialize_json(
                "200 OK",
                serde_json::json!({
                    "imported": true,
                    "slotId": slot_id,
                    "revisionId": revision.id,
                    "etag": portable_etag,
                }),
            )?
            .with_header_value("ETag", format!("\"{portable_etag}\""));
            transfer_identity_response(response, database, token.account_id, slot_id)
        }
        Err(error) => map_store_error(error),
    }
}

fn transfer_identity_response(
    response: HttpResponse,
    database: &ServiceDatabase,
    shell_id: i64,
    slot_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    Ok(response
        .with_header_value(INSTANCE_ID_HEADER, database.local_transfer_instance_id()?)
        .with_header_value(SHELL_ID_HEADER, shell_id.to_string())
        .with_header_value(SLOT_ID_HEADER, slot_id.to_string()))
}
