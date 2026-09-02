// audience: internal
// # transfer-binding-route-support
//
// 该模块校验传输绑定请求并把内部绑定和冲突映射为无凭据响应.

use super::super::client;
use super::super::*;
use crate::database::{
    TransferBindingStoreError, TransferConflictPolicy, TransferInstanceKind, TransferPullMode,
    TransferUploadMode, DEFAULT_TRANSFER_INTERVAL_SECONDS, MAX_TRANSFER_INTERVAL_SECONDS,
    MIN_TRANSFER_INTERVAL_SECONDS,
};
use serde_json::{json, Value};

pub(super) struct BindingModes {
    pub(super) upload: TransferUploadMode,
    pub(super) pull: TransferPullMode,
    pub(super) conflict: TransferConflictPolicy,
    pub(super) interval_seconds: i64,
}

// //// 映射不含凭据的绑定和冲突响应 [@x380kkm 2026-08-03] ////
pub(super) fn binding_response(binding: &TransferBinding) -> Value {
    json!({
        "binding_id": binding.id,
        "source": {
            "instance_id": binding.source_instance_id,
            "shell_id": binding.source_shell_id,
            "slot_id": binding.source_slot_id,
        },
        "target": {
            "profile_id": binding.target_profile_id,
            "instance_kind": binding.target_instance_kind.as_str(),
            "instance_id": binding.target_instance_id,
            "shell_id": binding.target_shell_id,
            "slot_id": binding.target_slot_id,
        },
        "upload_mode": binding.upload_mode.as_str(),
        "pull_mode": binding.pull_mode.as_str(),
        "conflict_policy": binding.conflict_policy.as_str(),
        "interval_seconds": binding.interval_seconds,
        "enabled": binding.enabled,
        "last_common_etag": binding.last_common_etag,
        "last_source_etag": binding.last_source_etag,
        "last_target_etag": binding.last_target_etag,
        "pending_direction": binding.pending_direction,
        "next_run_at": binding.next_run_at,
        "last_synced_at": binding.last_synced_at,
        "last_error": binding.last_error,
        "created_at": binding.created_at,
        "updated_at": binding.updated_at,
    })
}

pub(super) fn conflict_response(conflict: &TransferConflict) -> Value {
    json!({
        "conflict_id": conflict.id,
        "binding_id": conflict.binding_id,
        "source_revision_id": conflict.source_revision_id,
        "source_etag": conflict.source_etag,
        "target_revision_id": conflict.target_revision_id,
        "target_etag": conflict.target_etag,
        "detected_at": conflict.detected_at,
        "status": conflict.status.as_str(),
        "resolved_at": conflict.resolved_at,
    })
}
// //// /映射不含凭据的绑定和冲突响应 ////

// //// 校验绑定模式, 标识和槽 token [@x380kkm 2026-08-03] ////
pub(super) fn parse_modes(
    upload_mode: &str,
    pull_mode: &str,
    conflict_policy: &str,
    interval_seconds: i64,
) -> Option<BindingModes> {
    let upload = TransferUploadMode::parse(upload_mode)?;
    let pull = TransferPullMode::parse(pull_mode)?;
    let conflict = TransferConflictPolicy::parse(conflict_policy)?;
    if !(MIN_TRANSFER_INTERVAL_SECONDS..=MAX_TRANSFER_INTERVAL_SECONDS).contains(&interval_seconds)
    {
        return None;
    }
    Some(BindingModes {
        upload,
        pull,
        conflict,
        interval_seconds,
    })
}

pub(super) fn default_transfer_interval_seconds() -> i64 {
    DEFAULT_TRANSFER_INTERVAL_SECONDS
}

pub(super) fn parse_instance_kind(value: &str) -> Option<TransferInstanceKind> {
    match value {
        "local" => Some(TransferInstanceKind::Local),
        "remote" => Some(TransferInstanceKind::Remote),
        _ => None,
    }
}

