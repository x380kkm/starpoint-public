// audience: internal | external
// # local-save-sync
//
// 该模块配置密文存档服务器并为手动或自动上传提供同一密文路径. 下载始终创建
// 隔离槽位, API 从不返回远端密码.
// 上传前删除实例身份字段, 下载旧密文时也在创建槽位前删除实例身份字段.

use super::*;
use crate::database::SaveSyncBinding;
use serde_json::Value;

mod client;
mod targets;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadRequest {
    target_id: i64,
    object_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadRequest {
    target_id: i64,
    object_id: String,
    name: String,
}

#[derive(Serialize)]
struct UploadResponse {
    uploaded: bool,
    target_id: i64,
    slot_id: i64,
    object_id: String,
    etag: String,
    last_synced_at: String,
}

#[derive(Serialize)]
struct DownloadResponse {
    downloaded: bool,
    target_id: i64,
    object_id: String,
    etag: String,
    last_synced_at: String,
    slot: LocalSaveSlotResponse,
}

#[derive(Serialize)]
struct BindingResponse {
    target_id: i64,
    object_id: String,
    etag: String,
    last_synced_at: String,
}

const PLAYER_OBJECT_ID_MAX_LEN: usize = 40;

#[derive(Clone)]
pub(crate) struct SaveUploadIdentity {
    pub(crate) target_id: i64,
    pub(crate) slot_id: i64,
    pub(crate) object_id: String,
}

pub(crate) struct PreparedSaveUpload {
    pub(crate) identity: SaveUploadIdentity,
    target: crate::database::SaveSyncTarget,
    envelope: encryption::EncryptedSaveEnvelope,
    previous_etag: Option<String>,
}

pub(crate) struct CompletedSaveUpload {
    pub(crate) identity: SaveUploadIdentity,
    result: Result<String, SaveUploadError>,
}

pub(crate) enum SaveUploadError {
    TargetNotFound,
    LocalSaveNotFound,
    TargetUnusable,
    Authentication,
    CapacityExceeded,
    Conflict,
    RemoteNotFound,
    RemoteUnavailable,
    InvalidResponse,
    Storage(PersonalServiceError),
}

impl SaveUploadError {
    pub(crate) fn code(&self) -> &'static str {
        self.response_details()
            .map_or("save_sync_storage_failed", |(_, code, _)| code)
    }

    pub(crate) fn is_retryable(&self) -> bool {
        self.response_details()
            .is_some_and(|(_, _, is_retryable)| is_retryable)
    }

    fn response_details(&self) -> Option<(&'static str, &'static str, bool)> {
        match self {
            Self::TargetNotFound => Some(("404 Not Found", "save_sync_target_not_found", false)),
            Self::LocalSaveNotFound => Some(("404 Not Found", "local_save_not_found", false)),
            Self::TargetUnusable => Some(("400 Bad Request", "save_sync_target_unusable", false)),
            Self::Authentication => {
                Some(("502 Bad Gateway", "save_sync_authentication_failed", false))
            }
            Self::CapacityExceeded => {
                Some(("409 Conflict", "save_sync_remote_capacity_exceeded", false))
            }
            Self::Conflict => Some(("409 Conflict", "save_sync_remote_conflict", false)),
            Self::RemoteNotFound => Some(("404 Not Found", "save_sync_remote_not_found", false)),
            Self::RemoteUnavailable => {
                Some(("502 Bad Gateway", "save_sync_remote_unavailable", true))
            }
            Self::InvalidResponse => Some((
                "502 Bad Gateway",
                "save_sync_invalid_remote_response",
                false,
            )),
            Self::Storage(_) => None,
        }
    }
}

pub(super) fn is_path(path: &str) -> bool {
    targets::is_path(path)
        || (path.starts_with("/v1/local-saves/") && path.ends_with("/sync/upload"))
        || (path.starts_with("/v1/local-saves/") && path.ends_with("/sync-bindings"))
        || path == "/v1/local-saves/sync/download"
}

