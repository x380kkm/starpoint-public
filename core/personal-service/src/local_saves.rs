// audience: internal
// # personal-service-local-save-api
//
// 该模块提供受管理 token 保护的本地存档槽 API. 导入创建新槽而不覆盖现有槽,
// 回滚先保存当前状态. 自动快照带有来源标记并限制保留数量. 导出内容不包含实例
// 身份, 关系, 权限, 管理凭据或加密密钥.

use crate::database::{
    LocalIssuedTransferToken, LocalSaveExport, LocalSaveRevision, LocalSaveSlot, LocalSaveSnapshot,
    LocalSaveState, LocalSaveStoreError, LocalTransferPermission, LocalTransferTokenMetadata,
    ServiceDatabase,
};
use crate::http::{HttpRequest, HttpResponse};
use crate::management;
use crate::portable_save;
use crate::PersonalServiceError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod automation;
mod encryption;
mod recovery;
mod remote_target;
mod routes;
mod sync;
mod transfer;
mod transfer_bindings;

pub(crate) use automation::SaveAutomationRunner;
pub(crate) use transfer_bindings::TransferBindingRunner;

const LOCAL_SAVES_PATH: &str = "/v1/local-saves";
const PLAYER_LOCAL_SAVES_PATH: &str = "/v1/player/local-saves";
const LEGACY_EXPORT_FORMAT: &str = "starpoint-local-save";
const LEGACY_EXPORT_VERSION: i64 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivateRequest {
    device_id: i64,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotRequest {
    label: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedImportRequest {
    #[serde(default)]
    name: Option<String>,
    envelope: encryption::EncryptedSaveEnvelope,
}

#[derive(Serialize)]
struct LocalSaveSlotResponse {
    id: i64,
    name: String,
    created_at: String,
    updated_at: String,
    snapshot_count: i64,
}

#[derive(Serialize)]
struct LocalSaveDeviceResponse {
    device_id: i64,
    active_slot_id: i64,
}

#[derive(Serialize)]
struct LocalSaveContextResponse {
    slot: LocalSaveSlotResponse,
    account_id: i64,
    viewer_id: Option<i64>,
    active_device_ids: Vec<i64>,
}

#[derive(Serialize)]
struct LocalSaveStateResponse {
    slots: Vec<LocalSaveSlotResponse>,
    devices: Vec<LocalSaveDeviceResponse>,
}

#[derive(Serialize)]
struct LocalSaveSnapshotResponse {
    id: i64,
    slot_id: i64,
    label: String,
    created_at: String,
}

#[derive(Serialize)]
struct LocalSaveRevisionResponse {
    id: String,
    slot_id: i64,
    parent_revision_id: Option<String>,
    etag: String,
    label: String,
    created_at: String,
    pinned: bool,
}

#[derive(Serialize)]
struct LocalSaveRevisionStateResponse {
    current_revision_id: String,
    revisions: Vec<LocalSaveRevisionResponse>,
}

#[derive(Serialize)]
struct RestoreResponse {
    restored: bool,
    safety_snapshot: LocalSaveSnapshotResponse,
}

#[derive(Serialize)]
struct RestoreRevisionResponse {
    restored: bool,
    revision: LocalSaveRevisionResponse,
}

struct ImportRequest {
    name: String,
    data: Value,
}

// //// 将数据库快照编码为可移植游戏数据 [@x380kkm 2026-08-03] ////
fn sanitize_serialized_game_data(data_json: &str) -> Result<String, PersonalServiceError> {
    let data = serde_json::from_str::<Value>(data_json).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode portable game data: {error}"))
    })?;
    let data = portable_save::sanitize_game_data(data)
        .ok_or_else(|| PersonalServiceError::new("portable game data is invalid"))?;
    serde_json::to_string(&data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode portable game data: {error}"))
    })
}
// //// /将数据库快照编码为可移植游戏数据 ////

