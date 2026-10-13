// audience: internal
// # personal-service-cn-activity-tests
//
// 该文件验证活动管理 API, 虚拟日期窗口和 CN 活动查询共享同一份持久化状态.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::fs;
use std::path::Path;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct GetRaidBossRequest {
    viewer_id: i64,
    event_id: i64,
}

#[derive(Serialize)]
struct RankingEventRequest {
    viewer_id: i64,
    ranking_event_id: i64,
    quest_kind: Option<i64>,
    api_count: Option<i64>,
}

fn response_json(response: &str) -> Value {
    let body = response
        .split_once("\r\n\r\n")
        .expect("JSON response has a body")
        .1;
    serde_json::from_str(body).expect("JSON response is valid")
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

// //// 读取领奖相关的玩家快照和邮件数量 [@x380kkm 2026-09-23] ////
fn reward_state(root: &Path) -> (String, i64) {
    let database = rusqlite::Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let snapshot = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| row.get(0))
        .expect("player snapshot is read");
    let mail_count = database
        .query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0))
        .expect("reward mails are counted");
    (snapshot, mail_count)
}
// //// /读取领奖相关的玩家快照和邮件数量 ////

// //// 把临时开放租约改为若干天前结束 [@x380kkm 2026-10-04] ////
fn end_temporary_open(root: &Path, activity_id: &str, days_ago: i64) {
    let day = 86_400_000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is after the epoch")
        .as_millis() as i64;
    rusqlite::Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened")
        .execute(
            "UPDATE activity_temporary_open_leases SET opened_at_ms=?1, expires_at_ms=?2 WHERE activity_id=?3",
            rusqlite::params![now - (days_ago + 1) * day, now - days_ago * day, activity_id],
        )
        .expect("temporary open lease is moved");
}
// //// /把临时开放租约改为若干天前结束 ////

fn write_default_activity_manifest(cdn_root: &Path) {
    fs::write(
        cdn_root.join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.1",
            "asset_version": "1.4.54",
            "generated_at": "2030-01-01T00:00:00Z",
            "activities": [
                {
                    "activity_id": "raid:9003",
                    "name": "默认窗口 Raid",
                    "kind": "raid",
                    "tags": ["test"],
                    "description": "manifest default window",
                    "default_start_at_ms": 1893542400000,
                    "default_end_at_ms": 1893715200000
                }
            ]
        }"#,
    )
    .expect("activity manifest is written");
}

// //// 验证活动管理状态贯通 CN raid 查询 [@x380kkm 2026-07-24] ////
#[test]
fn manages_and_reads_raid_boss_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 46 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let event_id = 9_001;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];

    let direct = request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/activities/raid-boss/{event_id}"),
        "application/json",
        &[],
        b"",
    );
    assert!(direct.starts_with("HTTP/1.1 200 OK"));

    let default_state = request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/activities/raid-boss/{event_id}"),
        "application/json",
        &authorization,
        b"",
    );
    assert_eq!(default_state.starts_with("HTTP/1.1 200 OK"), true);
    let default_body = response_json(&default_state);
    assert_eq!(default_body["hp_percentage"], 100);
    assert_eq!(default_body["total_kill_count"], 0);

    let updated = request_with_headers(
        service.port(),
        "PUT",
        &format!("/v1/activities/raid-boss/{event_id}"),
        "application/json",
        &authorization,
        br#"{"hp_percentage":42,"total_kill_count":17}"#,
    );
    assert!(updated.starts_with("HTTP/1.1 200 OK"));
    let updated_body = response_json(&updated);
    assert_eq!(updated_body["hp_percentage"], 42);
    assert_eq!(updated_body["total_kill_count"], 17);

    let raid = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/event/raid/get_boss",
        &encode_request(&GetRaidBossRequest {
            viewer_id,
            event_id,
        }),
    ));
    assert_eq!(raid.data["raid_boss"]["hp_percentage"], 42);
    assert_eq!(raid.data["raid_boss"]["total_kill_count"], 17);

    let opened = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/ranking%3A1/mode",
        "application/json",
        &authorization,
        br#"{"mode":"always"}"#,
    );
    assert_eq!(response_json(&opened)["status"], "open", "{opened}");
    let ranking = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/ranking_event/get_summary",
        &encode_request(&RankingEventRequest {
            viewer_id,
            ranking_event_id: 1,
            quest_kind: Some(1),
            api_count: Some(1),
        }),
    ));
    assert!(ranking.data["best_record"].is_null(), "{:?}", ranking.data);
    assert!(ranking.data.get("leader_character_id").is_none());

    let before_reward = reward_state(root.path());
    let reward = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/ranking_event/receive_reward",
        &encode_request(&RankingEventRequest {
            viewer_id,
            ranking_event_id: 1,
            quest_kind: None,
            api_count: Some(2),
        }),
    ));
    assert_eq!(reward.data["status"], 3);
    assert_eq!(reward_state(root.path()), before_reward);

    let unknown_ranking = cn_support::send_request(
        service.port(),
        "/api/index.php/ranking_event/get_summary",
        &encode_request(&RankingEventRequest {
            viewer_id,
            ranking_event_id: 99,
            quest_kind: None,
            api_count: Some(3),
        }),
    );
    assert!(unknown_ranking.starts_with("HTTP/1.1 400 Bad Request"));

    let invalid = request_with_headers(
        service.port(),
        "PUT",
        &format!("/v1/activities/raid-boss/{event_id}"),
        "application/json",
        &authorization,
        br#"{"hp_percentage":101,"total_kill_count":17}"#,
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");
}
// //// /验证活动管理状态贯通 CN raid 查询 ////

