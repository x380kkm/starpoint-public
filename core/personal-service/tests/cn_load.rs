// audience: internal
// # personal-service-cn-load-tests
//
// 该测试验证 CN 玩家快照的载入, 状态规范化, 隔离和时间推进.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

use cn_support::{
    assert_valid_signup_response, decode_response, encode_request, send_request,
    send_request_with_resource_version, Envelope, LoadRequest, SignupData, SignupRequest,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

static RESOURCE_VERSION_ENV: Mutex<()> = Mutex::new(());

// //// 操作 CN 测试账号和玩家快照 [@x380kkm 2026-07-22] ////
fn signup(port: u16, device_id: i64) -> Envelope<SignupData> {
    let body = encode_request(&SignupRequest { device_id });
    let response = send_request(port, "/api/index.php/tool/signup", &body);
    let signup = decode_response(&response);
    assert_valid_signup_response(&signup);
    signup
}

fn load_with_resource_version(
    port: u16,
    viewer_id: i64,
    resource_version: &str,
) -> Envelope<Value> {
    let body = encode_request(&LoadRequest {
        keychain: viewer_id,
        viewer_id,
    });
    let response =
        send_request_with_resource_version(port, "/api/index.php/load", &body, resource_version);
    decode_response(&response)
}

fn load_without_resource_version(port: u16, viewer_id: i64) -> Envelope<Value> {
    let body = encode_request(&LoadRequest {
        keychain: viewer_id,
        viewer_id,
    });
    let response = send_request(port, "/api/index.php/load", &body);
    decode_response(&response)
}

// //// 发送带资源平台头的 CN 载入请求 [@x380kkm 2026-08-29] ////
fn load_with_resource_version_and_device(
    port: u16,
    viewer_id: i64,
    resource_version: &str,
    device_kind: &str,
) -> Envelope<Value> {
    let body = encode_request(&LoadRequest {
        keychain: viewer_id,
        viewer_id,
    });
    let response = support::request_with_headers(
        port,
        "POST",
        "/api/index.php/load",
        "application/x-www-form-urlencoded",
        &[("res_ver", resource_version), ("DEVICE", device_kind)],
        body.as_bytes(),
    );
    decode_response(&response)
}
// //// /发送带资源平台头的 CN 载入请求 ////

fn update_only_player_snapshot(root: &Path, update: impl FnOnce(&mut Value)) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let (account_id, serialized) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("one player snapshot exists");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    update(&mut player_data);
    let serialized = serde_json::to_string(&player_data).expect("player snapshot is encoded");
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![serialized, account_id],
        )
        .expect("player snapshot is updated");
}

fn read_only_player_snapshot(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let serialized = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("one player snapshot exists");
    serde_json::from_str(&serialized).expect("player snapshot is JSON")
}

// //// 写入 CN iOS 资源覆盖归档 [@x380kkm 2026-08-29] ////
fn write_ios_voice_override_archive(root: &Path) {
    let path = root
        .join("cdn")
        .join("override")
        .join("archive-ios-diff")
        .join("starpoint-cn-voice-overlay-ios.zip");
    fs::create_dir_all(path.parent().expect("voice archive has a parent directory"))
        .expect("voice archive directory is created");
    fs::write(path, b"voice overlay").expect("voice archive is written");
}
// //// /写入 CN iOS 资源覆盖归档 ////

// //// 写入用于资源版本测试的 CN path 清单 [@x380kkm 2026-08-29] ////
fn write_asset_path_manifest(root: &Path, version: &str) {
    let path = root.join("cdn").join("cn").join("path");
    fs::create_dir_all(path.parent().expect("path manifest has a parent directory"))
        .expect("path manifest directory is created");
    let document = json!({
        "info": {
            "client_asset_version": version,
            "target_asset_version": version,
            "eventual_target_asset_version": version,
            "is_initial": true,
            "latest_maj_first_version": "1.4.0"
        },
        "full": {"version": "1.4.0", "archive": []},
        "diff": [],
        "asset_version_hash": "fixture-hash"
    });
    fs::write(
        path,
        serde_json::to_vec(&document).expect("path manifest is encoded"),
    )
    .expect("path manifest is written");
}
// //// /写入用于资源版本测试的 CN path 清单 ////
// //// /操作 CN 测试账号和玩家快照 ////

