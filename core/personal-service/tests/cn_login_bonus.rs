// audience: internal
// # personal-service-cn-login-bonus-tests
// 登录奖励通过载入, 展示确认和服务重启验证到账资产, 日期边界及持久化记录.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 操作隔离账号的登录时间和奖励记录 [@x380kkm 2026-09-22] ////
fn send(service: &PersonalService, path: &str, body: Value) -> String {
    cn_support::send_request(service.port(), path, &encode_request(&body))
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&send(
        service,
        "/api/index.php/tool/signup",
        json!({"device_id":92231}),
    ))
    .data_headers
    .viewer_id
}

fn load(service: &PersonalService, viewer: i64) -> Value {
    decode_response::<Value>(&send(
        service,
        "/api/index.php/load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    ))
    .data
}

fn shown(service: &PersonalService, viewer: i64) {
    let response = send(
        service,
        "/api/index.php/bonus/shown",
        json!({"viewer_id":viewer}),
    );
    assert_eq!(decode_response::<Value>(&response).data, json!([]));
}

fn set_time(service: &PersonalService, iso: &str) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", token.as_str())],
        json!({"enabled":true,"iso":iso,"rate":1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn snapshot(root: &Path) -> Value {
    let serialized: String = database(root)
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&serialized).unwrap()
}

fn update_snapshot(root: &Path, update: impl FnOnce(&mut Value)) {
    let mut player = snapshot(root);
    update(&mut player);
    database(root)
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
}

fn pending_index(player: &Value, group: &str) -> Option<i64> {
    player["bonus_index_list"]
        .as_array()?
        .iter()
        .find(|entry| entry["bonus_group_id"] == group)?["index"]
        .as_i64()
}

fn history_count(root: &Path) -> i64 {
    database(root)
        .query_row(
            "SELECT COUNT(*) FROM player_receive_history WHERE event_key LIKE 'login-bonus:%'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}
// //// /操作隔离账号的登录时间和奖励记录 ////

// //// 同一现实日期在重启和虚拟时间调整后保留领取记录 [@x380kkm 2026-09-24] ////
#[test]
fn keeps_daily_receipts_across_restart_and_virtual_time_changes() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2030-01-01T15:00:00.000Z");
    let viewer = signup(&service);
    let initial = snapshot(root.path());
    let first = load(&service, viewer);
    assert_eq!(pending_index(&first, "normal_2022"), Some(1));
    assert_eq!(
        first["user_info"]["free_vmoney"],
        initial["user_info"]["free_vmoney"].as_i64().unwrap() + 50
    );
    let first_history = history_count(root.path());
    assert!(first.get("login_bonus_state").is_none());
    assert!(first.get("login_bonus_received_date").is_none());
    let date: String = database(root.path())
        .query_row("SELECT date('now', 'localtime')", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        snapshot(root.path())["login_bonus_state"]["groups"]["normal_2022"]["received_dates"],
        json!([date])
    );
    shown(&service, viewer);
    shown(&service, viewer);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    for iso in ["2030-01-02T21:00:00.000Z", "2099-01-01T12:00:00.000Z", "2029-12-31T12:00:00.000Z"] {
        set_time(&service, iso);
        let repeated = load(&service, viewer);
        assert_eq!(pending_index(&repeated, "normal_2022"), None);
        assert_eq!(history_count(root.path()), first_history);
        assert_eq!(repeated["user_info"]["free_vmoney"], first["user_info"]["free_vmoney"]);
    }
    service.stop().unwrap();
}
// //// /同一现实日期在重启和虚拟时间调整后保留领取记录 ////

// //// 合并旧存档展示记录并从下一奖励继续领取 [@x380kkm 2026-09-22] ////
#[test]
fn recovers_pending_receipts_before_load_or_shown() {
    for acknowledge_first in [false, true] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        set_time(&service, "2030-01-01T12:00:00.000Z");
        let viewer = signup(&service);
        let first = load(&service, viewer);
        update_snapshot(root.path(), |player| {
            player.as_object_mut().unwrap().remove("login_bonus_state");
            player["user_info"]["last_login_time"] = json!("2040-01-01 12:00:00");
        });
        let receipts = history_count(root.path());
        if acknowledge_first {
            shown(&service, viewer);
        }
        let reloaded = load(&service, viewer);
        assert_eq!(
            reloaded["user_info"]["free_vmoney"],
            first["user_info"]["free_vmoney"]
        );
        assert_eq!(reloaded["item_list"], first["item_list"]);
        assert_eq!(history_count(root.path()), receipts);
        shown(&service, viewer);
        set_time(&service, "2030-01-02T21:00:00.000Z");
        assert_eq!(
            pending_index(&load(&service, viewer), "normal_2022"),
            None
        );
        service.stop().unwrap();
    }
}
// //// /合并旧存档展示记录并从下一奖励继续领取 ////


// //// 领取记录写入失败时保留资产和领取位置 [@x380kkm 2026-09-22] ////
#[test]
fn rolls_back_rewards_when_history_cannot_be_saved() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2030-01-01T12:00:00.000Z");
    let viewer = signup(&service);
    let initial = snapshot(root.path());
    database(root.path()).execute_batch("CREATE TRIGGER reject_login_history BEFORE INSERT ON player_receive_history
        WHEN NEW.event_key LIKE 'login-bonus:%' BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;").unwrap();
    let response = send(
        &service,
        "/api/index.php/load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(snapshot(root.path()), initial);
    assert_eq!(history_count(root.path()), 0);
    database(root.path())
        .execute_batch("DROP TRIGGER reject_login_history;")
        .unwrap();
    let recovered = load(&service, viewer);
    assert_eq!(pending_index(&recovered, "normal_2022"), Some(1));
    assert_eq!(
        recovered["user_info"]["free_vmoney"],
        initial["user_info"]["free_vmoney"].as_i64().unwrap() + 50
    );
    let history = decode_response::<Value>(&send(
        &service,
        "/api/index.php/history/receive",
        json!({"viewer_id":viewer}),
    ))
    .data;
    assert!(history["history"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["number"] == 50));
    service.stop().unwrap();
}
// //// /领取记录写入失败时保留资产和领取位置 ////
