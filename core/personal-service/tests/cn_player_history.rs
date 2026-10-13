// audience: internal
// # personal-service-cn-player-history-tests
// 回忆录请求覆盖独立开放期, 实际存档统计, 卡片编辑和访问任务的原子领奖.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, send_request, Envelope};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn api(service: &PersonalService, route: &str, body: Value) -> Envelope<Value> {
    decode_response(&send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    ))
}

fn admin(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let bearer = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &bearer)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn set_time(service: &PersonalService, iso: &str) {
    admin(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"iso":iso,"rate":1.0}),
    );
}

fn start(root: &Path, cdn: &Path) -> (PersonalService, i64) {
    std::fs::write(
        cdn.join("activity-catalog.json"),
        json!({
            "format_version":1,"activities":[
                {"activity_id":"collect-item:100009","kind":"collect-item","name":"回忆录访问任务"}
            ]
        })
        .to_string(),
    )
    .unwrap();
    let service = PersonalService::start_with_cdn_root(root, 0, cdn).unwrap();
    set_time(&service, "2025-06-20T12:00:00.000Z");
    let signup = api(&service, "tool/signup", json!({"device_id":98206}));
    (service, signup.data_headers.viewer_id)
}

fn snapshot(root: &Path) -> Value {
    let connection = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = connection
        .query_row(
            "SELECT data_json FROM player_snapshots ORDER BY account_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn currency(root: &Path) -> i64 {
    snapshot(root)["user_info"]["free_vmoney"].as_i64().unwrap()
}

fn completed(response: &Value) -> Vec<i64> {
    response["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["mission_id"].as_i64().unwrap())
        .collect()
}

fn open_mission(service: &PersonalService) {
    admin(
        service,
        "PUT",
        "/v1/activities/collect-item:100009/mode",
        json!({"mode":"always"}),
    );
}

// //// 仅在回忆录与所属任务同时合法时领奖并支持管理重置 [@x380kkm 2026-09-26] ////
#[test]
fn requires_independent_display_and_mission_windows_and_awards_once() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let (service, viewer) = start(root.path(), cdn.path());
    let request = json!({"viewer_id":viewer});
    let before = currency(root.path());
    assert_eq!(
        api(&service, "player_history/index", request.clone())
            .data_headers
            .result_code,
        11101
    );
    assert_eq!(currency(root.path()), before);
    assert!(snapshot(root.path()).get("player_history_cards").is_none());
    assert!(snapshot(root.path())
        .get("player_history_captures")
        .is_none());

    set_time(&service, "2025-07-18T12:00:00.000Z");
    let display = api(&service, "player_history/index", request.clone());
    assert_eq!(display.data_headers.result_code, 1);
    assert_eq!(display.data["player_history_id"], 1);
    assert_eq!(
        display.data["player_history_topic_list"]
            .as_object()
            .unwrap()
            .len(),
        27
    );
    assert!(completed(&display.data).is_empty());
    assert_eq!(currency(root.path()), before);

    open_mission(&service);
    let awarded = api(&service, "player_history/index", request.clone());
    assert_eq!(
        awarded.data["player_history_topic_list"]["11"]["value_list"],
        display.data["player_history_topic_list"]["11"]["value_list"]
    );
    assert_eq!(completed(&awarded.data), vec![10166]);
    assert_eq!(currency(root.path()), before + 1500);
    assert!(completed(&api(&service, "player_history/index", request.clone()).data).is_empty());
    assert_eq!(currency(root.path()), before + 1500);

    let slot = admin(&service, "GET", "/v1/local-saves", json!({}))["slots"][0]["id"]
        .as_i64()
        .unwrap();
    admin(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot}/activities/collect-item:100009/reset"),
        json!({"expected_round":0}),
    );
    assert_eq!(
        completed(&api(&service, "player_history/index", request.clone()).data),
        vec![10166]
    );
    assert_eq!(currency(root.path()), before + 3000);

    admin(
        &service,
        "PUT",
        "/v1/activities/calendar/player-history%3A1",
        json!({
            "enabled":false,"start_at_ms":0,"end_at_ms":253402300799000_i64
        }),
    );
    assert_eq!(
        api(&service, "player_history/index", request.clone())
            .data_headers
            .result_code,
        11101
    );
    assert_eq!(
        api(&service, "player_history/edit", request)
            .data_headers
            .result_code,
        11101
    );
    assert_eq!(currency(root.path()), before + 3000);
    let persisted = snapshot(root.path());
    let loaded = api(&service, "load", json!({"viewer_id":viewer,"keychain":viewer}));
    assert_eq!(loaded.data_headers.result_code, 1);
    let reloaded = snapshot(root.path());
    for field in ["mission_action_receipts", "player_history_cards", "player_history_captures"] {
        assert!(persisted.get(field).is_some());
        assert!(loaded.data.get(field).is_none());
        assert_eq!(reloaded[field], persisted[field]);
    }
    service.stop().unwrap();
}

