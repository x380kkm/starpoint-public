// audience: internal
// # personal-service-cn-box-gacha-retry-tests
// 请求重放覆盖箱重置和活动关闭, 资产与保存的响应保持一致.

use super::*;

// //// 通过隔离服务管理接口设置箱池活动状态 [@x380kkm 2026-09-26] ////
fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = request_with_headers(
        service.port(), method, path, "application/json",
        &[("Authorization", token.as_str())], body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}
// //// /通过隔离服务管理接口设置箱池活动状态 ////

// //// 重放抽取时保留箱重置和活动关闭后的资产 [@x380kkm 2026-09-26] ////
#[test]
fn replays_draws_after_box_reset_and_activity_closure() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        json!({"format_version":1,"activities":[
            {"activity_id":"box-gacha:1","name":"箱池","kind":"box-gacha"}
        ]}).to_string(),
    ).expect("activity catalog is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 71 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    manage(&service, "PUT", "/v1/time",
        json!({"enabled":true,"iso":"2030-01-02T12:00:00.000Z","rate":1.0}));
    set_box_currency(&root, 100);
    let mut request = BoxExecRequest {
        viewer_id, box_gacha_id: 1, box_id: 1, number: 1,
        stop_on_featured_rewards: false, api_count: Some(431),
    };
    let send = |request: &BoxExecRequest| cn_support::send_request(
        service.port(), "/api/index.php/box_gacha/exec", &encode_request(request),
    );
    let first = send(&request);
    assert_eq!(decode_response::<Value>(&first).data["item_list"]["30101"], 90);
    let committed = read_player_snapshot(&root);
    assert_eq!(send(&request), first);
    assert_eq!(read_player_snapshot(&root), committed);

    request.number = 2;
    assert!(send(&request).ends_with("{\"error\":\"box_gacha_request_conflict\"}"));
    assert_eq!(read_player_snapshot(&root), committed);
    request.number = 1;
    decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/box_gacha/reset",
        &encode_request(&BoxResetRequest { viewer_id, box_gacha_id: 1, box_id: 1 }),
    ));
    let reset = read_player_snapshot(&root);
    assert_eq!(reset["box_gacha_list"]["1"][0]["reset_times"], 1);
    assert_eq!(send(&request), first);
    assert_eq!(read_player_snapshot(&root), reset);

    request.api_count = Some(432);
    let second = send(&request);
    assert_eq!(decode_response::<Value>(&second).data["item_list"]["30101"], 80);
    let committed = read_player_snapshot(&root);
    manage(&service, "PUT", "/v1/activities/calendar/box-gacha:1",
        json!({"enabled":false,"start_at_ms":1893456000000_i64,"end_at_ms":1893715200000_i64}));
    assert_eq!(send(&request), second);
    assert_eq!(read_player_snapshot(&root), committed);
    request.api_count = Some(433);
    assert!(send(&request).ends_with("{\"error\":\"activity_disabled\"}"));
    assert_eq!(read_player_snapshot(&root), committed);
    service.stop().expect("service stops cleanly");
}
// //// /重放抽取时保留箱重置和活动关闭后的资产 ////
