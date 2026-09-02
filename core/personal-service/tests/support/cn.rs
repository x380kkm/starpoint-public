// audience: internal
// # personal-service-cn-test-support
//
// 该模块编码和解码 CN 客户端使用的 base64 MessagePack 测试请求.

use crate::support::{request_with_body, request_with_headers};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct DataHeaders {
    pub(crate) force_update: bool,
    pub(crate) asset_update: bool,
    pub(crate) short_udid: i64,
    pub(crate) viewer_id: i64,
    pub(crate) servertime: i64,
    pub(crate) result_code: i64,
}

#[derive(Deserialize)]
pub(crate) struct Envelope<T> {
    pub(crate) data_headers: DataHeaders,
    pub(crate) data: T,
}

#[derive(Deserialize)]
pub(crate) struct SignupData {
    pub(crate) login_token: String,
    #[serde(rename = "newAccount")]
    pub(crate) new_account: i64,
    #[serde(rename = "roleName")]
    pub(crate) role_name: String,
    #[serde(rename = "accountName")]
    pub(crate) account_name: String,
    pub(crate) sign: String,
    #[serde(rename = "createDate")]
    pub(crate) create_date: String,
    #[serde(rename = "serverName")]
    pub(crate) server_name: String,
    #[serde(rename = "serverId")]
    pub(crate) server_id: i64,
}

#[derive(Serialize)]
pub(crate) struct SignupRequest {
    pub(crate) device_id: i64,
}

#[derive(Serialize)]
pub(crate) struct LoadRequest {
    pub(crate) keychain: i64,
    pub(crate) viewer_id: i64,
}

// //// 发送和解码 CN MessagePack 请求 [@x380kkm 2026-07-22] ////
pub(crate) fn decode_response<T: serde::de::DeserializeOwned>(response: &str) -> Envelope<T> {
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("Content-Type: application/x-msgpack"));
    let body = response
        .split_once("\r\n\r\n")
        .expect("response contains a body")
        .1;
    let packed = STANDARD.decode(body).expect("response body is base64");
    rmp_serde::from_slice(&packed).expect("response body is MessagePack")
}

pub(crate) fn send_request(port: u16, path: &str, body: &str) -> String {
    request_with_body(
        port,
        "POST",
        path,
        "application/x-www-form-urlencoded",
        body.as_bytes(),
    )
}

pub(crate) fn send_request_with_resource_version(
    port: u16,
    path: &str,
    body: &str,
    resource_version: &str,
) -> String {
    request_with_headers(
        port,
        "POST",
        path,
        "application/x-www-form-urlencoded",
        &[("res_ver", resource_version)],
        body.as_bytes(),
    )
}

pub(crate) fn encode_request<T: Serialize>(body: &T) -> String {
    STANDARD.encode(rmp_serde::to_vec_named(body).expect("request is encoded"))
}

pub(crate) fn assert_valid_signup_response(signup: &Envelope<SignupData>) {
    assert!(!signup.data_headers.force_update);
    assert!(!signup.data_headers.asset_update);
    assert_eq!(signup.data_headers.short_udid, 0);
    assert!((100_000_000..999_999_999).contains(&signup.data_headers.viewer_id));
    assert!(signup.data_headers.servertime > 1_700_000_000);
    assert_eq!(signup.data_headers.result_code, 1);
    assert_eq!(signup.data.login_token.len(), 32);
    assert!(signup
        .data
        .login_token
        .bytes()
        .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit()));
    assert!((0..=1).contains(&signup.data.new_account));
    assert!(signup.data.role_name.starts_with("Player"));
    assert_eq!(signup.data.account_name, signup.data.role_name);
    assert_eq!(signup.data.sign, "dummy_sign");
    assert!(signup.data.create_date.ends_with('Z'));
    assert_eq!(signup.data.server_name, "StarPoint CN");
    assert_eq!(signup.data.server_id, 1);
}
// //// /发送和解码 CN MessagePack 请求 ////
