// audience: internal
// # personal-service-cn-character-election-tests
// 投票使用真实候选与活动日期, 管理重置保留旧轮选择, 存储失败保留可重试状态.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, send_request, Envelope};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn api(service: &PersonalService, route: &str, body: Value) -> Envelope<Value> {
    decode_response(&send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    ))
}

fn admin(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let bearer = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &bearer)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn set_time(service: &PersonalService, iso: &str) {
    admin(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"iso":iso,"rate":1.0}),
    );
}

fn start(root: &Path, cdn: &Path) -> PersonalService {
    std::fs::write(
        cdn.join("activity-catalog.json"),
        json!({"format_version":1,"activities":[{
            "activity_id":"character-election:1", "kind":"character-election", "name":"角色投票"
        }]})
        .to_string(),
    )
    .unwrap();
    PersonalService::start_with_cdn_root(root, 0, cdn).unwrap()
}

fn signup(service: &PersonalService, device: i64) -> i64 {
    api(service, "tool/signup", json!({"device_id":device}))
        .data_headers
        .viewer_id
}

fn snapshot(root: &Path) -> Value {
    let connection = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = connection
        .query_row(
            "SELECT data_json FROM player_snapshots ORDER BY account_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn status(service: &PersonalService, viewer: i64) -> Envelope<Value> {
    api(
        service,
        "character_election/get_vote_status",
        json!({"viewer_id":viewer,"election_id":1}),
    )
}

fn vote(service: &PersonalService, viewer: i64, keyword: i64, count: i64) -> Envelope<Value> {
    api(
        service,
        "character_election/vote",
        json!({
            "viewer_id":viewer,"election_id":1,"keyword_id":keyword,"api_count":count
        }),
    )
}

fn history(service: &PersonalService, slot: i64) -> Value {
    admin(
        service,
        "GET",
        &format!("/v1/local-saves/{slot}/activities/character-election:1/history"),
        json!({}),
    )
}

// //// 投票保存首次选择并在重启和重置后保留各轮历史 [@x380kkm 2026-09-23] ////
#[test]
fn persists_first_vote_across_restart_and_management_reset() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    set_time(&service, "2022-05-04T12:00:00.000Z");
    let viewer = signup(&service, 981);
    api(&service, "load", json!({"viewer_id":viewer}));
    let before = snapshot(root.path());
    assert_eq!(status(&service, viewer).data["is_voted"], false);
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(
        vote(&service, viewer, 1000001, 1).data_headers.result_code,
        1
    );
    let first = snapshot(root.path());
    assert_eq!(first["user_info"]["free_vmoney"].as_i64().unwrap(), before["user_info"]["free_vmoney"].as_i64().unwrap() + 300);
    assert_eq!(first["item_list"], before["item_list"]);
    assert_eq!(
        first["character_election_votes"]["1"]["keyword_id"],
        1000001
    );
    vote(&service, viewer, 1000001, 1);
    vote(&service, viewer, 1000010, 2);
    assert_eq!(snapshot(root.path()), first);
    let slot = admin(&service, "GET", "/v1/local-saves", json!({}))["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let first_history = history(&service, slot);
    assert_eq!(first_history["can_reset"], true);
    assert_eq!(first_history["entries"].as_array().unwrap().iter().filter(|entry| entry["kind"] == "vote").count(), 1);
    assert!(first_history["entries"].as_array().unwrap().iter().any(|entry| entry["kind"] == "claim"));
    let other = signup(&service, 982);
    assert_eq!(status(&service, other).data["is_voted"], false);
    let loaded = api(&service, "load", json!({"viewer_id":viewer}));
    assert!(loaded.data.get("character_election_votes").is_none());
    service.stop().unwrap();

    let service = start(root.path(), cdn.path());
    assert_eq!(status(&service, viewer).data["is_voted"], true);
    admin(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot}/activities/character-election:1/reset"),
        json!({"expected_round":0}),
    );
    assert_eq!(status(&service, viewer).data["is_voted"], false);
    vote(&service, viewer, 1000010, 1);
    assert_eq!(snapshot(root.path())["user_info"]["free_vmoney"].as_i64().unwrap(), first["user_info"]["free_vmoney"].as_i64().unwrap() + 300);
    let records = history(&service, slot);
    let votes = records["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "vote")
        .collect::<Vec<_>>();
    assert_eq!(votes.len(), 2);
    assert!(votes.iter().any(
        |entry| entry["participation_round"] == 0 && entry["details"]["keyword_id"] == 1000001
    ));
    assert!(votes.iter().any(
        |entry| entry["participation_round"] == 1 && entry["details"]["keyword_id"] == 1000010
    ));
    service.stop().unwrap();
}

// //// 候选保密日期和活动开关保持一致并允许既有投票重放 [@x380kkm 2026-09-23] ////
#[test]
fn follows_candidates_and_activity_windows_without_spending_assets() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    set_time(&service, "2022-01-01T12:00:00.000Z");
    let viewer = signup(&service, 983);
    let initial = snapshot(root.path());
    assert_eq!(status(&service, viewer).data_headers.result_code, 11003);
    assert_eq!(
        vote(&service, viewer, 1000001, 1).data_headers.result_code,
        11003
    );
    set_time(&service, "2022-05-04T12:00:00.000Z");
    for keyword in [99999999, 1253013, 1141111] {
        let response = send_request(
            service.port(),
            "/api/index.php/character_election/vote",
            &encode_request(&json!({"viewer_id":viewer,"election_id":1,"keyword_id":keyword})),
        );
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    }
    assert_eq!(snapshot(root.path()), initial);
    set_time(&service, "2025-01-01T12:00:00.000Z");
    assert_eq!(status(&service, viewer).data_headers.result_code, 11003);
    admin(
        &service,
        "PUT",
        "/v1/activities/calendar/character-election%3A1",
        json!({"enabled":true,"start_at_ms":0,"end_at_ms":253402300799000_i64}),
    );
    assert_eq!(status(&service, viewer).data["is_voted"], false);
    assert_eq!(
        vote(&service, viewer, 1141111, 1).data_headers.result_code,
        1
    );
    let stored = snapshot(root.path());
    assert_eq!(stored["user_info"]["free_vmoney"].as_i64().unwrap(), initial["user_info"]["free_vmoney"].as_i64().unwrap() + 300);
    assert_eq!(stored["item_list"], initial["item_list"]);
    admin(
        &service,
        "PUT",
        "/v1/activities/calendar/character-election%3A1",
        json!({"enabled":false,"start_at_ms":0,"end_at_ms":253402300799000_i64}),
    );
    assert_eq!(status(&service, viewer).data_headers.result_code, 11003);
    assert_eq!(
        vote(&service, viewer, 1141111, 1).data_headers.result_code,
        1
    );
    assert_eq!(snapshot(root.path()), stored);
    service.stop().unwrap();
}

