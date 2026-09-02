// audience: internal
// # personal-service-cn-single-battle-tests
//
// 该测试验证 CN 单机战斗在服务重启前后的开始, 继续, 结算, 掉落和终止行为.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

use cn_support::{
    assert_valid_signup_response, decode_response, encode_request, send_request,
    send_request_with_resource_version, LoadRequest, SignupData, SignupRequest,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 发送 CN 单机战斗流程请求 [@x380kkm 2026-08-23] ////
fn send_battle_request(port: u16, route: &str, body: &Value) -> String {
    send_request(
        port,
        &format!("/api/index.php/single_battle_quest/{route}"),
        &encode_request(body),
    )
}

fn start_battle(port: u16, viewer_id: i64) -> String {
    start_battle_with_boost(port, viewer_id, false)
}

fn start_battle_with_boost(port: u16, viewer_id: i64, use_boost_point: bool) -> String {
    start_battle_for(
        port,
        viewer_id,
        1,
        1_001_002,
        "cn-single-battle-test",
        use_boost_point,
    )
}

fn start_battle_for(
    port: u16,
    viewer_id: i64,
    category: i64,
    quest_id: i64,
    play_id: &str,
    use_boost_point: bool,
) -> String {
    send_battle_request(
        port,
        "start",
        &json!({
            "viewer_id": viewer_id,
            "api_count": 1,
            "quest_id": quest_id,
            "use_boss_boost_point": false,
            "use_boost_point": use_boost_point,
            "category": category,
            "play_id": play_id,
            "is_auto_start_mode": false,
            "party_id": 1,
        }),
    )
}

fn finish_battle(port: u16, viewer_id: i64) -> String {
    finish_battle_for(
        port,
        viewer_id,
        1,
        1_001_002,
        "cn-single-battle-test",
        100_000,
        1_000,
    )
}

fn finish_battle_for(
    port: u16,
    viewer_id: i64,
    category: i64,
    quest_id: i64,
    play_id: &str,
    elapsed_time_ms: i64,
    score: i64,
) -> String {
    send_battle_request(
        port,
        "finish",
        &json!({
            "viewer_id": viewer_id,
            "api_count": 1,
            "is_restored": false,
            "continue_count": 0,
            "elapsed_time_ms": elapsed_time_ms,
            "quest_id": quest_id,
            "play_id": play_id,
            "category": category,
            "score": score,
            "add_mana": 7,
            "is_accomplished": true,
            "statistics": {
                "clear_phase": 1,
                "zones": [{
                    "use_power_flip_count": 2,
                    "use_dash_count": 3,
                    "use_skill_count": 4
                }],
                "party": {
                    "characters": [{ "id": 1 }, null, null],
                    "unison_characters": [null, null, null],
                    "equipments": [null, null, null],
                    "ability_soul_ids": [null, null, null],
                },
            },
        }),
    )
}

fn continue_battle(port: u16, viewer_id: i64) -> String {
    continue_battle_with_request_state(port, viewer_id, 1, None)
}

fn continue_battle_with_request_state(
    port: u16,
    viewer_id: i64,
    api_count: i64,
    retry_count: Option<i64>,
) -> String {
    send_battle_request(
        port,
        "play_continue",
        &json!({
            "viewer_id": viewer_id,
            "api_count": api_count,
            "payment_type": 0,
            "quest_id": 1001002,
            "play_id": "cn-single-battle-test",
            "category": 1,
            "retry_count": retry_count,
        }),
    )
}

fn abort_battle(port: u16, viewer_id: i64, play_id: &str) -> String {
    send_battle_request(
        port,
        "abort",
        &json!({
            "viewer_id": viewer_id,
            "api_count": 1,
            "play_id": play_id,
        }),
    )
}

fn load_player(port: u16, viewer_id: i64) -> Value {
    let request = encode_request(&LoadRequest {
        keychain: viewer_id,
        viewer_id,
    });
    decode_response::<Value>(&send_request_with_resource_version(
        port,
        "/api/index.php/load",
        &request,
        "1.4.99-single-battle",
    ))
    .data
}

fn set_virtual_time(service: &PersonalService, iso: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let body = format!(r#"{{"enabled":true,"iso":"{iso}","rate":1.0}}"#);
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}

fn gameplay_settings_request(service: &PersonalService, method: &str, body: &[u8]) -> String {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    support::request_with_headers(
        service.port(),
        method,
        "/v1/gameplay-settings",
        "application/json",
        &authorization,
        body,
    )
}

fn decode_json_response(response: &str) -> Value {
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("Content-Type: application/json"));
    serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("JSON response has a body"),
    )
    .expect("response body is JSON")
}
// //// /发送 CN 单机战斗流程请求 ////

