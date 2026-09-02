// audience: internal
// # local-save-routes
//
// 该模块分派本地存档管理 API 并调用存储接口.

use super::*;

// //// 分派本地存档管理请求 [@x380kkm 2026-07-23] ////
pub(super) fn route_authorized(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    prefix: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    if request.path() == LOCAL_SAVES_PATH {
        return match request.method() {
            "GET" => list_local_saves(database),
            _ => Ok(method_not_allowed()),
        };
    }
    let suffix = request.path().strip_prefix(prefix).unwrap_or_default();
    let segments = suffix.split('/').collect::<Vec<_>>();
    match (request.method(), segments.as_slice()) {
        ("POST", ["import"]) => import_local_save(request, database),
        ("POST", ["import-encrypted"]) => import_encrypted_local_save(request, database),
        ("GET", [slot_id, "export"]) => export_local_save(database, slot_id),
        ("GET", [slot_id, "encrypted-export"]) => export_encrypted_local_save(database, slot_id),
        ("POST", [slot_id, "copy"]) => copy_local_save(request, database, slot_id),
        ("POST", [slot_id, "activate"]) => activate_local_save(request, database, slot_id),
        ("GET", [slot_id, "context"]) => local_save_context(database, slot_id),
        ("GET", [slot_id, "snapshots"]) => list_snapshots(database, slot_id),
        ("POST", [slot_id, "snapshots"]) => create_snapshot(request, database, slot_id),
        ("POST", [slot_id, "snapshots", snapshot_id, "restore"]) => {
            restore_snapshot(database, slot_id, snapshot_id)
        }
        ("GET", [slot_id, "revisions"]) => list_revisions(database, slot_id),
        ("POST", [slot_id, "revisions", revision_id, "restore"]) => {
            restore_revision(request, database, slot_id, revision_id)
        }
        (_, ["import"])
        | (_, ["import-encrypted"])
        | (_, [_, "export"])
        | (_, [_, "encrypted-export"])
        | (_, [_, "copy"])
        | (_, [_, "activate"])
        | (_, [_, "context"])
        | (_, [_, "snapshots"])
        | (_, [_, "snapshots", _, "restore"])
        | (_, [_, "revisions"])
        | (_, [_, "revisions", _, "restore"]) => Ok(method_not_allowed()),
        _ => Ok(json_error("404 Not Found", "local_save_route_not_found")),
    }
}
// //// /分派本地存档管理请求 ////

// //// 列出和导出本地存档 [@x380kkm 2026-07-23] ////
fn list_local_saves(database: &ServiceDatabase) -> Result<HttpResponse, PersonalServiceError> {
    serialize_json("200 OK", state_response(database.list_local_saves()?))
}

fn export_local_save(
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(export) = database.export_local_save(slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    serialize_export(export)
}

pub(super) fn serialize_export(
    export: LocalSaveExport,
) -> Result<HttpResponse, PersonalServiceError> {
    let data = serde_json::from_str(&export.data_json).map_err(|error| {
        PersonalServiceError::new(format!("failed to decode local save export: {error}"))
    })?;
    let package = portable_save::create_package(
        data,
        export.slot.updated_at.clone(),
        portable_save::PortableSaveSource {
            instance_kind: "local".to_string(),
            slot_id: Some(export.slot.id.to_string()),
            slot_name: Some(export.slot.name),
            revision_id: Some(export.revision_id),
        },
    )?;
    serialize_json("200 OK", package)
        .map(|response| response.with_header_value("ETag", format!("\"{}\"", export.etag)))
}

fn export_encrypted_local_save(
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(export) = database.export_local_save(slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    let key = database.get_or_create_local_save_encryption_key()?;
    let data_json = sanitize_serialized_game_data(&export.data_json)?;
    let envelope = encryption::encrypt_player_data(&data_json, &key)?;
    serialize_json("200 OK", envelope)
}
// //// /列出和导出本地存档 ////

// //// 复制, 导入和切换本地存档 [@x380kkm 2026-07-23] ////
fn copy_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<NameRequest>(request))
    else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_copy"));
    };
    let Some(name) = normalize_text(&body.name) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_name"));
    };
    map_slot_result(database.copy_local_save(slot_id, &name))
}

fn import_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
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
    map_slot_result(database.import_local_save(&import.name, &data_json))
}

fn import_encrypted_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
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
    map_slot_result(database.import_local_save(&name, &data_json))
}

fn activate_local_save(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<ActivateRequest>(request))
    else {
        return Ok(json_error(
            "400 Bad Request",
            "invalid_local_save_activation",
        ));
    };
    match database.activate_local_save(slot_id, body.device_id) {
        Ok(()) => list_local_saves(database),
        Err(error) => map_store_error(error),
    }
}
// //// /复制, 导入和切换本地存档 ////

