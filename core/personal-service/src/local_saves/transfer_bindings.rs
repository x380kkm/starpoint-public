// audience: internal
// # local-save-transfer-bindings
//
// 该模块同步一个本地槽和一个明确的远端实例槽.
// 共同基线使用可移植存档摘要, 双方都偏离基线时停止传输并保存冲突.
// 下载覆盖和远端上传都使用现有 revision 与条件 ETag 保护当前版本.

use super::transfer::{create_local_transfer_save, LocalTransferSave};
use super::*;
use crate::database::{
    TransferBinding, TransferBindingStoreError, TransferConflict, TransferPullMode,
    TransferUploadMode,
};

mod client;
mod resolution;
mod routes;
mod runner;

pub(crate) use resolution::{resolve_open_transfer_conflict, TransferConflictResolution};
pub(crate) use runner::TransferBindingRunner;

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    routes::route(request, database)
}

const TRANSFER_RETRY_SECONDS: i64 = 5;

#[derive(Clone, Copy)]
pub(crate) enum TransferSyncTrigger {
    Manual,
    Interval,
}

#[derive(Clone, Copy)]
pub(crate) enum TransferSyncAction {
    Unchanged,
    Uploaded,
    Downloaded,
    Deferred,
}

impl TransferSyncAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Uploaded => "uploaded",
            Self::Downloaded => "downloaded",
            Self::Deferred => "deferred",
        }
    }
}

pub(crate) enum TransferSyncOutcome {
    Synchronized {
        action: TransferSyncAction,
        binding: Box<TransferBinding>,
    },
    Conflict(TransferConflict),
}

pub(crate) enum TransferOperationError {
    BindingNotFound,
    BindingDisabled,
    ConflictOpen,
    ConflictNotFound,
    ConflictChanged,
    LocalSaveNotFound,
    LocalSaveBusy,
    InvalidTarget,
    TargetIdentityMismatch,
    Authentication,
    RemoteConflict,
    RemoteNotFound,
    RemoteUnavailable,
    InvalidResponse,
    StaleBinding,
    Storage(PersonalServiceError),
}

impl TransferOperationError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::BindingNotFound => "transfer_binding_not_found",
            Self::BindingDisabled => "transfer_binding_disabled",
            Self::ConflictOpen => "transfer_conflict_open",
            Self::ConflictNotFound => "transfer_conflict_not_found",
            Self::ConflictChanged => "transfer_conflict_changed",
            Self::LocalSaveNotFound => "local_save_not_found",
            Self::LocalSaveBusy => "local_save_import_blocked",
            Self::InvalidTarget => "transfer_target_invalid",
            Self::TargetIdentityMismatch => "transfer_target_identity_mismatch",
            Self::Authentication => "transfer_target_authentication_failed",
            Self::RemoteConflict => "transfer_target_revision_conflict",
            Self::RemoteNotFound => "transfer_target_slot_not_found",
            Self::RemoteUnavailable => "transfer_target_unavailable",
            Self::InvalidResponse => "transfer_target_invalid_response",
            Self::StaleBinding => "transfer_binding_changed",
            Self::Storage(_) => "transfer_storage_failed",
        }
    }

    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::RemoteUnavailable)
    }
}

impl From<PersonalServiceError> for TransferOperationError {
    fn from(error: PersonalServiceError) -> Self {
        Self::Storage(error)
    }
}

pub(crate) struct PreparedTransferBindingSync {
    binding: TransferBinding,
    source: LocalTransferSave,
    trigger: TransferSyncTrigger,
}

pub(crate) struct CompletedTransferBindingSync {
    binding: TransferBinding,
    source: LocalTransferSave,
    result: Result<RemoteTransferDecision, TransferOperationError>,
}

enum RemoteTransferDecision {
    Unchanged(String),
    Uploaded(String),
    Downloaded(client::DownloadedTransferSave),
    Conflict(client::DownloadedTransferSave),
    Deferred(client::DownloadedTransferSave),
}

// //// 准备, 执行并提交一次绑定同步 [@x380kkm 2026-08-03] ////
pub(crate) fn synchronize_transfer_binding(
    database: &mut ServiceDatabase,
    binding_id: &str,
    trigger: TransferSyncTrigger,
) -> Result<TransferSyncOutcome, TransferOperationError> {
    let prepared = prepare_transfer_binding_sync(database, binding_id, trigger)?;
    let completed = execute_prepared_transfer_binding_sync(prepared);
    commit_completed_transfer_binding_sync(database, completed)
}

pub(crate) fn prepare_transfer_binding_sync(
    database: &mut ServiceDatabase,
    binding_id: &str,
    trigger: TransferSyncTrigger,
) -> Result<PreparedTransferBindingSync, TransferOperationError> {
    let binding = database
        .get_transfer_binding(binding_id)
        .map_err(TransferOperationError::Storage)?
        .ok_or(TransferOperationError::BindingNotFound)?;
    if !binding.enabled {
        return Err(TransferOperationError::BindingDisabled);
    }
    if database
        .get_open_transfer_conflict(binding_id)
        .map_err(TransferOperationError::Storage)?
        .is_some()
    {
        return Err(TransferOperationError::ConflictOpen);
    }
    let source = create_local_transfer_save(database, binding.source_slot_id)?
        .ok_or(TransferOperationError::LocalSaveNotFound)?;
    Ok(PreparedTransferBindingSync {
        binding,
        source,
        trigger,
    })
}

