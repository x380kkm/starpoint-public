// audience: internal
// # personal-service-competition-management-tests
// 真实管理和客户端接口共同验证手动结算, 领取重放, 期别冲突和统计历史.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> String {
    let token = format!("Bearer {}", service.management_token());
    support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    )
}
fn value(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let response = manage(service, method, path, body);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}
fn send(service: &PersonalService, path: &str, body: Value) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        path,
        &encode_request(&body),
    ))
    .data
}
fn signup(service: &PersonalService) -> (i64, i64) {
    let viewer = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 631 }),
    ))
    .data_headers
    .viewer_id;
    let slots = value(service, "GET", "/v1/local-saves", json!({}));
    (viewer, slots["slots"][0]["id"].as_i64().unwrap())
}
fn entry(service: &PersonalService, slot: i64, activity: &str) -> Value {
    value(
        service,
        "GET",
        &format!("/v1/local-saves/{slot}/competitions"),
        json!({}),
    )["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["activity_id"] == activity)
        .unwrap()
        .clone()
}
fn settle_body(entry: &Value) -> Value {
    json!({"expected_round":entry["participation_round"],"expected_period_start_ms":entry["period_start_ms"]})
}
fn battle(service: &PersonalService, viewer: i64, category: i64, quest: i64, play: &str) {
    send(
        service,
        "/api/index.php/single_battle_quest/start",
        json!({"viewer_id":viewer,"category":category,
        "quest_id":quest,"party_id":1,"play_id":play,"use_boost_point":false,"use_boss_boost_point":false,"is_auto_start_mode":false}),
    );
    send(
        service,
        "/api/index.php/single_battle_quest/finish",
        json!({"viewer_id":viewer,"category":category,"quest_id":quest,"play_id":play,
        "elapsed_time_ms":40000,"score":5000,"add_mana":0,"is_accomplished":true,
        "statistics":{"zones":[],"party":{"characters":[{"id":1}],"unison_characters":[],"equipments":[],"ability_soul_ids":[]}}}),
    );
}

// //// 管理结算使用真实成绩并在重置后保留旧领奖历史 [@x380kkm 2026-09-28] ////
#[test]
fn ranking_manual_awards_share_receipts_and_preserve_selected_tier_history() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    std::fs::write(
        cdn.path().join("activity-catalog.json"),
        json!({"format_version":1,"activities":[
        {"activity_id":"ranking:1","name":"竞速","kind":"ranking"}]})
        .to_string(),
    )
    .unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let (viewer, slot) = signup(&service);
    Connection::open(root.path().join("personal-service.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE player_snapshots SET data_json=json_set(data_json,'$.user_info.stamina',100)",
            [],
        )
        .unwrap();
    let path = format!("/v1/local-saves/{slot}/competitions/ranking:1");
    let before = entry(&service, slot, "ranking:1");
    assert!(!before["has_record"].as_bool().unwrap());
    assert_eq!(
        value(
            &service,
            "POST",
            &format!("{path}/settle"),
            settle_body(&before)
        )["status"],
        "no_reward"
    );
    value(
        &service,
        "PUT",
        "/v1/activities/ranking:1/mode",
        json!({"mode":"always"}),
    );
    value(
        &service,
        "PUT",
        &format!("{path}/reward-mode"),
        json!({"mode":"manual"}),
    );
    battle(&service, viewer, 11, 1001, "management-ranking");
    let client = json!({"viewer_id":viewer,"ranking_event_id":1,"quest_kind":1});
    assert_eq!(
        send(
            &service,
            "/api/index.php/ranking_event/receive_reward",
            client.clone()
        )["status"],
        3
    );
    let current = entry(&service, slot, "ranking:1");
    let selected = current["tiers"].as_array().unwrap().last().unwrap()["id"].clone();
    let mut request = settle_body(&current);
    request["tier_id"] = selected.clone();
    Connection::open(root.path().join("personal-service.sqlite3"))
        .unwrap()
        .execute("DELETE FROM sessions WHERE type=2", [])
        .unwrap();
    let delivered = value(&service, "POST", &format!("{path}/settle"), request.clone());
    assert_eq!(delivered["status"], "delivered");
    assert_eq!(delivered["settlement"]["tier_id"], selected);
    assert!(delivered["settlement"]["degree_list"][0]
        .get("viewer_id")
        .is_none());
    assert_eq!(
        delivered["settlement"]["best_record"]["elapsed_time_ms"],
        40000
    );
    assert_eq!(
        value(&service, "POST", &format!("{path}/settle"), request.clone())["status"],
        "already_received"
    );
    let (viewer, restored_slot) = signup(&service);
    assert_eq!(restored_slot, slot);
    let client = json!({"viewer_id":viewer,"ranking_event_id":1,"quest_kind":1});
    let replay = send(
        &service,
        "/api/index.php/ranking_event/receive_reward",
        client,
    );
    assert_eq!(replay["status"], 2);
    assert_eq!(replay["degree_list"][0]["viewer_id"], viewer);
    let history_path = format!("/v1/local-saves/{slot}/activities/ranking:1/history");
    let history = value(&service, "GET", &history_path, json!({}));
    let claim = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "claim")
        .unwrap();
    assert_eq!(claim["details"]["receipt"]["source"], "management");
    assert_eq!(claim["details"]["receipt"]["tier_id"], selected);
    value(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot}/activities/ranking:1/reset"),
        json!({"expected_round":0}),
    );
    assert!(manage(&service, "POST", &format!("{path}/settle"), request)
        .contains("activity_round_changed"));
    assert_eq!(entry(&service, slot, "ranking:1")["reward_mode"], "manual");
    battle(&service, viewer, 11, 1001, "management-ranking-again");
    let current = entry(&service, slot, "ranking:1");
    assert_eq!(
        value(
            &service,
            "POST",
            &format!("{path}/settle"),
            settle_body(&current)
        )["status"],
        "delivered"
    );
    let count: i64 = Connection::open(root.path().join("personal-service.sqlite3"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
    service.stop().unwrap();
}

// //// 手动模式将全服统计配置和奖励发放分别提交 [@x380kkm 2026-09-28] ////
#[test]
fn raid_manual_mode_keeps_unclaimed_totals_until_management_settlement() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = signup(&service);
    let path = format!("/v1/local-saves/{slot}/competitions/raid:1");
    value(
        &service,
        "PUT",
        "/v1/activities/raid:1/mode",
        json!({"mode":"always"}),
    );
    value(
        &service,
        "PUT",
        &format!("{path}/reward-mode"),
        json!({"mode":"manual"}),
    );
    let current = entry(&service, slot, "raid:1");
    let update = json!({"expected_period_start_ms":current["period_start_ms"],"hp_percentage":75,"total_kill_count":1000});
    value(&service, "PUT", &format!("{path}/server-state"), update);
    let response = send(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"viewer_id":viewer,"event_id":1}),
    );
    assert_eq!(response["raid_boss"]["total_kill_count"], 1000);
    assert_eq!(response["kill_count_reward_data"]["received_up_to"], 0);
    assert_eq!(response["kill_count_reward_data"]["reward_list"], json!([]));
    let current = entry(&service, slot, "raid:1");
    assert!(current["tiers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tier| tier["pending_count"].as_i64().unwrap() > 0));
    let delivered = value(
        &service,
        "POST",
        &format!("{path}/settle"),
        settle_body(&current),
    );
    assert_eq!(delivered["status"], "delivered");
    assert_eq!(delivered["settlement"]["received_up_to"], 1000);
    assert_eq!(
        value(
            &service,
            "POST",
            &format!("{path}/settle"),
            settle_body(&current)
        )["status"],
        "already_received"
    );
    let invalid =
        json!({"expected_period_start_ms":123,"hp_percentage":50,"total_kill_count":2000});
    assert!(
        manage(&service, "PUT", &format!("{path}/server-state"), invalid)
            .contains("competition_period_changed")
    );
    let boss = entry(&service, slot, "raid:1");
    assert_eq!(boss["global_state"]["total_kill_count"], 1000);
    service.stop().unwrap();
}

// //// 狂热管理奖励保留原成绩且只使用目录中的排名区间 [@x380kkm 2026-09-28] ////
#[test]
fn rush_manual_rank_award_keeps_real_record_and_replays_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let (viewer, slot) = signup(&service);
    let path = format!("/v1/local-saves/{slot}/competitions/rush:700006");
    value(
        &service,
        "PUT",
        "/v1/activities/rush:700006/mode",
        json!({"mode":"always"}),
    );
    value(
        &service,
        "PUT",
        &format!("{path}/reward-mode"),
        json!({"mode":"manual"}),
    );
    battle(&service, viewer, 24, 700006008, "management-rush");
    let client = json!({"viewer_id":viewer,"event_id":700006});
    assert_eq!(
        send(&service, "/api/index.php/event/rush/reward", client.clone())["ranking_reward"]
            ["status"],
        3
    );
    let current = entry(&service, slot, "rush:700006");
    let mut request = settle_body(&current);
    request["rank_number"] = json!(4);
    let result = value(&service, "POST", &format!("{path}/settle"), request.clone());
    assert_eq!(result["settlement"]["rank_number"], 4);
    assert_eq!(result["status"], "delivered");
    assert_eq!(
        entry(&service, slot, "rush:700006")["summary"]["best_round"],
        1
    );
    assert_eq!(
        value(&service, "POST", &format!("{path}/settle"), request)["status"],
        "already_received"
    );
    assert_eq!(
        send(&service, "/api/index.php/event/rush/reward", client)["ranking_reward"]["status"],
        2
    );
    service.stop().unwrap();
}
