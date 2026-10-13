// audience: internal
// # personal-service-cn-mission-activity-action-tests
// 活动动作覆盖服务端领奖, 改期, 活动归属, 重放与管理重置.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, send_request};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn api(service: &PersonalService, route: &str, body: Value) -> Value {
    decode_response::<Value>(&send_request(service.port(), &format!("/api/index.php/{route}"), &encode_request(&body))).data
}

fn admin(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let bearer = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(service.port(), method, path, "application/json",
        &[("Authorization", &bearer)], body.to_string().as_bytes());
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn snapshot(root: &Path) -> Value {
    let connection = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = connection.query_row("SELECT data_json FROM player_snapshots ORDER BY account_id LIMIT 1", [], |row| row.get(0)).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn currency(root: &Path) -> i64 {
    snapshot(root)["user_info"]["free_vmoney"].as_i64().unwrap()
}

fn start(root: &Path, cdn: &Path) -> (PersonalService, i64) {
    std::fs::write(cdn.join("activity-catalog.json"), json!({"format_version":1,"activities":[
        {"activity_id":"raid:4","kind":"raid","name":"团队战四"},
        {"activity_id":"raid:5","kind":"raid","name":"团队战五"}
    ]}).to_string()).unwrap();
    let service = PersonalService::start_with_cdn_root(root, 0, cdn).unwrap();
    admin(&service, "PUT", "/v1/time", json!({"enabled":true,"iso":"2026-01-02T12:00:00.000Z","rate":1.0}));
    for id in [4, 5] {
        admin(&service, "PUT", &format!("/v1/activities/raid:{id}/mode"), json!({"mode":"always"}));
    }
    let viewer = decode_response::<Value>(&send_request(service.port(), "/api/index.php/tool/signup", &encode_request(&json!({"device_id":98201})))).data_headers.viewer_id;
    (service, viewer)
}

fn completed(response: &Value) -> Vec<i64> {
    response["mission_info"].as_array().unwrap().iter().map(|entry| entry["mission_id"].as_i64().unwrap()).collect()
}

fn edit_body(viewer: i64, party: &Value) -> Value {
    let mut party = party.clone();
    party["party_category"] = json!(3);
    party["party_edited"] = json!(true);
    json!({"viewer_id":viewer,"api_count":1,"main_party_id":1,"use_party_group_edit":false,"ignore_ngword":true,"party_info_list":[party]})
}

// //// 验证改期后的首页和三支队伍分别领奖并保持活动隔离 [@x380kkm 2026-09-26] ////
#[test]
fn completes_raid_actions_once_per_matching_activity_and_reset() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let (service, viewer) = start(root.path(), cdn.path());
    let before = currency(root.path());
    let body = json!({"viewer_id":viewer,"event_id":4});
    assert_eq!(completed(&api(&service, "event/raid/summary", body.clone())), vec![400053]);
    assert_eq!(currency(root.path()), before + 30);
    assert!(completed(&api(&service, "event/raid/summary", body.clone())).is_empty());
    let groups = api(&service, "event/raid/party", body.clone())["user_party_group_list"].clone();
    for (index, mission) in [400054, 400055, 400056].into_iter().enumerate() {
        let request = edit_body(viewer, &groups[0]["party_list"][index]);
        assert_eq!(completed(&api(&service, "party/edit", request.clone())), vec![mission]);
        assert!(completed(&api(&service, "party/edit", request)).is_empty());
    }
    assert_eq!(currency(root.path()), before + 120);
    let stored = snapshot(root.path());
    assert_eq!(stored["mission_window_progress"]["3:400071:event:1733371200"], Value::Null);
    assert_eq!(completed(&api(&service, "event/raid/summary", json!({"viewer_id":viewer,"event_id":5}))), vec![400071]);
    let slot = admin(&service, "GET", "/v1/local-saves", json!({}))["slots"][0]["id"].as_i64().unwrap();
    admin(&service, "POST", &format!("/v1/local-saves/{slot}/activities/raid:4/reset"), json!({"expected_round":0}));
    assert_eq!(completed(&api(&service, "event/raid/summary", body.clone())), vec![400053]);
    assert_eq!(currency(root.path()), before + 180);
    assert!(completed(&api(&service, "event/raid/summary", body)).is_empty());
    let response = send_request(service.port(), "/api/index.php/mission/update_mission_progress", &encode_request(&json!({
        "viewer_id":viewer,"api_count":2,"mission_param_list":[{"mission_pattern":"raid_event_set_edit_sub2","progress_value":1}]
    })));
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    service.stop().unwrap();
}

// //// 编队任务存储失败时回滚编队与奖励并允许重试 [@x380kkm 2026-09-26] ////
#[test]
fn rolls_back_party_and_mission_rewards_when_snapshot_write_fails() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let (service, viewer) = start(root.path(), cdn.path());
    let groups = api(&service, "event/raid/party", json!({"viewer_id":viewer,"event_id":4}))["user_party_group_list"].clone();
    let before = snapshot(root.path());
    let mut body = edit_body(viewer, &groups[0]["party_list"][1]);
    body["party_info_list"][0]["party_name"] = json!("第二支队伍");
    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_party_save BEFORE UPDATE ON player_snapshots BEGIN SELECT RAISE(ABORT, 'snapshot unavailable'); END;").unwrap();
    let response = send_request(service.port(), "/api/index.php/party/edit", &encode_request(&body));
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(snapshot(root.path()), before);
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM player_mission_counters WHERE pattern='raid_event_set_edit_sub1'", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
    connection.execute_batch("DROP TRIGGER reject_party_save;").unwrap();
    assert_eq!(completed(&api(&service, "party/edit", body)), vec![400055]);
    assert_eq!(currency(root.path()), before["user_info"]["free_vmoney"].as_i64().unwrap() + 30);
    admin(&service, "PUT", "/v1/activities/calendar/raid%3A4", json!({"enabled":false,"start_at_ms":0,"end_at_ms":253402300799000_i64}));
    let third_party = edit_body(viewer, &groups[0]["party_list"][2]);
    assert!(completed(&api(&service, "party/edit", third_party.clone())).is_empty());
    assert_eq!(currency(root.path()), before["user_info"]["free_vmoney"].as_i64().unwrap() + 30);
    admin(&service, "PUT", "/v1/activities/raid:4/mode", json!({"mode":"always"}));
    assert_eq!(completed(&api(&service, "party/edit", third_party)), vec![400056]);
    service.stop().unwrap();
}