pub(super) fn is_player_path(path: &str) -> bool {
    targets::is_player_path(path)
        || (path.starts_with("/v1/player/local-saves/")
            && (path.ends_with("/sync/upload") || path.ends_with("/sync-bindings")))
        || path == "/v1/player/local-saves/sync/download"
}

// //// 分派密文存档服务器和同步请求 [@x380kkm 2026-07-23] ////
pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if let Some(response) = targets::route(request, database) {
        return Some(response);
    }
    let prefix = format!("{LOCAL_SAVES_PATH}/");
    let segments = request
        .path()
        .strip_prefix(&prefix)
        .unwrap_or_default()
        .split('/')
        .collect::<Vec<_>>();
    match (request.method(), segments.as_slice()) {
        ("POST", [slot_id, "sync", "upload"]) => {
            Some(upload_local_save(request, database, slot_id))
        }
        ("POST", ["sync", "download"]) => Some(download_local_save(request, database)),
        ("GET", [slot_id, "sync-bindings"]) => Some(list_bindings(database, slot_id)),
        (_, [_, "sync", "upload"]) | (_, [_, "sync-bindings"]) | (_, ["sync", "download"]) => {
            Some(Ok(method_not_allowed()))
        }
        _ => None,
    }
}

pub(super) fn player_route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if let Some(response) = targets::player_route(request, database) {
        return Some(response);
    }
    let prefix = "/v1/player/local-saves/";
    let segments = request
        .path()
        .strip_prefix(prefix)
        .unwrap_or_default()
        .split('/')
        .collect::<Vec<_>>();
    let response = match (request.method(), segments.as_slice()) {
        ("POST", [slot_id, "sync", "upload"]) => {
            upload_player_local_save(request, database, account_id, slot_id)
        }
        ("POST", ["sync", "download"]) => download_player_local_save(request, database, account_id),
        ("GET", [slot_id, "sync-bindings"]) => list_player_bindings(database, account_id, slot_id),
        (_, [_, "sync", "upload"]) | (_, [_, "sync-bindings"]) | (_, ["sync", "download"]) => {
            Ok(method_not_allowed())
        }
        _ => return None,
    };
    Some(response)
}
// //// /分派密文存档服务器和同步请求 ////

// //// 上传本地存档密文并保存远端版本 [@x380kkm 2026-07-23] ////
fn upload_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<UploadRequest>(request))
    else {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_upload"));
    };
    if body.target_id <= 0 || !is_valid_object_id(&body.object_id) {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_upload"));
    }
    let prepared = match prepare_save_upload(database, slot_id, body.target_id, &body.object_id) {
        Ok(prepared) => prepared,
        Err(error) => return map_upload_error(error),
    };
    let completed = execute_prepared_save_upload(prepared);
    let binding = match commit_completed_save_upload(database, completed) {
        Ok(binding) => binding,
        Err(error) => return map_upload_error(error),
    };
    serialize_json("200 OK", upload_response(body.target_id, slot_id, binding))
}
// //// /上传本地存档密文并保存远端版本 ////

// //// 准备, 执行并提交可异步运行的密文上传 [@x380kkm 2026-07-23] ////
pub(crate) fn prepare_save_upload(
    database: &mut ServiceDatabase,
    slot_id: i64,
    target_id: i64,
    object_id: &str,
) -> Result<PreparedSaveUpload, SaveUploadError> {
    let target = database
        .find_save_sync_target(target_id)
        .map_err(SaveUploadError::Storage)?
        .ok_or(SaveUploadError::TargetNotFound)?;
    let export = database
        .export_local_save(slot_id)
        .map_err(SaveUploadError::Storage)?
        .ok_or(SaveUploadError::LocalSaveNotFound)?;
    prepare_save_upload_with_export(database, target, export, target_id, slot_id, object_id)
}

