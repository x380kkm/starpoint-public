// audience: internal
// # personal-service-remote-forward
//
// 该模块把当前远端配置的 CN API 请求转发到受限 HTTP 目标. 每次请求限制为 5 秒,
// 每个响应正文限制为 8 MiB. 管理凭据和逐跳 HTTP 头不离开本机.

use crate::database::{ServerProfile, ServerProfileMode, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use std::time::{Duration, Instant};

mod headers;
mod identity;
mod request;
mod response;

const CN_API_PREFIX: &str = "/api/index.php/";
const FORWARD_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(super) enum ForwardError {
    UnsupportedScheme,
    InvalidTarget,
    ForbiddenTarget,
    Unavailable,
    IdentityRequestInvalid,
    InvalidResponse,
    ResponseTooLarge,
}

// //// 选择当前远端配置的 CN API 请求 [@x380kkm 2026-07-23] ////
pub(crate) fn should_forward(request: &HttpRequest, profile: &ServerProfile) -> bool {
    profile.mode == ServerProfileMode::Remote
        && request.method() == "POST"
        && request.path().starts_with(CN_API_PREFIX)
}

pub(crate) fn forward(
    request: &HttpRequest,
    profile: &ServerProfile,
    personal_service_port: u16,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let prepared = match identity::prepare_request(request, profile.id, database) {
        Ok(prepared) => prepared,
        Err(identity::IdentityPreparationError::InvalidMessage) => {
            return Ok(forward_error_response(
                &ForwardError::IdentityRequestInvalid,
            ))
        }
        Err(identity::IdentityPreparationError::Storage(error)) => return Err(error),
    };
    let response = match forward_request(request, &prepared.body, profile, personal_service_port) {
        Ok(response) => response,
        Err(error) => return Ok(forward_error_response(&error)),
    };
    identity::capture_signup_identity(&prepared, profile.id, &response, database)?;
    Ok(response)
}
// //// /选择当前远端配置的 CN API 请求 ////

// //// 在总时限内发送一次远端请求 [@x380kkm 2026-07-23] ////
fn forward_request(
    incoming_request: &HttpRequest,
    forwarded_body: &[u8],
    profile: &ServerProfile,
    personal_service_port: u16,
) -> Result<HttpResponse, ForwardError> {
    let deadline = Instant::now() + FORWARD_TIMEOUT;
    let mut stream = request::connect_and_send(
        incoming_request,
        forwarded_body,
        profile,
        personal_service_port,
        deadline,
    )?;
    response::read(&mut stream, deadline)
}
// //// /在总时限内发送一次远端请求 ////

// //// 返回稳定的远端失败响应 [@x380kkm 2026-07-23] ////
fn forward_error_response(error: &ForwardError) -> HttpResponse {
    let error = match error {
        ForwardError::UnsupportedScheme => "remote_scheme_unsupported",
        ForwardError::InvalidTarget => "remote_target_invalid",
        ForwardError::ForbiddenTarget => "remote_target_forbidden",
        ForwardError::Unavailable => "remote_server_unavailable",
        ForwardError::IdentityRequestInvalid => "remote_identity_request_invalid",
        ForwardError::InvalidResponse => "remote_response_invalid",
        ForwardError::ResponseTooLarge => "remote_response_too_large",
    };
    HttpResponse::json("502 Bad Gateway", format!("{{\"error\":\"{error}\"}}"))
}
// //// /返回稳定的远端失败响应 ////