pub(super) fn parse_resolution(
    value: Option<&str>,
    policy: TransferConflictPolicy,
) -> Option<TransferConflictResolution> {
    match value {
        Some("local_wins") => Some(TransferConflictResolution::LocalWins),
        Some("remote_wins") => Some(TransferConflictResolution::RemoteWins),
        Some("keep_both") => Some(TransferConflictResolution::KeepBoth),
        Some(_) => None,
        None => match policy {
            TransferConflictPolicy::LocalWins => Some(TransferConflictResolution::LocalWins),
            TransferConflictPolicy::RemoteWins => Some(TransferConflictResolution::RemoteWins),
            TransferConflictPolicy::Ask => None,
        },
    }
}

pub(super) fn parse_object_id(value: &str) -> Option<&str> {
    (value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    .then_some(value)
}

pub(super) fn is_valid_instance_id(value: &str) -> bool {
    parse_object_id(value).is_some()
}

pub(super) fn is_valid_slot_token(value: &str) -> bool {
    let Some(secret) = value.strip_prefix("spt_slot_") else {
        return false;
    };
    value.len() <= 512
        && secret.len() >= 40
        && secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
// //// /校验绑定模式, 标识和槽 token ////

// //// 映射绑定存储和远端错误 [@x380kkm 2026-08-03] ////
pub(super) fn map_transfer_binding_store_error(
    error: TransferBindingStoreError,
) -> Result<HttpResponse, PersonalServiceError> {
    match error {
        TransferBindingStoreError::SourceSlotNotFound => {
            Ok(json_error("404 Not Found", "local_save_not_found"))
        }
        TransferBindingStoreError::TargetProfileNotFound => {
            Ok(json_error("404 Not Found", "server_profile_not_found"))
        }
        TransferBindingStoreError::TargetProfileIsLocal => Ok(json_error(
            "400 Bad Request",
            "transfer_target_profile_is_local",
        )),
        TransferBindingStoreError::BindingNotFound => {
            Ok(json_error("404 Not Found", "transfer_binding_not_found"))
        }
        TransferBindingStoreError::DuplicateBinding => Ok(json_error(
            "409 Conflict",
            "transfer_binding_already_exists",
        )),
        TransferBindingStoreError::ConflictNotFound
        | TransferBindingStoreError::ConflictAlreadyResolved => {
            Ok(json_error("404 Not Found", "transfer_conflict_not_found"))
        }
        TransferBindingStoreError::ConflictChanged => {
            Ok(json_error("409 Conflict", "transfer_conflict_changed"))
        }
        TransferBindingStoreError::LocalSaveBusy => {
            Ok(json_error("409 Conflict", "local_save_import_blocked"))
        }
        TransferBindingStoreError::StaleBinding => {
            Ok(json_error("409 Conflict", "transfer_binding_changed"))
        }
        TransferBindingStoreError::Storage(error) => Err(error),
    }
}

pub(super) fn map_transfer_binding_operation_error(
    error: TransferOperationError,
) -> Result<HttpResponse, PersonalServiceError> {
    let status = match error {
        TransferOperationError::BindingNotFound | TransferOperationError::ConflictNotFound => {
            "404 Not Found"
        }
        TransferOperationError::BindingDisabled
        | TransferOperationError::ConflictOpen
        | TransferOperationError::ConflictChanged
        | TransferOperationError::LocalSaveBusy
        | TransferOperationError::RemoteConflict
        | TransferOperationError::StaleBinding => "409 Conflict",
        TransferOperationError::LocalSaveNotFound | TransferOperationError::RemoteNotFound => {
            "404 Not Found"
        }
        TransferOperationError::InvalidTarget => "400 Bad Request",
        TransferOperationError::TargetIdentityMismatch
        | TransferOperationError::Authentication
        | TransferOperationError::RemoteUnavailable
        | TransferOperationError::InvalidResponse => "502 Bad Gateway",
        TransferOperationError::Storage(error) => return Err(error),
    };
    Ok(json_error(status, error.code()))
}

pub(super) fn map_transfer_binding_client_error(
    error: client::TransferClientError,
) -> TransferOperationError {
    super::super::map_client_error(error)
}
// //// /映射绑定存储和远端错误 ////