// //// 快照失败时回滚访问计数与奖励并允许原请求重试 [@x380kkm 2026-09-26] ////
#[test]
fn rolls_back_visit_and_reward_when_snapshot_storage_fails() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let (service, viewer) = start(root.path(), cdn.path());
    set_time(&service, "2025-07-18T12:00:00.000Z");
    open_mission(&service);
    let before = snapshot(root.path());
    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_history_save BEFORE UPDATE ON player_snapshots BEGIN SELECT RAISE(ABORT, 'snapshot unavailable'); END;").unwrap();
    let request = json!({"viewer_id":viewer});
    let failed = send_request(
        service.port(),
        "/api/index.php/player_history/index",
        &encode_request(&request),
    );
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(snapshot(root.path()), before);
    let counters: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM player_mission_counters WHERE pattern='player_history_check'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(counters, 0);
    connection
        .execute_batch("DROP TRIGGER reject_history_save;")
        .unwrap();
    let captured = api(&service, "player_history/index", request.clone()).data;
    assert_eq!(completed(&captured), vec![10166]);
    assert_eq!(
        captured["player_history_topic_list"]["11"]["value_list"]["int_values"][0]
            .as_i64()
            .unwrap_or_default(),
        0
    );
    let repeated = api(&service, "player_history/index", request).data;
    assert_eq!(
        repeated["player_history_topic_list"],
        captured["player_history_topic_list"]
    );
    assert!(completed(&repeated).is_empty());
    assert_eq!(
        currency(root.path()),
        before["user_info"]["free_vmoney"].as_i64().unwrap() + 1500
    );
    service.stop().unwrap();
}

