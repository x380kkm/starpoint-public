// audience: internal
// # personal-service-cn-mission-lifecycle-api-tests
//
// 隔离存档中的真实邮件, 商店和战斗结算验证任务资产变化, 激励领取窗口和重启持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_mission/battle_statistics.rs"]
mod battle_statistics;
#[path = "cn_mission/battle_conditions.rs"]
mod battle_conditions;
#[path = "cn_mission/awake.rs"]
mod awake;
#[path = "cn_mission/economy.rs"]
mod economy;
#[path = "cn_mission/mana_atomicity.rs"]
mod mana_atomicity;
#[path = "cn_mission/login_collections.rs"]
mod login_collections;
#[path = "cn_mission/companion_missions.rs"]
mod companion_missions;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn send(service: &PersonalService, path: &str, body: Value) -> String {
    cn_support::send_request(service.port(), path, &encode_request(&body))
}

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&send(service, "/api/index.php/tool/signup",
        serde_json::to_value(SignupRequest { device_id }).unwrap())).data_headers.viewer_id
}

fn load(service: &PersonalService, viewer_id: i64) -> Value {
    decode_response::<Value>(&send(service, "/api/index.php/load", json!({"viewer_id":viewer_id,"keychain":viewer_id}))).data
}

fn read_player(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database.query_row("SELECT data_json FROM player_snapshots", [], |row| row.get(0)).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn update_player(root: &Path, update: impl FnOnce(&mut Value)) {
    let mut player = read_player(root);
    update(&mut player);
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    database.execute("UPDATE player_snapshots SET data_json=?1", params![player.to_string()]).unwrap();
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(service.port(), method, path, "application/json",
        &[("Authorization", token.as_str())], body.to_string().as_bytes());
    assert!(response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.1 201"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn set_time(service: &PersonalService, iso: &str) {
    manage(service, "PUT", "/v1/time", json!({"enabled":true,"iso":iso,"rate":1}));
}

fn open_activity(service: &PersonalService, activity_id: &str) {
    manage(service, "PUT", &format!("/v1/activities/calendar/{activity_id}"),
        json!({"enabled":true,"start_at":"2020-01-01T00:00:00.000Z","end_at":"2099-01-01T00:00:00.000Z"}));
}

fn collect_progress(service: &PersonalService, viewer_id: i64, event_id: i64, mission_id: i64) -> i64 {
    let data = decode_response::<Value>(&send(service, "/api/index.php/mission/get_mission_progress",
        json!({"viewer_id":viewer_id,"api_count":1,"category_list":[{"category":4,"event_id":event_id}]}))).data;
    data["mission_progress_list"].as_array().unwrap().iter().find(|mission| mission["mission_id"] == mission_id)
        .unwrap()["progress_value"].as_i64().unwrap()
}

fn active_claim(service: &PersonalService, viewer_id: i64, mission_id: i64) -> Value {
    decode_response::<Value>(&send(service, "/api/index.php/active_mission/receive",
        json!({"viewer_id":viewer_id,"active_mission_list":[{"mission_id":mission_id,"stages":[1]}]}))).data
}

// //// 结算主动任务和历史遗漏奖励并核对重复载入资产 [@x380kkm 2026-09-21] ////
#[test]
fn recovers_legacy_receipts_and_claims_active_stones_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 921110);
    let original = load(&service, viewer_id);
    let stones = original["user_info"]["free_vmoney"].as_i64().unwrap();
    service.stop().unwrap();
    update_player(root.path(), |player| {
        player["mission_stage_receipts"] = json!({"1:1:1":true,"5:49000:1":true});
        player["active_mission_receipts"] = json!({"20002:1":true});
        player["all_active_mission_list"] = json!({"20003":{"progress":1,"stages":{"1":false}}});
    });
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 921110);
    let recovered = load(&service, viewer_id);
    assert_eq!(recovered["user_info"]["free_vmoney"], stones + 5);
    assert_eq!(recovered["item_list"], original["item_list"]);
    assert!(read_player(root.path())["degree_ids"].as_array().unwrap().iter().any(|degree| degree == 49000));
    let claimed = active_claim(&service, viewer_id, 20003);
    assert_eq!(claimed["user_info"]["free_vmoney"], stones + 255);
    active_claim(&service, viewer_id, 20003);
    assert_eq!(load(&service, viewer_id)["user_info"]["free_vmoney"], stones + 255);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 921110);
    let reloaded = load(&service, viewer_id);
    assert_eq!(reloaded["user_info"]["free_vmoney"], stones + 255);
    assert_eq!(reloaded["item_list"], original["item_list"]);
    service.stop().unwrap();
}
// //// /结算主动任务和历史遗漏奖励并核对重复载入资产 ////

