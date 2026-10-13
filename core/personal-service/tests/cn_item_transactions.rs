// audience: internal
// # personal-service-cn-item-transactions-tests
// 道具消耗, 体力与玛纳变更, 任务进度和请求回执共同持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_item_transactions/selection.rs"]
mod selection;

use cn_support::{decode_response, encode_request, send_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 准备具有道具和固定体力计时的玩家 [@x380kkm 2026-09-26] ////
fn prepare(root: &Path) -> (PersonalService, i64) {
    let service = PersonalService::start(root, 0).unwrap();
    let authorization = format!("Bearer {}", service.management_token());
    let clock = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", authorization.as_str())],
        br#"{"enabled":true,"iso":"2025-01-02T12:00:00.000Z","rate":1.0}"#,
    );
    assert!(clock.starts_with("HTTP/1.1 200"), "{clock}");
    let viewer = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 288 }),
    ))
    .data_headers
    .viewer_id;
    let mut player = snapshot(root);
    player["user_info"]["stamina"] = json!(20);
    player["user_info"]["stamina_heal_time"] = json!(1735819200_i64);
    player["user_info"]["free_mana"] = json!(1000);
    player["user_info"]["paid_mana"] = json!(0);
    player["item_list"]["106"] = json!(5);
    player["item_list"]["101"] = json!(4);
    player["item_list"]["1"] = json!(10);
    player["mission_economic_counters"] = json!({"mana_addition_count":0});
    save(root, &player);
    (service, viewer)
}
// //// /准备具有道具和固定体力计时的玩家 ////

// //// 读取持久化玩家快照 [@x380kkm 2026-09-26] ////
fn snapshot(root: &Path) -> Value {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let serialized: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&serialized).unwrap()
}
// //// /读取持久化玩家快照 ////

// //// 保存用例所需的库存与余额 [@x380kkm 2026-09-26] ////
fn save(root: &Path, player: &Value) {
    Connection::open(root.join("personal-service.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE player_snapshots SET data_json = ?1",
            [player.to_string()],
        )
        .unwrap();
}
// //// /保存用例所需的库存与余额 ////

// //// 使用真实会话发送道具请求 [@x380kkm 2026-09-26] ////
fn send(service: &PersonalService, viewer: i64, route: &str, mut body: Value) -> String {
    body["viewer_id"] = json!(viewer);
    send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    )
}
// //// /使用真实会话发送道具请求 ////

// //// 核对多种体力道具的合并数量和重启重放 [@x380kkm 2026-09-26] ////
#[test]
fn replays_stamina_items_after_restart_and_keeps_receipts_internal() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let request = json!({"api_count":1,"items":[{"id":106,"number":1},{"id":101,"number":2},{"id":106,"number":1}]});
    let used = decode_response::<Value>(&send(&service, viewer, "item/use_item", request));
    assert_eq!(used.data_headers.result_code, 1);
    assert_eq!(used.data["item_list"]["106"], 3);
    assert_eq!(used.data["item_list"]["101"], 2);
    assert_eq!(used.data["user_info"]["stamina"], 270);
    let saved = snapshot(root.path());
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let equivalent = json!({"api_count":1,"items":[{"id":101,"number":2},{"id":106,"number":2}]});
    let replay = decode_response::<Value>(&send(&service, viewer, "item/use_item", equivalent));
    assert_eq!(replay.data, used.data);
    assert_eq!(replay.data_headers.servertime, used.data_headers.servertime);
    assert_eq!(snapshot(root.path()), saved);
    let conflict = send(
        &service,
        viewer,
        "item/use_item",
        json!({"api_count":1,"items":[{"id":106,"number":3}]}),
    );
    assert!(
        conflict.starts_with("HTTP/1.1 400") && conflict.contains("item_request_conflict"),
        "{conflict}"
    );
    assert_eq!(snapshot(root.path()), saved);
    let next = decode_response::<Value>(&send(
        &service,
        viewer,
        "item/use_item",
        json!({"api_count":2,"items":[{"id":106,"number":1}]}),
    ));
    assert_eq!(next.data["item_list"]["106"], 2);
    assert_eq!(next.data["user_info"]["stamina"], 370);
    let loaded =
        decode_response::<Value>(&send(&service, viewer, "load", json!({"keychain":viewer})));
    assert!(loaded.data.get("item_request_receipts").is_none());
    assert!(snapshot(root.path())["item_request_receipts"]
        .as_object()
        .is_some_and(|receipts| receipts.len() == 2));
    service.stop().unwrap();
}
// //// /核对多种体力道具的合并数量和重启重放 ////

