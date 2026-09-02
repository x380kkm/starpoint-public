// audience: internal
// # personal-service-virtual-time-tests
//
// 该文件验证个人服务虚拟时间 API 的直接访问, 时间解析和重启持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde_json::json;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::fs;
use std::thread;
use std::time::Duration;
use support::{request, request_with_body, request_with_headers};
use tempfile::TempDir;

const FIRST_ACTIVITY_START_MS: i64 = 1_893_456_000_000;
const INITIAL_CALENDAR_TIME_MS: i64 = FIRST_ACTIVITY_START_MS + 60_000;

fn response_json(response: &str) -> Value {
    let (_, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response has a body");
    serde_json::from_str(body).expect("HTTP response contains JSON")
}

fn write_activity_manifest(root: &TempDir) {
    let asset_root = root.path().join("cdn/cn");
    fs::create_dir_all(&asset_root).expect("CN asset directory is created");
    fs::write(
        asset_root.join("activity-catalog.json"),
        serde_json::to_vec_pretty(&json!({
            "format_version": 1,
            "region": "cn",
            "activities": [
                {
                    "activity_id": "story:unscheduled",
                    "name": "无时间窗口活动",
                    "kind": "story"
                },
                {
                    "activity_id": "story:zero-start",
                    "name": "零起点活动",
                    "kind": "story",
                    "default_start_at_ms": 0,
                    "default_end_at_ms": 86_400_000
                },
                {
                    "activity_id": "story:permanent",
                    "name": "永久活动",
                    "kind": "story",
                    "default_start_at_ms": 1_000,
                    "default_end_at_ms": 253_402_300_799_000_i64
                },
                {
                    "activity_id": "gacha:legacy-placeholder",
                    "name": "旧抽卡占位时间",
                    "kind": "gacha",
                    "default_start_at_ms": 2_000,
                    "default_end_at_ms": 3_000
                },
                {
                    "activity_id": "daily:legacy-placeholder",
                    "name": "旧日常占位时间",
                    "kind": "daily",
                    "default_start_at_ms": 3_000,
                    "default_end_at_ms": 4_000
                },
                {
                    "activity_id": "raid:later",
                    "name": "较晚活动",
                    "kind": "raid",
                    "default_start_at_ms": FIRST_ACTIVITY_START_MS + 86_400_000,
                    "default_end_at_ms": FIRST_ACTIVITY_START_MS + 172_800_000
                },
                {
                    "activity_id": "raid:first",
                    "name": "首个活动",
                    "kind": "raid",
                    "default_start_at_ms": FIRST_ACTIVITY_START_MS,
                    "default_end_at_ms": FIRST_ACTIVITY_START_MS + 86_400_000
                }
            ]
        }))
        .expect("activity manifest is encoded"),
    )
    .expect("activity manifest is written");
}

// //// 验证首次启动进入首个活动窗口并继续推进 [@x380kkm 2026-08-23] ////
#[test]
fn initializes_pristine_time_inside_first_scheduled_activity() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_activity_manifest(&root);

    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let first = response_json(&request(service.port(), "GET", "/v1/time"));
    let first_time = first["unix_time_ms"]
        .as_i64()
        .expect("virtual Unix time is returned");
    assert_eq!(first["enabled"], true);
    assert_eq!(first["rate"], 1.0);
    assert!((INITIAL_CALENDAR_TIME_MS..INITIAL_CALENDAR_TIME_MS + 2_000).contains(&first_time));

    thread::sleep(Duration::from_millis(25));
    let advanced = response_json(&request(service.port(), "GET", "/v1/time"));
    assert!(
        advanced["unix_time_ms"]
            .as_i64()
            .expect("advanced virtual Unix time is returned")
            > first_time
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证首次启动进入首个活动窗口并继续推进 ////

// //// 验证用户时间不会被活动预置覆盖 [@x380kkm 2026-08-20] ////
#[test]
fn preserves_user_time_when_activity_manifest_is_present() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_activity_manifest(&root);
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let body = br#"{"enabled":true,"iso":"2040-01-02T03:04:05.000Z","rate":2.0}"#;
    let updated = request_with_body(service.port(), "PUT", "/v1/time", "application/json", body);
    assert!(updated.starts_with("HTTP/1.1 200 OK"));
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let restored = response_json(&request(restarted.port(), "GET", "/v1/time"));
    assert_eq!(restored["enabled"], true);
    assert_eq!(restored["rate"], 2.0);
    assert!(restored["iso"]
        .as_str()
        .expect("virtual ISO time is returned")
        .starts_with("2040-01-02T03:04:05."));
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证用户时间不会被活动预置覆盖 ////

// //// 验证旧实例关闭虚拟时间后不会重新预置 [@x380kkm 2026-08-20] ////
#[test]
fn preserves_non_pristine_disabled_time_when_manifest_appears() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts without manifest");
    let body = br#"{"enabled":false,"iso":"2035-06-07T08:09:10.000Z","rate":1.0}"#;
    let updated = request_with_body(service.port(), "PUT", "/v1/time", "application/json", body);
    assert!(updated.starts_with("HTTP/1.1 200 OK"));
    assert!(updated.contains("\"enabled\":false"));
    service.stop().expect("service stops cleanly");

    write_activity_manifest(&root);
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts with manifest");
    let restored = response_json(&request(restarted.port(), "GET", "/v1/time"));
    assert_eq!(restored["enabled"], false);
    assert_eq!(restored["rate"], 1.0);
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证旧实例关闭虚拟时间后不会重新预置 ////

// //// 验证虚拟时间可直接读取并跨重启保留 [@x380kkm 2026-08-20] ////
#[test]
fn persists_virtual_time_across_restart_without_configuration() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let direct = request(service.port(), "GET", "/v1/time");
    assert!(direct.starts_with("HTTP/1.1 200 OK"));

    let body = br#"{"enabled":true,"iso":"2030-01-01T00:00:00.000Z","rate":10.0}"#;
    let updated = request_with_body(service.port(), "PUT", "/v1/time", "application/json", body);
    assert!(updated.starts_with("HTTP/1.1 200 OK"));
    assert!(updated.contains("\"enabled\":true"));
    assert!(updated.contains("\"iso\":\"2030-01-01T00:00:00."));
    assert!(updated.contains("\"rate\":10.0"));

    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 3 }),
    ));
    assert!(signup.data_headers.servertime >= 1_893_456_000);

    let current = request(service.port(), "GET", "/v1/time");
    let current_json = response_json(&current);
    assert_eq!(current_json["enabled"], true);
    assert_eq!(current_json["rate"], 10.0);
    assert!(current_json["unix_time_ms"]
        .as_i64()
        .is_some_and(|value| (1_893_456_000_000..1_893_456_060_000).contains(&value)));
    assert!(current_json["iso"]
        .as_str()
        .is_some_and(|iso| iso.starts_with("2030-01-01T00:00:")));
    assert!(!current.contains("management_authorization_required"));
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let restored = request(restarted.port(), "GET", "/v1/time");
    assert!(restored.contains("\"enabled\":true"));
    assert!(restored.contains("\"rate\":10.0"));
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证虚拟时间可直接读取并跨重启保留 ////