pub(crate) fn execute_prepared_transfer_binding_sync(
    prepared: PreparedTransferBindingSync,
) -> CompletedTransferBindingSync {
    let endpoint = client::TransferEndpoint::from(&prepared.binding);
    let result = client::download(&endpoint)
        .map_err(map_client_error)
        .and_then(|target| select_remote_transfer(&prepared, &endpoint, target));
    CompletedTransferBindingSync {
        binding: prepared.binding,
        source: prepared.source,
        result,
    }
}

pub(crate) fn commit_completed_transfer_binding_sync(
    database: &mut ServiceDatabase,
    completed: CompletedTransferBindingSync,
) -> Result<TransferSyncOutcome, TransferOperationError> {
    let current = create_local_transfer_save(database, completed.binding.source_slot_id)?
        .ok_or(TransferOperationError::LocalSaveNotFound)?;
    let decision = match completed.result {
        Ok(decision) => decision,
        Err(error) => {
            record_transfer_error(database, &completed.binding, &current.etag, None, &error)?;
            return Err(error);
        }
    };
    match decision {
        RemoteTransferDecision::Unchanged(target_etag) => commit_transfer_success(
            database,
            completed.binding,
            current,
            target_etag,
            TransferSyncAction::Unchanged,
        ),
        RemoteTransferDecision::Uploaded(target_etag) => commit_transfer_success(
            database,
            completed.binding,
            current,
            target_etag,
            TransferSyncAction::Uploaded,
        ),
        RemoteTransferDecision::Downloaded(target) => commit_transfer_download(
            database,
            completed.binding,
            completed.source,
            current,
            target,
        ),
        RemoteTransferDecision::Conflict(target) => {
            record_transfer_conflict(database, completed.binding, current, target)
        }
        RemoteTransferDecision::Deferred(target) => {
            database
                .record_transfer_binding_failure(
                    &completed.binding.id,
                    completed.binding.revision,
                    &current.etag,
                    Some(&target.etag),
                    "transfer_direction_not_scheduled",
                    completed.binding.interval_seconds,
                )
                .map_err(map_binding_store_error)?;
            let binding = database
                .get_transfer_binding(&completed.binding.id)
                .map_err(TransferOperationError::Storage)?
                .ok_or(TransferOperationError::BindingNotFound)?;
            Ok(TransferSyncOutcome::Synchronized {
                action: TransferSyncAction::Deferred,
                binding: Box::new(binding),
            })
        }
    }
}
fn select_remote_transfer(
    prepared: &PreparedTransferBindingSync,
    endpoint: &client::TransferEndpoint,
    target: client::DownloadedTransferSave,
) -> Result<RemoteTransferDecision, TransferOperationError> {
    if prepared.source.etag == target.etag {
        return Ok(RemoteTransferDecision::Unchanged(target.etag));
    }
    let Some(common_etag) = prepared.binding.last_common_etag.as_deref() else {
        return select_initial_transfer(prepared, endpoint, target);
    };
    let source_changed = prepared.source.etag != common_etag;
    let target_changed = target.etag != common_etag;
    match (source_changed, target_changed) {
        (true, true) => Ok(RemoteTransferDecision::Conflict(target)),
        (true, false) if should_upload(prepared) => {
            upload_source(endpoint, &prepared.source, target)
        }
        (false, true) if should_pull(prepared) => Ok(RemoteTransferDecision::Downloaded(target)),
        (false, false) => Ok(RemoteTransferDecision::Unchanged(target.etag)),
        _ => Ok(RemoteTransferDecision::Deferred(target)),
    }
}

fn select_initial_transfer(
    prepared: &PreparedTransferBindingSync,
    endpoint: &client::TransferEndpoint,
    target: client::DownloadedTransferSave,
) -> Result<RemoteTransferDecision, TransferOperationError> {
    if should_upload(prepared) {
        upload_source(endpoint, &prepared.source, target)
    } else if should_pull(prepared) {
        Ok(RemoteTransferDecision::Downloaded(target))
    } else {
        Ok(RemoteTransferDecision::Deferred(target))
    }
}

fn should_upload(prepared: &PreparedTransferBindingSync) -> bool {
    matches!(prepared.trigger, TransferSyncTrigger::Manual)
        || prepared.binding.upload_mode == TransferUploadMode::Interval
}

fn should_pull(prepared: &PreparedTransferBindingSync) -> bool {
    matches!(prepared.trigger, TransferSyncTrigger::Manual)
        || prepared.binding.pull_mode == TransferPullMode::Interval
}

