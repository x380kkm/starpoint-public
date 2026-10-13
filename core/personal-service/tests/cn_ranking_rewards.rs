// audience: internal
// # personal-service-cn-ranking-reward-tests
// 此文件验证竞速成绩分类, 领奖时段, 真实奖励投递和重复领取.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_ranking_rewards/records.rs"]
mod records;
#[path = "cn_ranking_rewards/temporary.rs"]
mod temporary;
#[path = "cn_ranking_rewards/presentation.rs"]
mod presentation;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn send(service: &PersonalService, path: &str, body: Value) -> String {
    cn_support::send_request(service.port(), path, &encode_request(&body))
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&send(service, "/api/index.php/tool/signup",
        serde_json::to_value(SignupRequest { device_id: 710 }).unwrap())).data_headers.viewer_id
}

fn update_snapshot(root: &Path, update: impl FnOnce(&mut Value)) {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let (id, serialized): (i64, String) = db.query_row("SELECT account_id, data_json FROM player_snapshots LIMIT 1",
        [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    let mut player: Value = serde_json::from_str(&serialized).unwrap();
    update(&mut player);
    db.execute("UPDATE player_snapshots SET data_json=?1 WHERE account_id=?2",
        params![player.to_string(), id]).unwrap();
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(service.port(), method, path, "application/json",
        &[("Authorization", token.as_str())], body.to_string().as_bytes());
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn set_time(service: &PersonalService, iso: &str) {
    manage(service, "PUT", "/v1/time", json!({"enabled":true,"iso":iso,"rate":1}));
}

fn ranking_request(viewer_id: i64, event_id: i64, api_count: i64) -> Value {
    json!({"viewer_id":viewer_id,"ranking_event_id":event_id,"quest_kind":1,"api_count":api_count})
}

// //// 核对全部竞速关卡与经验关卡撞号时的成绩和百分数 [@x380kkm 2026-09-21] ////
#[test]
fn reads_all_ranking_records_from_the_ranking_category() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    let definitions = [(1,1001,54410),(2,2001,25800),(3,3001,18880),(4,4001,31720),
        (5,5001,6540),(1000,1000001,54410),(1001,1001001,25800)];
    update_snapshot(root.path(), |player| {
        player["quest_progress"]["14"] = json!([{"quest_id":1001,"best_elapsed_time_ms":1,"high_score":99}]);
        player["quest_progress"]["11"] = json!(definitions.iter().map(|(_,quest_id,time)| json!({
            "quest_id":quest_id,"finished":true,"best_elapsed_time_ms":time*2,"best_elapsed_score":73,"high_score":99,
            "best_leader_character_id":111001
        })).collect::<Vec<_>>());
    });
    for (index, (event_id, _, reference)) in definitions.into_iter().enumerate() {
        let response = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/get_summary",
            ranking_request(viewer_id, event_id, index as i64 + 1))).data;
        assert_eq!(response["best_record"]["elapsed_time_ms"], reference * 2);
        assert_eq!(response["best_record"]["score"], 73);
        assert_eq!(response["rank_percentage"], 50.0);
    }
    update_snapshot(root.path(), |player| player["quest_progress"]["11"] = json!([]));
    let response = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/get_summary",
        ranking_request(viewer_id, 1, 20))).data;
    assert!(response["best_record"].is_null());
    service.stop().unwrap();
}

// //// 在统计后领奖并核对邮件到账和持久化幂等 [@x380kkm 2026-09-21] ////
#[test]
fn delivers_ranking_rewards_after_aggregation_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service);
    update_snapshot(root.path(), |player| {
        player["quest_progress"]["11"] = json!([{"quest_id":1001,"finished":true,"best_elapsed_time_ms":40000,"high_score":20}]);
    });
    manage(&service, "PUT", "/v1/activities/calendar/ranking:1",
        json!({"enabled":true,"start_at":"2030-01-01T00:00:00.000Z","end_at":"2030-01-02T00:00:00.000Z"}));
    for time in ["2030-01-01T12:00:00.000Z", "2030-01-03T00:00:00.000Z"] {
        set_time(&service, time);
        let response = send(&service, "/api/index.php/ranking_event/receive_reward", ranking_request(viewer_id, 1, 1));
        assert!(response.contains("ranking_reward_not_ready"), "{response}");
    }
    set_time(&service, "2030-01-05T00:00:01.000Z");
    let first = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/receive_reward",
        ranking_request(viewer_id, 1, 2))).data;
    assert_eq!(first["status"], 1);
    assert_eq!(first["rank_percentage"], 0.0);
    assert_eq!(first["degree_list"][0]["degree_id"], 54040);
    assert_eq!(first["mail_arrived"], true);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let (mail_id, serialized): (i64, String) = db.query_row("SELECT id,rewards_json FROM player_mails", [],
        |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
    let rewards: Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(rewards["freeVmoney"], 1500);
    assert_eq!(rewards["itemList"]["49100"], 50);
    let received = decode_response::<Value>(&send(&service, "/api/index.php/mail/receive",
        json!({"viewer_id":viewer_id,"mail_id":mail_id,"api_count":3}))).data;
    let stone = received["user_info"]["free_vmoney"].as_i64().unwrap();
    let repeated = send(&service, "/api/index.php/mail/receive",
        json!({"viewer_id":viewer_id,"mail_id":mail_id,"api_count":4}));
    assert_eq!(decode_response::<Value>(&repeated).data_headers.result_code, 2002);
    let saved_stone: i64 = db.query_row("SELECT json_extract(data_json,'$.user_info.free_vmoney') FROM player_snapshots", [], |row| row.get(0)).unwrap();
    assert_eq!(saved_stone, stone);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let repeated = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/receive_reward",
        ranking_request(viewer_id, 1, 5))).data;
    assert_eq!(repeated["status"], 2);
    let mail_count: i64 = db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0)).unwrap();
    assert_eq!(mail_count, 1);
    set_time(&service, "2030-01-13T00:00:00.000Z");
    let expired = send(&service, "/api/index.php/ranking_event/receive_reward", ranking_request(viewer_id, 1, 6));
    assert!(expired.contains("ranking_reward_expired"), "{expired}");
    service.stop().unwrap();
}
