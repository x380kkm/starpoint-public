// audience: internal
// # personal-service-cn-crazy-gacha-tests
// 真实 HTTP 请求覆盖候选, 槽位, 重载, 最终发奖和事务失败恢复.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, send_request};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::BTreeMap;
use std::path::Path;
use support::request_with_headers;
use tempfile::TempDir;

// //// 创建持有乐享扭蛋券的独立账号 [@x380kkm 2026-09-23] ////
fn setup(root: &Path, tickets: i64) -> (PersonalService, i64) {
    let service = PersonalService::start(root, 0).unwrap();
    let signup = decode_response::<Value>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&json!({"device_id": 881})),
    ));
    let viewer = signup.data_headers.viewer_id;
    let bearer = format!("Bearer {}", service.management_token());
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/calendar/gacha%3A100",
        "application/json",
        &[("Authorization", bearer.as_str())],
        br#"{"enabled":true,"start_at_ms":0,"end_at_ms":253402300799000}"#,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let mail = json!({
        "viewer_id": viewer, "title": "乐享扭蛋券", "body": "乐享扭蛋券", "sender": "Admin",
        "rewards": {"itemList": {"999012": tickets}},
    });
    let response = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &[("Authorization", bearer.as_str())],
        mail.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 201 Created"), "{response}");
    api(&service, "/mail/receive_all", json!({"viewer_id": viewer}));
    (service, viewer)
}

fn raw(service: &PersonalService, path: &str, body: Value) -> String {
    send_request(
        service.port(),
        &format!("/api/index.php{path}"),
        &encode_request(&body),
    )
}

fn api(service: &PersonalService, path: &str, body: Value) -> Value {
    decode_response::<Value>(&raw(service, path, body)).data
}

fn draw_request(viewer: i64, api_count: i64) -> Value {
    json!({"viewer_id": viewer, "api_count": api_count, "gacha_id": 100,
        "payment_type": 3, "type": 14, "number_of_exec": 1})
}

fn result_request(viewer: i64, api_count: i64, index: i64) -> Value {
    json!({"viewer_id": viewer, "api_count": api_count, "gacha_id": 100, "index": index})
}