// //// 设置载入测试的 UTC 虚拟日期 [@x380kkm 2026-08-23] ////
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
// //// /设置载入测试的 UTC 虚拟日期 ////

// //// 返回空的未完成关卡列表 [@x380kkm 2026-08-21] ////
#[test]
fn returns_empty_unfinished_quest_lists() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 1).data_headers.viewer_id;
    let loaded = load_with_resource_version(service.port(), viewer_id, "1.4.99-quest-list");

    assert_eq!(loaded.data["unfinished_quest_list"], serde_json::json!([]));
    assert_eq!(
        loaded.data["unfinished_multi_quest_list"],
        serde_json::json!([]),
    );
    service.stop().expect("service stops cleanly");
}
// //// /返回空的未完成关卡列表 ////

// //// 载入时修复缺失的实例关联标识 [@x380kkm 2026-09-01] ////
#[test]
fn repairs_missing_associate_token_in_loaded_snapshot() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 23).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    update_only_player_snapshot(root.path(), |player_data| {
        player_data["associate_token"] = Value::Null;
    });

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = load_with_resource_version(service.port(), viewer_id, "1.4.99-associate-token");
    assert_eq!(
        loaded.data["associate_token"].as_str(),
        Some("associate_token"),
    );
    service.stop().expect("service stops cleanly");

    assert_eq!(
        read_only_player_snapshot(root.path())["associate_token"].as_str(),
        Some("associate_token"),
    );
}
// //// /载入时修复缺失的实例关联标识 ////

// //// 仅在载入响应补齐每日周常随机表 [@x380kkm 2026-08-27] ////
#[test]
fn completes_daily_week_drawn_quests_without_changing_the_snapshot() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 2).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    update_only_player_snapshot(root.path(), |player_data| {
        let drawn_quests = player_data["drawn_quest_list"]
            .as_array_mut()
            .expect("drawn quests are an array");
        drawn_quests.retain(|quest| {
            if quest["category_id"].as_i64() != Some(6) {
                return true;
            }
            let quest_id = quest["quest_id"].as_i64().unwrap_or_default();
            (5_001..=5_005).contains(&quest_id) || (13_001..=19_018).contains(&quest_id)
        });
        let existing = drawn_quests
            .iter_mut()
            .find(|quest| quest["category_id"] == 6 && quest["quest_id"] == 5_001)
            .expect("legacy daily week quest is retained");
        existing["odds_id"] = Value::from(8);
        assert_eq!(
            drawn_quests
                .iter()
                .filter(|quest| quest["category_id"] == 6)
                .count(),
            59,
        );
    });
    let snapshot_before_load = read_only_player_snapshot(root.path());
    let stored_drawn_quests = snapshot_before_load["drawn_quest_list"].clone();
    let stored_exp_mana_quests = stored_drawn_quests
        .as_array()
        .expect("stored drawn quests are an array")
        .iter()
        .filter(|quest| quest["category_id"] == 14)
        .cloned()
        .collect::<Vec<_>>();

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = load_with_resource_version(service.port(), viewer_id, "1.4.99-daily-week");
    let response_drawn_quests = loaded.data["drawn_quest_list"]
        .as_array()
        .expect("response drawn quests are an array");
    assert_eq!(
        response_drawn_quests
            .iter()
            .filter(|quest| quest["category_id"] == 6)
            .count(),
        114,
    );
    assert_eq!(
        response_drawn_quests
            .iter()
            .find(|quest| quest["category_id"] == 6 && quest["quest_id"] == 5_001)
            .and_then(|quest| quest["odds_id"].as_i64()),
        Some(8),
    );
    assert_eq!(
        response_drawn_quests
            .iter()
            .find(|quest| quest["category_id"] == 6 && quest["quest_id"] == 1_001)
            .and_then(|quest| quest["odds_id"].as_i64()),
        Some(3),
    );
    assert_eq!(
        response_drawn_quests
            .iter()
            .filter(|quest| quest["category_id"] == 14)
            .cloned()
            .collect::<Vec<_>>(),
        stored_exp_mana_quests,
    );
    service.stop().expect("service stops cleanly");

    let snapshot_after_load = read_only_player_snapshot(root.path());
    assert_eq!(snapshot_after_load["drawn_quest_list"], stored_drawn_quests);
}
// //// /仅在载入响应补齐每日周常随机表 ////

