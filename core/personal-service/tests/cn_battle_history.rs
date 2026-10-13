// audience: internal
// # personal-service-cn-battle-history-tests
// HTTP 流程核对当场统计, 玩法隔离, 结算回滚和重启后的历史读取.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn send(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    )
}

fn data(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = send(service, route, body);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    if response.contains("Content-Type: application/json") {
        serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()["data"]
            .clone()
    } else {
        decode_response::<Value>(&response).data
    }
}

fn signup(service: &PersonalService, device: i64) -> i64 {
    decode_response::<SignupData>(&send(service, "tool/signup", &json!({"device_id":device})))
        .data_headers
        .viewer_id
}

fn start(service: &PersonalService, viewer: i64, category: i64, quest: i64, play: &str) {
    data(
        service,
        "single_battle_quest/start",
        &json!({
            "viewer_id":viewer, "category":category, "quest_id":quest, "play_id":play,
            "party_id":1, "is_auto_start_mode":false, "use_boost_point":false, "use_boss_boost_point":false,
        }),
    );
}

fn finish(viewer: i64, category: i64, quest: i64, play: &str) -> Value {
    json!({
        "viewer_id":viewer, "category":category, "quest_id":quest, "play_id":play,
        "elapsed_time_ms":3000, "score":2000000, "add_mana":0, "is_accomplished":true,
        "statistics":{
            "clear_phase":1,
            "party":{
                "characters":[{"id":1},null,null],
                "unison_characters":[null,null,null],
                "equipments":[{"id":1,"level":4,"enhancement_level":2},null,null],
                "ability_soul_ids":[null,null,null]
            },
            "zones":[
                {"playthrough_ms":1000,"damage_deal_total":2147483648.75,"members":[{"origin_damage":1073741824.75},null,null]},
                {"playthrough_ms":2000,"damage_deal_total":4294967296.5,"members":[{"origin_damage":3221225472.5},null,null]}
            ]
        }
    })
}

fn history(service: &PersonalService, viewer: i64, kind: &str) -> Value {
    data(
        service,
        &format!("history/{kind}"),
        &json!({"viewer_id":viewer}),
    )["history"]
        .clone()
}

fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", authorization.as_str())],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

// //// 当场队伍和伤害在重复结算与重启后保持一致 [@x380kkm 2026-09-23] ////
#[test]
fn practice_records_survive_replay_restart_and_other_accounts() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 92301);
    start(&service, viewer, 15, 1, "practice-finish");
    let request = finish(viewer, 15, 1, "practice-finish");
    data(&service, "single_battle_quest/finish", &request);
    let recorded = history(&service, viewer, "practice_battle");
    assert_eq!(recorded.as_array().unwrap().len(), 1);
    let entry = &recorded[0];
    assert_eq!(entry["quest_id"], 1);
    assert_eq!(entry["category_id"], 15);
    assert_eq!(entry["finish_kind"], 0);
    assert_eq!(entry["elapsed_time_ms"], 3000);
    assert_eq!(entry["total_damage"], 6_442_450_945.0);
    assert_eq!(entry["character_1_total_damage"], 4_294_967_297.0);
    assert_eq!(entry["character_id_1"], 1);
    assert_eq!(entry["equipment1_id"], 1);
    assert_eq!(entry["equipment_level_1"], 4);
    assert_eq!(entry["enhancement_level_1"], 2);
    assert!(entry["character_id_2"].is_null());
    assert!(entry["character_2_total_damage"].is_null());
    assert_eq!(entry["score"], 2000000);
    data(&service, "single_battle_quest/finish", &request);
    assert_eq!(history(&service, viewer, "practice_battle"), recorded);
    assert_eq!(
        history(&service, viewer, "score_attack_event_battle"),
        json!([])
    );
    let other = signup(&service, 92302);
    assert_eq!(history(&service, other, "practice_battle"), json!([]));
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(history(&service, viewer, "practice_battle"), recorded);
}

