// audience: internal
// # personal-service-cn-story-tests
//
// 该文件验证各分类剧情的两种结算入口, 可选计数字段校验和重复结算幂等性.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

#[derive(Clone, Copy, Serialize)]
struct StoryFinishRequest {
    party_id: i64,
    quest_id: i64,
    viewer_id: i64,
    category: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_count: Option<i64>,
}

// //// 验证 CN 剧情结算请求和奖励幂等性 [@x380kkm 2026-08-07] ////
fn signup_viewer(service: &PersonalService, device_id: i64) -> i64 {
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ));
    signup.data_headers.viewer_id
}

fn set_unfinished_main_story_progress(root: &Path) {
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
    player_data["quest_progress"]["1"] = serde_json::json!([{
        "quest_id": 1_001_003,
        "finished": false,
        "unlocked": true,
        "high_score": 12,
        "best_elapsed_time_ms": 1_234,
    }]);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id,
            ],
        )
        .expect("player snapshot is updated");
}

fn assert_story_settlement(data: &Value) {
    assert_eq!(data["user_info"]["free_vmoney"], 1_515);
    assert_eq!(data["user_info"]["free_mana"], 1_000);
    for field in [
        "character_list",
        "joined_character_id_list",
        "equipment_list",
    ] {
        assert!(data[field]
            .as_array()
            .is_some_and(|values| values.is_empty()));
    }
    assert!(data["item_list"]
        .as_object()
        .is_some_and(|items| items.is_empty()));
    assert!(data["presigned_quest_category"]
        .as_array()
        .is_some_and(|categories| categories.is_empty()));
}

fn verify_story_settlement(
    first_path: &str,
    repeated_path: &str,
    device_id: i64,
    first_api_count: Option<i64>,
) {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup_viewer(&service, device_id);
    set_unfinished_main_story_progress(root.path());
    let request = StoryFinishRequest {
        party_id: 1,
        quest_id: 1_001_003,
        viewer_id,
        category: 1,
        api_count: first_api_count,
        retry_count: None,
    };
    let finished = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        first_path,
        &encode_request(&request),
    ));
    assert_story_settlement(&finished.data);

    let repeated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        repeated_path,
        &encode_request(&request),
    ));
    assert_eq!(repeated.data, serde_json::json!([]));

    let retried = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        first_path,
        &encode_request(&StoryFinishRequest {
            retry_count: Some(1),
            ..request
        }),
    ));
    assert_eq!(retried.data, serde_json::json!([]));

    let invalid_battle_quest = cn_support::send_request(
        service.port(),
        first_path,
        &encode_request(&StoryFinishRequest {
            quest_id: 1_002_001,
            ..request
        }),
    );
    assert!(invalid_battle_quest.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["free_vmoney"], 1_515);
    assert_eq!(loaded.data["user_info"]["free_mana"], 1_000);
    let main_quest_progress = loaded.data["quest_progress"]["1"]
        .as_array()
        .expect("main quest progress is an array");
    let settled = main_quest_progress
        .iter()
        .find(|progress| progress["quest_id"] == 1_001_003)
        .expect("settled story quest is persisted");
    assert_eq!(
        settled,
        &serde_json::json!({
            "quest_id": 1_001_003,
            "finished": true,
            "unlocked": true,
            "high_score": 12,
            "clear_rank": 5,
            "best_elapsed_time_ms": 1_234,
        }),
    );
    assert_eq!(
        main_quest_progress
            .iter()
            .filter(|progress| progress["quest_id"] == 1_001_003)
            .count(),
        1,
    );
    restarted.stop().expect("restarted service stops cleanly");
}

#[test]
fn finishes_skipped_main_story_quest_once() {
    verify_story_settlement(
        "/api/index.php/story_quest/finish_with_skip",
        "/api/index.php/story_quest/finish",
        50,
        None,
    );
}

#[test]
fn finishes_main_story_quest_once() {
    verify_story_settlement(
        "/api/index.php/story_quest/finish",
        "/api/index.php/story_quest/finish_with_skip",
        51,
        Some(1),
    );
}

#[test]
fn finishes_advent_multi_story_in_its_category() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup_viewer(&service, 53);
    let request = StoryFinishRequest {
        party_id: 1,
        quest_id: 100_001_002,
        viewer_id,
        category: 8,
        api_count: None,
        retry_count: None,
    };

    let finished = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/story_quest/finish",
        &encode_request(&request),
    ));
    assert_story_settlement(&finished.data);

    let repeated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/story_quest/finish_with_skip",
        &encode_request(&request),
    ));
    assert_eq!(repeated.data, serde_json::json!([]));
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let advent_multi_progress = loaded.data["quest_progress"]["8"]
        .as_array()
        .expect("advent multi quest progress is an array");
    assert_eq!(
        advent_multi_progress,
        &[serde_json::json!({
            "quest_id": 100_001_002,
            "finished": true,
            "unlocked": false,
            "high_score": 0,
            "clear_rank": 5,
            "best_elapsed_time_ms": null,
        })],
    );
    assert!(loaded.data["quest_progress"]["1"]
        .as_array()
        .expect("main quest progress is an array")
        .iter()
        .all(|progress| progress["quest_id"] != 100_001_002));
    restarted.stop().expect("restarted service stops cleanly");
}

#[test]
fn rejects_negative_story_request_counts() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup_viewer(&service, 52);
    let request = StoryFinishRequest {
        party_id: 1,
        quest_id: 1_001_001,
        viewer_id,
        category: 1,
        api_count: None,
        retry_count: None,
    };

    let negative_retry_count = cn_support::send_request(
        service.port(),
        "/api/index.php/story_quest/finish_with_skip",
        &encode_request(&StoryFinishRequest {
            retry_count: Some(-1),
            ..request
        }),
    );
    assert!(negative_retry_count.starts_with("HTTP/1.1 400 Bad Request"));

    let negative_api_count = cn_support::send_request(
        service.port(),
        "/api/index.php/story_quest/finish_with_skip",
        &encode_request(&StoryFinishRequest {
            api_count: Some(-1),
            ..request
        }),
    );
    assert!(negative_api_count.starts_with("HTTP/1.1 400 Bad Request"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 剧情结算请求和奖励幂等性 ////