// //// 校验本地存档管理请求 [@x380kkm 2026-07-23] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let path = request.path();
    let prefix = format!("{LOCAL_SAVES_PATH}/");
    if path != LOCAL_SAVES_PATH
        && !path.starts_with(&prefix)
        && !sync::is_path(path)
        && !recovery::is_admin_path(path)
    {
        return None;
    }
    if !management::is_authorized(request, database) {
        return Some(Ok(management::unauthorized_response()));
    }
    if let Some(response) = crate::ai_teams::route(request, database) {
        return Some(response);
    }
    if let Some(response) = transfer::authorized_route(request, database) {
        return Some(response);
    }
    if let Some(response) = automation::route(request, database) {
        return Some(response);
    }
    if let Some(response) = transfer_bindings::route(request, database) {
        return Some(response);
    }
    if let Some(response) = sync::route(request, database) {
        return Some(response);
    }
    if recovery::is_admin_path(path) {
        return recovery::admin_route(request, database);
    }
    Some(routes::route_authorized(request, database, &prefix))
}
// //// /校验本地存档管理请求 ////

// //// 分派 transfer token 保护的可移植存档请求 [@x380kkm 2026-07-27] ////
pub(crate) fn transfer_route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    transfer::route(request, database)
}
// //// /分派 transfer token 保护的可移植存档请求 ////

// //// 分派玩家权限范围内的存档导入导出 [@x380kkm 2026-07-24] ////
pub(crate) fn player_route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let path = request.path();
    let prefix = format!("{PLAYER_LOCAL_SAVES_PATH}/");
    if path != PLAYER_LOCAL_SAVES_PATH
        && !path.starts_with(&prefix)
        && !sync::is_player_path(path)
        && !recovery::is_player_path(path)
    {
        return None;
    }
    let Some(account_id) = management::player_account_id(request, database) else {
        return Some(Ok(management::player_unauthorized_response()));
    };
    if recovery::is_player_path(path) {
        return recovery::player_route(request, database, account_id);
    }
    if sync::is_player_path(path) {
        return sync::player_route(request, database, account_id);
    }
    if path == PLAYER_LOCAL_SAVES_PATH {
        return Some(if request.method() == "GET" {
            database
                .list_local_saves_for_account(account_id)
                .and_then(|state| serialize_json("200 OK", state_response(state)))
        } else {
            Ok(method_not_allowed())
        });
    }
    let suffix = path.strip_prefix(&prefix).unwrap_or_default();
    let segments = suffix.split('/').collect::<Vec<_>>();
    let response = match (request.method(), segments.as_slice()) {
        ("POST", ["import"]) => import_player_local_save(request, database, account_id),
        ("POST", ["import-encrypted"]) => {
            import_player_encrypted_local_save(request, database, account_id)
        }
        ("GET", [slot_id, "export"]) => export_player_local_save(database, account_id, slot_id),
        ("GET", [slot_id, "encrypted-export"]) => {
            export_player_encrypted_local_save(database, account_id, slot_id)
        }
        ("POST", [slot_id, "activate"]) => {
            activate_player_local_save(request, database, account_id, slot_id)
        }
        (_, ["import"])
        | (_, ["import-encrypted"])
        | (_, [_, "export"])
        | (_, [_, "encrypted-export"])
        | (_, [_, "activate"]) => Ok(method_not_allowed()),
        _ => Ok(json_error(
            "404 Not Found",
            "player_local_save_route_not_found",
        )),
    };
    Some(response)
}
// //// /分派玩家权限范围内的存档导入导出 ////

fn import_player_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(import) = parse_import_request(request) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_import"));
    };
    let Some(data) = portable_save::sanitize_game_data(import.data) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_data"));
    };
    let data_json = serde_json::to_string(&data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode imported local save: {error}"))
    })?;
    map_slot_result(database.import_local_save_for_account(account_id, &import.name, &data_json))
}

fn import_player_encrypted_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(import) = parse_json::<EncryptedImportRequest>(request) else {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_encrypted_local_save_import",
        ));
    };
    let Some(name) = normalize_import_name(import.name.as_deref()) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_name"));
    };
    let key = database.get_or_create_local_save_encryption_key()?;
    let Some(data_json) = encryption::decrypt_player_data(&import.envelope, &key) else {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_encrypted_local_save",
        ));
    };
    let Ok(data) = serde_json::from_str::<Value>(&data_json) else {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_encrypted_local_save",
        ));
    };
    let Some(data) = portable_save::sanitize_game_data(data) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_data"));
    };
    let data_json = serde_json::to_string(&data).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode encrypted local save: {error}"))
    })?;
    map_slot_result(database.import_local_save_for_account(account_id, &name, &data_json))
}

