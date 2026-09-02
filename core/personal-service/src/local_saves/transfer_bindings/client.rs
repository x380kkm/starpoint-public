// audience: internal
// # transfer-binding-client
//
// 该模块用槽 token 访问另一服务实例的单个可移植存档槽.
// 每次成功响应都必须证明目标实例, 壳和槽身份.

use super::super::remote_target;
use crate::database::{TransferBinding, TransferInstanceKind};
use crate::portable_save::{self, StarpointSavePackage};
use serde_json::Value;
use std::io::Read;

const INSTANCE_ID_HEADER: &str = "x-starpoint-instance-id";
const SHELL_ID_HEADER: &str = "x-starpoint-shell-id";
const SLOT_ID_HEADER: &str = "x-starpoint-slot-id";

pub(super) struct TransferEndpoint {
    pub(super) instance_kind: TransferInstanceKind,
    pub(super) instance_id: String,
    pub(super) shell_id: Option<String>,
    pub(super) slot_id: i64,
    pub(super) token: String,
    pub(super) scheme: String,
    pub(super) host: String,
    pub(super) port: i64,
}

impl From<&TransferBinding> for TransferEndpoint {
    fn from(binding: &TransferBinding) -> Self {
        Self {
            instance_kind: binding.target_instance_kind,
            instance_id: binding.target_instance_id.clone(),
            shell_id: Some(binding.target_shell_id.clone()),
            slot_id: binding.target_slot_id,
            token: binding.target_token.clone(),
            scheme: binding.target_scheme.clone(),
            host: binding.target_host.clone(),
            port: binding.target_port,
        }
    }
}

pub(super) struct DownloadedTransferSave {
    pub(super) package: StarpointSavePackage,
    pub(super) etag: String,
    pub(super) shell_id: String,
}

pub(super) enum TransferClientError {
    InvalidTarget,
    TargetIdentityMismatch,
    Authentication,
    Conflict,
    NotFound,
    Unavailable,
    InvalidResponse,
}

// //// 下载并验证目标槽存档 [@x380kkm 2026-08-03] ////
pub(super) fn download(
    endpoint: &TransferEndpoint,
) -> Result<DownloadedTransferSave, TransferClientError> {
    let agent = remote_target::create_agent();
    let response = agent
        .get(&slot_url(endpoint)?)
        .set("Authorization", &format!("Bearer {}", endpoint.token))
        .call()
        .map_err(map_request_error)?;
    let shell_id = verify_target_identity(&response, endpoint)?;
    let etag = response_etag(&response)?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take((remote_target::RESPONSE_LIMIT + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|_| TransferClientError::Unavailable)?;
    if body.len() > remote_target::RESPONSE_LIMIT {
        return Err(TransferClientError::InvalidResponse);
    }
    let value =
        serde_json::from_slice::<Value>(&body).map_err(|_| TransferClientError::InvalidResponse)?;
    let package =
        portable_save::parse_package(value).ok_or(TransferClientError::InvalidResponse)?;
    if package.payload_sha256 != etag {
        return Err(TransferClientError::InvalidResponse);
    }
    Ok(DownloadedTransferSave {
        package,
        etag,
        shell_id,
    })
}
// //// /下载并验证目标槽存档 ////

// //// 条件覆盖目标槽并验证新版本 [@x380kkm 2026-08-03] ////
pub(super) fn upload(
    endpoint: &TransferEndpoint,
    package: &StarpointSavePackage,
    expected_etag: &str,
) -> Result<String, TransferClientError> {
    let response = remote_target::create_agent()
        .put(&slot_url(endpoint)?)
        .set("Authorization", &format!("Bearer {}", endpoint.token))
        .set("If-Match", &format!("\"{expected_etag}\""))
        .send_json(package)
        .map_err(map_request_error)?;
    verify_target_identity(&response, endpoint)?;
    let etag = response_etag(&response)?;
    if etag != package.payload_sha256 {
        return Err(TransferClientError::InvalidResponse);
    }
    Ok(etag)
}
// //// /条件覆盖目标槽并验证新版本 ////

fn slot_url(endpoint: &TransferEndpoint) -> Result<String, TransferClientError> {
    let origin = remote_target::origin(&endpoint.scheme, &endpoint.host, endpoint.port).map_err(
        |error| match error {
            remote_target::RemoteTargetError::Invalid => TransferClientError::InvalidTarget,
            remote_target::RemoteTargetError::Unavailable => TransferClientError::Unavailable,
        },
    )?;
    let prefix = match endpoint.instance_kind {
        TransferInstanceKind::Local => "/v1/transfer/v1",
        TransferInstanceKind::Remote => "/manage/transfer/v1",
    };
    Ok(format!("{origin}{prefix}/slots/{}", endpoint.slot_id))
}

fn verify_target_identity(
    response: &ureq::Response,
    endpoint: &TransferEndpoint,
) -> Result<String, TransferClientError> {
    let instance_matches =
        response.header(INSTANCE_ID_HEADER) == Some(endpoint.instance_id.as_str());
    let shell_id = response.header(SHELL_ID_HEADER).filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    });
    let shell_matches = match (endpoint.shell_id.as_deref(), shell_id) {
        (Some(expected), Some(actual)) => expected == actual,
        (None, Some(_)) => true,
        _ => false,
    };
    let slot_matches = response
        .header(SLOT_ID_HEADER)
        .and_then(|value| value.parse::<i64>().ok())
        == Some(endpoint.slot_id);
    if instance_matches && shell_matches && slot_matches {
        Ok(shell_id.expect("validated transfer shell ID").to_owned())
    } else {
        Err(TransferClientError::TargetIdentityMismatch)
    }
}

fn response_etag(response: &ureq::Response) -> Result<String, TransferClientError> {
    let etag = response
        .header("etag")
        .and_then(|value| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        .ok_or(TransferClientError::InvalidResponse)?;
    Ok(etag.to_owned())
}

fn map_request_error(error: ureq::Error) -> TransferClientError {
    match error {
        ureq::Error::Status(401 | 403, _) => TransferClientError::Authentication,
        ureq::Error::Status(404, _) => TransferClientError::NotFound,
        ureq::Error::Status(409 | 412, _) => TransferClientError::Conflict,
        ureq::Error::Status(_, _) => TransferClientError::InvalidResponse,
        ureq::Error::Transport(_) => TransferClientError::Unavailable,
    }
}