// //// 结算收据失败时回滚战绩并保留可重试战斗 [@x380kkm 2026-09-23] ////
#[test]
fn finish_and_abort_history_roll_back_with_battle_receipts() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 92303);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    for (play, route) in [
        ("history-finish-failure", "finish"),
        ("history-abort-failure", "abort"),
    ] {
        start(&service, viewer, 15, 1, play);
        let request = if route == "finish" {
            finish(viewer, 15, 1, play)
        } else {
            json!({"viewer_id":viewer,"play_id":play,"category":15,"finish_kind":1,
                "statistics":finish(viewer,15,1,play)["statistics"]})
        };
        let before = history(&service, viewer, "practice_battle");
        db.execute_batch(
            "CREATE TRIGGER reject_history BEFORE INSERT ON player_battle_history
            BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;",
        )
        .unwrap();
        let failed = send(&service, &format!("single_battle_quest/{route}"), &request);
        assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
        assert_eq!(history(&service, viewer, "practice_battle"), before);
        let active: i64 = db
            .query_row("SELECT COUNT(*) FROM active_single_quests", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(active, 1);
        db.execute_batch("DROP TRIGGER reject_history;").unwrap();
        data(&service, &format!("single_battle_quest/{route}"), &request);
        let recorded = history(&service, viewer, "practice_battle");
        assert_eq!(
            recorded.as_array().unwrap().len(),
            before.as_array().unwrap().len() + 1
        );
        data(&service, &format!("single_battle_quest/{route}"), &request);
        assert_eq!(history(&service, viewer, "practice_battle"), recorded);
        if route == "abort" {
            assert_eq!(recorded[0]["finish_kind"], 1);
            assert_eq!(recorded[0]["elapsed_time_ms"], 3000);
            assert!(recorded[0]["clear_rank"].is_null());
            assert!(recorded[0]["score"].is_null());
            assert_eq!(recorded[0]["total_damage"], 6_442_450_945.0);
            start(&service, viewer, 15, 1, "practice-after-abort");
            data(&service, "single_battle_quest/abort", &request);
            let active_play: String = db
                .query_row("SELECT play_id FROM active_single_quests", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(active_play, "practice-after-abort");
            assert_eq!(history(&service, viewer, "practice_battle"), recorded);
        }
    }
}

// //// 无限演武重置后继续保留每场成绩 [@x380kkm 2026-09-23] ////
#[test]
fn score_history_preserves_lower_scores_and_previous_rounds() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    std::fs::write(
        cdn.path().join("activity-catalog.json"),
        json!({"format_version":1,"activities":[
            {"activity_id":"score-attack:1","name":"无限演武","kind":"score-attack"}
        ]}).to_string(),
    ).unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let viewer = signup(&service, 92304);
    manage(
        &service,
        "PUT",
        "/v1/activities/score-attack:1/mode",
        json!({"mode":"always"}),
    );
    for (play, score) in [("score-high", 2000000), ("score-low", 1500000)] {
        start(&service, viewer, 27, 1001, play);
        let mut request = finish(viewer, 27, 1001, play);
        request["score"] = json!(score);
        data(&service, "single_battle_quest/finish", &request);
    }
    let before = history(&service, viewer, "score_attack_event_battle");
    assert_eq!(before.as_array().unwrap().len(), 2);
    assert_eq!(before[0]["score"], 1500000);
    assert_eq!(before[1]["score"], 2000000);
    let saves = manage(&service, "GET", "/v1/local-saves", json!({}));
    let path = format!(
        "/v1/local-saves/{}/activities/score-attack:1/reset",
        saves["slots"][0]["id"]
    );
    manage(&service, "POST", &path, json!({"expected_round":0}));
    assert_eq!(
        history(&service, viewer, "score_attack_event_battle"),
        before
    );
    start(&service, viewer, 27, 1001, "score-new-round");
    data(
        &service,
        "single_battle_quest/finish",
        &finish(viewer, 27, 1001, "score-new-round"),
    );
    let after = history(&service, viewer, "score_attack_event_battle");
    assert_eq!(after.as_array().unwrap().len(), 3);
    assert_eq!(
        &after.as_array().unwrap()[1..],
        before.as_array().unwrap().as_slice()
    );
}
