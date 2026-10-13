// audience: internal
// # personal-service-regional-event-state-tests
// 真实接口覆盖区域期别往返, 入场后调时, 奖励邮件去重和管理重置.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_regional_event_state/rush.rs"]
mod rush;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn start_service(root: &Path, cdn: &Path) -> PersonalService {
    std::fs::write(cdn.join("activity-catalog.json"), json!({"format_version":1,"activities":[
        {"activity_id":"ranking:2","name":"竞速","kind":"ranking"},
        {"activity_id":"raid:1","name":"团队战","kind":"raid"},
        {"activity_id":"rush:700006","name":"狂热激战","kind":"rush"},
    ]}).to_string()).unwrap();
    PersonalService::start_with_cdn_root(root, 0, cdn).unwrap()
}

fn send(service: &PersonalService, path: &str, body: Value) -> Value {
    let response = cn_support::send_request(service.port(), path, &encode_request(&body));
    decode_response::<Value>(&response).data
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(service.port(), method, path, "application/json",
        &[("Authorization", token.as_str())], body.to_string().as_bytes());
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn set_time(service: &PersonalService, iso: &str) {
    manage(service, "PUT", "/v1/time", json!({"enabled":true,"iso":iso,"rate":1}));
}

fn signup(service: &PersonalService) -> i64 {
    let response = cn_support::send_request(service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 973 }));
    decode_response::<SignupData>(&response).data_headers.viewer_id
}

