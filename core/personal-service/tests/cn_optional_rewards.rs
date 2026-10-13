// audience: internal
// # personal-service-cn-optional-reward-tests
//
// 隔离存档通过真实领取, 战斗和管理入口核对基金资格, 修行积分及重置后的奖励历史.

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

fn request(service: &PersonalService, route: &str, body: Value) -> (i64, Value) {
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    ));
    (response.data_headers.result_code, response.data)
}

fn send(service: &PersonalService, route: &str, body: Value) -> Value {
    let (code, data) = request(service, route, body);
    assert_eq!(code, 1, "{route}: {data}");
    data
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&json!({"device_id":920033})),
    ))
    .data_headers
    .viewer_id
}

fn stored(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database
        .query_row(
            "SELECT data_json FROM player_snapshots LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn update(root: &Path, update: impl FnOnce(&mut Value)) {
    let mut data = stored(root);
    update(&mut data);
    Connection::open(root.join("personal-service.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [data.to_string()],
        )
        .unwrap();
}

fn manage(service: &PersonalService, method: &str, route: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        route,
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn set_time(service: &PersonalService, iso: &str) {
    manage(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"iso":iso,"rate":1}),
    );
}

fn purchase(service: &PersonalService, viewer: i64, count: i64, product: &str) {
    send(
        service,
        "payment/start",
        json!({"viewer_id":viewer,"api_count":count,"payment":{"product_id":product}}),
    );
}

fn pass_state(service: &PersonalService, viewer: i64, event: i64) -> Value {
    send(
        service,
        "Pass_card/get_pass_card",
        json!({"viewer_id":viewer,"pass_card_id":event}),
    )
}

fn battle(service: &PersonalService, viewer: i64, play: &str) -> Value {
    send(
        service,
        "single_battle_quest/start",
        json!({"viewer_id":viewer,"category":1,"quest_id":10001001,"play_id":play,"party_id":1,"use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false}),
    );
    let body = json!({"viewer_id":viewer,"category":1,"quest_id":10001001,"play_id":play,"is_accomplished":true,"elapsed_time_ms":100000,"score":1000,"add_mana":0,"statistics":{"clear_phase":1,"zones":[{"use_dash_count":10,"send_emotion_count":10}],"party":{"characters":[{"id":1},null,null],"unison_characters":[null,null,null],"equipments":[null,null,null],"ability_soul_ids":[null,null,null]}}});
    send(service, "single_battle_quest/finish", body.clone());
    body
}

// //// 基金购买资格, 分类通关和请求重放共同约束到账 [@x380kkm 2026-09-22] ////
#[test]
fn growth_funds_require_their_entitlement_and_actual_quest_category() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let receive = json!({"viewer_id":viewer,"bonus_ids":[1,1]});
    assert_eq!(
        request(&service, "growth_fund/receive_fund_bonus", receive.clone()).0,
        20001
    );
    purchase(&service, viewer, 1, "com.leiting.wf.stonepack_Light");
    let before = stored(root.path())["user_info"]["free_vmoney"]
        .as_i64()
        .unwrap();
    let response = send(&service, "growth_fund/receive_fund_bonus", receive.clone());
    assert_eq!(response["user_info"]["free_vmoney"], before + 1500);
    assert_eq!(response["fund_receive_list"], json!([1]));
    assert_eq!(
        send(&service, "growth_fund/receive_fund_bonus", receive)["user_info"]["free_vmoney"],
        before + 1500
    );
    let definitions: Value =
        serde_json::from_str(include_str!("../assets/cn-growth-fund.json")).unwrap();
    let main = definitions["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fund_kind"] == "main_quest")
        .unwrap();
    let id = main["bonus_id"].as_i64().unwrap();
    let quest = main["quest_id"].as_i64().unwrap();
    purchase(&service, viewer, 2, "com.leiting.wf.stonepack_MAIN");
    update(root.path(), |data| {
        data["quest_progress"]["4"] = json!([{"quest_id":quest,"finished":true}])
    });
    let receive = json!({"viewer_id":viewer,"bonus_ids":[id]});
    assert_eq!(
        request(&service, "growth_fund/receive_fund_bonus", receive.clone()).0,
        20001
    );
    send(
        &service,
        "story_quest/finish",
        json!({"viewer_id":viewer,"category":1,"quest_id":quest,"party_id":1}),
    );
    let received = send(&service, "growth_fund/receive_fund_bonus", receive);
    assert!(received["fund_receive_list"]
        .as_array()
        .unwrap()
        .contains(&json!(id)));
    service.stop().unwrap();
}

// //// 战斗积分开启免费路线并按购买资格解锁高阶路线 [@x380kkm 2026-09-22] ////
#[test]
fn pass_card_earns_points_from_battle_and_tracks_separate_claims() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    set_time(&service, "2024-06-05T12:00:00.000Z");
    assert_eq!(pass_state(&service, viewer, 1)["is_buy"], false);
    let receive = json!({"viewer_id":viewer,"pass_card_id":1,"reward1_receive":[1]});
    assert_eq!(
        request(&service, "Pass_card/receive_all", receive.clone()).0,
        20001
    );
    let replay = battle(&service, viewer, "pass-card-battle");
    let point = pass_state(&service, viewer, 1)["point"].as_i64().unwrap();
    assert!(point >= 180, "单人, 冲刺与表情任务积分缺失: {point}");
    send(&service, "single_battle_quest/finish", replay);
    assert_eq!(pass_state(&service, viewer, 1)["point"], point);
    let free = send(&service, "Pass_card/receive_all", receive);
    assert_eq!(free["all_received_record"][0]["is_received_1"], 1);
    assert_eq!(free["all_received_record"][0]["is_received_2"], 0);
    let premium = json!({"viewer_id":viewer,"pass_card_id":1,"reward2_receive":[1]});
    assert_eq!(
        request(&service, "Pass_card/receive_all", premium.clone()).0,
        20001
    );
    purchase(&service, viewer, 1, "com.leiting.wf.pass_card");
    let before = stored(root.path())["item_list"]["999001"]
        .as_i64()
        .unwrap_or_default();
    let reward = send(&service, "Pass_card/receive_all", premium.clone());
    assert_eq!(reward["item_list"]["999001"], before + 1);
    send(&service, "Pass_card/receive_all", premium);
    assert_eq!(stored(root.path())["item_list"]["999001"], before + 1);
    set_time(&service, "2025-05-05T12:00:00.000Z");
    assert_eq!(pass_state(&service, viewer, 12)["is_buy"], true);
    service.stop().unwrap();
}