fn stored(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn received_count(root: &Path) -> i64 {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    database.query_row(
        "SELECT COALESCE(SUM(number), 0) FROM player_receive_history WHERE event_key LIKE 'gacha:crazy-select:%'",
        [], |row| row.get(0),
    ).unwrap()
}

fn character_count(player: &Value, id: i64) -> i64 {
    player["user_character_list"]
        .get(id.to_string())
        .map_or(0, |character| {
            1 + character["stack"].as_i64().unwrap_or_default()
        })
}
// //// /创建持有乐享扭蛋券的独立账号 ////

// //// 候选和保存槽重载后保持一致且最终只发所选十人 [@x380kkm 2026-09-23] ////
#[test]
fn preserves_candidates_across_restart_and_grants_selected_group_once() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = setup(root.path(), 1);
    let initial = stored(root.path());
    let first_request = draw_request(viewer, 1);
    let first = api(&service, "/gacha/exec", first_request.clone());
    assert_eq!(first["draw"].as_array().unwrap().len(), 10);
    assert_eq!(first["character_list"], json!([]));
    assert_eq!(first["item_list"]["999012"], 0);
    assert_eq!(
        stored(root.path())["user_character_list"],
        initial["user_character_list"]
    );
    assert_eq!(
        stored(root.path())["mission_economic_counters"],
        initial["mission_economic_counters"]
    );
    assert_eq!(received_count(root.path()), 0);
    assert_eq!(api(&service, "/gacha/exec", first_request), first);

    let first_slot = first["crazy_gacha_result_list"]["0"].clone();
    let save_request = json!({"viewer_id":viewer,"api_count":2,"index":1});
    let saved = api(&service, "/gacha/crazy_gacha_save", save_request.clone());
    assert_eq!(saved["crazy_gacha_result_list"]["1"], first_slot);
    assert_eq!(
        api(&service, "/gacha/crazy_gacha_save", save_request),
        saved
    );
    let second = api(&service, "/gacha/exec", draw_request(viewer, 3));
    assert_eq!(second["gacha_info_list"][0]["crazy_draw_count"], 2);
    assert_eq!(second["item_list"]["999012"], 0);
    assert_eq!(second["crazy_gacha_result_list"]["1"], first_slot);
    let overwrite = raw(
        &service,
        "/gacha/crazy_gacha_save",
        json!({"viewer_id":viewer,"api_count":4,"index":1}),
    );
    assert!(
        overwrite.contains("crazy_gacha_slot_occupied"),
        "{overwrite}"
    );
    let saved_second = api(
        &service,
        "/gacha/crazy_gacha_save",
        json!({"viewer_id":viewer,"api_count":5,"index":2}),
    );
    assert_eq!(
        saved_second["crazy_gacha_result_list"]["2"],
        second["crazy_gacha_result_list"]["0"]
    );
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    let loaded = api(
        &service,
        "/load",
        json!({"keychain": viewer, "viewer_id": viewer}),
    );
    assert_eq!(
        loaded["crazy_gacha_result_list"],
        saved_second["crazy_gacha_result_list"]
    );
    assert_eq!(loaded["last_crazy_gacha_draw_result"], second["draw"]);
    assert!(loaded.get("crazy_gacha_state").is_none());
    let before_select = stored(root.path());
    let selection = result_request(viewer, 6, 1);
    let selected = api(&service, "/gacha/crazy_gacha_select", selection.clone());
    let after_select = stored(root.path());
    assert_eq!(
        after_select["mission_economic_counters"]["total_gacha_character_count"]
            .as_i64()
            .unwrap(),
        before_select["mission_economic_counters"]["total_gacha_character_count"]
            .as_i64()
            .unwrap_or_default()
            + 10
    );
    let mut expected = BTreeMap::<i64, i64>::new();
    for id in first_slot.as_array().unwrap() {
        *expected.entry(id.as_i64().unwrap()).or_default() += 1;
    }
    for (id, count) in expected {
        assert_eq!(
            character_count(&after_select, id) - character_count(&before_select, id),
            count
        );
    }
    assert_eq!(received_count(root.path()), 10);
    assert_eq!(selected["last_crazy_gacha_draw_result"], json!([]));
    assert_eq!(
        selected["crazy_gacha_result_list"],
        json!({"0":[],"1":[],"2":[]})
    );
    assert_eq!(selected["gacha_info_list"][0]["crazy_draw_count"], 0);
    assert_eq!(
        api(&service, "/gacha/crazy_gacha_select", selection),
        selected
    );
    assert_eq!(stored(root.path()), after_select);
    api(
        &service,
        "/gacha/crazy_gacha_select",
        result_request(viewer, 7, 1),
    );
    assert_eq!(
        stored(root.path())["user_character_list"],
        after_select["user_character_list"]
    );
    assert_eq!(received_count(root.path()), 10);
    let empty_ticket = raw(&service, "/gacha/exec", draw_request(viewer, 8));
    assert!(
        empty_ticket.contains("not_enough_tickets"),
        "{empty_ticket}"
    );
    service.stop().unwrap();
}
// //// /候选和保存槽重载后保持一致且最终只发所选十人 ////

// //// 每张券最多产生五十组候选且下一张券开启独立选择 [@x380kkm 2026-09-23] ////
#[test]
fn limits_one_ticket_to_fifty_candidates_and_resets_for_the_next_ticket() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = setup(root.path(), 2);
    let initial = stored(root.path());
    for count in 1..=50 {
        let result = api(&service, "/gacha/exec", draw_request(viewer, count));
        assert_eq!(result["gacha_info_list"][0]["crazy_draw_count"], count);
        assert_eq!(result["item_list"]["999012"], 1);
    }
    let at_limit = stored(root.path());
    assert_eq!(
        at_limit["user_character_list"],
        initial["user_character_list"]
    );
    let exceeded = raw(&service, "/gacha/exec", draw_request(viewer, 51));
    assert!(exceeded.contains("crazy_gacha_draw_limit"), "{exceeded}");
    assert_eq!(stored(root.path()), at_limit);
    api(
        &service,
        "/gacha/crazy_gacha_select",
        result_request(viewer, 52, 0),
    );
    assert_eq!(received_count(root.path()), 10);
    let next = api(&service, "/gacha/exec", draw_request(viewer, 53));
    assert_eq!(next["gacha_info_list"][0]["crazy_draw_count"], 1);
    assert_eq!(next["item_list"]["999012"], 0);
    assert_eq!(next["crazy_gacha_result_list"]["1"], json!([]));
    assert_eq!(next["crazy_gacha_result_list"]["2"], json!([]));
    assert_eq!(received_count(root.path()), 10);
    service.stop().unwrap();
}
// //// /每张券最多产生五十组候选且下一张券开启独立选择 ////