// //// 固定真实统计并保持卡片可编辑及管理重置后的重新采样 [@x380kkm 2026-09-26] ////
#[test]
fn reads_recorded_statistics_and_retains_card_choices_after_restart() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let (service, viewer) = start(root.path(), cdn.path());
    set_time(&service, "2025-07-18T12:00:00.000Z");
    let mut stored = snapshot(root.path());
    stored["mission_login_days"] = json!([11, 12, 12]);
    stored["mission_economic_counters"] = json!({"total_used_mana_count":125000});
    let battles: Value =
        serde_json::from_str(include_str!("../assets/cn-single-battle.json")).unwrap();
    let rush = battles["quests"]
        .as_object()
        .unwrap()
        .values()
        .find_map(|quest| quest["rush_event_id"].as_i64())
        .unwrap();
    stored["cn_activity_state"]["event_families"]["rush"][rush.to_string()] = json!({
        "endless_battle_max_round":24
    });
    stored["user_equipment_list"]["5040020"] = json!({"level":5,"enhancement_level":120,"stack":1});
    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [stored.to_string()],
        )
        .unwrap();
    let request = json!({"viewer_id":viewer});
    let response = api(&service, "player_history/index", request.clone()).data;
    let topics = &response["player_history_topic_list"];
    assert_eq!(topics["19"]["value_list"]["int_values"], json!([rush, 24]));
    assert_eq!(
        topics["19"]["value_list"]["character_id_values"],
        json!([null, null, null])
    );
    assert_eq!(topics["2"]["value_list"]["int_values"], json!([2]));
    assert_eq!(topics["26"]["value_list"]["int_values"], json!([125000]));
    assert_eq!(topics["1"]["value_list"]["date_values"], json!([null]));
    assert_eq!(
        topics["17"]["value_list"]["equipment_id_values"],
        json!([5040020, 5100011, 5030028, 5010032, 5010056, 5010045])
    );
    let equipment = topics["17"]["value_list"]["int_values"].as_array().unwrap();
    assert_eq!(equipment.len(), 12);
    assert_eq!(&equipment[..2], &[json!(5), json!(120)]);
    assert!(equipment[2..].iter().all(Value::is_null));
    assert_eq!(
        response["favorite_character"]["character_ids"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        response["favorite_character"]["unison_character_ids"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let first_capture =
        snapshot(root.path())["player_history_captures"]["player-history:1:round:0"].clone();
    assert!(first_capture["captured_at"].as_i64().is_some());
    let mut progressed = snapshot(root.path());
    progressed["mission_login_days"] = json!([11, 12, 13]);
    progressed["mission_economic_counters"]["total_used_mana_count"] = json!(350000);
    connection
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [progressed.to_string()],
        )
        .unwrap();
    let repeated = api(&service, "player_history/index", request.clone()).data;
    assert_eq!(&repeated["player_history_topic_list"], topics);

    let edited = api(
        &service,
        "player_history/edit",
        json!({
            "viewer_id":viewer,"background_card_id":1002,"degree_id":null,"party_info":null,
            "player_history_topic_visible":{"1":false,"17":true}
        }),
    );
    assert_eq!(edited.data_headers.result_code, 1);
    let saved = snapshot(root.path());
    let invalid = send_request(
        service.port(),
        "/api/index.php/player_history/edit",
        &encode_request(&json!({
            "viewer_id":viewer,"background_card_id":999999,"player_history_topic_visible":{"1":true}
        })),
    );
    assert!(invalid.starts_with("HTTP/1.1 400"), "{invalid}");
    assert_eq!(snapshot(root.path()), saved);
    service.stop().unwrap();

    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let response = api(&service, "player_history/index", request.clone()).data;
    assert_eq!(response["background_card_id"], 1002);
    assert_eq!(
        response["player_history_topic_list"]["1"]["is_visible"],
        false
    );
    assert_eq!(
        response["player_history_topic_list"]["17"]["is_visible"],
        true
    );
    assert!(completed(&response).is_empty());
    for (index, topic) in topics.as_object().unwrap() {
        assert_eq!(
            response["player_history_topic_list"][index]["value_list"],
            topic["value_list"]
        );
    }
    assert_eq!(
        snapshot(root.path())["player_history_captures"]["player-history:1:round:0"],
        first_capture
    );
    let slot = admin(&service, "GET", "/v1/local-saves", json!({}))["slots"][0]["id"]
        .as_i64()
        .unwrap();
    admin(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot}/activities/player-history:1/reset"),
        json!({"expected_round":0}),
    );
    let refreshed = api(&service, "player_history/index", request).data;
    assert_eq!(
        refreshed["player_history_topic_list"]["2"]["value_list"]["int_values"],
        json!([3])
    );
    assert_eq!(
        refreshed["player_history_topic_list"]["26"]["value_list"]["int_values"],
        json!([350000])
    );
    assert_eq!(refreshed["background_card_id"], 1002);
    assert_eq!(
        refreshed["player_history_topic_list"]["1"]["is_visible"],
        false
    );
    let captures = snapshot(root.path())["player_history_captures"].clone();
    assert_eq!(captures.as_object().unwrap().len(), 2);
    assert_eq!(captures["player-history:1:round:0"], first_capture);
    assert_eq!(
        captures["player-history:1:round:1"]["topic_values"]["26"]["int_values"],
        json!([350000])
    );
    service.stop().unwrap();
}