fn export_player_local_save(
    database: &mut ServiceDatabase,
    account_id: i64,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(export) = database.export_local_save_for_account(account_id, slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    routes::serialize_export(export)
}

fn export_player_encrypted_local_save(
    database: &mut ServiceDatabase,
    account_id: i64,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(export) = database.export_local_save_for_account(account_id, slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    let key = database.get_or_create_local_save_encryption_key()?;
    let data_json = sanitize_serialized_game_data(&export.data_json)?;
    let envelope = encryption::encrypt_player_data(&data_json, &key)?;
    serialize_json("200 OK", envelope)
}

fn activate_player_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<ActivateRequest>(request))
    else {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_local_save_activation",
        ));
    };
    match database.activate_local_save_for_account(account_id, slot_id, body.device_id) {
        Ok(()) => serialize_json(
            "200 OK",
            state_response(database.list_local_saves_for_account(account_id)?),
        ),
        Err(error) => map_store_error(error),
    }
}

fn parse_import_request(request: &HttpRequest) -> Option<ImportRequest> {
    let body = parse_json::<Value>(request)?;
    let object = body.as_object()?;
    let requested_name = parse_optional_import_name(object)?;
    let direct_format = import_format(object)?;

    if direct_format == Some(portable_save::FORMAT) {
        let mut package_body = body.clone();
        package_body.as_object_mut()?.remove("name");
        return import_portable_package(package_body, requested_name);
    }

    let nested = object.get("data");
    let nested_format = match nested.and_then(Value::as_object) {
        Some(data) => import_format(data)?,
        None => None,
    };
    if direct_format.is_none() && nested_format == Some(portable_save::FORMAT) {
        return import_portable_package(nested?.clone(), requested_name);
    }

    if direct_format.is_some() && direct_format != Some(LEGACY_EXPORT_FORMAT) {
        return None;
    }
    if direct_format == Some(LEGACY_EXPORT_FORMAT) {
        if object.get("version")?.as_i64()? != LEGACY_EXPORT_VERSION {
            return None;
        }
        return Some(ImportRequest {
            name: requested_name
                .or_else(|| legacy_import_name(object))
                .unwrap_or_else(default_import_name),
            data: object.get("data")?.clone(),
        });
    }

    if nested_format == Some(LEGACY_EXPORT_FORMAT) {
        let nested = nested?.as_object()?;
        if nested.get("version")?.as_i64()? != LEGACY_EXPORT_VERSION {
            return None;
        }
        return Some(ImportRequest {
            name: requested_name
                .or_else(|| legacy_import_name(nested))
                .unwrap_or_else(default_import_name),
            data: nested.get("data")?.clone(),
        });
    }

    Some(ImportRequest {
        name: requested_name.unwrap_or_else(default_import_name),
        data: object.get("data").cloned().unwrap_or(body),
    })
}

fn import_portable_package(
    package: Value,
    requested_name: Option<String>,
) -> Option<ImportRequest> {
    let package = portable_save::parse_package(package)?;
    let source_name = package.source.slot_name.as_deref().and_then(normalize_text);
    Some(ImportRequest {
        name: requested_name
            .or(source_name)
            .unwrap_or_else(default_import_name),
        data: package.data,
    })
}

fn parse_optional_import_name(object: &serde_json::Map<String, Value>) -> Option<Option<String>> {
    match object.get("name") {
        None => Some(None),
        Some(Value::String(value)) if value.trim().is_empty() => Some(None),
        Some(Value::String(value)) => normalize_text(value).map(Some),
        Some(_) => None,
    }
}

fn import_format(object: &serde_json::Map<String, Value>) -> Option<Option<&str>> {
    match object.get("format") {
        None => Some(None),
        Some(Value::String(value)) => Some(Some(value)),
        Some(_) => None,
    }
}

fn legacy_import_name(object: &serde_json::Map<String, Value>) -> Option<String> {
    object
        .get("name")
        .and_then(Value::as_str)
        .and_then(normalize_text)
        .or_else(|| {
            object
                .get("slot")
                .and_then(Value::as_object)
                .and_then(|slot| slot.get("name"))
                .and_then(Value::as_str)
                .and_then(normalize_text)
        })
}