// //// 拒绝不完整或不合法的虚拟时间设置 [@x380kkm 2026-07-24] ////
#[test]
fn rejects_invalid_virtual_time_without_changing_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let invalid = request_with_body(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        br#"{"enabled":true,"iso":"2030-02-30T00:00:00Z","rate":1.0}"#,
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));

    let invalid = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        br#"{"enabled":true,"iso":"2030-02-30T00:00:00Z","rate":1.0}"#,
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));

    let invalid = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        br#"{"enabled":true,"unix_time_ms":1893456000000,"iso":"2030-01-01T00:00:00Z","rate":1.0}"#,
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
    let current = request_with_headers(
        service.port(),
        "GET",
        "/v1/time",
        "application/octet-stream",
        &authorization,
        &[],
    );
    assert!(current.contains("\"enabled\":false"));
    service.stop().expect("service stops cleanly");
}
// //// /拒绝不完整或不合法的虚拟时间设置 ////

// //// 验证账号在虚拟日期回退后仍可恢复和载入 [@x380kkm 2026-08-19] ////
#[test]
fn loads_existing_account_after_virtual_date_moves_backward() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let set_time = |iso: &str| {
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
    };

    set_time("2030-01-03T12:00:00.000Z");
    let first_signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 49 }),
    ));
    let registration_time = first_signup.data.create_date.clone();

    set_time("2030-01-01T12:00:00.000Z");
    let restored_signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 49 }),
    ));
    assert_eq!(restored_signup.data.create_date, registration_time);
    let viewer_id = restored_signup.data_headers.viewer_id;
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert!(loaded.data_headers.servertime >= 1_893_499_200);
    assert!(loaded.data_headers.servertime < 1_893_585_600);
    service.stop().expect("service stops cleanly");
}
// //// /验证账号在虚拟日期回退后仍可恢复和载入 ////