// //// 仅在载入响应过滤未知主动任务 [@x380kkm 2026-08-28] ////
#[test]
fn filters_unknown_active_missions_without_changing_the_snapshot() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 24).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    update_only_player_snapshot(root.path(), |player_data| {
        player_data["all_active_mission_list"] = json!({
            "11010": {"progress": 1, "stages": {"1": true}},
            "999999": {"progress": 7, "stages": {"1": false}},
        });
    });
    let stored_missions = read_only_player_snapshot(root.path())["all_active_mission_list"].clone();

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded =
        load_with_resource_version(service.port(), viewer_id, "1.4.99-active-mission-filter");
    assert_eq!(
        loaded.data["all_active_mission_list"],
        json!({"11010": {"progress": 1, "stages": {"1": true}}})
    );
    service.stop().expect("service stops cleanly");

    assert_eq!(
        read_only_player_snapshot(root.path())["all_active_mission_list"],
        stored_missions
    );
}
// //// /仅在载入响应过滤未知主动任务 ////

// //// 仅在载入响应投影当前可见的 Mana board [@x380kkm 2026-08-28] ////
#[test]
fn projects_time_visible_mana_boards_without_changing_the_snapshot() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 25).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    let stored_nodes = json!([
        {"mana_node_multiplied_id": 2_201, "awake_level": 0},
        2_202,
        {"multiplied_id": 2_203, "awake_level": 1},
        {"mana_node_multiplied_id": 2_401, "awake_level": 0},
        {"mana_node_multiplied_id": 222_004_201, "awake_level": 0},
    ]);
    update_only_player_snapshot(root.path(), |player_data| {
        player_data["user_character_list"]["1"]["mana_board_index"] = Value::from(2);
        player_data["user_character_mana_node_list"]["1"] = stored_nodes.clone();
    });

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    set_virtual_time(&service, "2014-03-01T00:00:00.000Z");
    let locked = load_with_resource_version(service.port(), viewer_id, "1.4.99-mana-locked");
    assert_eq!(
        locked.data["user_character_list"]["1"]["mana_board_index"],
        1
    );
    assert_eq!(
        locked.data["user_character_mana_node_list"]["1"],
        json!([
            {"multiplied_id": 2_201, "awake_level": 0},
            {"multiplied_id": 2_202, "awake_level": 0},
            {"multiplied_id": 2_203, "awake_level": 1},
        ])
    );
    assert!(locked.data["user_character_mana_node_list"]["1"]
        .as_array()
        .is_some_and(|nodes| {
            nodes.iter().all(|node| {
                node.is_object()
                    && node.get("multiplied_id").and_then(Value::as_i64).is_some()
                    && node.get("awake_level").and_then(Value::as_i64).is_some()
                    && node.get("mana_node_multiplied_id").is_none()
            })
        }));

    set_virtual_time(&service, "2016-03-01T00:00:00.000Z");
    let open = load_with_resource_version(service.port(), viewer_id, "1.4.99-mana-open");
    assert_eq!(open.data["user_character_list"]["1"]["mana_board_index"], 2);
    assert_eq!(
        open.data["user_character_mana_node_list"]["1"],
        json!([
            {"multiplied_id": 2_201, "awake_level": 0},
            {"multiplied_id": 2_202, "awake_level": 0},
            {"multiplied_id": 2_203, "awake_level": 1},
            {"multiplied_id": 2_401, "awake_level": 0},
        ])
    );
    service.stop().expect("service stops cleanly");

    assert_eq!(
        read_only_player_snapshot(root.path())["user_character_list"]["1"]["mana_board_index"],
        2
    );
    assert_eq!(
        read_only_player_snapshot(root.path())["user_character_mana_node_list"]["1"],
        stored_nodes
    );
}
// //// /仅在载入响应投影当前可见的 Mana board ////

