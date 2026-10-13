// audience: internal
// # personal-service-cn-event-reward-tests
// 此文件验证团队战邮件到账, 称号奖励类型和活动参与轮次之间的领取隔离.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "support/cn_rewards.rs"]
#[allow(dead_code)]
mod cn_reward_support;
#[allow(dead_code)]
mod support;
#[path = "cn_event_rewards/entry.rs"]
mod entry;
#[path = "cn_event_rewards/resources.rs"]
mod resources;
#[path = "cn_event_rewards/rush_result.rs"]
mod rush_result;
#[path = "cn_event_rewards/local_raid.rs"]
mod local_raid;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use cn_reward_support::sum_drop_amounts;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 构造隔离存档和活动请求 [@x380kkm 2026-09-21] ////
fn send(service: &PersonalService, path: &str, body: Value) -> String {
    cn_support::send_request(service.port(), path, &encode_request(&body))
}

fn request(service: &PersonalService, path: &str, body: Value) -> Value {
    decode_response::<Value>(&send(service, path, body)).data
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&send(
        service,
        "/api/index.php/tool/signup",
        serde_json::to_value(SignupRequest { device_id: 781 }).unwrap(),
    ))
    .data_headers
    .viewer_id
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) {
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", &authorization)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn snapshot(root: &Path) -> Value {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let serialized: String = db
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&serialized).unwrap()
}

fn update_snapshot(root: &Path, apply: impl FnOnce(&mut Value)) {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let mut player = snapshot(root);
    apply(&mut player);
    db.execute(
        "UPDATE player_snapshots SET data_json=?1",
        params![player.to_string()],
    )
    .unwrap();
}

fn battle(
    service: &PersonalService,
    viewer: i64,
    category: i64,
    quest: i64,
    play: &str,
    accomplished: bool,
) -> Value {
    let started = decode_response::<Value>(&send(
        service,
        "/api/index.php/single_battle_quest/start",
        json!({
            "viewer_id": viewer, "quest_id": quest, "category": category, "play_id": play, "party_id": 1,
            "use_boss_boost_point": false, "use_boost_point": false, "is_auto_start_mode": false,
        }),
    ));
    assert_eq!(started.data_headers.result_code, 1);
    request(
        service,
        "/api/index.php/single_battle_quest/finish",
        finish_body(viewer, category, quest, play, accomplished),
    )
}

fn finish_body(viewer: i64, category: i64, quest: i64, play: &str, accomplished: bool) -> Value {
    json!({"viewer_id": viewer, "quest_id": quest, "category": category, "play_id": play,
        "elapsed_time_ms": 100_000, "score": 5000, "add_mana": 0, "is_accomplished": accomplished,
        "statistics": {"zones": [], "party": {"characters": [{"id": 111001}], "unison_characters": [], "equipments": [], "ability_soul_ids": []}}
    })
}
// //// /构造隔离存档和活动请求 ////