fn prepare_save_upload_for_account(
    database: &mut ServiceDatabase,
    account_id: i64,
    slot_id: i64,
    target_id: i64,
    object_id: &str,
) -> Result<PreparedSaveUpload, SaveUploadError> {
    let target = database
        .find_save_sync_target(target_id)
        .map_err(SaveUploadError::Storage)?
        .ok_or(SaveUploadError::TargetNotFound)?;
    let export = database
        .export_local_save_for_account(account_id, slot_id)
        .map_err(SaveUploadError::Storage)?
        .ok_or(SaveUploadError::LocalSaveNotFound)?;
    prepare_save_upload_with_export(database, target, export, target_id, slot_id, object_id)
}

fn prepare_save_upload_with_export(
    database: &mut ServiceDatabase,
    target: crate::database::SaveSyncTarget,
    export: LocalSaveExport,
    target_id: i64,
    slot_id: i64,
    object_id: &str,
) -> Result<PreparedSaveUpload, SaveUploadError> {
    let data_json =
        sanitize_serialized_game_data(&export.data_json).map_err(SaveUploadError::Storage)?;
    let key = database
        .get_or_create_local_save_encryption_key()
        .map_err(SaveUploadError::Storage)?;
    let envelope =
        encryption::encrypt_player_data(&data_json, &key).map_err(SaveUploadError::Storage)?;
    let previous_etag = database
        .get_local_save_sync_binding(target_id, slot_id)
        .map_err(SaveUploadError::Storage)?
        .filter(|binding| binding.object_id == object_id)
        .map(|binding| binding.remote_etag);
    Ok(PreparedSaveUpload {
        identity: SaveUploadIdentity {
            target_id,
            slot_id,
            object_id: object_id.to_owned(),
        },
        target,
        envelope,
        previous_etag,
    })
}

pub(crate) fn execute_prepared_save_upload(prepared: PreparedSaveUpload) -> CompletedSaveUpload {
    let result = client::upload(
        &prepared.target,
        &prepared.identity.object_id,
        &prepared.envelope,
        prepared.previous_etag.as_deref(),
    )
    .map_err(SaveUploadError::from);
    CompletedSaveUpload {
        identity: prepared.identity,
        result,
    }
}

pub(crate) fn commit_completed_save_upload(
    database: &mut ServiceDatabase,
    completed: CompletedSaveUpload,
) -> Result<SaveSyncBinding, SaveUploadError> {
    let etag = completed.result?;
    database
        .save_local_save_sync_binding(
            completed.identity.target_id,
            completed.identity.slot_id,
            &completed.identity.object_id,
            &etag,
        )
        .map_err(SaveUploadError::Storage)
}

fn commit_completed_save_upload_for_account(
    database: &mut ServiceDatabase,
    account_id: i64,
    completed: CompletedSaveUpload,
) -> Result<SaveSyncBinding, SaveUploadError> {
    let etag = completed.result?;
    let identity = completed.identity;
    let binding = database
        .save_local_save_sync_binding_for_account(
            account_id,
            identity.target_id,
            identity.slot_id,
            &identity.object_id,
            &etag,
        )
        .map_err(SaveUploadError::Storage)?
        .ok_or(SaveUploadError::LocalSaveNotFound)?;
    Ok(binding)
}
// //// /准备, 执行并提交可异步运行的密文上传 ////

