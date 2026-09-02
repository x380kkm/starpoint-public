// audience: internal
// # transfer-binding-resolution
//
// 该模块用本地覆盖, 远端覆盖或保留双方分支解决一个已记录冲突.
// 两种覆盖路径都复用槽位 revision 和条件 ETag.

use super::*;
use crate::database::{TransferBindingEtagUpdate, TransferConflictStatus};

#[derive(Clone, Copy)]
pub(crate) enum TransferConflictResolution {
    LocalWins,
    RemoteWins,
    KeepBoth,
}

pub(crate) struct ResolvedTransferConflict {
    pub(crate) conflict: TransferConflict,
    pub(crate) binding: TransferBinding,
}

// //// 解决传输冲突 [@x380kkm 2026-08-03] ////
pub(crate) fn resolve_open_transfer_conflict(
    database: &mut ServiceDatabase,
    binding_id: &str,
    conflict_id: &str,
    resolution: TransferConflictResolution,
) -> Result<ResolvedTransferConflict, TransferOperationError> {
    let binding = database
        .get_transfer_binding(binding_id)
        .map_err(TransferOperationError::Storage)?
        .ok_or(TransferOperationError::BindingNotFound)?;
    let conflict = database
        .get_open_transfer_conflict(binding_id)
        .map_err(TransferOperationError::Storage)?
        .filter(|conflict| conflict.id == conflict_id)
        .ok_or(TransferOperationError::ConflictNotFound)?;
    let resolved = match resolution {
        TransferConflictResolution::LocalWins => resolve_with_local(database, &binding, &conflict)?,
        TransferConflictResolution::RemoteWins => {
            resolve_with_remote(database, &binding, &conflict)?
        }
        TransferConflictResolution::KeepBoth => database
            .resolve_transfer_conflict(
                &binding.id,
                &conflict.id,
                binding.revision,
                TransferConflictStatus::ResolvedKeepBoth,
                TransferBindingEtagUpdate::preserve_existing(),
            )
            .map_err(map_binding_store_error)?,
    };
    let binding = database
        .get_transfer_binding(binding_id)
        .map_err(TransferOperationError::Storage)?
        .ok_or(TransferOperationError::BindingNotFound)?;
    Ok(ResolvedTransferConflict {
        conflict: resolved,
        binding,
    })
}
// //// /解决传输冲突 ////

// //// 执行本地或远端覆盖 [@x380kkm 2026-08-03] ////
fn resolve_with_local(
    database: &mut ServiceDatabase,
    binding: &TransferBinding,
    conflict: &TransferConflict,
) -> Result<TransferConflict, TransferOperationError> {
    let source = create_local_transfer_save(database, binding.source_slot_id)?
        .ok_or(TransferOperationError::LocalSaveNotFound)?;
    let endpoint = client::TransferEndpoint::from(binding);
    let target_etag = client::upload(&endpoint, &source.package, &conflict.target_etag)
        .map_err(map_client_error)?;
    let current = create_local_transfer_save(database, binding.source_slot_id)?
        .ok_or(TransferOperationError::LocalSaveNotFound)?;
    database
        .resolve_transfer_conflict(
            &binding.id,
            &conflict.id,
            binding.revision,
            TransferConflictStatus::ResolvedLocalWins,
            TransferBindingEtagUpdate {
                common_etag: Some(&target_etag),
                source_etag: Some(&current.etag),
                target_etag: Some(&target_etag),
            },
        )
        .map_err(map_binding_store_error)
}
// //// /执行本地或远端覆盖 ////

fn resolve_with_remote(
    database: &mut ServiceDatabase,
    binding: &TransferBinding,
    conflict: &TransferConflict,
) -> Result<TransferConflict, TransferOperationError> {
    let endpoint = client::TransferEndpoint::from(binding);
    let target = client::download(&endpoint).map_err(map_client_error)?;
    if target.etag != conflict.target_etag {
        return Err(TransferOperationError::RemoteConflict);
    }
    let data_json = serde_json::to_string(&target.package.data).map_err(|error| {
        TransferOperationError::Storage(PersonalServiceError::new(format!(
            "failed to encode transfer conflict download: {error}"
        )))
    })?;
    database
        .resolve_transfer_conflict_with_download(
            &binding.id,
            &conflict.id,
            binding.revision,
            binding.source_slot_id,
            &conflict.source_etag,
            &data_json,
            &target.etag,
        )
        .map_err(map_binding_store_error)
}