// //// 持久化主线进度与编队客户端字段 [@x380kkm 2026-08-23] ////
#[test]
fn persists_main_quest_progress_and_party_battle_power() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 3).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    update_only_player_snapshot(root.path(), |player_data| {
        player_data["user_tutorial"] = json!({
            "skip_flag": true,
            "tutorial_step": 6,
            "viewer_id": 0,
        });
        player_data["user_triggered_tutorial"] = json!([12, 55]);
        player_data["quest_progress"] = json!({
            "1": [
                {
                    "quest_id": 1_001_002,
                    "finished": true,
                    "high_score": 60_000,
                    "best_elapsed_time_ms": 29_000,
                },
            ],
            "14": [{
                "quest_id": 1_001,
                "finished": true,
            }],
        });
        let party_groups = player_data["user_party_group_list"]
            .as_object_mut()
            .expect("party groups are an object");
        for group in party_groups.values_mut() {
            let parties = group["list"]
                .as_object_mut()
                .expect("party group list is an object");
            for party in parties.values_mut() {
                let party = party.as_object_mut().expect("party is an object");
                party.remove("current_battle_power");
                party.remove("before_battle_power");
            }
        }
        player_data["user_party_group_list"]["1"]["list"]["1"]["current_battle_power"] =
            Value::from(4_321);
    });

    let expected_main_quest_progress = json!([
        {
            "quest_id": 1_001_002,
            "finished": true,
            "unlocked": false,
            "high_score": 60_000,
            "clear_rank": 5,
            "best_elapsed_time_ms": 29_000,
        },
        {
            "quest_id": 1_001_001,
            "finished": true,
            "unlocked": false,
            "high_score": 0,
            "clear_rank": 5,
            "best_elapsed_time_ms": null,
        },
        {
            "quest_id": 1_001_003,
            "finished": true,
            "unlocked": false,
            "high_score": 0,
            "clear_rank": 5,
            "best_elapsed_time_ms": null,
        },
        {
            "quest_id": 1_002_001,
            "finished": true,
            "unlocked": false,
            "high_score": 61_350,
            "clear_rank": 5,
            "best_elapsed_time_ms": 35_700,
        },
    ]);
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = load_with_resource_version(service.port(), viewer_id, "1.4.99-tutorial");
    assert!(loaded.data["user_tutorial"].is_null());
    assert_eq!(
        loaded.data["quest_progress"]["1"],
        expected_main_quest_progress,
    );
    assert_eq!(
        loaded.data["quest_progress"]["14"],
        json!([{
            "quest_id": 1_001,
            "finished": true,
            "unlocked": false,
            "high_score": 0,
            "clear_rank": 5,
            "best_elapsed_time_ms": null,
        }]),
    );
    assert!(loaded.data["last_main_quest_id"].is_null());
    let party_groups = loaded.data["user_party_group_list"]
        .as_object()
        .expect("party groups are an object");
    for group in party_groups.values() {
        let parties = group["list"]
            .as_object()
            .expect("party group list is an object");
        for party in parties.values() {
            assert!(party["current_battle_power"].as_i64().is_some());
            assert!(party["before_battle_power"].as_i64().is_some());
        }
    }
    assert_eq!(
        loaded.data["user_party_group_list"]["1"]["list"]["1"]["current_battle_power"],
        4_321,
    );
    assert_eq!(
        loaded.data["user_party_group_list"]["1"]["list"]["1"]["before_battle_power"],
        0,
    );
    let favorite_groups = loaded.data["favorite_party_group_list"]
        .as_array()
        .expect("favorite party groups are an array");
    assert_eq!(favorite_groups.len(), party_groups.len());
    assert_eq!(favorite_groups[0]["party_group_id"], 1);
    assert_eq!(favorite_groups[0]["party_list"][0]["party_id"], 1);
    assert_eq!(favorite_groups[0]["party_list"][0]["party_name"], "Party A");
    assert_eq!(
        favorite_groups[0]["party_list"][0]["current_battle_power"],
        4_321,
    );
    assert_eq!(loaded.data["config"]["summon_com_seconds"], 5);

    let reloaded = load_with_resource_version(service.port(), viewer_id, "1.4.99-tutorial");
    assert_eq!(
        reloaded.data["quest_progress"]["1"],
        expected_main_quest_progress,
    );
    assert_eq!(
        reloaded.data["user_party_group_list"]["1"]["list"]["1"]["current_battle_power"],
        4_321,
    );
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    let serialized = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("player snapshot is read");
    let persisted = serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    assert_eq!(
        persisted["quest_progress"]["1"],
        expected_main_quest_progress,
    );
    assert_eq!(
        persisted["favorite_party_group_list"][0]["party_list"][0]["current_battle_power"],
        4_321,
    );
    assert_eq!(persisted["config"]["summon_com_seconds"], 5);
}
// //// /持久化主线进度与编队客户端字段 ////

