// audience: internal
// # personal-service-cn-story-expod-atomicity-tests
// 剧情, 经验, 角色突破和羁绊之证的资产, 任务与领取记录共享保存边界.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

#[path = "cn_story_expod_atomicity/board.rs"]
mod board;
#[path = "cn_story_expod_atomicity/bond_token.rs"]
mod bond_token;
#[path = "cn_story_expod_atomicity/character.rs"]
mod character;
#[path = "cn_story_expod_atomicity/expod.rs"]
mod expod;
#[path = "support/local_saves.rs"]
#[allow(dead_code)]
mod local_save_support;
#[path = "cn_story_expod_atomicity/materials.rs"]
mod materials;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

#[derive(PartialEq)]
struct PlayerState {
    snapshot: Value,
    progress: Vec<(i64, i64, i64)>,
    counters: Vec<(String, i64)>,
    history_counts: Vec<i64>,
}

// //// 读取玩家资产与任务持久化状态 [@x380kkm 2026-09-23] ////
fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn player_state(root: &Path) -> PlayerState {
    let connection = database(root);
    let serialized: String = connection
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut progress = connection.prepare(
        "SELECT category, mission_id, value FROM player_mission_progress ORDER BY category, mission_id",
    ).unwrap();
    let progress = progress
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut counters = connection
        .prepare("SELECT pattern, value FROM player_mission_counters ORDER BY pattern")
        .unwrap();
    let counters = counters
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let history_counts = ["player_receive_history", "player_activity_history"]
        .into_iter()
        .map(|table| {
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        })
        .collect();
    PlayerState {
        snapshot: serde_json::from_str(&serialized).unwrap(),
        progress,
        counters,
        history_counts,
    }
}

fn update_player(root: &Path, update: impl FnOnce(&mut Value)) {
    let mut snapshot = player_state(root).snapshot;
    update(&mut snapshot);
    database(root)
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [snapshot.to_string()],
        )
        .unwrap();
}
// //// /读取玩家资产与任务持久化状态 ////

// //// 创建固定日期的真实 HTTP 玩家会话 [@x380kkm 2026-09-23] ////
fn request(service: &PersonalService, route: &str, body: Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    )
}

fn setup() -> (TempDir, PersonalService, i64) {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", &authorization)],
        json!({"enabled":true,"iso":"2024-01-01T00:00:00.000Z","rate":1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let viewer = decode_response::<SignupData>(&request(
        &service,
        "tool/signup",
        json!({"device_id": 923_301}),
    ))
    .data_headers
    .viewer_id;
    (root, service, viewer)
}

fn reject_snapshot_when(root: &Path, condition: &str) {
    database(root)
        .execute_batch(&format!(
            "CREATE TRIGGER reject_final_snapshot BEFORE UPDATE OF data_json ON player_snapshots
         WHEN {condition}
         BEGIN SELECT RAISE(ABORT, 'final snapshot failure'); END;",
        ))
        .unwrap();
}

fn allow_snapshot(root: &Path) {
    database(root)
        .execute_batch("DROP TRIGGER reject_final_snapshot")
        .unwrap();
}

fn has_mission_reward(response: &Value, mission: i64, reward: i64) -> bool {
    response["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| {
            entry["mission_category_id"] == 1
                && entry["mission_id"] == mission
                && entry["mission_reward_id"] == reward
        })
}
// //// /创建固定日期的真实 HTTP 玩家会话 ////

// //// 剧情最后保存失败时回滚首通奖励与两类任务 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_character_story_and_missions_when_final_snapshot_fails() {
    for route in ["story_quest/finish", "story_quest/finish_with_skip"] {
        let (root, service, viewer) = setup();
        let body = json!({"viewer_id":viewer,"party_id":1,"category":3,"quest_id":101});
        let before = player_state(root.path());
        reject_snapshot_when(
            root.path(),
            r#"COALESCE(json_extract(NEW.data_json, '$.all_active_mission_list."11010".progress'), 0) > 0"#,
        );
        let failed = request(&service, route, body.clone());
        assert!(failed.starts_with("HTTP/1.1 500"), "{route}: {failed}");
        let after_failure = player_state(root.path());
        assert_eq!(
            after_failure.snapshot["user_info"]["free_vmoney"],
            before.snapshot["user_info"]["free_vmoney"],
            "{route}"
        );
        assert!(
            after_failure == before,
            "{route} keeps the full player state on failure"
        );

        allow_snapshot(root.path());
        let response = decode_response::<Value>(&request(&service, route, body.clone())).data;
        assert!(has_mission_reward(&response, 23, 23001));
        let completed = player_state(root.path());
        assert_eq!(
            completed.snapshot["user_info"]["free_vmoney"],
            before.snapshot["user_info"]["free_vmoney"]
                .as_i64()
                .unwrap()
                + 45
        );
        assert_eq!(
            completed.snapshot["all_active_mission_list"]["11010"]["progress"],
            1
        );
        assert!(completed
            .counters
            .contains(&("episode_clear_count".to_owned(), 1)));
        assert!(completed.progress.contains(&(1, 23, 1)));
        assert!(completed.history_counts[0] > before.history_counts[0]);
        assert_eq!(
            decode_response::<Value>(&request(&service, route, body.clone())).data,
            json!([])
        );
        assert!(player_state(root.path()) == completed);

        service.stop().unwrap();
        let restarted = PersonalService::start(root.path(), 0).unwrap();
        assert_eq!(
            decode_response::<Value>(&request(&restarted, route, body)).data,
            json!([])
        );
        assert!(player_state(root.path()) == completed);
        restarted.stop().unwrap();
    }
}
// //// /剧情最后保存失败时回滚首通奖励与两类任务 ////