// //// 跨服务重启结算并持久化单机战斗 [@x380kkm 2026-07-22] ////
#[test]
fn persists_and_finishes_cn_single_battle() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    assert!(support::request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let signup_body = encode_request(&SignupRequest { device_id: 1 });
    let signup_response = send_request(service.port(), "/api/index.php/tool/signup", &signup_body);
    let signup = decode_response::<SignupData>(&signup_response);
    assert_valid_signup_response(&signup);
    let viewer_id = signup.data_headers.viewer_id;

    let start_response = start_battle(service.port(), viewer_id);
    let started = decode_response::<Value>(&start_response);
    assert_eq!(
        started.data["user_info"]["last_main_quest_id"].as_i64(),
        Some(1001002),
    );
    assert_eq!(started.data["user_info"]["stamina"].as_i64(), Some(4));
    assert_eq!(
        started.data["user_info"]["stamina_heal_time"],
        started.data_headers.servertime,
    );
    assert_eq!(started.data["category_id"].as_i64(), Some(1));
    service.stop().expect("service stops with an active battle");

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = load_player(service.port(), viewer_id);
    assert_eq!(
        loaded["unfinished_quest_list"],
        json!([{ "play_id": "cn-single-battle-test", "continue_count": 0 }]),
    );
    assert_eq!(loaded["unfinished_multi_quest_list"], json!([]));

    let continue_response = continue_battle(service.port(), viewer_id);
    let continued = decode_response::<Value>(&continue_response);
    assert_eq!(
        continued.data["user_info"]["free_vmoney"].as_i64(),
        Some(1450),
    );
    assert_eq!(continued.data["user_info"]["vmoney"].as_i64(), Some(0));
    let continue_count = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("battle database is opened")
        .query_row(
            "SELECT continue_count FROM active_single_quests",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("continue count is stored");
    assert_eq!(continue_count, 1);
    assert_eq!(
        load_player(service.port(), viewer_id)["unfinished_quest_list"],
        json!([{ "play_id": "cn-single-battle-test", "continue_count": 1 }]),
    );
    let finish_response = finish_battle(service.port(), viewer_id);
    let finished = decode_response::<Value>(&finish_response);
    assert_eq!(finished.data["clear_rank"].as_i64(), Some(5));
    assert_eq!(finished.data["before_rank_point"].as_i64(), Some(10));
    assert_eq!(finished.data["user_info"]["rank_point"].as_i64(), Some(13));
    assert_eq!(finished.data["user_info"]["free_mana"].as_i64(), Some(1027));
    assert_eq!(
        finished.data["user_info"]["free_vmoney"].as_i64(),
        Some(1450),
    );
    assert_eq!(
        finished.data["rewards"]["reward_pool_exp"].as_i64(),
        Some(13)
    );
    assert_eq!(finished.data["rewards"]["reward_mana"].as_i64(), Some(20));
    assert_eq!(finished.data["rewards"]["field_mana"].as_i64(), Some(7));
    assert_eq!(finished.data["item_list"]["13"].as_i64(), Some(1));
    assert_eq!(
        finished.data["drop_score_reward_ids"][0],
        json!({ "group_id": 40000, "index": 1, "number": 1 }),
    );
    let updated_character = finished.data["character_list"]
        .as_array()
        .and_then(|characters| {
            characters
                .iter()
                .find(|character| character["character_id"].as_i64() == Some(1))
        })
        .expect("default character receives battle EXP");
    assert_eq!(updated_character["exp"].as_i64(), Some(23));
    let repeated_finish = finish_battle(service.port(), viewer_id);
    assert!(repeated_finish.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(
        decode_response::<Value>(&repeated_finish).data,
        finished.data
    );
    service
        .stop()
        .expect("service stops after battle settlement");
    let service = PersonalService::start(root.path(), 0).expect("service restores finish receipt");
    let repeated_after_restart = finish_battle(service.port(), viewer_id);
    assert!(repeated_after_restart.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(
        decode_response::<Value>(&repeated_after_restart).data,
        finished.data
    );

    let load_request = encode_request(&LoadRequest {
        keychain: viewer_id,
        viewer_id,
    });
    let load_response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/load",
        &load_request,
        "1.4.99-single-battle",
    );
    let loaded = decode_response::<Value>(&load_response);
    assert_eq!(loaded.data["user_info"]["free_mana"].as_i64(), Some(1027));
    assert_eq!(loaded.data["user_info"]["free_vmoney"].as_i64(), Some(1450));
    assert_eq!(loaded.data["user_info"]["rank_point"].as_i64(), Some(13));
    assert_eq!(
        loaded.data["user_character_list"]["1"]["exp"].as_i64(),
        Some(23)
    );
    assert_eq!(loaded.data["item_list"]["13"].as_i64(), Some(1));
    assert_eq!(
        loaded.data["user_info"]["total_stamina_used"].as_i64(),
        Some(6)
    );
    assert_eq!(loaded.data["user_info"]["total_powerflips"], 2);
    assert_eq!(loaded.data["user_info"]["total_dashes"], 3);
    assert_eq!(loaded.data["user_info"]["total_skills"], 4);
    assert!(loaded.data["character_clear_counts"].is_null());
    assert!(loaded.data["character_leader_clear_counts"].is_null());
    assert!(loaded.data["character_multi_clear_counts"].is_null());
    assert!(loaded.data["character_leader_multi_clear_counts"].is_null());
    assert!(loaded.data["character_leader_power_flip_counts"].is_null());
    assert!(loaded.data["party_member_co_clear_counts"].is_null());
    assert!(loaded.data["party_race_clear_counts"].is_null());
    let stored_snapshot = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("battle database is opened")
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("player snapshot is stored");
    let stored_snapshot =
        serde_json::from_str::<Value>(&stored_snapshot).expect("player snapshot is JSON");
    assert_eq!(stored_snapshot["character_clear_counts"]["1"], 1);
    assert_eq!(stored_snapshot["character_leader_clear_counts"]["1"], 1);
    assert_eq!(
        stored_snapshot["character_leader_power_flip_counts"]["1"],
        2
    );
    assert_eq!(stored_snapshot["party_race_clear_counts"]["Human"], 1);
    assert!(stored_snapshot["character_multi_clear_counts"].is_null());
    assert!(stored_snapshot["character_leader_multi_clear_counts"].is_null());
    assert!(stored_snapshot["quest_progress"]["1"]
        .as_array()
        .expect("main quest progress is an array")
        .iter()
        .any(|progress| {
            progress["quest_id"] == 1_001_002 && progress["leader_character_id"] == 1
        }));
    let loaded_progress = loaded.data["quest_progress"]["1"]
        .as_array()
        .expect("main quest progress is an array")
        .iter()
        .find(|progress| progress["quest_id"] == 1_001_002)
        .expect("main quest progress is returned");
    assert!(loaded_progress
        .as_object()
        .is_some_and(|progress| !progress.contains_key("leader_character_id")));
    assert_eq!(loaded.data["unfinished_quest_list"], json!([]));
    assert_eq!(loaded.data["unfinished_multi_quest_list"], json!([]));

    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    assert!(start_battle_with_boost(service.port(), viewer_id, true).starts_with("HTTP/1.1 200 OK"));
    let boosted = decode_response::<Value>(&finish_battle(service.port(), viewer_id));
    assert_eq!(boosted.data["item_list"]["13"], 3);
    assert_eq!(
        boosted.data["drop_score_reward_ids"][0],
        json!({ "group_id": 40000, "index": 1, "number": 2 })
    );
    assert_eq!(boosted.data["user_info"]["boost_point"], 2);

    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    assert!(start_battle(service.port(), viewer_id).starts_with("HTTP/1.1 200 OK"));
    let abort_response = send_battle_request(
        service.port(),
        "abort",
        &json!({
            "viewer_id": viewer_id,
            "api_count": 1,
            "finish_kind": 1,
            "quest_id": 1001002,
            "play_id": "cn-single-battle-test",
            "category": 1,
            "statistics": {
                "clear_phase": 0,
                "party": {
                    "characters": [{ "id": 1 }, null, null],
                    "unison_characters": [null, null, null],
                    "equipments": [null, null, null],
                    "ability_soul_ids": [null, null, null],
                },
            },
        }),
    );
    assert!(abort_response.starts_with("HTTP/1.1 200 OK"));
    let aborted = decode_json_response(&abort_response);
    assert_eq!(aborted["data"]["category_id"].as_i64(), Some(1));
    let stale_abort = send_battle_request(
        service.port(),
        "abort",
        &json!({
            "viewer_id": viewer_id,
            "play_id": "stale-single-battle",
            "quest_id": 7001001,
            "category": 7,
        }),
    );
    assert!(stale_abort.starts_with("HTTP/1.1 200 OK"));
    let stale_abort = decode_json_response(&stale_abort);
    assert_eq!(stale_abort["data"]["category_id"], 7);
    assert!(finish_battle(service.port(), viewer_id).starts_with("HTTP/1.1 400 Bad Request"));
    assert_eq!(
        load_player(service.port(), viewer_id)["unfinished_quest_list"],
        json!([]),
    );
    service.stop().expect("service stops cleanly");
}
// //// /跨服务重启结算并持久化单机战斗 ////

#[test]
fn applies_configured_item_drop_multiplier_to_score_reward_items() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let default_settings = gameplay_settings_request(&service, "GET", b"");
    assert!(default_settings.starts_with("HTTP/1.1 200 OK"));
    assert!(default_settings.contains(r#""drop_multiplier":1"#));
    let updated_settings = gameplay_settings_request(&service, "PUT", br#"{"drop_multiplier":3}"#);
    assert!(updated_settings.starts_with("HTTP/1.1 200 OK"));
    assert!(updated_settings.contains(r#""drop_multiplier":3"#));
    let invalid_settings =
        gameplay_settings_request(&service, "PUT", br#"{"drop_multiplier":101}"#);
    assert!(invalid_settings.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops after settings update");
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let stored_settings = gameplay_settings_request(&service, "GET", b"");
    assert!(stored_settings.contains(r#""drop_multiplier":3"#));

    let signup = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 29 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    assert!(start_battle_with_boost(service.port(), viewer_id, true).starts_with("HTTP/1.1 200 OK"));
    let finished = decode_response::<Value>(&finish_battle(service.port(), viewer_id));
    assert_eq!(finished.data["item_list"]["13"], 6);
    assert_eq!(finished.data["drop_score_reward_ids"][0]["number"], 6);
    assert_eq!(finished.data["rewards"]["reward_pool_exp"], 13);
    assert_eq!(finished.data["rewards"]["reward_mana"], 20);
    assert_eq!(finished.data["rewards"]["field_mana"], 7);
    assert_eq!(load_player(service.port(), viewer_id)["item_list"]["13"], 6,);
}

// //// 验证挑战迷宫稀有掉落按倍率写入响应和存档 [@x380kkm 2026-08-26] ////
#[test]
fn applies_drop_multiplier_to_challenge_dungeon_rare_pool_items() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let updated_settings = gameplay_settings_request(&service, "PUT", br#"{"drop_multiplier":5}"#);
    assert!(updated_settings.starts_with("HTTP/1.1 200 OK"));

    let signup = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 30 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let before = load_player(service.port(), viewer_id);
    let item_ids = [
        (2_101, 42),
        (2_102, 43),
        (2_103, 44),
        (2_131, 99),
        (2_104, 45),
        (2_105, 45),
    ];

    assert!(start_battle_for(
        service.port(),
        viewer_id,
        13,
        2_001,
        "challenge-dungeon-drop-multiplier",
        false,
    )
    .starts_with("HTTP/1.1 200 OK"));
    let finished = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        13,
        2_001,
        "challenge-dungeon-drop-multiplier",
        100_000,
        1_000,
    ));
    let rare_drops = finished.data["drop_rare_reward_ids"]
        .as_array()
        .expect("challenge dungeon returns rare drops");
    assert_eq!(rare_drops.len(), 15);

    let mut expected_item_deltas = std::collections::BTreeMap::<i64, i64>::new();
    for drop in rare_drops {
        let group_id = drop["group_id"]
            .as_i64()
            .expect("rare drop group ID is present");
        let amount = drop["number"]
            .as_i64()
            .expect("rare drop amount is present");
        assert_eq!(amount % 5, 0);
        let item_id = item_ids
            .iter()
            .find_map(|(candidate_group_id, item_id)| {
                (*candidate_group_id == group_id).then_some(*item_id)
            })
            .expect("challenge dungeon rare group has an item mapping");
        *expected_item_deltas.entry(item_id).or_default() += amount;
    }

    let loaded = load_player(service.port(), viewer_id);
    for (item_id, expected_delta) in expected_item_deltas {
        let key = item_id.to_string();
        let before_count = before["item_list"][&key].as_i64().unwrap_or_default();
        let expected_total = before_count + expected_delta;
        assert_eq!(finished.data["item_list"][&key], expected_total);
        assert_eq!(loaded["item_list"][&key], expected_total);
    }
}
// //// /验证挑战迷宫稀有掉落按倍率写入响应和存档 ////

// //// 验证单机战斗状态变更按 play_id 重放 [@x380kkm 2026-08-23] ////
#[test]
fn replays_cn_single_battle_state_changes() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 5 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;

    let started = decode_response::<Value>(&start_battle(service.port(), viewer_id));
    let repeated_start = decode_response::<Value>(&start_battle(service.port(), viewer_id));
    assert_eq!(repeated_start.data, started.data);
    assert_eq!(
        load_player(service.port(), viewer_id)["user_info"]["stamina"],
        4,
    );

    decode_response::<Value>(&continue_battle_with_request_state(
        service.port(),
        viewer_id,
        1,
        None,
    ));
    let second_continue = decode_response::<Value>(&continue_battle_with_request_state(
        service.port(),
        viewer_id,
        2,
        None,
    ));
    let retried_continue = decode_response::<Value>(&continue_battle_with_request_state(
        service.port(),
        viewer_id,
        2,
        Some(1),
    ));
    assert_eq!(retried_continue.data, second_continue.data);
    assert_eq!(
        load_player(service.port(), viewer_id)["user_info"]["free_vmoney"],
        1400,
    );
    let continue_count = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("battle database is opened")
        .query_row(
            "SELECT continue_count FROM active_single_quests",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("continue count is stored");
    assert_eq!(continue_count, 2);

    let aborted = decode_json_response(&abort_battle(
        service.port(),
        viewer_id,
        "cn-single-battle-test",
    ));
    let repeated_abort = decode_json_response(&abort_battle(
        service.port(),
        viewer_id,
        "cn-single-battle-test",
    ));
    assert_eq!(repeated_abort["data"], aborted["data"]);
    assert_eq!(aborted["data"]["category_id"], 1);

    service.stop().expect("service stops after battle abort");
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let repeated_after_restart = decode_json_response(&abort_battle(
        service.port(),
        viewer_id,
        "cn-single-battle-test",
    ));
    assert_eq!(repeated_after_restart["data"], aborted["data"]);
    service.stop().expect("service stops cleanly");
}
// //// /验证单机战斗状态变更按 play_id 重放 ////

// //// 验证活动类别结算字段和状态恢复 [@x380kkm 2026-08-22] ////
#[test]
fn settles_carnival_raid_rush_and_score_attack_quests() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let settings = gameplay_settings_request(&service, "PUT", br#"{"drop_multiplier":3}"#);
    assert!(settings.starts_with("HTTP/1.1 200 OK"));
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let viewer_id = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 4 }),
    ))
    .data_headers
    .viewer_id;

    assert!(
        start_battle_for(service.port(), viewer_id, 22, 2_001, "carnival", false)
            .starts_with("HTTP/1.1 200 OK")
    );
    let carnival = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        22,
        2_001,
        "carnival",
        100_000,
        5_000,
    ));
    assert_eq!(carnival.data["category_id"], 22);
    assert_eq!(
        carnival.data["carnival_event"]["score"]["difficulty_bonus"],
        2_000
    );
    assert_eq!(
        carnival.data["carnival_event"]["score"]["time_bonus"],
        8_000
    );

    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    assert!(
        start_battle_for(service.port(), viewer_id, 23, 1_001, "raid", false)
            .starts_with("HTTP/1.1 200 OK")
    );
    let raid = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        23,
        1_001,
        "raid",
        60_000,
        1_000,
    ));
    assert_eq!(raid.data["category_id"], 23);
    assert!(raid.data["rush_event"].is_null());

    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    assert!(start_battle_for(
        service.port(),
        viewer_id,
        24,
        700_006_004,
        "rush-folder",
        false,
    )
    .starts_with("HTTP/1.1 200 OK"));
    let rush_folder = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        24,
        700_006_004,
        "rush-folder",
        70_000,
        0,
    ));
    assert_eq!(
        rush_folder.data["rush_event"]["rush_battle_reward_list"][0]["number"],
        450
    );
    assert_eq!(rush_folder.data["item_list"]["2370006"], 450);

    assert!(
        start_battle_for(service.port(), viewer_id, 24, 700_006_008, "rush", false)
            .starts_with("HTTP/1.1 200 OK")
    );
    let rush = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        24,
        700_006_008,
        "rush",
        70_000,
        0,
    ));
    assert_eq!(rush.data["rush_event"]["endless_battle_next_round"], 2);
    assert_eq!(rush.data["rush_event"]["endless_battle_max_round"], 1);
    assert_eq!(rush.data["rush_event"]["high_score"], 70_000);

    set_virtual_time(&service, "2030-01-04T12:00:00.000Z");
    assert!(start_battle_for(
        service.port(),
        viewer_id,
        27,
        1_001,
        "score-below-border",
        false,
    )
    .starts_with("HTTP/1.1 200 OK"));
    let below_border = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        27,
        1_001,
        "score-below-border",
        30_000,
        1_000_000,
    ));
    assert!(below_border.data["item_list"]["40501"].is_null());
    assert!(load_player(service.port(), viewer_id)["quest_progress"]["27"].is_null());

    set_virtual_time(&service, "2030-01-05T12:00:00.000Z");
    assert!(
        start_battle_for(service.port(), viewer_id, 27, 1_001, "score", false)
            .starts_with("HTTP/1.1 200 OK")
    );
    let score_attack = decode_response::<Value>(&finish_battle_for(
        service.port(),
        viewer_id,
        27,
        1_001,
        "score",
        30_000,
        1_000_000_000,
    ));
    assert_eq!(score_attack.data["category_id"], 27);
    assert!(score_attack.data["drop_additional_reward_ids"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(score_attack.data["drop_periodic_reward_ids"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(score_attack.data["item_list"]["40501"], 9);
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = decode_response::<Value>(&send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let carnival_state = decode_response::<Value>(&send_request(
        restarted.port(),
        "/api/index.php/carnival_event/index",
        &encode_request(&json!({ "viewer_id": viewer_id, "event_id": 2 })),
    ));
    assert!(carnival_state.data["records"]
        .as_array()
        .is_some_and(|records| records
            .iter()
            .any(|record| { record["folder_id"] == 1 && record["best_score"] == 10_000 })));
    let raid_state = decode_response::<Value>(&send_request(
        restarted.port(),
        "/api/index.php/event/raid/summary",
        &encode_request(&json!({ "viewer_id": viewer_id, "event_id": 1 })),
    ));
    assert_eq!(
        raid_state.data["rush_battle_played_party_list"]["1001"]["character_id_1"],
        1
    );
    let rush_state = decode_response::<Value>(&send_request(
        restarted.port(),
        "/api/index.php/event/rush/summary",
        &encode_request(&json!({ "viewer_id": viewer_id, "event_id": 700_006 })),
    ));
    assert_eq!(rush_state.data["endless_battle_next_round"], 2);
    for category in [22, 23, 24, 27] {
        assert!(loaded.data["quest_progress"][category.to_string()]
            .as_array()
            .is_some_and(|quests| !quests.is_empty()));
    }
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证活动类别结算字段和状态恢复 ////

// //// 验证实际任务角色奖励和重复素材 [@x380kkm 2026-08-23] ////
#[test]
fn grants_character_and_duplicate_material_from_distinct_quests() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 5 }),
    ))
    .data_headers
    .viewer_id;
    let quests = [100_411_012, 200_105_012];

    assert!(start_battle_for(
        service.port(),
        viewer_id,
        18,
        quests[0],
        "character-a",
        false
    )
    .starts_with("HTTP/1.1 200 OK"));
    let first_response = finish_battle_for(
        service.port(),
        viewer_id,
        18,
        quests[0],
        "character-a",
        1,
        0,
    );
    assert!(
        first_response.starts_with("HTTP/1.1 200 OK"),
        "{first_response}"
    );
    let first = decode_response::<Value>(&first_response);
    assert_eq!(first.data["joined_character_id_list"], json!([253013]));

    assert!(start_battle_for(
        service.port(),
        viewer_id,
        18,
        quests[1],
        "character-b",
        false
    )
    .starts_with("HTTP/1.1 200 OK"));
    let second_response = finish_battle_for(
        service.port(),
        viewer_id,
        18,
        quests[1],
        "character-b",
        1,
        0,
    );
    assert!(
        second_response.starts_with("HTTP/1.1 200 OK"),
        "{second_response}"
    );
    let second = decode_response::<Value>(&second_response);
    assert!(second.data["joined_character_id_list"]
        .as_array()
        .unwrap()
        .is_empty());
    let duplicate = second.data["character_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|character| character["character_id"] == 253013)
        .unwrap();
    assert_eq!(duplicate["stack"], 1);
    assert_eq!(second.data["item_list"]["14017"], 1);
    let loaded = decode_response::<Value>(&send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(
        loaded.data["user_character_list"]["253013"]["entry_count"],
        1
    );
    assert_eq!(loaded.data["user_character_list"]["253013"]["stack"], 1);
    assert_eq!(loaded.data["item_list"]["14017"], 1);
    service.stop().expect("service stops cleanly");
}
// //// /验证实际任务角色奖励和重复素材 ////

// //// 验证单机战斗使用 viewer 绑定的活动记录 [@x380kkm 2026-08-22] ////
#[test]
fn uses_viewer_bound_battle_for_finish_and_abort() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let signup_response = send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 2 }),
    );
    let signup = decode_response::<SignupData>(&signup_response);
    assert_valid_signup_response(&signup);
    let viewer_id = signup.data_headers.viewer_id;
    let other_signup = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 3 }),
    ));
    let other_viewer_id = other_signup.data_headers.viewer_id;
    assert!(start_battle(service.port(), viewer_id).starts_with("HTTP/1.1 200 OK"));

    let wrong_continue = send_battle_request(
        service.port(),
        "play_continue",
        &json!({
            "viewer_id": viewer_id,
            "api_count": 1,
            "payment_type": 0,
            "quest_id": 1001002,
            "paly_id": "wrong-play-id",
            "category": 1,
        }),
    );
    assert!(wrong_continue.starts_with("HTTP/1.1 409 Conflict"));

    let wrong_finish = send_battle_request(
        service.port(),
        "finish",
        &json!({
            "viewer_id": viewer_id,
            "api_count": 1,
            "is_restored": false,
            "continue_count": 0,
            "elapsed_time_ms": 100000,
            "quest_id": 1001002,
            "play_id": "wrong-play-id",
            "category": 1,
            "score": 1000,
            "add_mana": 0,
            "is_accomplished": true,
            "statistics": {
                "clear_phase": 1,
                "party": {
                    "characters": [{ "id": 1 }, null, null],
                    "unison_characters": [null, null, null],
                },
            },
        }),
    );
    assert!(wrong_finish.starts_with("HTTP/1.1 409 Conflict"));
    assert!(finish_battle(service.port(), viewer_id).starts_with("HTTP/1.1 200 OK"));

    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    assert!(start_battle(service.port(), viewer_id).starts_with("HTTP/1.1 200 OK"));
    assert!(finish_battle(service.port(), other_viewer_id).starts_with("HTTP/1.1 400 Bad Request"));
    assert!(finish_battle(service.port(), viewer_id).starts_with("HTTP/1.1 200 OK"));

    set_virtual_time(&service, "2030-01-03T12:00:00.000Z");
    assert!(start_battle(service.port(), viewer_id).starts_with("HTTP/1.1 200 OK"));
    let wrong_abort = send_battle_request(
        service.port(),
        "abort",
        &json!({
            "viewer_id": viewer_id,
            "api_count": 1,
            "finish_kind": 1,
            "quest_id": 1001002,
            "play_id": "wrong-play-id",
            "category": 1,
        }),
    );
    assert!(wrong_abort.starts_with("HTTP/1.1 200 OK"));
    assert!(finish_battle(service.port(), viewer_id).starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");
}
// //// /验证单机战斗使用 viewer 绑定的活动记录 ////