fn snapshot(root: &Path) -> Value {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = db.query_row("SELECT data_json FROM player_snapshots LIMIT 1", [], |row| row.get(0)).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn seed(root: &Path, change: impl FnOnce(&mut Value)) {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let mut player = snapshot(root);
    change(&mut player);
    db.execute("UPDATE player_snapshots SET data_json=?1", params![player.to_string()]).unwrap();
}

fn ranking(service: &PersonalService, viewer: i64, action: &str) -> Value {
    send(service, &format!("/api/index.php/ranking_event/{action}"),
        json!({"viewer_id":viewer,"ranking_event_id":2,"quest_kind":1}))
}

fn start_ranking(service: &PersonalService, viewer: i64, play: &str) {
    send(service, "/api/index.php/single_battle_quest/start", json!({
        "viewer_id":viewer,"category":11,"quest_id":2001,"play_id":play,"party_id":1,
        "use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false,
    }));
}

fn finish_ranking(service: &PersonalService, viewer: i64, play: &str, score: i64, accomplished: bool) -> Value {
    send(service, "/api/index.php/single_battle_quest/finish", json!({
        "viewer_id":viewer,"category":11,"quest_id":2001,"play_id":play,"is_accomplished":accomplished,
        "elapsed_time_ms":30000,"score":score,"add_mana":0,
        "statistics":{"clear_phase":1,"zones":[],"party":{
            "characters":[{"id":1},null,null],"unison_characters":[null,null,null],
            "equipments":[null,null,null],"ability_soul_ids":[null,null,null],
        }},
    }))
}

fn history_path(service: &PersonalService, activity: &str, action: &str) -> String {
    let saves = manage(service, "GET", "/v1/local-saves", json!({}));
    format!("/v1/local-saves/{}/activities/{activity}/{action}", saves["slots"][0]["id"])
}

// //// 分离区域竞速成绩和奖励并在重置后重新参赛 [@x380kkm 2026-09-27] ////
#[test]
fn ranking_periods_preserve_original_records_and_independent_deliveries() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start_service(root.path(), cdn.path());
    let viewer = signup(&service);
    seed(root.path(), |player| {
        player["user_info"]["stamina"] = json!(1000);
        player["quest_progress"]["11"] = json!([{"quest_id":2001,"finished":true,
            "best_elapsed_time_ms":60000,"best_elapsed_score":100,"high_score":100}]);
    });
    set_time(&service, "2022-09-02T12:00:00Z");
    assert_eq!(ranking(&service, viewer, "receive_reward")["status"], 1);
    set_time(&service, "2020-11-12T12:00:00Z");
    assert!(ranking(&service, viewer, "get_summary")["best_record"].is_null());
    assert_eq!(ranking(&service, viewer, "receive_reward")["status"], 3);
    set_time(&service, "2020-11-02T12:00:00Z");
    start_ranking(&service, viewer, "regional-inflight");
    set_time(&service, "2022-09-02T12:00:00Z");
    assert_eq!(finish_ranking(&service, viewer, "regional-inflight", 777, true)["ranking_event"]["best_record"]["score"], 777);
    assert_eq!(ranking(&service, viewer, "get_summary")["best_record"]["elapsed_time_ms"], 60000);
    assert_eq!(snapshot(root.path())["cn_activity_state"]["event_families"]["ranking"]["2"]["best_record"]["elapsed_time_ms"], 60000);
    set_time(&service, "2020-11-12T12:00:00Z");
    assert_eq!(ranking(&service, viewer, "get_summary")["best_record"]["score"], 777);
    assert_eq!(ranking(&service, viewer, "receive_reward")["status"], 1);
    assert_eq!(ranking(&service, viewer, "receive_reward")["status"], 2);
    let before = snapshot(root.path());
    manage(&service, "POST", &history_path(&service, "ranking:2", "reset"), json!({"expected_round":0}));
    let reset = snapshot(root.path());
    for field in ["user_info", "item_list", "user_character_list", "degree_ids"] {
        assert_eq!(reset[field], before[field], "{field}");
    }
    assert_eq!(ranking(&service, viewer, "receive_reward")["status"], 3);
    set_time(&service, "2020-11-02T12:00:00Z");
    start_ranking(&service, viewer, "regional-reset");
    finish_ranking(&service, viewer, "regional-reset", 888, true);
    set_time(&service, "2020-11-12T12:00:00Z");
    assert_eq!(ranking(&service, viewer, "receive_reward")["status"], 1);
    set_time(&service, "2022-09-02T12:00:00Z");
    assert_eq!(ranking(&service, viewer, "receive_reward")["status"], 2);
    assert_eq!(ranking(&service, viewer, "get_summary")["best_record"]["elapsed_time_ms"], 60000);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get::<_, i64>(0)).unwrap(), 3);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM player_activity_history WHERE activity_id='ranking:2' AND kind='claim'", [], |row| row.get::<_, i64>(0)).unwrap(), 3);
    set_time(&service, "2022-08-20T12:00:00Z");
    start_ranking(&service, viewer, "original-after-regional");
    finish_ranking(&service, viewer, "original-after-regional", 42, false);
    let history = manage(&service, "GET", &history_path(&service, "ranking:2", "history"), json!({}));
    let records = history["entries"].as_array().unwrap();
    let original = records.iter().find(|entry| entry["details"]["play_id"] == "original-after-regional").unwrap();
    assert_eq!(original["details"]["best_record"]["elapsed_time_ms"], 60000);
    assert_eq!(original["details"]["best_record"]["score"], 100);
    assert!(original["details"]["period_start_ms"].is_null());
    let regional = records.iter().find(|entry| entry["details"]["play_id"] == "regional-inflight").unwrap();
    assert_eq!(regional["details"]["best_record"]["elapsed_time_ms"], 30000);
    assert_eq!(regional["details"]["period_start_ms"], 1604116800000i64);
    service.stop().unwrap();
}