// //// 验证激励资格与统计后投递并领取同一封邮件 [@x380kkm 2026-09-21] ////
#[test]
fn delivers_qualified_incentive_after_aggregation_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 921111);
    let incentive = |service: &PersonalService, mission_id| send(service, "/api/index.php/active_mission/receive_incentive",
        json!({"viewer_id":viewer_id,"mission_id":mission_id}));
    assert!(incentive(&service, 20002).contains("mission_has_no_incentive"));
    assert!(incentive(&service, 21010).contains("mission_not_completed"));
    update_player(root.path(), |player| {
        player["all_active_mission_list"] = json!({"21010":{"progress":1,"stages":{"1":false}},"21020":{"progress":1,"stages":{"1":true}}});
    });
    active_claim(&service, viewer_id, 21010);
    manage(&service, "PUT", "/v1/activities/calendar/active-mission:3",
        json!({"enabled":true,"start_at":"2030-01-01T00:00:00.000Z","end_at":"2030-01-02T00:00:00.000Z"}));
    for iso in ["2030-01-01T12:00:00.000Z", "2030-01-04T12:00:00.000Z"] {
        set_time(&service, iso);
        assert_eq!(decode_response::<Value>(&incentive(&service, 21010)).data_headers.result_code, 2479);
    }
    set_time(&service, "2030-01-05T00:00:01.000Z");
    let initial = load(&service, viewer_id);
    let first = decode_response::<Value>(&incentive(&service, 21010)).data;
    let reward_id = first["active_mission_incentive"]["ingame_reward_id"].as_i64().unwrap();
    let catalog: Value = serde_json::from_str(include_str!("../assets/cn-active-mission-incentives.json")).unwrap();
    let reward = catalog["rewards"].as_array().unwrap().iter().find(|reward| reward["id"] == reward_id).unwrap();
    assert_eq!(reward["event_id"], 3);
    assert!(reward["weight"].as_u64().unwrap() > 0);
    let repeated = decode_response::<Value>(&incentive(&service, 21010)).data;
    assert_eq!(repeated["active_mission_incentive"]["ingame_reward_id"], reward_id);
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let (mail_id, mail_count): (i64, i64) = database.query_row("SELECT MIN(id),COUNT(*) FROM player_mails", [],
        |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(mail_count, 1);
    decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", json!({"viewer_id":viewer_id,"mail_id":mail_id})));
    let received = load(&service, viewer_id);
    for field in ["free_vmoney", "free_mana", "exp_pool"] {
        assert_eq!(received["user_info"][field].as_i64().unwrap(), initial["user_info"][field].as_i64().unwrap() + reward[field].as_i64().unwrap(), "{field}");
    }
    for (id, count) in reward["items"].as_object().unwrap() {
        assert_eq!(received["item_list"][id].as_i64().unwrap(), initial["item_list"][id].as_i64().unwrap_or_default() + count.as_i64().unwrap());
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let replayed = decode_response::<Value>(&incentive(&service, 21010)).data;
    assert_eq!(replayed["active_mission_incentive"]["ingame_reward_id"], reward_id);
    assert_eq!(replayed["mail_arrived"], false);
    assert_eq!(database.query_row::<i64, _, _>("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0)).unwrap(), 1);
    set_time(&service, "2030-01-13T00:00:00.000Z");
    assert_eq!(decode_response::<Value>(&incentive(&service, 21020)).data_headers.result_code, 2479);
    service.stop().unwrap();
}
// //// /验证激励资格与统计后投递并领取同一封邮件 ////

// //// 从实际邮件和兑换收获累计物品并保留已消费数量 [@x380kkm 2026-09-21] ////
#[test]
fn counts_mail_and_shop_acquisitions_without_counting_existing_inventory() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 921112);
    update_player(root.path(), |player| {
        player["item_list"]["80001"] = json!(5000);
        player["item_list"]["51000"] = json!(10_000);
        player["item_list"]["51001"] = json!(700);
    });
    open_activity(&service, "collect-item:1");
    open_activity(&service, "collect-item:13001");
    open_activity(&service, "advent:300001");
    assert_eq!(collect_progress(&service, viewer_id, 1, 1500), 0);
    assert_eq!(collect_progress(&service, viewer_id, 13001, 2180), 0);
    let mana = load(&service, viewer_id)["user_info"]["free_mana"].as_i64().unwrap();
    for (amount, progress) in [(20, 20), (30, 50)] {
        let created = manage(&service, "POST", "/v1/mails", json!({
            "viewer_id":viewer_id,"title":"任务物品","body":"收集物品.","sender":"Starpoint","rewards":{"itemList":{"80001":amount}},
        }));
        let request = json!({"viewer_id":viewer_id,"mail_id":created["id"]});
        decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", request.clone()));
        let repeated = decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", request));
        assert_eq!(repeated.data_headers.result_code, 2002);
        assert_eq!(collect_progress(&service, viewer_id, 1, 1500), progress);
    }
    assert_eq!(load(&service, viewer_id)["user_info"]["free_mana"], mana + 10_000);
    let before = load(&service, viewer_id)["item_list"]["4"].as_i64().unwrap_or_default();
    let purchased = decode_response::<Value>(&send(&service, "/api/index.php/shop/buy",
        json!({"viewer_id":viewer_id,"api_count":2,"shop_type":4,"shop_item_id":380080,"number":60}))).data;
    assert_eq!(purchased["item_list"]["51000"], 4000);
    assert_eq!(purchased["item_list"]["51001"], 1000);
    assert_eq!(purchased["item_list"]["4"], before + 50);
    assert_eq!(collect_progress(&service, viewer_id, 13001, 2180), 300);
    decode_response::<Value>(&send(&service, "/api/index.php/shop/buy",
        json!({"viewer_id":viewer_id,"api_count":3,"shop_type":4,"shop_item_id":380081,"number":1})));
    assert_eq!(collect_progress(&service, viewer_id, 13001, 2180), 300);
    assert_eq!(collect_progress(&service, viewer_id, 13001, 2190), 3);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(collect_progress(&service, viewer_id, 1, 1500), 50);
    assert_eq!(collect_progress(&service, viewer_id, 13001, 2180), 300);
    assert_eq!(load(&service, viewer_id)["item_list"]["4"], before + 50);
    service.stop().unwrap();
}
// //// /从实际邮件和兑换收获累计物品并保留已消费数量 ////

// //// 将战斗实际掉落计入对应活动并阻止结算重试重复累计 [@x380kkm 2026-09-21] ////
#[test]
fn counts_battle_drops_once_per_finished_battle() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 921113);
    open_activity(&service, "advent:100002");
    open_activity(&service, "collect-item:10005");
    let before = load(&service, viewer_id)["item_list"]["50300"].as_i64().unwrap_or_default();
    decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/start", json!({
        "viewer_id":viewer_id,"api_count":1,"category":7,"quest_id":100002003,"play_id":"collect-battle",
        "party_id":1,"use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false,
    })));
    let finish = json!({
        "viewer_id":viewer_id,"api_count":2,"category":7,"quest_id":100002003,"play_id":"collect-battle",
        "is_restored":false,"continue_count":0,"elapsed_time_ms":100000,"score":1000,"add_mana":0,"is_accomplished":true,
        "statistics":{"clear_phase":1,"zones":[{"use_power_flip_count":2,"use_dash_count":3,"use_skill_count":4}],
            "party":{"characters":[{"id":1},null,null],"unison_characters":[null,null,null],"equipments":[null,null,null],"ability_soul_ids":[null,null,null]}},
    });
    decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/finish", finish.clone()));
    let amount = load(&service, viewer_id)["item_list"]["50300"].as_i64().unwrap() - before;
    assert!(amount > 0);
    assert_eq!(collect_progress(&service, viewer_id, 10005, 1801), amount);
    decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/finish", finish));
    assert_eq!(load(&service, viewer_id)["item_list"]["50300"].as_i64().unwrap(), before + amount);
    assert_eq!(collect_progress(&service, viewer_id, 10005, 1801), amount);
    service.stop().unwrap();
}
// //// /将战斗实际掉落计入对应活动并阻止结算重试重复累计 ////