// //// 历史写入失败时投票状态与记录共同回滚 [@x380kkm 2026-09-23] ////
#[test]
fn retries_vote_after_history_storage_failure() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    set_time(&service, "2022-05-04T12:00:00.000Z");
    let viewer = signup(&service, 984);
    let before = snapshot(root.path());
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TRIGGER reject_vote BEFORE INSERT ON player_activity_history
        WHEN NEW.kind='vote' BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;",
        )
        .unwrap();
    let body = json!({"viewer_id":viewer,"election_id":1,"keyword_id":1000001,"api_count":1});
    let response = send_request(
        service.port(),
        "/api/index.php/character_election/vote",
        &encode_request(&body),
    );
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(status(&service, viewer).data["is_voted"], false);
    database.execute_batch("DROP TRIGGER reject_vote;").unwrap();
    assert_eq!(
        vote(&service, viewer, 1000001, 1).data_headers.result_code,
        1
    );
    assert_eq!(status(&service, viewer).data["is_voted"], true);
    let count: i64 = database
        .query_row(
            "SELECT COUNT(*) FROM player_activity_history WHERE kind='vote'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    service.stop().unwrap();
}

// //// 读取既有投票时结算尚缺的任务奖励并保持重放幂等 [@x380kkm 2026-09-26] ////
#[test]
fn settles_mission_for_existing_vote_without_changing_the_selection() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    set_time(&service, "2022-05-04T12:00:00.000Z");
    let viewer = signup(&service, 985);
    let mut stored = snapshot(root.path());
    let before_currency = stored["user_info"]["free_vmoney"].as_i64().unwrap();
    stored["character_election_votes"] = json!({"1":{"keyword_id":1000001,"created_at":1651665600,"participation_round":0}});
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database.execute("UPDATE player_snapshots SET data_json=?1", [stored.to_string()]).unwrap();
    let recovered = status(&service, viewer);
    assert_eq!(recovered.data["is_voted"], true);
    assert_eq!(recovered.data["mission_info"][0]["mission_id"], 2389);
    let received = snapshot(root.path());
    assert_eq!(received["user_info"]["free_vmoney"], before_currency + 300);
    assert_eq!(received["character_election_votes"], stored["character_election_votes"]);
    assert!(status(&service, viewer).data["mission_info"].as_array().unwrap().is_empty());
    assert_eq!(snapshot(root.path()), received);
    service.stop().unwrap();
}