// //// 持久化创建时间并累积经验池 [@x380kkm 2026-07-22] ////
#[test]
fn persists_creation_times_and_accumulates_pooled_exp() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    assert!(support::request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    let viewer_id = signup(service.port(), 1).data_headers.viewer_id;
    let loaded = load_with_resource_version(service.port(), viewer_id, "1.4.99-state");
    let stamina_heal_time = loaded.data["user_info"]["stamina_heal_time"]
        .as_i64()
        .expect("stamina time is numeric");
    let first_login_time = loaded.data["user_info"]["last_login_time"]
        .as_str()
        .expect("login time is text")
        .to_owned();
    let first_character = loaded.data["user_character_list"]
        .as_object()
        .and_then(|characters| characters.values().next())
        .expect("default character exists");
    let join_time = first_character["join_time"]
        .as_i64()
        .expect("join time is numeric");
    let update_time = first_character["update_time"]
        .as_i64()
        .expect("update time is numeric");
    assert!(stamina_heal_time <= loaded.data_headers.servertime);
    assert!(stamina_heal_time >= loaded.data_headers.servertime - 60);
    assert_eq!(join_time, update_time);
    service.stop().expect("service stops cleanly");

    update_only_player_snapshot(root.path(), |player_data| {
        player_data["user_info"]["exp_pool"] = Value::from(0);
        player_data["user_info"]["exp_pooled_time"] =
            Value::from(loaded.data_headers.servertime - 120);
    });
    thread::sleep(Duration::from_millis(1_100));
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let reloaded = load_with_resource_version(service.port(), viewer_id, "1.4.99-state");
    assert!(reloaded.data_headers.servertime > loaded.data_headers.servertime);
    assert_eq!(reloaded.data["user_info"]["exp_pool"].as_i64(), Some(2));
    assert_eq!(
        reloaded.data["user_info"]["exp_pooled_time"].as_i64(),
        Some(reloaded.data_headers.servertime),
    );
    assert_eq!(
        reloaded.data["user_info"]["stamina_heal_time"].as_i64(),
        Some(stamina_heal_time),
    );
    assert_ne!(
        reloaded.data["user_info"]["last_login_time"].as_str(),
        Some(first_login_time.as_str()),
    );
    let reloaded_character = reloaded.data["user_character_list"]
        .as_object()
        .and_then(|characters| characters.values().next())
        .expect("reloaded default character exists");
    assert_eq!(reloaded_character["join_time"].as_i64(), Some(join_time));
    assert_eq!(
        reloaded_character["update_time"].as_i64(),
        Some(update_time),
    );
    service.stop().expect("service stops cleanly");
}
// //// /持久化创建时间并累积经验池 ////