// //// 无效槽位和批量候选请求保持资产与候选不变 [@x380kkm 2026-09-23] ////
#[test]
fn rejects_invalid_actions_without_spending_or_granting() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = setup(root.path(), 2);
    let initial = stored(root.path());
    let mut bulk = draw_request(viewer, 1);
    bulk["number_of_exec"] = json!(2);
    assert!(raw(&service, "/gacha/exec", bulk).contains("invalid_gacha_request"));
    assert!(raw(
        &service,
        "/gacha/crazy_gacha_select",
        result_request(viewer, 2, 0)
    )
    .contains("crazy_gacha_result_missing"));
    assert_eq!(stored(root.path()), initial);
    api(&service, "/gacha/exec", draw_request(viewer, 3));
    let candidate = stored(root.path());
    for index in [-1, 3] {
        assert!(raw(
            &service,
            "/gacha/crazy_gacha_select",
            result_request(viewer, 4, index)
        )
        .contains("invalid_crazy_gacha_slot"));
    }
    assert!(raw(
        &service,
        "/gacha/crazy_gacha_select",
        result_request(viewer, 5, 1)
    )
    .contains("crazy_gacha_result_missing"));
    assert!(raw(
        &service,
        "/gacha/crazy_gacha_save",
        json!({"viewer_id":viewer,"api_count":6,"index":0})
    )
    .contains("invalid_crazy_gacha_slot"));
    let mut wrong_pool = result_request(viewer, 7, 0);
    wrong_pool["gacha_id"] = json!(1);
    assert!(raw(&service, "/gacha/crazy_gacha_select", wrong_pool)
        .contains("invalid_crazy_gacha_request"));
    assert_eq!(stored(root.path()), candidate);
    let bearer = format!("Bearer {}", service.management_token());
    let closed = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/calendar/gacha%3A100",
        "application/json",
        &[("Authorization", bearer.as_str())],
        br#"{"enabled":false,"start_at_ms":0,"end_at_ms":253402300799000}"#,
    );
    assert!(closed.starts_with("HTTP/1.1 200 OK"), "{closed}");
    api(
        &service,
        "/gacha/crazy_gacha_select",
        result_request(viewer, 8, 0),
    );
    assert_eq!(received_count(root.path()), 10);
    service.stop().unwrap();
}
// //// /无效槽位和批量候选请求保持资产与候选不变 ////

// //// 奖励记录失败时保留候选并允许同一请求重新结算 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_selection_when_reward_history_cannot_commit() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = setup(root.path(), 1);
    api(&service, "/gacha/exec", draw_request(viewer, 1));
    let before = stored(root.path());
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TRIGGER reject_crazy_reward BEFORE INSERT ON player_receive_history
         WHEN NEW.event_key LIKE 'gacha:crazy-select:%'
         BEGIN SELECT RAISE(ABORT, 'injected crazy reward failure'); END;",
        )
        .unwrap();
    let request = result_request(viewer, 2, 0);
    let failed = raw(&service, "/gacha/crazy_gacha_select", request.clone());
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(stored(root.path()), before);
    assert_eq!(received_count(root.path()), 0);
    database
        .execute_batch("DROP TRIGGER reject_crazy_reward;")
        .unwrap();
    api(&service, "/gacha/crazy_gacha_select", request);
    assert_eq!(received_count(root.path()), 10);
    assert_eq!(
        stored(root.path())["last_crazy_gacha_draw_result"],
        json!([])
    );
    service.stop().unwrap();
}
// //// /奖励记录失败时保留候选并允许同一请求重新结算 ////