// //// 满级批量领取保留历史并支持重置后重新完成任务 [@x380kkm 2026-09-22] ////
#[test]
fn claims_every_level_and_preserves_history_across_activity_reset() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    std::fs::write(cdn.path().join("activity-catalog.json"), json!({"format_version":1,"activities":[{"activity_id":"pass-card:1","name":"修行","kind":"pass-card"}]}).to_string()).unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let viewer = signup(&service);
    set_time(&service, "2024-06-05T12:00:00.000Z");
    pass_state(&service, viewer, 1);
    purchase(&service, viewer, 1, "com.leiting.wf.pass_card");
    update(root.path(), |data| {
        data["pass_card_state"]["events"]["1"]["point"] = json!(6000)
    });
    let paid = stored(root.path())["user_info"]["vmoney"].as_i64().unwrap();
    let receive =
        json!({"viewer_id":viewer,"pass_card_id":1,"all_receive":(1..=60).collect::<Vec<_>>()});
    let response = send(&service, "Pass_card/receive_all", receive.clone());
    assert_eq!(
        response["all_received_record"].as_array().unwrap().len(),
        60
    );
    assert!(response["all_received_record"]
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record["is_received_1"] == 1 && record["is_received_2"] == 1));
    assert_eq!(stored(root.path())["user_info"]["vmoney"], paid + 500);
    let delivered = stored(root.path());
    send(&service, "Pass_card/receive_all", receive);
    assert_eq!(stored(root.path())["item_list"], delivered["item_list"]);
    assert_eq!(stored(root.path())["user_info"]["vmoney"], paid + 500);
    let saves = manage(&service, "GET", "/v1/local-saves", json!({}));
    let prefix = format!(
        "/v1/local-saves/{}/activities/pass-card:1",
        saves["slots"][0]["id"]
    );
    let history = manage(&service, "GET", &format!("{prefix}/history"), json!({}));
    assert!(!history["entries"].as_array().unwrap().is_empty());
    manage(
        &service,
        "POST",
        &format!("{prefix}/reset"),
        json!({"expected_round":0}),
    );
    assert_eq!(pass_state(&service, viewer, 1)["point"], 0);
    assert_eq!(stored(root.path())["item_list"], delivered["item_list"]);
    battle(&service, viewer, "pass-card-reset-battle");
    let before = stored(root.path())["item_list"]["999003"]
        .as_i64()
        .unwrap_or_default();
    send(
        &service,
        "Pass_card/receive_all",
        json!({"viewer_id":viewer,"pass_card_id":1,"reward1_receive":[1]}),
    );
    assert_eq!(stored(root.path())["item_list"]["999003"], before + 1);
    let history = manage(&service, "GET", &format!("{prefix}/history"), json!({}));
    assert_eq!(history["participation_round"], 1);
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "reset"));
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 0));
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 1));
    service.stop().unwrap();
}

// //// 修行登录任务按五点日切累计并保留同日和回拨后的进度 [@x380kkm 2026-09-22] ////
#[test]
fn pass_card_login_progress_changes_at_the_game_day_boundary() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let fixture: Value =
        serde_json::from_str(include_str!("../assets/cn-mission-master.json")).unwrap();
    let mission = fixture["categories"]["8"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mission| mission["event_id"] == 1 && mission["pattern"] == "total_login_days")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let progress = || {
        let response = send(
            &service,
            "mission/get_mission_progress",
            json!({"viewer_id":viewer,"api_count":1,"category_list":[{"category":8,"event_id":1}]}),
        );
        response["mission_progress_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["mission_id"] == mission)
            .unwrap()["progress_value"]
            .as_i64()
            .unwrap()
    };
    set_time(&service, "2024-06-04T20:59:00.000Z");
    send(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert_eq!(progress(), 1);
    send(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert_eq!(progress(), 1);
    set_time(&service, "2024-06-04T21:01:00.000Z");
    send(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert_eq!(progress(), 2);
    set_time(&service, "2024-06-04T20:59:00.000Z");
    send(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert_eq!(progress(), 2);
    service.stop().unwrap();
}
