// audience: internal
// # personal-service-additional-battle-rewards-api-tests
// 单人和多人追加掉落复用入场资格, 奖励收据同时保存客户端索引和最终库存.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 通过战斗和管理接口读写隔离存档 [@x380kkm 2026-09-28] ////
fn api(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{route}: {response}");
    decode_response::<Value>(&response).data
}

fn signup(service: &PersonalService) -> i64 {
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&json!({"device_id":928881})),
    );
    decode_response::<SignupData>(&response)
        .data_headers
        .viewer_id
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", token.as_str())],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{path}: {response}");
}

fn time(service: &PersonalService, iso: &str) {
    manage(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"iso":iso,"rate":1}),
    );
}

fn snapshot(database: &Connection) -> Value {
    let encoded: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&encoded).unwrap()
}

fn prepare_player(database: &Connection, unlocked: bool) {
    let mut player = snapshot(database);
    player["user_info"]["stamina"] = json!(10_000);
    let progress = player["quest_progress"]["1"].as_array_mut().unwrap();
    progress.retain(|entry| entry["quest_id"] != 1004002);
    progress.push(json!({"quest_id":1004002,"finished":unlocked,"clear_rank":5,"high_score":1000}));
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
}

fn battle_request(viewer: i64, category: i64, quest: i64, play: &str) -> Value {
    json!({
        "viewer_id":viewer,"api_count":1,"category":category,"quest_id":quest,"play_id":play,"party_id":1,
        "use_boss_boost_point":false,"use_boost_point":false,"is_auto_start_mode":false,
        "is_accomplished":true,"elapsed_time_ms":1000,"score":1000,"add_mana":0,
        "statistics":{"clear_phase":1,"zones":[],"party":{"characters":[{"id":1},null,null],
            "unison_characters":[null,null,null],"equipments":[null,null,null],"ability_soul_ids":[null,null,null]}}
    })
}
// //// /通过战斗和管理接口读写隔离存档 ////

// //// 在关闭活动和重启服务后重放塔罗斯追加奖励收据 [@x380kkm 2026-09-28] ////
#[test]
fn preserves_started_additional_rewards_across_closure_and_restart() {
    let storage = TempDir::new().unwrap();
    let service = PersonalService::start(storage.path(), 0).unwrap();
    time(&service, "2030-01-01T12:00:00Z");
    let viewer = signup(&service);
    let database = Connection::open(storage.path().join("personal-service.sqlite3")).unwrap();
    prepare_player(&database, true);
    for activity in ["world-story:400016", "collect-item:100008"] {
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/{activity}/mode"),
            json!({"mode":"always"}),
        );
    }
    manage(
        &service,
        "PUT",
        "/v1/gameplay-settings",
        json!({"drop_multiplier":3}),
    );
    let request = battle_request(viewer, 18, 400016102, "additional-talos");
    let started = api(&service, "single_battle_quest/start", &request);
    let before = snapshot(&database);
    manage(
        &service,
        "POST",
        "/v1/activities/collect-item%3A100008/close",
        json!({}),
    );
    time(&service, "2030-01-02T12:00:00Z");
    let result = api(&service, "single_battle_quest/finish", &request);
    let saved = snapshot(&database);
    assert_eq!(result["start_time"], started["start_time"]);
    assert_eq!(result["drop_score_reward_ids"], json!([]));
    assert_eq!(result["drop_rare_reward_ids"], json!([]));
    assert_eq!(
        result["drop_additional_reward_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["group_id"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![10000038, 10000041, 10000046, 10000049, 10000051, 10000050, 10000052]
    );
    for (item, count) in [("10000053", 4500), ("10000054", 300)] {
        let expected = before["item_list"][item].as_i64().unwrap_or_default() + count;
        assert_eq!(saved["item_list"][item], expected);
        assert_eq!(result["item_list"][item], expected);
    }
    assert_eq!(
        api(&service, "single_battle_quest/finish", &request),
        result
    );
    assert_eq!(snapshot(&database), saved);
    service.stop().unwrap();
    let service = PersonalService::start(storage.path(), 0).unwrap();
    assert_eq!(
        api(&service, "single_battle_quest/finish", &request),
        result
    );
    assert_eq!(snapshot(&database), saved);
    let next = battle_request(viewer, 18, 400016102, "additional-closed");
    api(&service, "single_battle_quest/start", &next);
    let closed = api(&service, "single_battle_quest/finish", &next);
    assert_eq!(closed["drop_additional_reward_ids"], json!([]));
}