// //// 跨日重置每日玩家状态 [@x380kkm 2026-08-23] ////
#[test]
fn resets_daily_player_state_once_per_virtual_date() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let viewer_id = signup(service.port(), 4).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    let spent_daily_state = || {
        json!({
            "challenge_points": [
                {
                    "campaign_list": [{"additional_point": 3, "campaign_id": 2023013101}],
                    "id": 1,
                    "point": 0,
                },
                {"campaign_list": [], "id": 999_999, "point": 7},
            ],
            "gacha_info": [{
                "gacha_exchange_point": 17,
                "gacha_id": 80000,
                "is_account_first": false,
                "is_daily_first": false,
            }],
            "gacha_campaigns": [
                {"campaign_id": 12, "count": 0, "gacha_id": 80000},
                {"campaign_id": 999_999, "count": 0, "gacha_id": 999_999},
            ],
        })
    };
    update_only_player_snapshot(root.path(), |player_data| {
        let spent = spent_daily_state();
        player_data["user_daily_challenge_point_list"] = spent["challenge_points"].clone();
        player_data["gacha_info_list"] = spent["gacha_info"].clone();
        player_data["gacha_campaign_list"] = spent["gacha_campaigns"].clone();
    });

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    let next_day = load_without_resource_version(service.port(), viewer_id);
    let challenge_points = next_day.data["user_daily_challenge_point_list"]
        .as_array()
        .expect("challenge points are an array");
    let point = |challenge_id| {
        challenge_points
            .iter()
            .find(|entry| entry["id"].as_i64() == Some(challenge_id))
            .and_then(|entry| entry["point"].as_i64())
    };
    assert_eq!(point(1), Some(3));
    assert_eq!(point(251), Some(2));
    assert_eq!(point(5_001), Some(10));
    assert_eq!(point(10_008), Some(1));
    assert_eq!(point(999_999), Some(7));
    assert_eq!(
        next_day.data["gacha_info_list"][0]["is_daily_first"].as_bool(),
        Some(false),
    );
    let campaigns = next_day.data["gacha_campaign_list"]
        .as_array()
        .expect("gacha campaigns are an array");
    let refreshed_campaign = campaigns
        .iter()
        .find(|campaign| campaign["gacha_id"] == 80000)
        .expect("active campaign is retained");
    assert_eq!(refreshed_campaign["campaign_id"], 12);
    assert_eq!(refreshed_campaign["count"], 1);
    assert!(campaigns
        .iter()
        .all(|campaign| campaign["gacha_id"] != 999_999));
    service.stop().expect("service stops cleanly");

    update_only_player_snapshot(root.path(), |player_data| {
        let spent = spent_daily_state();
        player_data["user_daily_challenge_point_list"] = spent["challenge_points"].clone();
        player_data["gacha_info_list"] = spent["gacha_info"].clone();
        player_data["gacha_campaign_list"] = spent["gacha_campaigns"].clone();
    });
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    set_virtual_time(&service, "2030-01-02T18:00:00.000Z");
    let same_day = load_without_resource_version(service.port(), viewer_id);
    let challenge_points = same_day.data["user_daily_challenge_point_list"]
        .as_array()
        .expect("challenge points are an array");
    assert_eq!(challenge_points.len(), 2);
    assert_eq!(challenge_points[0]["point"], 0);
    assert_eq!(challenge_points[1]["point"], 7);
    assert_eq!(
        same_day.data["gacha_info_list"][0]["is_daily_first"].as_bool(),
        Some(false),
    );
    let campaigns = same_day.data["gacha_campaign_list"]
        .as_array()
        .expect("gacha campaigns are an array");
    let preserved_campaign = campaigns
        .iter()
        .find(|campaign| campaign["gacha_id"] == 80000)
        .expect("active campaign is retained");
    assert_eq!(preserved_campaign["campaign_id"], 12);
    assert_eq!(preserved_campaign["count"], 0);
    assert!(campaigns
        .iter()
        .all(|campaign| campaign["gacha_id"] != 999_999));
    service.stop().expect("service stops cleanly");
}
// //// /跨日重置每日玩家状态 ////

// //// 缺失玩家快照时返回载入错误 [@x380kkm 2026-08-22] ////
#[test]
fn returns_an_error_for_a_missing_player_snapshot() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 1).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    assert_eq!(
        database
            .execute("DELETE FROM player_snapshots", [])
            .expect("snapshot is removed"),
        1,
    );
    drop(database);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
        "1.4.99-migration",
    );
    assert!(response.starts_with("HTTP/1.1 500 Internal Server Error"));
    assert!(response.ends_with("{\"error\":\"no_player_data\"}"));
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is reopened");
    let snapshot_count = database
        .query_row("SELECT COUNT(*) FROM player_snapshots", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("snapshot count is read");
    assert_eq!(snapshot_count, 0);
}
// //// /缺失玩家快照时返回载入错误 ////

