// audience: internal
// # personal-service-activity-history-tests
// 该文件通过战斗, 领奖和管理 API 核对活动历史持久化, 存档隔离和重置后的再次发奖.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "activity_history/restore.rs"]
mod restore;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn send(service: &PersonalService, path: &str, body: Value) -> Value {
    decode_response::<Value>(&cn_support::send_request(service.port(), path, &encode_request(&body))).data
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> String {
    let token = format!("Bearer {}", service.management_token());
    support::request_with_headers(service.port(), method, path, "application/json", &[("Authorization", token.as_str())], body.to_string().as_bytes())
}

fn management_value(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let response = manage(service, method, path, body);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn start_service(root: &Path, cdn: &Path) -> PersonalService {
    std::fs::write(cdn.join("activity-catalog.json"), json!({"format_version":1,"activities":[
        {"activity_id":"ranking:1","name":"竞速","kind":"ranking"},
        {"activity_id":"solo-time-attack:1","name":"极时试炼","kind":"solo-time-attack"},
    ]}).to_string()).unwrap();
    PersonalService::start_with_cdn_root(root, 0, cdn).unwrap()
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest{device_id:620}))).data_headers.viewer_id
}

fn snapshot(root: &Path) -> Value {
    let data: String = Connection::open(root.join("personal-service.sqlite3")).unwrap()
        .query_row("SELECT data_json FROM player_snapshots LIMIT 1",[], |row| row.get(0)).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn activity_path(service: &PersonalService, activity_id: &str, action: &str) -> String {
    let saves = management_value(service,"GET","/v1/local-saves",json!({}));
    format!("/v1/local-saves/{}/activities/{activity_id}/{action}",saves["slots"][0]["id"])
}

fn start_battle(service: &PersonalService, viewer: i64, category: i64, play_id: &str) {
    send(service,"/api/index.php/single_battle_quest/start",json!({
        "viewer_id":viewer,"quest_id":1001,"category":category,"play_id":play_id,
        "use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false,"party_id":1,
    }));
}

fn finish_battle(service: &PersonalService, viewer: i64, category: i64, play_id: &str, elapsed: i64, accomplished: bool) -> Value {
    send(service,"/api/index.php/single_battle_quest/finish",json!({
        "viewer_id":viewer,"quest_id":1001,"category":category,"play_id":play_id,"elapsed_time_ms":elapsed,
        "score":5000,"add_mana":0,"is_accomplished":accomplished,
        "statistics":{"zones":[],"party":{"characters":[{"id":1},null,null],"unison_characters":[null,null,null]}},
    }))
}

fn history(service: &PersonalService, activity_id: &str) -> Value {
    management_value(service,"GET",&activity_path(service,activity_id,"history"),json!({}))
}

// //// 重置竞速资格时保留奖励邮件和其他关卡的成绩 [@x380kkm 2026-09-21] ////
#[test]
fn preserves_ranking_history_and_assets_across_participation_rounds() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start_service(root.path(),cdn.path());
    let viewer = signup(&service);
    management_value(&service,"PUT","/v1/activities/ranking:1/mode",json!({"mode":"always"}));
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mut initial = snapshot(root.path());
    initial["user_info"]["stamina"] = json!(1000);
    initial["quest_progress"]["14"] = json!([{"quest_id":1001,"high_score":999}]);
    db.execute("UPDATE player_snapshots SET data_json=?1",[initial.to_string()]).unwrap();
    start_battle(&service,viewer,11,"ranking-round-zero");
    finish_battle(&service,viewer,11,"ranking-round-zero",40000,true);
    let request = json!({"viewer_id":viewer,"ranking_event_id":1,"quest_kind":1,"api_count":1});
    assert_eq!(send(&service,"/api/index.php/ranking_event/receive_reward",request.clone())["status"],1);
    let mail: i64 = db.query_row("SELECT id FROM player_mails WHERE title LIKE '%排名奖励%'",[],|row| row.get(0)).unwrap();
    let before_mail = snapshot(root.path())["user_info"]["free_vmoney"].as_i64().unwrap();
    send(&service,"/api/index.php/mail/receive",json!({"viewer_id":viewer,"mail_id":mail}));
    assert_eq!(snapshot(root.path())["user_info"]["free_vmoney"], before_mail+1500);
    let recorded = history(&service,"ranking:1");
    let records = recorded["entries"].as_array().unwrap();
    assert_eq!(records.iter().filter(|entry|entry["kind"]=="statistics").count(),1);
    assert_eq!(records.iter().filter(|entry|entry["kind"]=="delivery").count(),1);
    assert!(records.iter().find(|entry|entry["kind"]=="delivery").unwrap()["received_at"].as_i64().is_some());
    let before_reset = snapshot(root.path());
    let reset_path = activity_path(&service,"ranking:1","reset");
    let unauthorized = support::request_with_headers(service.port(),"POST",&reset_path,"application/json",&[("Authorization","Bearer invalid")],b"{\"expected_round\":0}");
    assert!(unauthorized.starts_with("HTTP/1.1 401"), "{unauthorized}");
    assert_eq!(snapshot(root.path()),before_reset);
    management_value(&service,"POST",&reset_path,json!({"expected_round":0}));
    let reset = snapshot(root.path());
    for field in ["user_info","item_list","user_character_list","user_equipment_list","degree_ids"] { assert_eq!(reset[field],before_reset[field],"{field}"); }
    assert_eq!(reset["quest_progress"]["14"],initial["quest_progress"]["14"]);
    assert_eq!(reset["quest_progress"]["11"],json!([]));
    assert!(manage(&service,"POST",&reset_path,json!({"expected_round":0})).contains("activity_round_changed"));
    assert_eq!(send(&service,"/api/index.php/ranking_event/receive_reward",request.clone())["status"],3);
    start_battle(&service,viewer,11,"ranking-round-one");
    finish_battle(&service,viewer,11,"ranking-round-one",40000,true);
    assert_eq!(send(&service,"/api/index.php/ranking_event/receive_reward",request.clone())["status"],1);
    assert_eq!(send(&service,"/api/index.php/ranking_event/receive_reward",request)["status"],2);
    let count: i64 = db.query_row("SELECT COUNT(*) FROM player_mails WHERE title LIKE '%排名奖励%'",[],|row|row.get(0)).unwrap();
    assert_eq!(count,2);
    service.stop().unwrap();
    let service = start_service(root.path(),cdn.path());
    let recorded = history(&service,"ranking:1");
    let entries = recorded["entries"].as_array().unwrap();
    assert_eq!(entries.iter().filter(|entry|entry["kind"]=="statistics").count(),2);
    assert_eq!(entries.iter().filter(|entry|entry["kind"]=="claim").count(),2);
    assert_eq!(entries.iter().filter(|entry|entry["kind"]=="delivery").count(),2);
    assert_eq!(entries.iter().filter(|entry|entry["kind"]=="reset").count(),1);
    assert_eq!(recorded["participation_round"],1);
    service.stop().unwrap();
}