// //// 下载远端密文并创建隔离的本地槽 [@x380kkm 2026-07-23] ////
fn download_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json::<DownloadRequest>(request) else {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_download"));
    };
    let Some(name) = normalize_text(&body.name) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_name"));
    };
    if body.target_id <= 0 || !is_valid_object_id(&body.object_id) {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_download"));
    }
    let Some(target) = database.find_save_sync_target(body.target_id)? else {
        return Ok(json_error("404 Not Found", "save_sync_target_not_found"));
    };
    let downloaded = match client::download(&target, &body.object_id) {
        Ok(downloaded) => downloaded,
        Err(error) => return map_client_error(error),
    };
    let key = database.get_or_create_local_save_encryption_key()?;
    let Some(data_json) = encryption::decrypt_player_data(&downloaded.envelope, &key) else {
        return Ok(json_error("409 Conflict", "save_sync_key_unavailable"));
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
        PersonalServiceError::new(format!("failed to encode downloaded local save: {error}"))
    })?;
    let (slot, binding) = match database.import_synced_local_save(
        &name,
        &data_json,
        body.target_id,
        &body.object_id,
        &downloaded.etag,
    ) {
        Ok(result) => result,
        Err(error) => return map_store_error(error),
    };
    serialize_json(
        "201 Created",
        DownloadResponse {
            downloaded: true,
            target_id: body.target_id,
            object_id: binding.object_id,
            etag: binding.remote_etag,
            last_synced_at: binding.last_synced_at,
            slot: slot_response(slot),
        },
    )
}
// //// /下载远端密文并创建隔离的本地槽 ////

fn list_bindings(
    database: &ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(bindings) = database.list_local_save_sync_bindings(slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    serialize_json(
        "200 OK",
        bindings
            .into_iter()
            .map(|binding| BindingResponse {
                target_id: binding.target_id,
                object_id: binding.object_id,
                etag: binding.remote_etag,
                last_synced_at: binding.last_synced_at,
            })
            .collect::<Vec<_>>(),
    )
}

//// 上传玩家作用域的密文存档 [@x380kkm 2026-07-24] ////
fn upload_player_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<UploadRequest>(request))
    else {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_upload"));
    };
    if body.target_id <= 0 || !is_valid_player_object_id(&body.object_id) {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_upload"));
    }
    let remote_scope = database.get_or_create_player_remote_scope(account_id)?;
    let remote_object_id = scope_player_object_id(&remote_scope, &body.object_id);
    let prepared = match prepare_save_upload_for_account(
        database,
        account_id,
        slot_id,
        body.target_id,
        &remote_object_id,
    ) {
        Ok(prepared) => prepared,
        Err(error) => return map_upload_error(error),
    };
    let completed = execute_prepared_save_upload(prepared);
    let binding = match commit_completed_save_upload_for_account(database, account_id, completed) {
        Ok(binding) => binding,
        Err(error) => return map_upload_error(error),
    };
    serialize_json(
        "200 OK",
        UploadResponse {
            uploaded: true,
            target_id: body.target_id,
            slot_id,
            object_id: unscoped_player_object_id(&binding.object_id),
            etag: binding.remote_etag,
            last_synced_at: binding.last_synced_at,
        },
    )
}
//// /上传玩家作用域的密文存档 ////

//// 下载密文存档到玩家作用域 [@x380kkm 2026-07-24] ////
fn download_player_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json::<DownloadRequest>(request) else {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_download"));
    };
    let Some(name) = normalize_text(&body.name) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_name"));
    };
    if body.target_id <= 0 || !is_valid_player_object_id(&body.object_id) {
        return Ok(json_error("400 Bad Request", "invalid_save_sync_download"));
    }
    let Some(target) = database.find_save_sync_target(body.target_id)? else {
        return Ok(json_error("404 Not Found", "save_sync_target_not_found"));
    };
    let remote_scope = database.get_or_create_player_remote_scope(account_id)?;
    let remote_object_id = scope_player_object_id(&remote_scope, &body.object_id);
    let downloaded = match client::download(&target, &remote_object_id) {
        Ok(downloaded) => downloaded,
        Err(client::SyncClientError::NotFound) => {
            let legacy_object_id = scope_player_object_id(&account_id.to_string(), &body.object_id);
            match client::download(&target, &legacy_object_id) {
                Ok(downloaded) => downloaded,
                Err(error) => return map_client_error(error),
            }
        }
        Err(error) => return map_client_error(error),
    };
    let key = database.get_or_create_local_save_encryption_key()?;
    let Some(data_json) = encryption::decrypt_player_data(&downloaded.envelope, &key) else {
        return Ok(json_error("409 Conflict", "save_sync_key_unavailable"));
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
        PersonalServiceError::new(format!("failed to encode downloaded local save: {error}"))
    })?;
    let (slot, binding) = match database.import_synced_local_save_for_account(
        account_id,
        &name,
        &data_json,
        body.target_id,
        &remote_object_id,
        &downloaded.etag,
    ) {
        Ok(result) => result,
        Err(error) => return map_store_error(error),
    };
    serialize_json(
        "201 Created",
        DownloadResponse {
            downloaded: true,
            target_id: body.target_id,
            object_id: unscoped_player_object_id(&binding.object_id),
            etag: binding.remote_etag,
            last_synced_at: binding.last_synced_at,
            slot: slot_response(slot),
        },
    )
}
//// /下载密文存档到玩家作用域 ////

