// audience: internal
// # personal-service-local-save-test-support
//
// 该模块发送本地存档管理请求并执行 CN 注册和载入.

use crate::cn_support::{
    assert_valid_signup_response, decode_response, encode_request, send_request,
    send_request_with_resource_version, Envelope, LoadRequest, SignupData, SignupRequest,
};
use crate::support;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;

// //// 发送本地存档管理请求和 CN 客户端请求 [@x380kkm 2026-07-23] ////
pub(crate) fn authorized_request(
    service: &PersonalService,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> String {
    authorized_request_with_headers(service, method, path, &[], body)
}

pub(crate) fn authorized_request_with_headers(
    service: &PersonalService,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&Value>,
) -> String {
    let encoded = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
    let authorization = format!("Bearer {}", service.management_token());
    let mut headers = vec![("Authorization", authorization.as_str())];
    headers.extend_from_slice(extra_headers);
    support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &headers,
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

pub(crate) fn assert_status(response: &str, status: &str) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status}")),
        "unexpected response: {response}"
    );
}

pub(crate) fn signup(port: u16, device_id: i64) -> Envelope<SignupData> {
    let body = encode_request(&SignupRequest { device_id });
    let response = send_request(port, "/api/index.php/tool/signup", &body);
    let signup = decode_response(&response);
    assert_valid_signup_response(&signup);
    signup
}

pub(crate) fn load(port: u16, viewer_id: i64, resource_version: &str) -> Envelope<Value> {
    let body = encode_request(&LoadRequest {
        keychain: viewer_id,
        viewer_id,
    });
    let response =
        send_request_with_resource_version(port, "/api/index.php/load", &body, resource_version);
    decode_response(&response)
}

pub(crate) fn list_local_saves(service: &PersonalService) -> Value {
    let response = authorized_request(service, "GET", "/v1/local-saves", None);
    assert_status(&response, "200 OK");
    response_body(&response)
}

pub(crate) fn export_local_save(service: &PersonalService, slot_id: i64) -> Value {
    let response = authorized_request(
        service,
        "GET",
        &format!("/v1/local-saves/{slot_id}/export"),
        None,
    );
    assert_status(&response, "200 OK");
    response_body(&response)
}

pub(crate) fn activate_local_save(service: &PersonalService, slot_id: i64, device_id: i64) {
    let response = authorized_request(
        service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/activate"),
        Some(&json!({ "device_id": device_id })),
    );
    assert_status(&response, "200 OK");
    assert_eq!(
        response_body(&response)["devices"][0]["active_slot_id"].as_i64(),
        Some(slot_id),
    );
}
// //// /发送本地存档管理请求和 CN 客户端请求 ////

// //// 修改指定槽位的当前玩家快照 [@x380kkm 2026-08-03] ////
pub(crate) fn update_slot_player_snapshot(
    root: &Path,
    slot_id: i64,
    update: impl FnOnce(&mut Value),
) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let (account_id, serialized) = database
        .query_row(
            "SELECT slots.account_id, player_snapshots.data_json
             FROM local_save_slots AS slots
             JOIN player_snapshots ON player_snapshots.account_id = slots.account_id
             WHERE slots.id = ?1",
            params![slot_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("slot player snapshot is read");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    update(&mut player_data);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id,
            ],
        )
        .expect("slot player snapshot is updated");
}
// //// /修改指定槽位的当前玩家快照 ////