// //// 在解锁条件和战斗结果限制内选择追加掉落 [@x380kkm 2026-09-28] ////
#[test]
fn requires_unlocked_event_matching_quest_and_reward_eligibility() {
    let storage = TempDir::new().unwrap();
    let service = PersonalService::start(storage.path(), 0).unwrap();
    time(&service, "2030-01-01T12:00:00Z");
    let viewer = signup(&service);
    let database = Connection::open(storage.path().join("personal-service.sqlite3")).unwrap();
    manage(
        &service,
        "PUT",
        "/v1/activities/world-story:400016/mode",
        json!({"mode":"always"}),
    );
    manage(
        &service,
        "PUT",
        "/v1/activities/collect-item:100008/mode",
        json!({"mode":"always"}),
    );
    for (play, unlocked, category, quest, accomplished) in [
        ("additional-locked", false, 18, 400016102, true),
        ("additional-failed", true, 18, 400016102, false),
        ("additional-outside-range", true, 1, 1001002, true),
    ] {
        prepare_player(&database, unlocked);
        let mut request = battle_request(viewer, category, quest, play);
        request["is_accomplished"] = json!(accomplished);
        api(&service, "single_battle_quest/start", &request);
        let before = snapshot(&database)["item_list"].clone();
        let result = api(&service, "single_battle_quest/finish", &request);
        assert_eq!(result["drop_additional_reward_ids"], json!([]), "{play}");
        let saved = snapshot(&database);
        for item in ["10000053", "10000054"] {
            assert_eq!(saved["item_list"][item], before[item], "{play}:{item}");
        }
    }
}

// //// 将多人等级追加掉落计入同一收集任务和重放收据 [@x380kkm 2026-09-28] ////
#[test]
fn grants_multiplayer_level_rewards_and_collect_mission_progress() {
    let storage = TempDir::new().unwrap();
    let service = PersonalService::start(storage.path(), 0).unwrap();
    time(&service, "2030-01-01T12:00:00Z");
    let viewer = signup(&service);
    let database = Connection::open(storage.path().join("personal-service.sqlite3")).unwrap();
    prepare_player(&database, true);
    manage(
        &service,
        "PUT",
        "/v1/activities/collect-item:1/mode",
        json!({"mode":"always"}),
    );
    let room = api(
        &service,
        "multi_battle_quest/create_room",
        &json!({"viewer_id":viewer,"category":2,"quest_id":1002001,"party_id":1}),
    );
    let mut request = battle_request(viewer, 2, 1002001, "additional-multi");
    request["room_number"] = room["room_number"].clone();
    let started = api(&service, "multi_battle_quest/start", &request);
    let before = snapshot(&database);
    manage(
        &service,
        "POST",
        "/v1/activities/collect-item%3A1/close",
        json!({}),
    );
    time(&service, "2030-01-02T12:00:00Z");
    let result = api(&service, "multi_battle_quest/finish", &request);
    assert_eq!(result["start_time"], started["start_time"]);
    assert_eq!(
        result["drop_additional_reward_ids"],
        json!([
            {"group_id":200007,"index":1,"number":100},
            {"group_id":200006,"index":1,"number":200}
        ])
    );
    let saved = snapshot(&database);
    let expected = before["item_list"]["80001"].as_i64().unwrap_or_default() + 300;
    assert_eq!(result["item_list"]["80001"], expected);
    assert_eq!(saved["item_list"]["80001"], expected);
    let progress = saved["mission_window_progress"].as_object().unwrap();
    assert!(progress
        .iter()
        .any(|(key, count)| key.starts_with("4:1500:event:") && *count == 300));
    assert_eq!(api(&service, "multi_battle_quest/finish", &request), result);
    assert_eq!(snapshot(&database), saved);
}