//// 列出玩家存档的密文同步绑定 [@x380kkm 2026-07-24] ////
fn list_player_bindings(
    database: &ServiceDatabase,
    account_id: i64,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(bindings) = database.list_local_save_sync_bindings_for_account(account_id, slot_id)?
    else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    serialize_json(
        "200 OK",
        bindings
            .into_iter()
            .map(|binding| BindingResponse {
                target_id: binding.target_id,
                object_id: unscoped_player_object_id(&binding.object_id),
                etag: binding.remote_etag,
                last_synced_at: binding.last_synced_at,
            })
            .collect::<Vec<_>>(),
    )
}
//// /列出玩家存档的密文同步绑定 ////

pub(super) fn is_valid_object_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn is_valid_player_object_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= PLAYER_OBJECT_ID_MAX_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn scope_player_object_id(remote_scope: &str, object_id: &str) -> String {
    format!("p{remote_scope}-{object_id}")
}

fn unscoped_player_object_id(object_id: &str) -> String {
    let Some(scoped) = object_id.strip_prefix('p') else {
        return object_id.to_owned();
    };
    if scoped.len() > 16 && scoped.as_bytes()[16] == b'-' {
        return scoped[17..].to_owned();
    }
    let Some((legacy_scope, logical_id)) = scoped.split_once('-') else {
        return object_id.to_owned();
    };
    if !legacy_scope.is_empty() && legacy_scope.bytes().all(|byte| byte.is_ascii_digit()) {
        logical_id.to_owned()
    } else {
        object_id.to_owned()
    }
}

fn upload_response(target_id: i64, slot_id: i64, binding: SaveSyncBinding) -> UploadResponse {
    UploadResponse {
        uploaded: true,
        target_id,
        slot_id,
        object_id: binding.object_id,
        etag: binding.remote_etag,
        last_synced_at: binding.last_synced_at,
    }
}

fn map_client_error(error: client::SyncClientError) -> Result<HttpResponse, PersonalServiceError> {
    map_upload_error(SaveUploadError::from(error))
}

fn map_upload_error(error: SaveUploadError) -> Result<HttpResponse, PersonalServiceError> {
    match error {
        SaveUploadError::Storage(error) => Err(error),
        error => {
            let (status, code, _) = error
                .response_details()
                .expect("non-storage upload errors have response details");
            Ok(json_error(status, code))
        }
    }
}

impl From<client::SyncClientError> for SaveUploadError {
    fn from(error: client::SyncClientError) -> Self {
        match error {
            client::SyncClientError::InvalidTarget => Self::TargetUnusable,
            client::SyncClientError::Authentication => Self::Authentication,
            client::SyncClientError::CapacityExceeded => Self::CapacityExceeded,
            client::SyncClientError::Conflict => Self::Conflict,
            client::SyncClientError::NotFound => Self::RemoteNotFound,
            client::SyncClientError::Unavailable => Self::RemoteUnavailable,
            client::SyncClientError::InvalidResponse => Self::InvalidResponse,
        }
    }
}