// //// 验证单机战斗恢复字段和身份状态清理 [@x380kkm 2026-08-18] ////
#[test]
fn creates_battle_recovery_fields_and_rejects_unidentified_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let database_path = root.path().join("personal-service.sqlite3");
    let database = Connection::open(&database_path).expect("legacy database is opened");
    database
        .execute_batch(
            "CREATE TABLE active_single_quests (
                 account_id INTEGER PRIMARY KEY,
                 quest_id INTEGER NOT NULL,
                 category INTEGER NOT NULL,
                 use_boss_boost_point INTEGER NOT NULL,
                 use_boost_point INTEGER NOT NULL,
                 is_auto_start_mode INTEGER NOT NULL
             );
             INSERT INTO active_single_quests (
                 account_id, quest_id, category, use_boss_boost_point,
                 use_boost_point, is_auto_start_mode
             ) VALUES (1, 1001002, 1, 0, 0, 0);
             CREATE TABLE single_battle_finish_receipts (
                 account_id INTEGER PRIMARY KEY,
                 category INTEGER NOT NULL,
                 quest_id INTEGER NOT NULL,
                 response_json TEXT NOT NULL
             );",
        )
        .expect("legacy active battle is created");
    drop(database);

    let service = PersonalService::start(root.path(), 0).expect("service migrates legacy battle");
    service.stop().expect("service stops cleanly");

    let database = Connection::open(database_path).expect("migrated database is opened");
    let columns = database
        .prepare("PRAGMA table_info(active_single_quests)")
        .expect("active battle columns are read")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("active battle column query succeeds")
        .collect::<Result<Vec<_>, _>>()
        .expect("active battle columns are collected");
    assert!(columns.iter().any(|column| column == "play_id"));
    assert!(columns.iter().any(|column| column == "continue_count"));
    let receipt_columns = database
        .prepare("PRAGMA table_info(single_battle_finish_receipts)")
        .expect("finish receipt columns are read")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("finish receipt column query succeeds")
        .collect::<Result<Vec<_>, _>>()
        .expect("finish receipt columns are collected");
    assert!(receipt_columns.iter().any(|column| column == "play_id"));
    let active_battle_count = database
        .query_row("SELECT COUNT(*) FROM active_single_quests", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("active battle count is read");
    assert_eq!(active_battle_count, 0);
}
// //// /验证单机战斗恢复字段和身份状态清理 ////
