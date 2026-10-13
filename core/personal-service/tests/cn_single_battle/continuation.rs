// audience: internal
// # personal-service-cn-single-continuation-tests
// 复活费用, 续命次数和响应回执共享战斗事务.

use super::*;
use std::path::Path;

// //// 读取战斗玩家的持久化快照 [@x380kkm 2026-09-26] ////
fn snapshot(root: &Path) -> Value {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let stored: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&stored).unwrap()
}
// //// /读取战斗玩家的持久化快照 ////

// //// 设置复活请求的两类余额 [@x380kkm 2026-09-26] ////
fn set_balance(root: &Path, free: i64, paid: i64) {
    let mut data = snapshot(root);
    data["user_info"]["free_vmoney"] = json!(free);
    data["user_info"]["vmoney"] = json!(paid);
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    db.execute(
        "UPDATE player_snapshots SET data_json = ?1",
        [data.to_string()],
    )
    .unwrap();
}
// //// /设置复活请求的两类余额 ////

// //// 发送客户端复活参数 [@x380kkm 2026-09-26] ////
fn revive(service: &PersonalService, viewer: i64, retry: i64) -> String {
    send_battle_request(
        service.port(),
        "play_continue",
        &json!({
            "viewer_id":viewer,"api_count":2,"retry_count":retry,"payment_type":1,
            "category":1,"quest_id":1001002,"play_id":"cn-single-battle-test"
        }),
    )
}
// //// /发送客户端复活参数 ////

// //// 读取持久化续命次数 [@x380kkm 2026-09-26] ////
fn continue_count(root: &Path) -> i64 {
    Connection::open(root.join("personal-service.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT continue_count FROM active_single_quests",
            [],
            |row| row.get(0),
        )
        .unwrap()
}
// //// /读取持久化续命次数 ////

// //// 核对免费优先扣款与重启后的复活重放 [@x380kkm 2026-09-26] ////
#[test]
fn spends_combined_balance_and_replays_continuation() {
    for (free, paid, remaining_free, remaining_paid) in
        [(20, 30, 0, 0), (20, 60, 0, 30), (80, 40, 30, 40)]
    {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let viewer = decode_response::<SignupData>(&send_request(
            service.port(),
            "/api/index.php/tool/signup",
            &encode_request(&SignupRequest { device_id: 286 }),
        ))
        .data_headers
        .viewer_id;
        let started = decode_response::<Value>(&start_battle(service.port(), viewer));
        assert_eq!(started.data_headers.result_code, 1);
        set_balance(root.path(), free, paid);
        let continued = decode_response::<Value>(&revive(&service, viewer, 0));
        assert_eq!(continued.data_headers.result_code, 1);
        assert_eq!(continued.data["user_info"]["free_vmoney"], remaining_free);
        assert_eq!(continued.data["user_info"]["vmoney"], remaining_paid);
        let saved = snapshot(root.path());
        assert_eq!(saved["user_info"]["free_vmoney"], remaining_free);
        assert_eq!(saved["user_info"]["vmoney"], remaining_paid);
        assert_eq!(continue_count(root.path()), 1);
        service.stop().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let replay = decode_response::<Value>(&revive(&service, viewer, 1));
        assert_eq!(replay.data, continued.data);
        assert_eq!(snapshot(root.path()), saved);
        assert_eq!(continue_count(root.path()), 1);
        service.stop().unwrap();
    }
}
// //// /核对免费优先扣款与重启后的复活重放 ////

// //// 核对余额不足和回执写入失败时的战斗回滚 [@x380kkm 2026-09-26] ////
#[test]
fn rolls_back_continuation_assets_and_count() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 287 }),
    ))
    .data_headers
    .viewer_id;
    decode_response::<Value>(&start_battle(service.port(), viewer));
    set_balance(root.path(), 20, 20);
    let before = snapshot(root.path());
    let rejected = revive(&service, viewer, 0);
    assert!(rejected.starts_with("HTTP/1.1 400"), "{rejected}");
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(continue_count(root.path()), 0);
    set_balance(root.path(), 20, 60);
    let before = snapshot(root.path());
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_continue_receipt BEFORE INSERT ON single_battle_action_receipts WHEN NEW.action = 'continue' BEGIN SELECT RAISE(ABORT, 'receipt unavailable'); END;").unwrap();
    let failed = revive(&service, viewer, 0);
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(continue_count(root.path()), 0);
    db.execute_batch("DROP TRIGGER reject_continue_receipt")
        .unwrap();
    let continued = decode_response::<Value>(&revive(&service, viewer, 0));
    assert_eq!(continued.data["user_info"]["free_vmoney"], 0);
    assert_eq!(continued.data["user_info"]["vmoney"], 30);
    assert_eq!(continue_count(root.path()), 1);
    service.stop().unwrap();
}
// //// /核对余额不足和回执写入失败时的战斗回滚 ////
