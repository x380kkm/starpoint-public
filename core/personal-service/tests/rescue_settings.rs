// audience: internal
// # personal-service-rescue-settings-tests
// 管理请求保留未修改的救援设置, 无效关卡选择保持原配置.

#[allow(dead_code)]
mod support;

use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn request(service: &PersonalService, method: &str, body: Value) -> String {
    request_at(service, method, "/v1/rescue-settings", body)
}

fn request_at(service: &PersonalService, method: &str, path: &str, body: Value) -> String {
    let authorization = format!("Bearer {}", service.management_token());
    support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", authorization.as_str())],
        body.to_string().as_bytes(),
    )
}

fn data(response: &str) -> Value {
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

// //// 部分关闭与无效选择保持已保存的救援关卡 [@x380kkm 2026-09-23] ////
#[test]
fn partial_disable_preserves_selection_and_invalid_updates_leave_it_unchanged() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let selected = json!([{"category_id":26,"quest_id":1001}]);
    let saved = data(&request(
        &service,
        "PUT",
        json!({
            "enabled":true, "spawn_interval_seconds":90,
            "room_lifetime_seconds":240, "max_rooms":2, "quests":selected
        }),
    ));
    assert_eq!(saved["quests"], selected);
    let invalid = request(
        &service,
        "PUT",
        json!({
            "quests":[{"category_id":23,"quest_id":1001}]
        }),
    );
    assert!(invalid.starts_with("HTTP/1.1 400"));
    let closed = data(&request(&service, "PUT", json!({"enabled":false})));
    assert_eq!(closed["quests"], selected);
    assert_eq!(closed["spawn_interval_seconds"], 90);
    assert_eq!(closed["room_lifetime_seconds"], 240);
    assert_eq!(closed["max_rooms"], 2);
    assert_eq!(closed["enabled"], false);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(data(&request(&service, "GET", json!({}))), closed);
    service.stop().unwrap();
}
// //// /部分关闭与无效选择保持已保存的救援关卡 ////

// //// 关卡铃铛独立更新并遵守开放窗口 [@x380kkm 2026-09-26] ////
#[test]
fn individual_quest_updates_preserve_other_choices_and_reject_closed_quests() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    data(&request(
        &service,
        "PUT",
        json!({
            "enabled":false, "spawn_interval_seconds":90,
            "quests":[{"category_id":2,"quest_id":1002001}]
        }),
    ));
    let path = "/v1/rescue-settings/quests/2/1001001";
    let opened = data(&request_at(&service, "PUT", path, json!({"enabled":true})));
    assert_eq!(opened["enabled"], true);
    assert_eq!(opened["spawn_interval_seconds"], 90);
    assert_eq!(opened["quests"].as_array().unwrap().len(), 2);
    let replay = data(&request_at(&service, "PUT", path, json!({"enabled":true})));
    assert_eq!(opened, replay);
    data(&request_at(
        &service,
        "POST",
        "/v1/activities/quest%3A2%3A1001001/close",
        json!({}),
    ));
    let rejected = request_at(&service, "PUT", path, json!({"enabled":true}));
    assert!(rejected.starts_with("HTTP/1.1 400"), "{rejected}");
    assert!(rejected.contains("rescue_quest_closed"));
    let closed = data(&request_at(&service, "PUT", path, json!({"enabled":false})));
    assert_eq!(
        closed["quests"],
        json!([{"category_id":2,"quest_id":1002001}])
    );
    service.stop().unwrap();
}
// //// /关卡铃铛独立更新并遵守开放窗口 ////
