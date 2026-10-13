// audience: internal
// # personal-service-cn-multiplayer-mission-api-tests
//
// 房间身份和客户端区域统计驱动协战任务与修行积分. 隔离存档覆盖领取, 请求重放和结算写入失败.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 通过真实接口构造协战参与和任务查询 [@x380kkm 2026-09-23] ////
fn send(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(service.port(), &format!("/api/index.php/{route}"), &encode_request(body))
}

fn data(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = send(service, route, body);
    assert!(response.starts_with("HTTP/1.1 200"), "{route}: {response}");
    decode_response::<Value>(&response).data
}

fn signup(service: &PersonalService, device: i64) -> i64 {
    decode_response::<SignupData>(&send(service, "tool/signup", &json!({"device_id":device})))
        .data_headers.viewer_id
}

fn load(service: &PersonalService, viewer: i64) -> Value {
    data(service, "load", &json!({"viewer_id":viewer,"keychain":viewer}))
}

fn set_time(service: &PersonalService, iso: &str) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(service.port(), "PUT", "/v1/time", "application/json",
        &[("Authorization", token.as_str())], json!({"enabled":true,"iso":iso,"rate":1}).to_string().as_bytes());
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn room(service: &PersonalService, viewer: i64) -> Value {
    data(service, "multi_battle_quest/create_room",
        &json!({"viewer_id":viewer,"category":2,"quest_id":1002001,"party_id":1}))
}

fn start(service: &PersonalService, viewer: i64, room: &Value, play: &str) {
    data(service, "multi_battle_quest/start", &json!({
        "viewer_id":viewer,"category":2,"quest_id":1002001,"party_id":1,"play_id":play,
        "room_number":room["room_number"],"use_boss_boost_point":false,
        "use_boost_point":false,"is_auto_start_mode":false,
    }));
}

fn finish_request(viewer: i64, play: &str, zones: Value) -> Value {
    json!({
        "viewer_id":viewer,"category":2,"quest_id":1002001,"play_id":play,
        "is_accomplished":true,"elapsed_time_ms":100000,"score":1000,"add_mana":0,
        "statistics":{"zones":zones,"party":{"characters":[{"id":1},null,null],
            "unison_characters":[null,null,null],"equipments":[null,null,null],"ability_soul_ids":[null,null,null]}},
    })
}

fn unlock_coop_missions(service: &PersonalService, viewer: i64) {
    let play = format!("unlock-coop-{viewer}");
    data(service, "single_battle_quest/start", &json!({
        "viewer_id":viewer,"category":1,"quest_id":1004002,"party_id":1,"play_id":play,
        "use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false,
    }));
    let mut request = finish_request(viewer, &play, json!([]));
    request["category"] = json!(1);
    request["quest_id"] = json!(1004002);
    let response = data(service, "single_battle_quest/finish", &request);
    assert!(has_reward(&response, 1, 42));
}

fn progress(service: &PersonalService, viewer: i64, category: i64, mission: i64) -> i64 {
    let response = data(service, "mission/get_mission_progress",
        &json!({"viewer_id":viewer,"api_count":1,"category_list":[{"category":category,"event_id":1}]}));
    response["mission_progress_list"].as_array().unwrap().iter()
        .find(|entry| entry["mission_id"] == mission).unwrap()["progress_value"].as_i64().unwrap()
}

fn has_reward(response: &Value, category: i64, mission: i64) -> bool {
    response["mission_info"].as_array().unwrap().iter()
        .any(|entry| entry["mission_category_id"] == category && entry["mission_id"] == mission)
}