fn default_import_name() -> String {
    "Imported save".to_owned()
}

fn normalize_import_name(value: Option<&str>) -> Option<String> {
    match value {
        None => Some(default_import_name()),
        Some(value) if value.trim().is_empty() => Some(default_import_name()),
        Some(value) => normalize_text(value),
    }
}

fn parse_json<T: DeserializeOwned>(request: &HttpRequest) -> Option<T> {
    if !request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return None;
    }
    serde_json::from_slice(request.body()).ok()
}

fn normalize_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= 64 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn parse_id(value: &str) -> Option<i64> {
    value.parse::<i64>().ok().filter(|value| *value > 0)
}

fn state_response(state: LocalSaveState) -> LocalSaveStateResponse {
    LocalSaveStateResponse {
        slots: state.slots.into_iter().map(slot_response).collect(),
        devices: state
            .devices
            .into_iter()
            .map(|device| LocalSaveDeviceResponse {
                device_id: device.device_id,
                active_slot_id: device.active_slot_id,
            })
            .collect(),
    }
}

fn slot_response(slot: LocalSaveSlot) -> LocalSaveSlotResponse {
    LocalSaveSlotResponse {
        id: slot.id,
        name: slot.name,
        created_at: slot.created_at,
        updated_at: slot.updated_at,
        snapshot_count: slot.snapshot_count,
    }
}

fn snapshot_response(snapshot: LocalSaveSnapshot) -> LocalSaveSnapshotResponse {
    LocalSaveSnapshotResponse {
        id: snapshot.id,
        slot_id: snapshot.slot_id,
        label: snapshot.label,
        created_at: snapshot.created_at,
    }
}

fn revision_response(revision: LocalSaveRevision) -> LocalSaveRevisionResponse {
    LocalSaveRevisionResponse {
        id: revision.id,
        slot_id: revision.slot_id,
        parent_revision_id: revision.parent_revision_id,
        etag: revision.etag,
        label: revision.label,
        created_at: revision.created_at,
        pinned: revision.pinned,
    }
}

fn transfer_token_response(metadata: LocalTransferTokenMetadata) -> serde_json::Value {
    serde_json::json!({
        "id": metadata.id,
        "slotId": metadata.slot_id,
        "permission": metadata.permission.map(transfer_permission_response),
        "deviceName": metadata.device_name,
        "createdAt": metadata.created_at,
        "expiresAt": metadata.expires_at,
        "revokedAt": metadata.revoked_at,
    })
}

fn transfer_permission_response(permission: LocalTransferPermission) -> &'static str {
    match permission {
        LocalTransferPermission::Upload => "upload",
        LocalTransferPermission::Download => "download",
        LocalTransferPermission::Both => "both",
    }
}

fn map_slot_result(
    result: Result<LocalSaveSlot, LocalSaveStoreError>,
) -> Result<HttpResponse, PersonalServiceError> {
    match result {
        Ok(slot) => serialize_json("201 Created", slot_response(slot)),
        Err(error) => map_store_error(error),
    }
}

fn map_store_error(error: LocalSaveStoreError) -> Result<HttpResponse, PersonalServiceError> {
    match error {
        LocalSaveStoreError::NotFound => Ok(json_error("404 Not Found", "local_save_not_found")),
        LocalSaveStoreError::Busy => Ok(json_error("409 Conflict", "local_save_busy")),
        LocalSaveStoreError::InvalidState => {
            Ok(json_error("409 Conflict", "local_save_invalid_state"))
        }
        LocalSaveStoreError::Storage(error) => Err(error),
    }
}

fn serialize_json<T: Serialize>(
    status: &'static str,
    value: T,
) -> Result<HttpResponse, PersonalServiceError> {
    serde_json::to_string(&value)
        .map(|body| HttpResponse::json(status, body))
        .map_err(|error| {
            PersonalServiceError::new(format!("failed to encode local save response: {error}"))
        })
}

fn method_not_allowed() -> HttpResponse {
    json_error("405 Method Not Allowed", "method_not_allowed")
}

fn json_error(status: &'static str, error: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{error}\"}}"))
}