fn upload_source(
    endpoint: &client::TransferEndpoint,
    source: &LocalTransferSave,
    target: client::DownloadedTransferSave,
) -> Result<RemoteTransferDecision, TransferOperationError> {
    match client::upload(endpoint, &source.package, &target.etag) {
        Ok(etag) => Ok(RemoteTransferDecision::Uploaded(etag)),
        Err(client::TransferClientError::Conflict) => client::download(endpoint)
            .map(RemoteTransferDecision::Conflict)
            .map_err(map_client_error),
        Err(error) => Err(map_client_error(error)),
    }
}

fn commit_transfer_success(
    database: &mut ServiceDatabase,
    binding: TransferBinding,
    current: LocalTransferSave,
    target_etag: String,
    action: TransferSyncAction,
) -> Result<TransferSyncOutcome, TransferOperationError> {
    let binding = database
        .record_transfer_binding_success(
            &binding.id,
            binding.revision,
            &target_etag,
            &current.etag,
            &target_etag,
        )
        .map_err(map_binding_store_error)?;
    Ok(TransferSyncOutcome::Synchronized {
        action,
        binding: Box::new(binding),
    })
}

fn commit_transfer_download(
    database: &mut ServiceDatabase,
    binding: TransferBinding,
    prepared_source: LocalTransferSave,
    current: LocalTransferSave,
    target: client::DownloadedTransferSave,
) -> Result<TransferSyncOutcome, TransferOperationError> {
    if current.etag != prepared_source.etag {
        return record_transfer_conflict(database, binding, current, target);
    }
    let data_json = serde_json::to_string(&target.package.data).map_err(|error| {
        TransferOperationError::Storage(PersonalServiceError::new(format!(
            "failed to encode transfer binding download: {error}"
        )))
    })?;
    let binding = match database.commit_transfer_binding_download(
        &binding.id,
        binding.revision,
        binding.source_slot_id,
        &data_json,
        &target.etag,
    ) {
        Ok(binding) => binding,
        Err(TransferBindingStoreError::LocalSaveBusy) => {
            let error = TransferOperationError::LocalSaveBusy;
            record_transfer_error(
                database,
                &binding,
                &current.etag,
                Some(&target.etag),
                &error,
            )?;
            return Err(error);
        }
        Err(error) => return Err(map_binding_store_error(error)),
    };
    Ok(TransferSyncOutcome::Synchronized {
        action: TransferSyncAction::Downloaded,
        binding: Box::new(binding),
    })
}

fn record_transfer_conflict(
    database: &mut ServiceDatabase,
    binding: TransferBinding,
    source: LocalTransferSave,
    target: client::DownloadedTransferSave,
) -> Result<TransferSyncOutcome, TransferOperationError> {
    let conflict = database
        .record_transfer_conflict(
            &binding.id,
            binding.revision,
            &source.revision_id,
            &source.etag,
            target.package.source.revision_id.as_deref(),
            &target.etag,
        )
        .map_err(map_binding_store_error)?;
    Ok(TransferSyncOutcome::Conflict(conflict))
}

fn record_transfer_error(
    database: &mut ServiceDatabase,
    binding: &TransferBinding,
    source_etag: &str,
    target_etag: Option<&str>,
    error: &TransferOperationError,
) -> Result<(), TransferOperationError> {
    let retry_seconds = if error.is_retryable() {
        TRANSFER_RETRY_SECONDS
    } else {
        binding.interval_seconds
    };
    database
        .record_transfer_binding_failure(
            &binding.id,
            binding.revision,
            source_etag,
            target_etag,
            error.code(),
            retry_seconds,
        )
        .map_err(map_binding_store_error)
}

fn map_client_error(error: client::TransferClientError) -> TransferOperationError {
    match error {
        client::TransferClientError::InvalidTarget => TransferOperationError::InvalidTarget,
        client::TransferClientError::TargetIdentityMismatch => {
            TransferOperationError::TargetIdentityMismatch
        }
        client::TransferClientError::Authentication => TransferOperationError::Authentication,
        client::TransferClientError::Conflict => TransferOperationError::RemoteConflict,
        client::TransferClientError::NotFound => TransferOperationError::RemoteNotFound,
        client::TransferClientError::Unavailable => TransferOperationError::RemoteUnavailable,
        client::TransferClientError::InvalidResponse => TransferOperationError::InvalidResponse,
    }
}

fn map_binding_store_error(error: TransferBindingStoreError) -> TransferOperationError {
    match error {
        TransferBindingStoreError::BindingNotFound => TransferOperationError::BindingNotFound,
        TransferBindingStoreError::ConflictNotFound => TransferOperationError::ConflictNotFound,
        TransferBindingStoreError::ConflictAlreadyResolved => {
            TransferOperationError::ConflictNotFound
        }
        TransferBindingStoreError::ConflictChanged => TransferOperationError::ConflictChanged,
        TransferBindingStoreError::LocalSaveBusy => TransferOperationError::LocalSaveBusy,
        TransferBindingStoreError::StaleBinding => TransferOperationError::StaleBinding,
        TransferBindingStoreError::Storage(error) => TransferOperationError::Storage(error),
        _ => TransferOperationError::Storage(PersonalServiceError::new(
            "transfer binding state is invalid",
        )),
    }
}

// //// /准备, 执行并提交一次绑定同步 ////