// //// 分离团队战全局领主与个人讨伐领奖状态 [@x380kkm 2026-09-27] ////
#[test]
fn raid_periods_keep_boss_settings_and_claims_separate() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start_service(root.path(), cdn.path());
    let viewer = signup(&service);
    let body = json!({"viewer_id":viewer,"event_id":1});
    let summary_path = "/api/index.php/event/raid/summary";
    set_time(&service, "2023-09-10T12:00:00Z");
    seed(root.path(), |player| {
        player["user_info"]["stamina"] = json!(1000);
        player["cn_activity_state"]["event_families"]["raid"]["1"] = json!({
            "legacy_state_imported":true,"quest_list":{"1001":{"kill_count":9}},
        });
        player["quest_progress"]["23"] = json!([{"quest_id":1001,"finished":true,
            "best_elapsed_time_ms":10000,"best_elapsed_score":999,"high_score":999}]);
    });
    manage(&service, "PUT", "/v1/activities/raid-boss/1", json!({"hp_percentage":45,"total_kill_count":2}));
    assert_eq!(send(&service, summary_path, body.clone())["kill_count_reward_data"]["received_up_to"], 2);
    set_time(&service, "2021-12-01T12:00:00Z");
    let regional = send(&service, summary_path, body.clone());
    assert_eq!(regional["raid_boss"]["total_kill_count"], 0);
    assert_eq!(regional["quest_list"]["1001"]["kill_count"], 0);
    assert_eq!(regional["kill_count_reward_data"]["received_up_to"], 0);
    manage(&service, "PUT", "/v1/activities/raid-boss/1", json!({"hp_percentage":70,"total_kill_count":2}));
    let claimed = send(&service, summary_path, body.clone());
    assert!(!claimed["kill_count_reward_data"]["reward_list"].as_array().unwrap().is_empty());
    assert!(send(&service, summary_path, body.clone())["kill_count_reward_data"]["reward_list"].as_array().unwrap().is_empty());
    send(&service, "/api/index.php/event/raid/battle/start", json!({
        "viewer_id":viewer,"event_id":1,"quest_id":1001,"play_id":"raid-regional-inflight",
        "party_group_id":1,"is_auto_start_mode":false,
    }));
    set_time(&service, "2023-09-10T12:00:00Z");
    let finish = send(&service, "/api/index.php/single_battle_quest/finish", json!({
        "viewer_id":viewer,"category":23,"quest_id":1001,"play_id":"raid-regional-inflight",
        "is_accomplished":true,"elapsed_time_ms":30000,"score":100,"add_mana":0,
        "statistics":{"clear_phase":1,"zones":[],"party":{
            "characters":[{"id":1},null,null],"unison_characters":[null,null,null],
            "equipments":[null,null,null],"ability_soul_ids":[null,null,null],
        }},
    }));
    assert_eq!(finish["raid_event"]["raid_boss"]["hp_percentage"], 70);
    assert_eq!(finish["raid_event"]["raid_boss"]["total_kill_count"], 3);
    assert_eq!(finish["raid_event"]["quest_boss"]["kill_count"], 1);
    let original = send(&service, summary_path, body.clone());
    assert_eq!(original["raid_boss"]["hp_percentage"], 45);
    assert_eq!(original["raid_boss"]["total_kill_count"], 2);
    assert_eq!(original["quest_list"]["1001"]["kill_count"], 9);
    assert!(original["kill_count_reward_data"]["reward_list"].as_array().unwrap().is_empty());
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM player_activity_history WHERE activity_id='raid:1' AND kind='claim' AND json_extract(details_json,'$.receipt_field')='kill_reward_receipts'", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    let history = manage(&service, "GET", &history_path(&service, "raid:1", "history"), json!({}));
    let regional = history["entries"].as_array().unwrap().iter()
        .find(|entry| entry["details"]["play_id"] == "raid-regional-inflight").unwrap();
    assert_eq!(regional["details"]["best_record"]["elapsed_time_ms"], 30000);
    assert_eq!(regional["details"]["best_record"]["high_score"], 100);
    set_time(&service, "2021-12-01T12:00:00Z");
    let earned = send(&service, summary_path, body.clone());
    assert_eq!(earned["raid_boss"]["total_kill_count"], 3);
    assert_eq!(earned["kill_count_reward_data"]["received_up_to"], 3);
    assert_eq!(earned["kill_count_reward_data"]["reward_list"], json!([
        {"kind":1,"kind_id":49100,"number":15},
        {"kind":4,"kind_id":null,"number":1000},
    ]));
    assert!(send(&service, summary_path, body)["kill_count_reward_data"]["reward_list"].as_array().unwrap().is_empty());
    assert_eq!(db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get::<_, i64>(0)).unwrap(), 3);
    service.stop().unwrap();
}
