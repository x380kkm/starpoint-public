// audience: internal
// # personal-service-cn-solo-time-attack-tests
//
// 该文件验证极时试炼结算字段, 请求重放和服务重启后的时间奖励领取状态.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

use cn_support::{decode_response, encode_request, send_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 发送极时试炼战斗请求并读取持久化玩家 [@x380kkm 2026-09-21] ////
fn request(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = send_request(
        service.port(),
        &format!("/api/index.php/single_battle_quest/{route}"),
        &encode_request(body),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    decode_response::<Value>(&response).data
}

fn start(service: &PersonalService, viewer_id: i64, play_id: &str) {
    request(
        service,
        "start",
        &json!({
            "viewer_id": viewer_id, "api_count": 1, "quest_id": 1001, "category": 25,
            "play_id": play_id, "use_boss_boost_point": false, "use_boost_point": false,
            "is_auto_start_mode": false, "party_id": 1,
        }),
    );
}

fn finish(
    service: &PersonalService,
    viewer_id: i64,
    play_id: &str,
    elapsed_time_ms: i64,
    score: i64,
) -> Value {
    request(
        service,
        "finish",
        &json!({
            "viewer_id": viewer_id, "api_count": 1, "is_restored": false, "continue_count": 0,
            "elapsed_time_ms": elapsed_time_ms, "quest_id": 1001, "play_id": play_id, "category": 25,
            "score": score, "add_mana": 0, "is_accomplished": true,
            "statistics": {
                "clear_phase": 1, "zones": [],
                "party": {
                    "characters": [{"id": 1}, null, null], "unison_characters": [null, null, null],
                    "equipments": [null, null, null], "ability_soul_ids": [null, null, null],
                },
            },
        }),
    )
}

fn snapshot(root: &Path) -> Value {
    let json = Connection::open(root.join("personal-service.sqlite3"))
        .unwrap()
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap();
    serde_json::from_str(&json).unwrap()
}
// //// /发送极时试炼战斗请求并读取持久化玩家 ////

// //// 结算重放保持资产且最佳分数随最短时间持久化 [@x380kkm 2026-09-21] ////
#[test]
fn persists_time_rewards_and_the_fastest_battle_score() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 19 }),
    ))
    .data_headers
    .viewer_id;
    start(&service, viewer_id, "time-reward-first");
    let first = finish(&service, viewer_id, "time-reward-first", 300_000, 5000);
    let event = &first["solo_time_attack_event"];
    assert_eq!(event["reward_ids"].as_array().unwrap().len(), 13);
    assert_eq!(event["new_degree_ids"], json!([54530]));
    assert_eq!(event["elapsed_time_ms"], 300_000);
    assert_eq!(event["main_character_ids"], json!({"0": 1}));
    assert!(event["previous_best_elapsed_time_ms"].is_null());
    assert!(event["previous_best_elapsed_score"].is_null());
    let saved = snapshot(root.path());
    assert_eq!(
        finish(&service, viewer_id, "time-reward-first", 300_000, 5000),
        first
    );
    assert_eq!(snapshot(root.path()), saved);
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    start(&service, viewer_id, "time-reward-faster");
    let faster = finish(&service, viewer_id, "time-reward-faster", 180_000, 4000);
    let event = &faster["solo_time_attack_event"];
    assert_eq!(event["reward_ids"].as_array().unwrap().len(), 12);
    assert_eq!(event["new_degree_ids"], json!([54520]));
    assert_eq!(event["previous_best_elapsed_time_ms"], 300_000);
    assert_eq!(event["previous_best_elapsed_score"], 5000);
    start(&service, viewer_id, "time-reward-slower");
    let slower = finish(&service, viewer_id, "time-reward-slower", 240_000, 9000);
    assert_eq!(slower["solo_time_attack_event"]["reward_ids"], json!([]));
    assert_eq!(
        slower["solo_time_attack_event"]["previous_best_elapsed_score"],
        4000
    );
    start(&service, viewer_id, "time-reward-repeat");
    let repeated = finish(&service, viewer_id, "time-reward-repeat", 180_000, 4000);
    let event = &repeated["solo_time_attack_event"];
    assert_eq!(event["previous_best_elapsed_time_ms"], 180_000);
    assert_eq!(event["previous_best_elapsed_score"], 4000);
    assert_eq!(event["reward_ids"], json!([]));
    assert_eq!(event["new_degree_ids"], json!([]));
    assert_eq!(
        snapshot(root.path())["solo_time_attack_reward_receipts"]
            .as_object()
            .unwrap()
            .len(),
        25
    );
    service.stop().unwrap();
}
// //// /结算重放保持资产且最佳分数随最短时间持久化 ////
