// audience: internal
// # personal-service-cn-score-attack-tests
// 无限演武经真实战斗接口结算, 管理重置保留逐次领取和成绩记录.

use super::*;

// //// 读取管理端活动历史并重置参加轮次 [@x380kkm 2026-09-26] ////
fn management_request(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
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
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

// //// 打开主表日期已到的常驻演武关卡 [@x380kkm 2026-09-26] ////
fn open_permanent_quests(service: &PersonalService) {
    open_activity(service, "score-attack:1");
    manage(
        service,
        "/v1/activities/calendar/score-attack:1",
        json!({"enabled":true,"start_at":"2029-01-01T00:00:00.000Z","end_at":"2031-01-01T00:00:00.000Z"}),
    );
}

// //// 档位跨越只发新增奖励且管理重置保留历史 [@x380kkm 2026-09-26] ////
#[test]
fn score_attack_tiers_survive_retries_restart_and_reset() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    std::fs::write(
        cdn.path().join("activity-catalog.json"),
        json!({
            "format_version":1, "activities":[
                {"activity_id":"score-attack:1","name":"无限演武","kind":"score-attack"}
            ]
        })
        .to_string(),
    )
    .unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let viewer = signup(&service);
    open_permanent_quests(&service);
    refill_stamina(root.path());
    let mut earned = 0;
    for (play, score, elapsed, added, mission_reward) in [
        ("score-below", 289_239, 1, 0, 0),
        ("score-first", 289_240, 300_000, 1, 6),
        ("score-second", 1_796_332, 1, 1, 0),
        ("score-repeat", 289_240, 100_000, 0, 0),
    ] {
        start(&service, viewer, 27, 1163, play);
        let before = snapshot(root.path());
        let mut body = finish_body(viewer, 27, 1163, play);
        body["score"] = json!(score);
        body["elapsed_time_ms"] = json!(elapsed);
        let response = finish(&service, body.clone());
        let after = snapshot(root.path());
        assert_eq!(response["score_attack_event"]["main_character_ids"], json!({"0":1}));
        let announced = response["score_attack_event"]["reward_ids"].as_array().unwrap();
        assert_eq!(announced.len(), added as usize);
        for id in announced {
            let receipt = format!("1163:{}", id.as_i64().unwrap());
            assert!(before["score_attack_reward_receipts"][&receipt].is_null());
            assert!(after["score_attack_reward_receipts"][&receipt].is_object());
        }
        assert_eq!(response["clear_rank"], 1);
        assert_eq!(
            after["item_list"]["40501"].as_i64().unwrap_or_default(),
            before["item_list"]["40501"].as_i64().unwrap_or_default() + added + mission_reward
        );
        earned += added as usize;
        assert_eq!(
            after["score_attack_reward_receipts"]
                .as_object()
                .map_or(0, |r| r.len()),
            earned
        );
        assert_eq!(finish(&service, body), response);
        assert_eq!(snapshot(root.path()), after);
    }
    service.stop().unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    start(&service, viewer, 27, 1163, "score-persisted");
    let before = snapshot(root.path());
    let mut body = finish_body(viewer, 27, 1163, "score-persisted");
    body["score"] = json!(1_796_332);
    finish(&service, body);
    assert_eq!(
        snapshot(root.path())["item_list"]["40501"],
        before["item_list"]["40501"]
    );

    let slots = management_request(&service, "GET", "/v1/local-saves", json!({}));
    let base = format!(
        "/v1/local-saves/{}/activities/score-attack:1",
        slots["slots"][0]["id"]
    );
    let history = management_request(&service, "GET", &format!("{base}/history"), json!({}));
    assert!(history["can_reset"].as_bool().unwrap());
    management_request(
        &service,
        "POST",
        &format!("{base}/reset"),
        json!({"expected_round":0}),
    );
    assert!(snapshot(root.path())["score_attack_reward_receipts"]
        .as_object()
        .unwrap()
        .is_empty());
    start(&service, viewer, 27, 1163, "score-reset");
    let before = snapshot(root.path());
    let mut body = finish_body(viewer, 27, 1163, "score-reset");
    body["score"] = json!(1_796_332);
    finish(&service, body);
    let after = snapshot(root.path());
    assert_eq!(
        after["item_list"]["40501"].as_i64().unwrap(),
        before["item_list"]["40501"].as_i64().unwrap() + 2
    );
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM player_activity_history WHERE activity_id='score-attack:1' AND event_key LIKE 'score_attack_reward_receipts:%'", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
    assert_eq!(db.query_row("SELECT COUNT(DISTINCT participation_round) FROM player_activity_history WHERE activity_id='score-attack:1' AND event_key LIKE 'score_attack_reward_receipts:%'", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(
        db.query_row(
            "SELECT SUM(number) FROM player_receive_history WHERE type_id=40501",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        10
    );
    service.stop().unwrap();
}
// //// /档位跨越只发新增奖励且管理重置保留历史 ////

// //// 战绩写入失败时回滚资产和领取凭据 [@x380kkm 2026-09-26] ////
#[test]
fn score_attack_rewards_roll_back_with_battle_history() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    open_permanent_quests(&service);
    start(&service, viewer, 27, 1101, "score-history-failure");
    let before = snapshot(root.path());
    let mut body = finish_body(viewer, 27, 1101, "score-history-failure");
    body["score"] = json!(1_171_000_000);
    body["elapsed_time_ms"] = json!(300_000);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_score_history BEFORE INSERT ON player_activity_history BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;").unwrap();
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/single_battle_quest/finish",
        &encode_request(&body),
    );
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM active_single_quests", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    db.execute_batch("DROP TRIGGER reject_score_history;")
        .unwrap();
    let response = finish(&service, body.clone());
    let after = snapshot(root.path());
    assert_eq!(
        response["score_attack_event"]["reward_ids"],
        json!((101001..=101090).collect::<Vec<_>>())
    );
    assert_eq!(response["clear_rank"], 5);
    assert_eq!(
        after["item_list"]["49100"].as_i64().unwrap(),
        before["item_list"]["49100"].as_i64().unwrap_or_default() + 2
    );
    assert!(after["score_attack_reward_receipts"]["1101:101001"].is_object());
    assert!(after["score_attack_reward_receipts"]["1101:101075"].is_object());
    assert!(after["score_attack_reward_receipts"]["1101:101090"].is_object());
    assert_eq!(finish(&service, body), response);
    assert_eq!(snapshot(root.path()), after);
    service.stop().unwrap();
}
// //// /战绩写入失败时回滚资产和领取凭据 ////