// //// 经验注入最后保存失败时回滚消耗升级与任务奖励 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_exp_injection_and_missions_when_final_snapshot_fails() {
    let (root, service, viewer) = setup();
    update_player(root.path(), |snapshot| {
        snapshot["user_info"]["exp_pool"] = json!(1000);
        snapshot["user_character_list"]["1"]["exp"] = json!(10);
    });
    let body = json!({"viewer_id":viewer,"character_id":1,"exp":400});
    let before = player_state(root.path());
    reject_snapshot_when(
        root.path(),
        r#"COALESCE(json_extract(NEW.data_json, '$.all_active_mission_list."11090".progress'), 0) > 0"#,
    );
    let failed = request(&service, "expod/inject_exp", body.clone());
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    let after_failure = player_state(root.path());
    assert_eq!(
        after_failure.snapshot["user_info"]["exp_pool"],
        before.snapshot["user_info"]["exp_pool"]
    );
    assert_eq!(
        after_failure.snapshot["user_character_list"]["1"]["exp"],
        10
    );
    assert!(
        after_failure == before,
        "failed EXP injection keeps the full player state"
    );

    allow_snapshot(root.path());
    let response = decode_response::<Value>(&request(&service, "expod/inject_exp", body)).data;
    assert!(has_mission_reward(&response, 9, 9001));
    let completed = player_state(root.path());
    assert_eq!(completed.snapshot["user_info"]["exp_pool"], 600);
    assert_eq!(completed.snapshot["user_character_list"]["1"]["exp"], 410);
    assert_eq!(
        completed.snapshot["user_info"]["free_vmoney"],
        before.snapshot["user_info"]["free_vmoney"]
            .as_i64()
            .unwrap()
            + 10
    );
    assert_eq!(
        completed.snapshot["all_active_mission_list"]["11090"]["progress"],
        1
    );
    assert!(completed
        .counters
        .contains(&("character_level_achievement".to_owned(), 10)));
    assert!(completed.progress.contains(&(1, 9, 10)));
    assert!(completed.history_counts[0] > before.history_counts[0]);

    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    let loaded = decode_response::<Value>(&request(
        &restarted,
        "load",
        json!({"keychain":viewer,"viewer_id":viewer}),
    ))
    .data;
    assert_eq!(loaded["user_info"]["exp_pool"], 600);
    assert_eq!(loaded["user_character_list"]["1"]["exp"], 410);
    assert_eq!(
        player_state(root.path()).snapshot["all_active_mission_list"]["11090"]["progress"],
        1
    );
    restarted.stop().unwrap();
}
// //// /经验注入最后保存失败时回滚消耗升级与任务奖励 ////

// //// 单次和批量副本转换在存档失败时保持原有资产 [@x380kkm 2026-09-23] ////
#[test]
fn keeps_stack_conversion_assets_together_when_snapshot_fails() {
    for route in ["expod/stack_to_exp", "expod/bulk_stack_to_exp"] {
        let (root, service, viewer) = setup();
        update_player(root.path(), |snapshot| {
            snapshot["user_character_list"]["1"]["stack"] = json!(2);
            snapshot["user_character_list"]["1"]["over_limit_step"] = json!(6);
        });
        let body = json!({"viewer_id":viewer,"character_id":1,"number":2});
        let before = player_state(root.path());
        reject_snapshot_when(
            root.path(),
            r#"json_extract(NEW.data_json, '$.user_character_list."1".stack') = 0"#,
        );
        let failed = request(&service, route, body.clone());
        assert!(failed.starts_with("HTTP/1.1 500"), "{route}: {failed}");
        assert!(
            player_state(root.path()) == before,
            "{route} preserves assets and mission state"
        );

        allow_snapshot(root.path());
        let response = decode_response::<Value>(&request(&service, route, body)).data;
        assert_eq!(response["converted_exp_info"]["add_exp"], 4000);
        let completed = player_state(root.path());
        assert_eq!(completed.snapshot["user_character_list"]["1"]["stack"], 0);
        assert_eq!(
            completed.snapshot["user_info"]["exp_pool"],
            before.snapshot["user_info"]["exp_pool"].as_i64().unwrap() + 4000
        );
        assert_eq!(
            completed.snapshot["item_list"]["990008"],
            before.snapshot["item_list"]["990008"]
        );
        assert!(completed.progress == before.progress);
        assert!(completed.counters == before.counters);
        service.stop().unwrap();
    }
}
// //// /单次和批量副本转换在存档失败时保持原有资产 ////
