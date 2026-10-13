// audience: internal
// # personal-service-cn-currency-exchange-tests
// 星屑与羁绊证兑换按真实目录检查资产变化, 目录窗口, 请求重放和领取历史.

#[path = "cn_currency_exchange/bond_claim.rs"]
mod bond_claim;
#[path = "cn_currency_exchange/catalogue.rs"]
mod catalogue;
#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "cn_currency_exchange/lifecycle.rs"]
mod lifecycle;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn request(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    )
}

fn send(service: &PersonalService, route: &str, body: &Value) -> Value {
    let raw = request(service, route, body);
    assert!(raw.starts_with("HTTP/1.1 200"), "{route}: {raw}");
    let response = decode_response::<Value>(&raw);
    assert_eq!(
        response.data_headers.result_code, 1,
        "{route}: {}",
        response.data
    );
    response.data
}

fn signup(service: &PersonalService, device: i64) -> i64 {
    decode_response::<SignupData>(&request(
        service,
        "tool/signup",
        &json!({"device_id":device}),
    ))
    .data_headers
    .viewer_id
}

fn stored(root: &Path, viewer: i64) -> Value {
    let encoded: String = database(root).query_row(
        "SELECT data_json FROM player_snapshots WHERE account_id=(SELECT account_id FROM sessions WHERE token=?1)",
        [viewer.to_string()], |row| row.get(0),
    ).unwrap();
    serde_json::from_str(&encoded).unwrap()
}

fn update(root: &Path, viewer: i64, change: impl FnOnce(&mut Value)) {
    let mut data = stored(root, viewer);
    change(&mut data);
    database(root).execute(
        "UPDATE player_snapshots SET data_json=?1 WHERE account_id=(SELECT account_id FROM sessions WHERE token=?2)",
        params![data.to_string(), viewer.to_string()],
    ).unwrap();
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn set_time(service: &PersonalService, seconds: i64) {
    manage(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"unix_time_ms":seconds*1000,"rate":1}),
    );
}

fn local_time(value: &str) -> i64 {
    Connection::open_in_memory()
        .unwrap()
        .query_row(
            "SELECT CAST(strftime('%s', ?1) AS INTEGER)-28800",
            [value],
            |row| row.get(0),
        )
        .unwrap()
}

fn star_catalog() -> Value {
    serde_json::from_str(include_str!("../../../assets/star_crumb_exchange.json")).unwrap()
}

fn star_request(viewer: i64, id: i64, sequence: i64) -> Value {
    json!({"viewer_id":viewer,"exchange_id":id,"api_count":sequence})
}

fn bond_request(viewer: i64, id: i64, sequence: i64) -> Value {
    json!({"viewer_id":viewer,"equipment_id":id,"api_count":sequence})
}

fn history_count(root: &Path, viewer: i64) -> i64 {
    database(root).query_row(
        "SELECT COUNT(*) FROM player_receive_history WHERE event_key LIKE 'exchange:%' AND account_id=(SELECT account_id FROM sessions WHERE token=?1)",
        [viewer.to_string()], |row| row.get(0),
    ).unwrap()
}

fn history(service: &PersonalService) -> Value {
    let saves = manage(service, "GET", "/v1/local-saves", json!({}));
    let slot = saves["slots"][0]["id"].as_i64().unwrap();
    manage(
        service,
        "GET",
        &format!("/v1/local-saves/{slot}/reward-history"),
        json!({}),
    )
}