// //// 逐场统计包含未刷新最佳成绩的战斗并阻止战斗中重置 [@x380kkm 2026-09-21] ////
#[test]
fn records_every_attempt_without_duplicating_retries_and_reopens_time_rewards() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start_service(root.path(),cdn.path());
    let viewer = signup(&service);
    let reset_path = activity_path(&service,"solo-time-attack:1","reset");
    for (play_id,time,accomplished) in [("solo-fast",300000,true),("solo-slow",350000,true),("solo-failed",600000,false)] {
        start_battle(&service,viewer,25,play_id);
        let before = snapshot(root.path());
        assert!(manage(&service,"POST",&reset_path,json!({"expected_round":0})).contains("activity_battle_active"));
        assert_eq!(snapshot(root.path()),before);
        let result = finish_battle(&service,viewer,25,play_id,time,accomplished);
        let saved = snapshot(root.path());
        assert_eq!(finish_battle(&service,viewer,25,play_id,time,accomplished),result);
        assert_eq!(snapshot(root.path()),saved);
    }
    let recorded = history(&service,"solo-time-attack:1");
    let attempts = recorded["entries"].as_array().unwrap().iter().filter(|entry|entry["kind"]=="statistics").collect::<Vec<_>>();
    assert_eq!(attempts.len(),3);
    assert_eq!(attempts[0]["details"]["is_accomplished"],false);
    assert_eq!(attempts[0]["details"]["best_record"]["best_elapsed_time_ms"],300000);
    let before = snapshot(root.path());
    management_value(&service,"POST",&reset_path,json!({"expected_round":0}));
    assert_eq!(snapshot(root.path())["user_info"],before["user_info"]);
    start_battle(&service,viewer,25,"solo-new-round");
    let result = finish_battle(&service,viewer,25,"solo-new-round",300000,true);
    assert_eq!(result["solo_time_attack_event"]["reward_ids"].as_array().unwrap().len(),13);
    assert!(snapshot(root.path())["user_info"]["free_vmoney"].as_i64().unwrap()>before["user_info"]["free_vmoney"].as_i64().unwrap());
    let recorded = history(&service,"solo-time-attack:1");
    assert_eq!(recorded["entries"].as_array().unwrap().iter().filter(|entry|entry["kind"]=="statistics").count(),4);
    service.stop().unwrap();
}
