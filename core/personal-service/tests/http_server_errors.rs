// audience: internal
// # personal-service-http-server-error-tests
// 战斗事务异常在管理诊断中持久化, 请求正文和查询参数留在请求边界.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, send_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;

// //// 读取管理页面使用的故障记录 [@x380kkm 2026-09-27] ////
fn reports(service: &PersonalService) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "GET",
        "/v1/http-observations",
        "application/json",
        &[("Authorization", &token)],
        b"",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        ["client_reports"]
        .clone()
}

// //// 保存事务失败原因并允许原战斗在故障解除后结算 [@x380kkm 2026-09-27] ////
#[test]
fn retains_server_failure_without_changing_battle_retry() {
    let root = tempfile::tempdir().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&json!({"device_id":927500})),
    );
    let viewer = decode_response::<SignupData>(&signup)
        .data_headers
        .viewer_id;
    let start = json!({"viewer_id":viewer,"category":1,"quest_id":1002002,"play_id":"diagnostic-battle",
        "party_id":1,"is_auto_start_mode":false,"use_boost_point":false,"use_boss_boost_point":false});
    let response = send_request(
        service.port(),
        "/api/index.php/single_battle_quest/start",
        &encode_request(&start),
    );
    assert_eq!(
        decode_response::<Value>(&response).data_headers.result_code,
        1
    );
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_battle_receipt BEFORE INSERT ON single_battle_finish_receipts
        BEGIN SELECT RAISE(ABORT, 'receipt unavailable'); END;",
    )
    .unwrap();
    let finish = json!({"viewer_id":viewer,"category":1,"quest_id":1002002,"play_id":"diagnostic-battle",
        "elapsed_time_ms":1000,"score":1000,"add_mana":7,"is_accomplished":true,"is_lose":false,
        "private_value":"unreported-request-body","statistics":{"clear_phase":1,"zones":[],"party":{
            "characters":[{"id":1},null,null],"unison_characters":[null,null,null],
            "equipments":[null,null,null],"ability_soul_ids":[null,null,null]}}});
    let request = encode_request(&finish);
    let endpoint = "/api/index.php/single_battle_quest/finish?private=unreported-query";
    let response = send_request(service.port(), endpoint, &request);
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert!(!response.contains("receipt unavailable"));
    let recorded = reports(&service);
    let details = &recorded[0]["details"];
    assert_eq!(details["source"], "server");
    assert_eq!(details["code"], "HTTP 500");
    assert_eq!(details["method"], "POST");
    assert_eq!(details["path"], "/api/index.php/single_battle_quest/finish");
    assert!(details["error"]
        .as_str()
        .unwrap()
        .contains("receipt unavailable"));
    assert!(!recorded.to_string().contains("unreported-"));
    db.execute_batch(
        "CREATE TRIGGER reject_diagnostic BEFORE INSERT ON client_reports
        BEGIN SELECT RAISE(ABORT, 'diagnostics unavailable'); END;",
    )
    .unwrap();
    let response = send_request(service.port(), endpoint, &request);
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(reports(&service), recorded);
    db.execute_batch("DROP TRIGGER reject_diagnostic; DROP TRIGGER reject_battle_receipt;")
        .unwrap();
    let response = send_request(service.port(), endpoint, &request);
    assert_eq!(
        decode_response::<Value>(&response).data_headers.result_code,
        1
    );
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(reports(&service), recorded);
    service.stop().unwrap();
}