// //// 领取团队战总讨伐邮件并保持历史轮次和实际资产 [@x380kkm 2026-09-21] ////
#[test]
fn delivers_raid_kill_rewards_once_per_participation_round() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    manage(
        &service,
        "PUT",
        "/v1/activities/calendar/raid:1",
        json!({"enabled":true, "start_at":"2030-01-01T00:00:00.000Z", "end_at":"2030-01-02T00:00:00.000Z"}),
    );
    manage(
        &service,
        "PUT",
        "/v1/time",
        json!({"enabled":true, "iso":"2030-01-01T12:00:00.000Z", "rate":1}),
    );
    manage(
        &service,
        "PUT",
        "/v1/activities/raid-boss/1",
        json!({"hp_percentage": 37, "total_kill_count": 0}),
    );
    let cleared = battle(&service, viewer, 23, 1001, "raid-success", true);
    assert_eq!(cleared["raid_event"]["quest_boss"]["kill_count"], 1);
    assert_eq!(
        cleared["raid_event"]["raid_boss"],
        json!({"hp_percentage": 37, "total_kill_count": 1})
    );
    assert_eq!(
        request(
            &service,
            "/api/index.php/single_battle_quest/finish",
            finish_body(viewer, 23, 1001, "raid-success", true)
        ),
        cleared
    );
    let failed = battle(&service, viewer, 23, 1001, "raid-failed", false);
    assert_eq!(failed["raid_event"]["quest_boss"]["kill_count"], 1);
    assert_eq!(failed["raid_event"]["raid_boss"]["total_kill_count"], 1);
    manage(
        &service,
        "PUT",
        "/v1/time",
        json!({"enabled":true, "iso":"2030-01-03T00:00:00.000Z", "rate":1}),
    );
    let first = request(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"event_id": 1, "viewer_id": viewer}),
    );
    assert_eq!(first["quest_list"]["1001"]["kill_count"], 1);
    assert_eq!(first["kill_count_reward_data"]["received_up_to"], 1);
    assert_eq!(
        first["kill_count_reward_data"]["reward_list"],
        json!([
            {"kind": 1, "kind_id": 49100, "number": 5}, {"kind": 4, "kind_id": null, "number": 500}
        ])
    );
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let (mail_id, raw): (i64, String) = db
        .query_row("SELECT id,rewards_json FROM player_mails", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    let reward: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reward["freeVmoney"], 500);
    assert_eq!(reward["itemList"]["49100"], 5);
    let before = snapshot(root.path());
    request(
        &service,
        "/api/index.php/mail/receive",
        json!({"viewer_id": viewer, "mail_id": mail_id}),
    );
    let after = snapshot(root.path());
    assert_eq!(
        after["user_info"]["free_vmoney"].as_i64().unwrap()
            - before["user_info"]["free_vmoney"].as_i64().unwrap(),
        500
    );
    assert_eq!(
        after["item_list"]["49100"].as_i64().unwrap()
            - before["item_list"]["49100"].as_i64().unwrap_or(0),
        5
    );
    let repeat = request(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"event_id": 1, "viewer_id": viewer, "api_count": 19}),
    );
    assert_eq!(repeat["kill_count_reward_data"]["reward_list"], json!([]));
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let repeat = request(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"event_id": 1, "viewer_id": viewer}),
    );
    assert_eq!(repeat["kill_count_reward_data"]["reward_list"], json!([]));
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    update_snapshot(root.path(), |player| {
        player["cn_activity_state"]["event_families"]["raid"]["1"] =
            json!({"legacy_state_imported": true});
        player["activity_participation_rounds"] = json!({"raid:1": 1});
    });
    let assets = snapshot(root.path())["user_info"].clone();
    let reset = request(
        &service,
        "/api/index.php/event/raid/summary",
        json!({"event_id": 1, "viewer_id": viewer}),
    );
    assert_eq!(
        reset["kill_count_reward_data"]["reward_list"],
        first["kill_count_reward_data"]["reward_list"]
    );
    assert_eq!(snapshot(root.path())["user_info"], assets);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(db.query_row("SELECT COUNT(DISTINCT participation_round) FROM player_activity_history WHERE activity_id='raid:1' AND kind='delivery'", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    service.stop().unwrap();
}

// //// 按真实称号类型结算狂热激战并在统计后允许领奖 [@x380kkm 2026-09-21] ////
#[test]
fn grants_rush_degrees_after_aggregation_without_creating_items() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    update_snapshot(root.path(), |player| {
        player["cn_activity_state"] = json!({"event_families": {"rush": {"700001": {
            "rank_number": 2, "endless_battle_max_round": 2, "endless_battle_max_round_time": 5000,
        }}}});
    });
    manage(
        &service,
        "PUT",
        "/v1/activities/calendar/rush:700001",
        json!({"enabled":true, "start_at":"2030-01-01T00:00:00.000Z", "end_at":"2030-01-02T00:00:00.000Z"}),
    );
    manage(
        &service,
        "PUT",
        "/v1/time",
        json!({"enabled":true, "iso":"2030-01-03T00:00:00.000Z", "rate":1}),
    );
    let body = json!({"event_id":700001,"viewer_id":viewer,"api_count":1});
    assert!(
        send(&service, "/api/index.php/event/rush/reward", body.clone())
            .contains("activity_reward_not_ready")
    );
    manage(
        &service,
        "PUT",
        "/v1/time",
        json!({"enabled":true, "iso":"2030-01-05T00:00:01.000Z", "rate":1}),
    );
    let first = request(&service, "/api/index.php/event/rush/reward", body.clone());
    assert_eq!(first["ranking_reward"]["status"], 1);
    assert_eq!(
        first["ranking_reward"]["reward_list"],
        json!([{"kind":13,"kind_id":64000,"number":1}])
    );
    assert!(snapshot(root.path())["degree_ids"]
        .as_array()
        .unwrap()
        .contains(&json!(64000)));
    assert!(snapshot(root.path())["item_list"]["64000"].is_null());
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let repeat = request(
        &service,
        "/api/index.php/event/rush/reward",
        json!({"event_id":700001,"viewer_id":viewer,"api_count":95}),
    );
    assert_eq!(repeat["ranking_reward"]["status"], 2);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    update_snapshot(root.path(), |player| {
        player["cn_activity_state"]["event_families"]["rush"]["700001"] =
            json!({"rank_number": 4, "legacy_state_imported": true});
        player["activity_participation_rounds"] = json!({"rush:700001": 1});
    });
    let reset = request(&service, "/api/index.php/event/rush/reward", body);
    assert_eq!(reset["ranking_reward"]["status"], 1);
    let degrees = snapshot(root.path())["degree_ids"]
        .as_array()
        .unwrap()
        .clone();
    assert!(degrees.contains(&json!(64000)) && degrees.contains(&json!(64001)));
    service.stop().unwrap();
}

// //// 失败战斗保持狂热激战进度和关卡组奖励 [@x380kkm 2026-09-21] ////
#[test]
fn failed_rush_battles_do_not_advance_rounds_or_grant_folder_rewards() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    manage(&service, "PUT", "/v1/activities/rush:700006/mode", json!({"mode":"always"}));
    update_snapshot(root.path(), |player| player["user_info"]["stamina"] = json!(1000));
    let initial_coins = snapshot(root.path())["item_list"]["2370006"]
        .as_i64()
        .unwrap_or(0);
    let failed = battle(&service, viewer, 24, 700006004, "rush-folder-failed", false);
    assert!(failed["rush_event"].is_null());
    assert_eq!(
        snapshot(root.path())["item_list"]["2370006"]
            .as_i64()
            .unwrap_or(0),
        initial_coins
    );
    let cleared = battle(&service, viewer, 24, 700006004, "rush-folder-clear", true);
    assert_eq!(
        cleared["rush_event"]["rush_battle_reward_list"][0]["number"],
        150
    );
    let dropped_coins = sum_drop_amounts(&cleared);
    assert_eq!(cleared["drop_score_reward_ids"].as_array().unwrap().len(), 4);
    assert_eq!(snapshot(root.path())["item_list"]["2370006"], initial_coins + 150 + dropped_coins);
    let failed = battle(
        &service,
        viewer,
        24,
        700006008,
        "rush-endless-failed",
        false,
    );
    assert!(failed["rush_event"].is_null());
    let cleared = battle(&service, viewer, 24, 700006008, "rush-endless-clear", true);
    assert_eq!(cleared["rush_event"]["endless_battle_max_round"], 1);
    assert_eq!(cleared["rush_event"]["endless_battle_next_round"], 2);
    service.stop().unwrap();
}
