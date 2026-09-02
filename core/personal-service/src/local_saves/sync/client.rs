// audience: internal
// # local-save-sync-client
//
// 该模块通过 HTTPS 登录密文存档服务器. 明文 HTTP 仅允许 loopback 测试目标.

use super::super::encryption::EncryptedSaveEnvelope;
use super::super::remote_target;
use crate::database::SaveSyncTarget;
use serde::Serialize;
use std::io::Read;

const SESSION_COOKIE_NAME: &str = "starpoint_management_session";

pub(super) struct DownloadedEncryptedSave {
    pub(super) envelope: EncryptedSaveEnvelope,
    pub(super) etag: String,
}

pub(super) enum SyncClientError {
    InvalidTarget,
    Authentication,
    CapacityExceeded,
    Conflict,
    NotFound,
    Unavailable,
    InvalidResponse,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
}

// //// 上传密文存档并返回远端 ETag [@x380kkm 2026-07-23] ////
pub(super) fn upload(
    target: &SaveSyncTarget,
    object_id: &str,
    envelope: &EncryptedSaveEnvelope,
    previous_etag: Option<&str>,
) -> Result<String, SyncClientError> {
    let origin = target_origin(target)?;
    let agent = remote_target::create_agent();
    let cookie = login(&agent, &origin, target)?;
    let request = agent
        .put(&format!("{origin}/manage/api/encrypted-saves/{object_id}"))
        .set("Cookie", &cookie);
    let request = match previous_etag {
        Some(etag) => request.set("If-Match", &format!("\"{etag}\"")),
        None => request.set("If-None-Match", "*"),
    };
    let result = request
        .send_json(envelope)
        .map_err(map_object_request_error)
        .and_then(|response| parse_etag(&response));
    logout(&agent, &origin, &cookie);
    result
}
// //// /上传密文存档并返回远端 ETag ////

// //// 下载密文存档和远端 ETag [@x380kkm 2026-07-23] ////
pub(super) fn download(
    target: &SaveSyncTarget,
    object_id: &str,
) -> Result<DownloadedEncryptedSave, SyncClientError> {
    let origin = target_origin(target)?;
    let agent = remote_target::create_agent();
    let cookie = login(&agent, &origin, target)?;
    let result = agent
        .get(&format!("{origin}/manage/api/encrypted-saves/{object_id}"))
        .set("Cookie", &cookie)
        .call()
        .map_err(map_object_request_error)
        .and_then(read_download);
    logout(&agent, &origin, &cookie);
    result
}
// //// /下载密文存档和远端 ETag ////

fn login(
    agent: &ureq::Agent,
    origin: &str,
    target: &SaveSyncTarget,
) -> Result<String, SyncClientError> {
    let response = agent
        .post(&format!("{origin}/manage/api/auth/login"))
        .send_json(LoginRequest {
            username: &target.username,
            password: &target.password,
        })
        .map_err(map_login_request_error)?;
    let cookie = response
        .header("set-cookie")
        .and_then(|value| value.split(';').next())
        .and_then(|value| value.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")))
        .filter(|value| !value.is_empty())
        .ok_or(SyncClientError::InvalidResponse)?;
    Ok(format!("{SESSION_COOKIE_NAME}={cookie}"))
}

fn logout(agent: &ureq::Agent, origin: &str, cookie: &str) {
    let _ = agent
        .post(&format!("{origin}/manage/api/auth/logout"))
        .set("Cookie", cookie)
        .call();
}

fn read_download(response: ureq::Response) -> Result<DownloadedEncryptedSave, SyncClientError> {
    let etag = parse_etag(&response)?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take((remote_target::RESPONSE_LIMIT + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|_| SyncClientError::Unavailable)?;
    if body.len() > remote_target::RESPONSE_LIMIT {
        return Err(SyncClientError::InvalidResponse);
    }
    let envelope = serde_json::from_slice(&body).map_err(|_| SyncClientError::InvalidResponse)?;
    Ok(DownloadedEncryptedSave { envelope, etag })
}

fn parse_etag(response: &ureq::Response) -> Result<String, SyncClientError> {
    let etag = response
        .header("etag")
        .and_then(|value| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(SyncClientError::InvalidResponse)?;
    Ok(etag.to_ascii_lowercase())
}

enum RequestFailure {
    Authentication,
    Status(u16),
    Unavailable,
}

fn map_login_request_error(error: ureq::Error) -> SyncClientError {
    match classify_request_failure(error) {
        RequestFailure::Authentication => SyncClientError::Authentication,
        RequestFailure::Status(_) => SyncClientError::InvalidResponse,
        RequestFailure::Unavailable => SyncClientError::Unavailable,
    }
}

fn map_object_request_error(error: ureq::Error) -> SyncClientError {
    match classify_request_failure(error) {
        RequestFailure::Authentication => SyncClientError::Authentication,
        RequestFailure::Status(404) => SyncClientError::NotFound,
        RequestFailure::Status(409) => SyncClientError::CapacityExceeded,
        RequestFailure::Status(412) => SyncClientError::Conflict,
        RequestFailure::Status(_) => SyncClientError::InvalidResponse,
        RequestFailure::Unavailable => SyncClientError::Unavailable,
    }
}

fn classify_request_failure(error: ureq::Error) -> RequestFailure {
    match error {
        ureq::Error::Status(401 | 403, _) => RequestFailure::Authentication,
        ureq::Error::Status(status, _) => RequestFailure::Status(status),
        ureq::Error::Transport(_) => RequestFailure::Unavailable,
    }
}

fn target_origin(target: &SaveSyncTarget) -> Result<String, SyncClientError> {
    remote_target::origin(&target.scheme, &target.host, target.port).map_err(|error| match error {
        remote_target::RemoteTargetError::Invalid => SyncClientError::InvalidTarget,
        remote_target::RemoteTargetError::Unavailable => SyncClientError::Unavailable,
    })
}
