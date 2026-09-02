// audience: internal
// # personal-service-transfer-binding-types
//
// 该模块定义本地槽与另一服务实例槽之间的绑定和冲突数据.

use crate::PersonalServiceError;

pub(crate) const DEFAULT_TRANSFER_INTERVAL_SECONDS: i64 = 900;
pub(crate) const MIN_TRANSFER_INTERVAL_SECONDS: i64 = 60;
pub(crate) const MAX_TRANSFER_INTERVAL_SECONDS: i64 = 2_592_000;

// //// 定义传输绑定的枚举值 [@x380kkm 2026-08-03] ////
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferInstanceKind {
    Local,
    Remote,
}

impl TransferInstanceKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "remote" => Some(Self::Remote),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferUploadMode {
    Manual,
    OnConnect,
    Interval,
}

impl TransferUploadMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::OnConnect => "on_connect",
            Self::Interval => "interval",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "on_connect" => Some(Self::OnConnect),
            "interval" => Some(Self::Interval),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferPullMode {
    Manual,
    OnDisconnect,
    Interval,
}

impl TransferPullMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::OnDisconnect => "on_disconnect",
            Self::Interval => "interval",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "on_disconnect" => Some(Self::OnDisconnect),
            "interval" => Some(Self::Interval),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferConflictPolicy {
    LocalWins,
    RemoteWins,
    Ask,
}

impl TransferConflictPolicy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LocalWins => "local_wins",
            Self::RemoteWins => "remote_wins",
            Self::Ask => "ask",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "local_wins" => Some(Self::LocalWins),
            "remote_wins" => Some(Self::RemoteWins),
            "ask" => Some(Self::Ask),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferConflictStatus {
    Open,
    ResolvedLocalWins,
    ResolvedRemoteWins,
    ResolvedKeepBoth,
}

impl TransferConflictStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::ResolvedLocalWins => "resolved_local_wins",
            Self::ResolvedRemoteWins => "resolved_remote_wins",
            Self::ResolvedKeepBoth => "resolved_keep_both",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "resolved_local_wins" => Some(Self::ResolvedLocalWins),
            "resolved_remote_wins" => Some(Self::ResolvedRemoteWins),
            "resolved_keep_both" => Some(Self::ResolvedKeepBoth),
            _ => None,
        }
    }
}
// //// /定义传输绑定的枚举值 ////

// //// 定义传输绑定和冲突数据 [@x380kkm 2026-08-03] ////
pub(crate) struct TransferBinding {
    pub(crate) id: String,
    pub(crate) source_instance_id: String,
    pub(crate) source_shell_id: String,
    pub(crate) source_slot_id: i64,
    pub(crate) target_profile_id: i64,
    pub(crate) target_instance_kind: TransferInstanceKind,
    pub(crate) target_instance_id: String,
    pub(crate) target_shell_id: String,
    pub(crate) target_slot_id: i64,
    pub(crate) target_token: String,
    pub(crate) target_scheme: String,
    pub(crate) target_host: String,
    pub(crate) target_port: i64,
    pub(crate) upload_mode: TransferUploadMode,
    pub(crate) pull_mode: TransferPullMode,
    pub(crate) conflict_policy: TransferConflictPolicy,
    pub(crate) interval_seconds: i64,
    pub(crate) enabled: bool,
    pub(crate) last_common_etag: Option<String>,
    pub(crate) last_source_etag: Option<String>,
    pub(crate) last_target_etag: Option<String>,
    pub(crate) pending_direction: String,
    pub(crate) next_run_at: String,
    pub(crate) last_synced_at: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) revision: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

pub(crate) struct CreateTransferBindingInput {
    pub(crate) source_slot_id: i64,
    pub(crate) target_profile_id: i64,
    pub(crate) target_instance_kind: TransferInstanceKind,
    pub(crate) target_instance_id: String,
    pub(crate) target_shell_id: String,
    pub(crate) target_slot_id: i64,
    pub(crate) target_token: String,
    pub(crate) upload_mode: TransferUploadMode,
    pub(crate) pull_mode: TransferPullMode,
    pub(crate) conflict_policy: TransferConflictPolicy,
    pub(crate) interval_seconds: i64,
    pub(crate) enabled: bool,
    pub(crate) observed_source_etag: String,
    pub(crate) observed_target_etag: String,
}

pub(crate) struct UpdateTransferBindingInput {
    pub(crate) upload_mode: TransferUploadMode,
    pub(crate) pull_mode: TransferPullMode,
    pub(crate) conflict_policy: TransferConflictPolicy,
    pub(crate) interval_seconds: i64,
    pub(crate) enabled: bool,
    pub(crate) target_token: Option<String>,
}

#[derive(Debug)]
pub(crate) struct TransferConflict {
    pub(crate) id: String,
    pub(crate) binding_id: String,
    pub(crate) source_revision_id: String,
    pub(crate) source_etag: String,
    pub(crate) target_revision_id: Option<String>,
    pub(crate) target_etag: String,
    pub(crate) detected_at: String,
    pub(crate) status: TransferConflictStatus,
    pub(crate) resolved_at: Option<String>,
}

pub(crate) struct TransferBindingEtagUpdate<'a> {
    pub(crate) common_etag: Option<&'a str>,
    pub(crate) source_etag: Option<&'a str>,
    pub(crate) target_etag: Option<&'a str>,
}

impl<'a> TransferBindingEtagUpdate<'a> {
    pub(crate) fn synchronized(etag: &'a str) -> Self {
        Self {
            common_etag: Some(etag),
            source_etag: Some(etag),
            target_etag: Some(etag),
        }
    }

    pub(crate) fn preserve_existing() -> Self {
        Self {
            common_etag: None,
            source_etag: None,
            target_etag: None,
        }
    }
}

pub(crate) enum TransferBindingStoreError {
    SourceSlotNotFound,
    TargetProfileNotFound,
    TargetProfileIsLocal,
    BindingNotFound,
    DuplicateBinding,
    ConflictNotFound,
    ConflictAlreadyResolved,
    ConflictChanged,
    LocalSaveBusy,
    StaleBinding,
    Storage(PersonalServiceError),
}
// //// /定义传输绑定和冲突数据 ////