// //// 缺失账号玩家时返回请求错误 [@x380kkm 2026-08-22] ////
#[test]
fn returns_a_bad_request_for_an_account_without_a_player() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 2).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    database
        .execute_batch("DELETE FROM player_snapshots; DELETE FROM players;")
        .expect("account player is removed");
    drop(database);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let response = send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    );
    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.ends_with("{\"error\":\"no_player\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /缺失账号玩家时返回请求错误 ////

// //// 隔离不同账号的玩家快照 [@x380kkm 2026-07-22] ////
#[test]
fn isolates_player_snapshots_between_accounts() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let first_viewer_id = signup(service.port(), 11).data_headers.viewer_id;
    let second_viewer_id = signup(service.port(), 22).data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    let serialized = database
        .query_row(
            "SELECT player_snapshots.data_json
             FROM player_snapshots
             JOIN accounts ON accounts.id = player_snapshots.account_id
             WHERE accounts.idp_id = 'cn:11'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("first account snapshot is read");
    let mut first_player_data =
        serde_json::from_str::<Value>(&serialized).expect("first snapshot is JSON");
    first_player_data["user_info"]["name"] = Value::from("First Account");
    database
        .execute(
            "UPDATE player_snapshots
             SET data_json = ?1
             WHERE account_id = (SELECT id FROM accounts WHERE idp_id = 'cn:11')",
            params![serde_json::to_string(&first_player_data).expect("first snapshot is encoded")],
        )
        .expect("first account snapshot is updated");
    drop(database);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let first_loaded =
        load_with_resource_version(service.port(), first_viewer_id, "1.4.99-isolation");
    let second_loaded =
        load_with_resource_version(service.port(), second_viewer_id, "1.4.99-isolation");
    assert_eq!(
        first_loaded.data["user_info"]["name"].as_str(),
        Some("First Account"),
    );
    assert_ne!(
        second_loaded.data["user_info"]["name"].as_str(),
        Some("First Account"),
    );
    service.stop().expect("service stops cleanly");
}
// //// /隔离不同账号的玩家快照 ////

// //// 保持载入响应和资源路径的覆盖版本一致 [@x380kkm 2026-08-29] ////
#[test]
fn uses_the_platform_override_version_in_load() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_asset_path_manifest(root.path(), "1.4.58");
    write_ios_voice_override_archive(root.path());
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 28).data_headers.viewer_id;

    let request_body = encode_request(&json!({"viewer_id": viewer_id}));
    let path_response = support::request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/asset/get_path",
        "application/x-www-form-urlencoded",
        &[("res_ver", "1.4.58"), ("DEVICE", "1")],
        request_body.as_bytes(),
    );
    let path = decode_response::<Value>(&path_response);
    let ios_load = load_with_resource_version_and_device(service.port(), viewer_id, "1.4.58", "1");
    let android_load =
        load_with_resource_version_and_device(service.port(), viewer_id, "1.4.58", "2");

    assert_eq!(path.data["info"]["target_asset_version"], "1.4.59");
    assert_eq!(ios_load.data["available_asset_version"], "1.4.59");
    assert_eq!(android_load.data["available_asset_version"], "1.4.58");
    service.stop().expect("service stops cleanly");
}
// //// /保持载入响应和资源路径的覆盖版本一致 ////

// //// 使用环境配置的 CN 资源版本 [@x380kkm 2026-07-22] ////
#[test]
fn uses_configured_resource_version_without_request_header() {
    let _environment_lock = RESOURCE_VERSION_ENV
        .lock()
        .expect("resource version environment is locked");
    let previous_resource_version = std::env::var_os("CN_RES_VERSION");
    std::env::set_var("CN_RES_VERSION", "1.4.98-environment");

    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(service.port(), 1).data_headers.viewer_id;
    let loaded = load_without_resource_version(service.port(), viewer_id);
    assert_eq!(
        loaded.data["available_asset_version"].as_str(),
        Some("1.4.98-environment"),
    );
    service.stop().expect("service stops cleanly");

    match previous_resource_version {
        Some(value) => std::env::set_var("CN_RES_VERSION", value),
        None => std::env::remove_var("CN_RES_VERSION"),
    }
}
// //// /使用环境配置的 CN 资源版本 ////