// //// 返回本地存档槽的账号和当前 viewer 上下文 [@x380kkm 2026-08-18] ////
fn local_save_context(
    database: &ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(context) = database.local_save_context(slot_id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    serialize_json(
        "200 OK",
        LocalSaveContextResponse {
            slot: slot_response(context.slot),
            account_id: context.account_id,
            viewer_id: context.viewer_id,
            active_device_ids: context.active_device_ids,
        },
    )
}
// //// /返回本地存档槽的账号和当前 viewer 上下文 ////

// //// 创建, 列出和恢复本地存档快照 [@x380kkm 2026-07-23] ////
fn list_snapshots(
    database: &ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    match database.list_local_save_snapshots(slot_id)? {
        Some(snapshots) => serialize_json(
            "200 OK",
            snapshots
                .into_iter()
                .map(snapshot_response)
                .collect::<Vec<_>>(),
        ),
        None => Ok(json_error("404 Not Found", "local_save_not_found")),
    }
}

fn create_snapshot(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(body)) = (parse_id(slot_id), parse_json::<SnapshotRequest>(request))
    else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_snapshot"));
    };
    let label = body.label.as_deref().unwrap_or("Manual snapshot");
    let Some(label) = normalize_text(label) else {
        return Ok(json_error("400 Bad Request", "invalid_snapshot_label"));
    };
    match database.create_local_save_snapshot(slot_id, &label) {
        Ok(snapshot) => serialize_json("201 Created", snapshot_response(snapshot)),
        Err(error) => map_store_error(error),
    }
}

fn restore_snapshot(
    database: &mut ServiceDatabase,
    slot_id: &str,
    snapshot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(slot_id), Some(snapshot_id)) = (parse_id(slot_id), parse_id(snapshot_id)) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_snapshot"));
    };
    match database.restore_local_save_snapshot(slot_id, snapshot_id) {
        Ok(snapshot) => serialize_json(
            "200 OK",
            RestoreResponse {
                restored: true,
                safety_snapshot: snapshot_response(snapshot),
            },
        ),
        Err(error) => map_store_error(error),
    }
}
// //// /创建, 列出和恢复本地存档快照 ////

// //// 列出不可变本地 revision [@x380kkm 2026-07-27] ////
fn list_revisions(
    database: &mut ServiceDatabase,
    slot_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let current = match database.ensure_local_save_revision(slot_id, "Current state") {
        Ok(revision) => revision,
        Err(error) => return map_store_error(error),
    };
    match database.list_local_save_revisions(slot_id) {
        Ok(Some(revisions)) => serialize_json(
            "200 OK",
            LocalSaveRevisionStateResponse {
                current_revision_id: current.id,
                revisions: revisions.into_iter().map(revision_response).collect(),
            },
        ),
        Ok(None) => Ok(json_error("404 Not Found", "local_save_not_found")),
        Err(error) => map_store_error(error),
    }
}
// //// /列出不可变本地 revision ////

// //// 恢复不可变本地 revision [@x380kkm 2026-07-27] ////
fn restore_revision(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    slot_id: &str,
    revision_id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(slot_id) = parse_id(slot_id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let current = match database.ensure_local_save_revision(slot_id, "Before restore") {
        Ok(revision) => revision,
        Err(error) => return map_store_error(error),
    };
    if let Some(raw_etag) = request.header("if-match") {
        let Some(expected_etag) = normalize_revision_etag(raw_etag) else {
            return Ok(json_error("400 Bad Request", "invalid_save_revision_etag"));
        };
        if expected_etag != current.etag {
            return serialize_json(
                "409 Conflict",
                serde_json::json!({
                    "error": "save_revision_conflict",
                    "currentRevisionId": current.id,
                    "currentEtag": current.etag,
                }),
            );
        }
    }
    match database.restore_local_save_revision(slot_id, revision_id) {
        Ok(revision) => {
            let etag = revision.etag.clone();
            serialize_json(
                "200 OK",
                RestoreRevisionResponse {
                    restored: true,
                    revision: revision_response(revision),
                },
            )
            .map(|response| response.with_header_value("ETag", format!("\"{etag}\"")))
        }
        Err(error) => map_store_error(error),
    }
}
// //// /恢复不可变本地 revision ////

// //// 规范化本地 revision ETag [@x380kkm 2026-07-27] ////
fn normalize_revision_etag(value: &str) -> Option<String> {
    let value = value.trim().strip_prefix("W/").unwrap_or(value.trim());
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    (value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    .then(|| value.to_owned())
}
// //// /规范化本地 revision ETag ////