// //// 核对售出道具和玛纳任务仅随一次请求结算 [@x380kkm 2026-09-26] ////
#[test]
fn replays_item_sale_without_repeating_inventory_or_missions() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let request = json!({"api_count":1,"item_id":1,"sell_number":2});
    let sold = decode_response::<Value>(&send(&service, viewer, "item/sell", request.clone()));
    assert_eq!(sold.data_headers.result_code, 1);
    assert_eq!(sold.data["item_list"]["1"], 8);
    assert_eq!(sold.data["user_info"]["free_mana"], 1010);
    let saved = snapshot(root.path());
    assert_eq!(
        saved["mission_economic_counters"]["mana_addition_count"],
        10
    );
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let replay = decode_response::<Value>(&send(&service, viewer, "item/sell", request));
    assert_eq!(replay.data, sold.data);
    assert_eq!(replay.data_headers.servertime, sold.data_headers.servertime);
    assert_eq!(snapshot(root.path()), saved);
    let conflict = send(
        &service,
        viewer,
        "item/sell",
        json!({"api_count":1,"item_id":1,"sell_number":3}),
    );
    assert!(
        conflict.starts_with("HTTP/1.1 400") && conflict.contains("item_request_conflict"),
        "{conflict}"
    );
    assert_eq!(snapshot(root.path()), saved);
    let next = decode_response::<Value>(&send(
        &service,
        viewer,
        "item/sell",
        json!({"api_count":2,"item_id":1,"sell_number":3}),
    ));
    assert_eq!(next.data["item_list"]["1"], 5);
    assert_eq!(next.data["user_info"]["free_mana"], 1025);
    assert_eq!(
        snapshot(root.path())["mission_economic_counters"]["mana_addition_count"],
        25
    );
    service.stop().unwrap();
}
// //// /核对售出道具和玛纳任务仅随一次请求结算 ////

// //// 核对库存不足与快照写入失败时的整单回滚 [@x380kkm 2026-09-26] ////
#[test]
fn rolls_back_item_mutations_and_allows_retry_after_failure() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let before = snapshot(root.path());
    let insufficient = send(
        &service,
        viewer,
        "item/use_item",
        json!({"api_count":1,"items":[{"id":101,"number":2},{"id":106,"number":6}]}),
    );
    assert!(insufficient.starts_with("HTTP/1.1 400"), "{insufficient}");
    assert_eq!(snapshot(root.path()), before);
    for (route, request) in [
        (
            "item/use_item",
            json!({"api_count":1,"items":[{"id":106,"number":2}]}),
        ),
        (
            "item/sell",
            json!({"api_count":1,"item_id":1,"sell_number":2}),
        ),
    ] {
        let before = snapshot(root.path());
        let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER reject_item_snapshot BEFORE UPDATE ON player_snapshots BEGIN SELECT RAISE(ABORT, 'snapshot unavailable'); END;").unwrap();
        let failed = send(&service, viewer, route, request.clone());
        assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
        assert_eq!(snapshot(root.path()), before);
        db.execute_batch("DROP TRIGGER reject_item_snapshot")
            .unwrap();
        let succeeded = decode_response::<Value>(&send(&service, viewer, route, request));
        assert_eq!(succeeded.data_headers.result_code, 1);
    }
    let saved = snapshot(root.path());
    assert_eq!(saved["item_list"]["106"], 3);
    assert_eq!(saved["user_info"]["stamina"], 220);
    assert_eq!(saved["item_list"]["1"], 8);
    assert_eq!(saved["user_info"]["free_mana"], 1010);
    assert_eq!(
        saved["mission_economic_counters"]["mana_addition_count"],
        10
    );
    service.stop().unwrap();
}
// //// /核对库存不足与快照写入失败时的整单回滚 ////

// //// 核对体力已满时的成功重放保留原库存 [@x380kkm 2026-09-26] ////
#[test]
fn replays_item_use_after_reaching_stamina_limit() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let mut player = snapshot(root.path());
    player["user_info"]["stamina"] = json!(950);
    save(root.path(), &player);
    let request = json!({"api_count":1,"items":[{"id":106,"number":1}]});
    let used = decode_response::<Value>(&send(&service, viewer, "item/use_item", request.clone()));
    assert_eq!(used.data["user_info"]["stamina"], 999);
    assert_eq!(used.data["item_list"]["106"], 4);
    let saved = snapshot(root.path());
    let replay = decode_response::<Value>(&send(&service, viewer, "item/use_item", request));
    assert_eq!(replay.data, used.data);
    assert_eq!(snapshot(root.path()), saved);
    service.stop().unwrap();
}
// //// /核对体力已满时的成功重放保留原库存 ////

// //// 按玛纳合计上限核对整单出售和失败后重试 [@x380kkm 2026-09-26] ////
#[test]
fn preserves_sale_items_when_combined_mana_has_insufficient_capacity() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let mut player = snapshot(root.path());
    player["user_info"]["free_mana"] = json!(999999989);
    player["user_info"]["paid_mana"] = json!(5);
    save(root.path(), &player);
    let request = json!({"api_count":1,"item_id":1,"sell_number":2});
    let failed = send(&service, viewer, "item/sell", request.clone());
    assert!(
        failed.starts_with("HTTP/1.1 400") && failed.contains("mana_capacity_exceeded"),
        "{failed}"
    );
    assert_eq!(snapshot(root.path()), player);
    player["user_info"]["free_mana"] = json!(999999984);
    save(root.path(), &player);
    let sold = decode_response::<Value>(&send(&service, viewer, "item/sell", request.clone()));
    assert_eq!(sold.data["user_info"]["free_mana"], 999999994);
    assert_eq!(sold.data["item_list"]["1"], 8);
    let saved = snapshot(root.path());
    assert_eq!(saved["user_info"]["paid_mana"], 5);
    assert_eq!(
        saved["mission_economic_counters"]["mana_addition_count"],
        10
    );
    let replay = decode_response::<Value>(&send(&service, viewer, "item/sell", request));
    assert_eq!(replay.data, sold.data);
    assert_eq!(snapshot(root.path()), saved);
    service.stop().unwrap();
}
// //// /按玛纳合计上限核对整单出售和失败后重试 ////