// //// 验证活动窗口随虚拟日期前进和回退重新计算 [@x380kkm 2026-08-19] ////
#[test]
fn recalculates_activity_windows_when_virtual_time_moves_backward() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 47 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let event_id = 9_002;
    let schedule_path = format!("/v1/activities/calendar/raid:{event_id}");
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let schedule = request_with_headers(
        service.port(),
        "PUT",
        &schedule_path,
        "application/json",
        &authorization,
        br#"{
            "enabled": true,
            "start_at": "2030-01-02T00:00:00.000Z",
            "end_at": "2030-01-04T00:00:00.000Z"
        }"#,
    );
    assert!(schedule.starts_with("HTTP/1.1 200 OK"));

    let raid_request = || {
        cn_support::send_request(
            service.port(),
            "/api/index.php/event/raid/get_boss",
            &encode_request(&GetRaidBossRequest {
                viewer_id,
                event_id,
            }),
        )
    };

    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    assert!(raid_request().ends_with("{\"error\":\"activity_not_started\"}"));

    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    let open_schedule = request_with_headers(
        service.port(),
        "GET",
        &schedule_path,
        "application/json",
        &authorization,
        b"",
    );
    assert_eq!(response_json(&open_schedule)["status"], "open");
    let open_raid = decode_response::<Value>(&raid_request());
    assert_eq!(open_raid.data["raid_boss"]["hp_percentage"], 100);

    set_virtual_time(&service, "2030-01-05T12:00:00.000Z");
    assert!(raid_request().ends_with("{\"error\":\"activity_ended\"}"));

    set_virtual_time(&service, "2030-01-03T18:00:00.000Z");
    let reopened_raid = decode_response::<Value>(&raid_request());
    assert_eq!(reopened_raid.data["raid_boss"]["hp_percentage"], 100);

    let disabled = request_with_headers(
        service.port(),
        "PUT",
        &schedule_path,
        "application/json",
        &authorization,
        br#"{
            "enabled": false,
            "start_at_ms": 1893542400000,
            "end_at_ms": 1893715200000
        }"#,
    );
    assert_eq!(response_json(&disabled)["status"], "disabled");
    assert!(raid_request().ends_with("{\"error\":\"activity_disabled\"}"));

    let deleted = request_with_headers(
        service.port(),
        "DELETE",
        &schedule_path,
        "application/json",
        &authorization,
        b"",
    );
    assert_eq!(response_json(&deleted)["deleted"], true);
    let unscheduled_raid = decode_response::<Value>(&raid_request());
    assert_eq!(unscheduled_raid.data["raid_boss"]["hp_percentage"], 100);
    service.stop().expect("service stops cleanly");
}
// //// /验证活动窗口随虚拟日期前进和回退重新计算 ////

