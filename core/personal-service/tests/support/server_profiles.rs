// audience: internal
// # personal-service-server-profile-test-support
//
// 该文件通过受保护的管理 API 创建和切换测试使用的远端服务器配置.

use crate::support::request_with_headers;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;

// //// 创建和切换远端服务器配置 [@x380kkm 2026-07-23] ////
pub(crate) fn authorized_request(
    port: u16,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> String {
    let encoded = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
    let authorization = format!("Bearer {token}");
    request_with_headers(
        port,
        method,
        path,
        "application/json",
        &[("Authorization", &authorization)],
        &encoded,
    )
}

pub(crate) fn response_body(response: &str) -> Value {
    serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .expect("response contains a body")
            .1,
    )
    .expect("response body is JSON")
}

pub(crate) fn create_remote_profile(
    service: &PersonalService,
    name: &str,
    scheme: &str,
    host: &str,
    port: u16,
) -> i64 {
    let profile = json!({
        "name": name,
        "scheme": scheme,
        "host": host,
        "port": port,
    });
    let response = authorized_request(
        service.port(),
        service.management_token(),
        "POST",
        "/v1/server-profiles",
        Some(&profile),
    );
    assert!(response.starts_with("HTTP/1.1 201 Created"));
    response_body(&response)["id"]
        .as_i64()
        .expect("created profile has an id")
}

pub(crate) fn activate_profile(service: &PersonalService, profile_id: i64) {
    let response = authorized_request(
        service.port(),
        service.management_token(),
        "POST",
        &format!("/v1/server-profiles/{profile_id}/activate"),
        None,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(response_body(&response)["active_profile_id"], profile_id);
}
// //// /创建和切换远端服务器配置 ////
