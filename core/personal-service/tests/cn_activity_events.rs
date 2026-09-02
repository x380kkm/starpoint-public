// audience: internal
// # personal-service-cn-activity-event-tests
//
// 该文件验证 CN 嘉年华, raid 和 rush 入口共享活动窗口并持久化活动进度.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use support::request_with_headers;
use tempfile::TempDir;

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}

fn send(service: &PersonalService, path: &str, body: Value) -> String {
    cn_support::send_request(service.port(), path, &encode_request(&body))
}

fn set_virtual_time(service: &PersonalService, iso: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let body = format!(r#"{{"enabled":true,"iso":"{iso}","rate":1.0}}"#);
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}

fn set_activity_window(service: &PersonalService, activity_id: &str, start_at: &str, end_at: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let path = format!(
        "/v1/activities/calendar/{}",
        activity_id.replace(':', "%3A")
    );
    let body = format!(r#"{{"enabled":true,"start_at":"{start_at}","end_at":"{end_at}"}}"#);
    let response = request_with_headers(
        service.port(),
        "PUT",
        &path,
        "application/json",
        &authorization,
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}

fn close_activity(service: &PersonalService, activity_id: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let path = format!("/v1/activities/{}/close", activity_id.replace(':', "%3A"));
    let response = request_with_headers(
        service.port(),
        "POST",
        &path,
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}

// //// 验证嘉年华入口共享结束状态 [@x380kkm 2026-08-22] ////
#[test]
fn applies_carnival_window_to_index_and_party() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 61);
    let event_id = 5_001;

    let index = decode_response::<Value>(&send(
        &service,
        "/api/index.php/carnival_event/index",
        json!({ "event_id": event_id, "viewer_id": viewer_id, "api_count": 1 }),
    ));
    assert!(index.data["records"].is_array());
    assert!(index.data["user_party_group_list"].is_array());

    set_activity_window(
        &service,
        &format!("carnival:{event_id}"),
        "2030-01-02T00:00:00.000Z",
        "2030-01-04T00:00:00.000Z",
    );
    set_virtual_time(&service, "2030-01-05T12:00:00.000Z");
    let blocked_index = send(
        &service,
        "/api/index.php/carnival_event/index",
        json!({ "event_id": event_id, "viewer_id": viewer_id, "api_count": 2 }),
    );
    let blocked_party = send(
        &service,
        "/api/index.php/carnival_event/get_party",
        json!({ "viewer_id": viewer_id, "api_count": 3 }),
    );
    assert!(blocked_index.ends_with("{\"error\":\"activity_ended\"}"));
    assert!(blocked_party.ends_with("{\"error\":\"activity_ended\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证嘉年华入口共享结束状态 ////

// //// 验证嘉年华队伍在没有活动上下文时使用默认队伍 [@x380kkm 2026-08-22] ////
#[test]
fn returns_default_carnival_party_without_activity_context() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 67);
    let party = decode_response::<Value>(&send(
        &service,
        "/api/index.php/carnival_event/get_party",
        json!({"viewer_id": viewer_id, "api_count": 1}),
    ));
    assert!(!party.data["user_party_group_list"]
        .as_array()
        .expect("carnival party groups are an array")
        .is_empty());
    service.stop().expect("service stops cleanly");
}
// //// /验证嘉年华队伍在没有活动上下文时使用默认队伍 ////

// //// 验证 raid 全部入口共享禁用状态 [@x380kkm 2026-08-22] ////
#[test]
fn applies_disabled_raid_state_to_every_entry() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 62);
    let event_id = 9_004;

    close_activity(&service, &format!("raid:{event_id}"));

    let requests = [
        (
            "/api/index.php/event/raid/get_boss",
            json!({ "event_id": event_id, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/raid/summary",
            json!({ "event_id": event_id, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/raid/party",
            json!({ "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/raid/ranking",
            json!({ "event_id": event_id, "viewer_id": viewer_id, "page": 0 }),
        ),
        (
            "/api/index.php/event/raid/ranking/party",
            json!({ "quest_id": 9_004_001, "viewer_id": viewer_id, "rank_number": 1 }),
        ),
        (
            "/api/index.php/event/raid/battle/start",
            json!({
                "quest_id": 9_004_001,
                "party_group_id": 1,
                "play_id": "closed-raid",
                "is_auto_start_mode": false,
                "viewer_id": viewer_id
            }),
        ),
        (
            "/api/index.php/event/raid/select_folder",
            json!({ "event_id": event_id, "folder_id": 1, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/raid/reset",
            json!({ "event_id": event_id, "quest_type": 1, "viewer_id": viewer_id }),
        ),
    ];
    for (path, body) in requests {
        let response = send(&service, path, body);
        assert!(
            response.ends_with("{\"error\":\"activity_disabled\"}"),
            "{path} did not share the disabled state: {response}"
        );
    }
    service.stop().expect("service stops cleanly");
}
// //// /验证 raid 全部入口共享禁用状态 ////

// //// 验证 rush 入口共享未开始状态并持久化进度 [@x380kkm 2026-08-22] ////
#[test]
fn applies_rush_window_and_persists_folder_party_reward_and_battle() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 63);
    let event_id = 700_001;
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    set_activity_window(
        &service,
        &format!("rush:{event_id}"),
        "2030-01-02T00:00:00.000Z",
        "2030-01-04T00:00:00.000Z",
    );

    let blocked_requests = [
        (
            "/api/index.php/event/rush/summary",
            json!({ "event_id": event_id, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/select_folder",
            json!({ "event_id": event_id, "folder_id": 3, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/ranking",
            json!({ "event_id": event_id, "page": 0, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/ranking/played_party",
            json!({ "event_id": event_id, "rank_number": 1, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/aggregated_time",
            json!({ "event_id": event_id, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/party",
            json!({ "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/battle/start",
            json!({
                "is_auto_start_mode": false,
                "party_id": 1,
                "play_id": "closed-rush",
                "quest_id": 700_001_001,
                "viewer_id": viewer_id
            }),
        ),
        (
            "/api/index.php/event/rush/reset",
            json!({ "event_id": event_id, "quest_type": 1, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/reward",
            json!({ "event_id": event_id, "viewer_id": viewer_id }),
        ),
        (
            "/api/index.php/event/rush/endless_battle",
            json!({ "event_id": event_id, "viewer_id": viewer_id }),
        ),
    ];
    for (path, body) in blocked_requests {
        let response = send(&service, path, body);
        assert!(
            response.ends_with("{\"error\":\"activity_not_started\"}"),
            "{path} did not share the not-started state: {response}"
        );
    }

    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    let party = decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/party",
        json!({ "viewer_id": viewer_id }),
    ));
    assert!(party.data["user_party_group_list"].is_array());
    decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/summary",
        json!({ "event_id": event_id, "viewer_id": viewer_id }),
    ));
    decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/select_folder",
        json!({ "event_id": event_id, "folder_id": 3, "viewer_id": viewer_id }),
    ));
    decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/battle/start",
        json!({
            "is_auto_start_mode": false,
            "party_id": 1,
            "play_id": "persisted-rush",
            "quest_id": 700_001_001,
            "viewer_id": viewer_id
        }),
    ));
    decode_response::<Value>(&send(
        &service,
        "/api/index.php/event/rush/reward",
        json!({ "event_id": event_id, "viewer_id": viewer_id }),
    ));
    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let summary = decode_response::<Value>(&send(
        &restarted,
        "/api/index.php/event/rush/summary",
        json!({ "event_id": event_id, "viewer_id": viewer_id }),
    ));
    assert_eq!(summary.data["active_rush_battle_folder_id"], 3);
    decode_response::<Value>(&send(
        &restarted,
        "/api/index.php/event/rush/reset",
        json!({ "event_id": event_id, "quest_type": 1, "viewer_id": viewer_id }),
    ));
    let reset_summary = decode_response::<Value>(&send(
        &restarted,
        "/api/index.php/event/rush/summary",
        json!({ "event_id": event_id, "viewer_id": viewer_id }),
    ));
    assert_eq!(
        reset_summary.data["active_rush_battle_folder_id"],
        Value::Null
    );
    restarted.stop().expect("service stops cleanly");
}
// //// /验证 rush 入口共享未开始状态并持久化进度 ////