// //// 验证静态 CN raid 在未配置日历时保持可调用 [@x380kkm 2026-08-19] ////
#[test]
fn allows_unscheduled_cn_raid_across_manifest_default_window() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    write_default_activity_manifest(cdn_root.path());
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 49 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let raid_request = || {
        cn_support::send_request(
            service.port(),
            "/api/index.php/event/raid/get_boss",
            &encode_request(&GetRaidBossRequest {
                viewer_id,
                event_id: 9003,
            }),
        )
    };

    set_virtual_time(&service, "2029-12-31T12:00:00.000Z");
    let before = decode_response::<Value>(&raid_request());
    assert_eq!(before.data["raid_boss"]["hp_percentage"], 100);
    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    let open = decode_response::<Value>(&raid_request());
    assert_eq!(open.data["raid_boss"]["hp_percentage"], 100);
    set_virtual_time(&service, "2030-01-05T12:00:00.000Z");
    let after = decode_response::<Value>(&raid_request());
    assert_eq!(after.data["raid_boss"]["hp_percentage"], 100);
    service.stop().expect("service stops cleanly");
}
// //// /验证静态 CN raid 在未配置日历时保持可调用 ////

// //// 验证排行活动奖励受同一时间窗口约束 [@x380kkm 2026-08-19] ////
#[test]
fn applies_activity_calendar_to_ranking_rewards() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 48 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let disabled = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/ranking%3A1/close",
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(disabled.starts_with("HTTP/1.1 200 OK"));
    let reward_request = encode_request(&RankingEventRequest {
        viewer_id,
        ranking_event_id: 1,
        quest_kind: None,
        api_count: Some(1),
    });
    let blocked = cn_support::send_request(
        service.port(),
        "/api/index.php/ranking_event/receive_reward",
        &reward_request,
    );
    assert!(blocked.ends_with("{\"error\":\"activity_disabled\"}"));

    let opened = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/ranking%3A1/open",
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert_eq!(response_json(&opened)["status"], "open");
    let before_reward = reward_state(root.path());
    let reward_request_at = |api_count| {
        encode_request(&RankingEventRequest {
            viewer_id,
            ranking_event_id: 1,
            quest_kind: None,
            api_count: Some(api_count),
        })
    };
    let during_play = cn_support::send_request(
        service.port(),
        "/api/index.php/ranking_event/receive_reward",
        &reward_request_at(2),
    );
    assert!(during_play.ends_with("{\"error\":\"ranking_reward_not_ready\"}"));
    assert_eq!(reward_state(root.path()), before_reward);
    // 租约结束 4 天后已过 3 天统计期, 仍在 10 天领奖期内.
    end_temporary_open(root.path(), "ranking:1", 4);
    let reward = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/ranking_event/receive_reward",
        &reward_request_at(3),
    ));
    assert_eq!(reward.data["status"], 3);
    assert_eq!(reward_state(root.path()), before_reward);
    service.stop().expect("service stops cleanly");
}
// //// /验证排行活动奖励受同一时间窗口约束 ////

// //// 验证复刻排行编号和虚拟时间窗口 [@x380kkm 2026-08-22] ////
#[test]
fn supports_revival_ranking_ids_with_virtual_time() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 49 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let schedule = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/calendar/ranking:1000",
        "application/json",
        &authorization,
        br#"{"enabled":true,"start_at":"2030-01-02T00:00:00.000Z","end_at":"2030-01-04T00:00:00.000Z"}"#,
    );
    assert!(schedule.starts_with("HTTP/1.1 200 OK"));
    let request = |ranking_event_id| {
        cn_support::send_request(
            service.port(),
            "/api/index.php/ranking_event/get_summary",
            &encode_request(&RankingEventRequest {
                viewer_id,
                ranking_event_id,
                quest_kind: Some(1),
                api_count: Some(1),
            }),
        )
    };
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    assert!(request(1_000).ends_with("{\"error\":\"activity_not_started\"}"));
    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    let revival = decode_response::<Value>(&request(1_000));
    assert!(revival.data["best_record"].is_null());
    let second_revival = decode_response::<Value>(&request(1_001));
    assert!(second_revival.data["best_record"].is_null());
    set_virtual_time(&service, "2030-01-05T12:00:00.000Z");
    let ended = decode_response::<Value>(&request(1_000));
    assert_eq!(ended.data, revival.data);
    service.stop().expect("service stops cleanly");
}
// //// /验证复刻排行编号和虚拟时间窗口 ////
