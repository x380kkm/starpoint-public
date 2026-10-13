// audience: internal
// # personal-service-cn-stamina-purchase-tests
// 体力购买的余额, 体力和请求回执共同持久化.

use super::*;

// //// 核对体力购买的整单回滚与上限处重放 [@x380kkm 2026-09-26] ////
#[test]
fn persists_stamina_purchase_and_receipt_together() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_virtual_time(&service, STAMINA_TEST_TIME);
    let viewer = signup(&service, 286);
    load(service.port(), viewer);
    set_resources(root.path(), 950, STAMINA_TEST_TIME_SECONDS, 20, 80);
    let before = read_player_snapshot(root.path());
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_stamina_purchase BEFORE UPDATE ON player_snapshots BEGIN SELECT RAISE(ABORT, 'snapshot unavailable'); END;").unwrap();
    let failed = recover_stamina(service.port(), viewer);
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(read_player_snapshot(root.path()), before);
    db.execute_batch("DROP TRIGGER reject_stamina_purchase")
        .unwrap();
    drop(db);
    let recovered = decode_response::<Value>(&recover_stamina(service.port(), viewer));
    assert_eq!(recovered.data_headers.result_code, 1);
    assert_eq!(recovered.data["user_info"]["stamina"], 999);
    assert_eq!(recovered.data["user_info"]["free_vmoney"], 0);
    assert_eq!(recovered.data["user_info"]["vmoney"], 50);
    let saved = read_player_snapshot(root.path());
    let replay = decode_response::<Value>(&recover_stamina(service.port(), viewer));
    assert_eq!(replay.data, recovered.data);
    assert_eq!(
        replay.data_headers.servertime,
        recovered.data_headers.servertime
    );
    let capped = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/recover_stamina",
        &encode_request(&RecoverStaminaRequest {
            api_count: 2,
            viewer_id: viewer,
        }),
    ));
    assert_eq!(capped.data_headers.result_code, 2102);
    assert_eq!(read_player_snapshot(root.path()), saved);
    set_resources(root.path(), 20, STAMINA_TEST_TIME_SECONDS, 0, 50);
    let next = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/recover_stamina",
        &encode_request(&RecoverStaminaRequest {
            api_count: 2,
            viewer_id: viewer,
        }),
    ));
    assert_eq!(next.data["user_info"]["stamina"], 120);
    assert_eq!(next.data["user_info"]["vmoney"], 0);
    assert_eq!(read_player_snapshot(root.path())["user_info"]["vmoney"], 0);
    service.stop().unwrap();
}
// //// /核对体力购买的整单回滚与上限处重放 ////