fn pass_points(service: &PersonalService, viewer: i64) -> i64 {
    data(service, "Pass_card/get_pass_card", &json!({"viewer_id":viewer,"pass_card_id":1}))["point"]
        .as_i64().unwrap()
}

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn persistent_state(root: &Path) -> Value {
    let db = database(root);
    let mut rows = db.prepare("SELECT data_json FROM player_snapshots ORDER BY account_id").unwrap();
    let snapshots = rows.query_map([], |row| row.get::<_, String>(0)).unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap().into_iter()
        .map(|serialized| serde_json::from_str::<Value>(&serialized).unwrap()).collect::<Vec<_>>();
    let mut rows = db.prepare("SELECT account_id,category,mission_id,value FROM player_mission_progress ORDER BY account_id,category,mission_id").unwrap();
    let progress = rows.query_map([], |row| Ok((row.get::<_, i64>(0)?,row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,row.get::<_, i64>(3)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    let history: i64 = db.query_row("SELECT COUNT(*) FROM player_receive_history", [], |row| row.get(0)).unwrap();
    let active: i64 = db.query_row("SELECT COUNT(*) FROM active_single_quests", [], |row| row.get(0)).unwrap();
    json!({"snapshots":snapshots,"progress":progress,"history":history,"active":active})
}
// //// /通过真实接口构造协战参与和任务查询 ////

// //// 按房间身份发放房主和客机任务奖励并保留重放结果 [@x380kkm 2026-09-23] ////
#[test]
fn derives_host_and_guest_missions_from_the_joined_room() {
    let storage = TempDir::new().unwrap();
    let service = PersonalService::start(storage.path(), 0).unwrap();
    let host = signup(&service, 923301);
    let guest = signup(&service, 923302);
    load(&service, host);
    load(&service, guest);
    database(storage.path()).execute(
        "UPDATE player_snapshots SET data_json=json_set(data_json,'$.user_info.stamina',100)", []).unwrap();
    unlock_coop_missions(&service, host);
    unlock_coop_missions(&service, guest);
    let room = room(&service, host);
    data(&service, "multi_battle_quest/select_room", &json!({
        "viewer_id":guest,"category":2,"quest_id":1002001,"party_id":1,"room_number":room["room_number"],
    }));
    start(&service, host, &room, "mission-host");
    start(&service, guest, &room, "mission-guest");
    assert_eq!(progress(&service, host, 1, 26), 0);
    assert_eq!(progress(&service, guest, 1, 27), 0);

    let mut host_request = finish_request(host, "mission-host", json!([]));
    host_request["statistics"]["is_mvp"] = json!(true);
    let host_response = data(&service, "multi_battle_quest/finish", &host_request);
    assert!(has_reward(&host_response, 1, 26));
    assert!(!has_reward(&host_response, 1, 27));
    assert!(has_reward(&host_response, 1, 29));
    let mut guest_request = finish_request(guest, "mission-guest", json!([]));
    guest_request["statistics"]["is_host"] = json!(true);
    guest_request["statistics"]["is_mvp"] = json!(false);
    let guest_response = data(&service, "multi_battle_quest/finish", &guest_request);
    assert!(has_reward(&guest_response, 1, 27));
    assert!(!has_reward(&guest_response, 1, 26));
    assert!(!has_reward(&guest_response, 1, 29));
    assert_eq!(progress(&service, host, 1, 26), 1);
    assert_eq!(progress(&service, host, 1, 27), 0);
    assert_eq!(progress(&service, guest, 1, 26), 0);
    assert_eq!(progress(&service, guest, 1, 27), 1);
    let settled = persistent_state(storage.path());
    assert_eq!(settled["snapshots"][0]["mission_stage_receipts"]["1:26:1"]["granted_rewards"][0]["amount"], 5);
    assert_eq!(settled["snapshots"][1]["mission_stage_receipts"]["1:27:1"]["granted_rewards"][0]["amount"], 5);
    assert_eq!(data(&service, "multi_battle_quest/finish", &host_request), host_response);
    assert_eq!(data(&service, "multi_battle_quest/finish", &guest_request), guest_response);
    assert_eq!(persistent_state(storage.path()), settled);
    service.stop().unwrap();

    let service = PersonalService::start(storage.path(), 0).unwrap();
    assert_eq!(data(&service, "multi_battle_quest/finish", &host_request), host_response);
    assert_eq!(data(&service, "multi_battle_quest/finish", &guest_request), guest_response);
    assert_eq!(persistent_state(storage.path()), settled);
    service.stop().unwrap();
}
// //// /按房间身份发放房主和客机任务奖励并保留重放结果 ////

// //// 发送表情统计累加到当周修行奖励并随结算失败整体回滚 [@x380kkm 2026-09-23] ////
#[test]
fn counts_sent_emotions_separately_and_retries_pass_rewards_atomically() {
    let storage = TempDir::new().unwrap();
    let service = PersonalService::start(storage.path(), 0).unwrap();
    let viewer = signup(&service, 923303);
    set_time(&service, "2024-06-05T12:00:00.000Z");
    load(&service, viewer);
    database(storage.path()).execute(
        "UPDATE player_snapshots SET data_json=json_set(data_json,'$.user_info.stamina',100)", []).unwrap();

    let first_room = room(&service, viewer);
    let summoned = data(&service, "multi_battle_quest/summon", &json!({
        "viewer_id":viewer,"category_id":2,"quest_id":1002001,"room_number":first_room["room_number"],
    }));
    assert!(summoned["mate1"].is_object() && summoned["mate2"].is_object());
    start(&service, viewer, &first_room, "emotion-use");
    data(&service, "multi_battle_quest/finish",
        &finish_request(viewer, "emotion-use", json!([{"use_emotion_count":10,"send_emotion_count":0}])));
    assert_eq!(progress(&service, viewer, 7, 3), 0);

    let second_room = room(&service, viewer);
    start(&service, viewer, &second_room, "emotion-four");
    data(&service, "multi_battle_quest/finish",
        &finish_request(viewer, "emotion-four", json!([{"use_emotion_count":0,"send_emotion_count":4}])));
    assert_eq!(progress(&service, viewer, 7, 3), 4);
    let third_room = room(&service, viewer);
    start(&service, viewer, &third_room, "emotion-six");
    let points_before = pass_points(&service, viewer);
    let before = persistent_state(storage.path());
    let request = finish_request(viewer, "emotion-six",
        json!([{"use_emotion_count":0,"send_emotion_count":2},{"use_emotion_count":0,"send_emotion_count":4}]));
    database(storage.path()).execute_batch(
        "CREATE TRIGGER reject_mission_finish BEFORE INSERT ON multiplayer_battle_action_receipts
         WHEN NEW.action='finish' BEGIN SELECT RAISE(ABORT, 'finish unavailable'); END;").unwrap();
    let failed = send(&service, "multi_battle_quest/finish", &request);
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(persistent_state(storage.path()), before);
    database(storage.path()).execute_batch("DROP TRIGGER reject_mission_finish;").unwrap();

    let response = data(&service, "multi_battle_quest/finish", &request);
    assert!(has_reward(&response, 7, 3));
    assert_eq!(progress(&service, viewer, 7, 3), 10);
    assert_eq!(pass_points(&service, viewer), points_before + 80);
    let settled = persistent_state(storage.path());
    assert_eq!(data(&service, "multi_battle_quest/finish", &request), response);
    assert_eq!(persistent_state(storage.path()), settled);

    let claim = json!({"viewer_id":viewer,"pass_card_id":1,"reward1_receive":[1]});
    let before_claim = load(&service, viewer);
    let granted = data(&service, "Pass_card/receive_all", &claim);
    assert_eq!(granted["all_received_record"][0]["is_received_1"], 1);
    let after_claim = load(&service, viewer);
    assert!(after_claim["item_list"] != before_claim["item_list"]
        || after_claim["user_info"]["free_vmoney"] != before_claim["user_info"]["free_vmoney"]);
    data(&service, "Pass_card/receive_all", &claim);
    assert_eq!(load(&service, viewer)["item_list"], after_claim["item_list"]);
    assert_eq!(load(&service, viewer)["user_info"]["free_vmoney"], after_claim["user_info"]["free_vmoney"]);
    service.stop().unwrap();

    let service = PersonalService::start(storage.path(), 0).unwrap();
    assert_eq!(progress(&service, viewer, 7, 3), 10);
    assert_eq!(pass_points(&service, viewer), points_before + 80);
    set_time(&service, "2024-06-12T12:00:00.000Z");
    assert_eq!(progress(&service, viewer, 7, 3), 0);
    let next_room = room(&service, viewer);
    start(&service, viewer, &next_room, "emotion-next-week");
    let next = data(&service, "multi_battle_quest/finish",
        &finish_request(viewer, "emotion-next-week", json!([{"send_emotion_count":10}])));
    assert!(has_reward(&next, 7, 3));
    assert_eq!(progress(&service, viewer, 7, 3), 10);
    let state = persistent_state(storage.path());
    let receipts = state["snapshots"][0]["mission_stage_receipts"].as_object().unwrap();
    assert_eq!(receipts.keys().filter(|key| key.starts_with("7:3:week:")).count(), 2);
    service.stop().unwrap();
}
// //// /发送表情统计累加到当周修行奖励并随结算失败整体回滚 ////
