// audience: internal
// # personal-service-regional-rush-tests
// 狂热激战以开战期别保存成绩, 原始领取凭据和管理重置保留其他期别资产与记录.

use super::*;

const EVENT: i64 = 700006;
const PERIOD: i64 = 1690776000000;

fn rush(service: &PersonalService, viewer: i64, action: &str) -> Value {
    send(
        service,
        &format!("/api/index.php/event/rush/{action}"),
        json!({"viewer_id":viewer,"event_id":EVENT}),
    )
}

fn competition(service: &PersonalService, slot: i64) -> Value {
    manage(
        service,
        "GET",
        &format!("/v1/local-saves/{slot}/competitions"),
        json!({}),
    )["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["activity_id"] == "rush:700006")
        .unwrap()
        .clone()
}

fn start_battle(service: &PersonalService, viewer: i64, play: &str) {
    send(
        service,
        "/api/index.php/event/rush/battle/start",
        json!({
            "viewer_id":viewer,"event_id":EVENT,"quest_id":700006008,"play_id":play,
            "party_id":1,"is_auto_start_mode":false,
        }),
    );
}

fn finish_battle(service: &PersonalService, viewer: i64, play: &str) -> Value {
    send(
        service,
        "/api/index.php/single_battle_quest/finish",
        json!({
            "viewer_id":viewer,"category":24,"quest_id":700006008,"play_id":play,
            "is_accomplished":true,"elapsed_time_ms":30000,"score":100,"add_mana":0,
            "statistics":{"zones":[],"party":{
                "characters":[{"id":1},null,null],"unison_characters":[null,null,null],
                "equipments":[null,null,null],"ability_soul_ids":[null,null,null],
            }},
        }),
    )
}

// //// 往返期别和重置保留原始自动领奖凭据并独立投递区域奖励 [@x380kkm 2026-09-28] ////
#[test]
fn rush_periods_keep_scores_claims_and_resets_independent() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start_service(root.path(), cdn.path());
    let viewer = signup(&service);
    seed(root.path(), |player| {
        player["user_info"]["stamina"] = json!(100);
        player["rush_event_progress"][EVENT.to_string()] = json!({
            "endless_battle_max_round":3,"endless_battle_next_round":4,
            "best_elapsed_time_ms":60000,
        });
    });
    let slot = manage(&service, "GET", "/v1/local-saves", json!({}))["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let management = format!("/v1/local-saves/{slot}/competitions/rush:{EVENT}");
    set_time(&service, "2025-03-20T12:00:00Z");
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        1
    );
    seed(root.path(), |player| {
        let receipt = player["cn_activity_state"]["event_families"]["rush"][EVENT.to_string()]
            ["ranking_reward_receipt"]
            .as_object_mut()
            .unwrap();
        for key in ["participation_round", "source", "period_start_ms"] {
            receipt.remove(key);
        }
    });
    set_time(&service, "2023-08-15T12:00:00Z");
    assert!(rush(&service, viewer, "summary")["endless_battle_my_ranking"].is_null());
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        3
    );
    let empty = competition(&service, slot);
    assert_eq!(empty["period_start_ms"], PERIOD);
    assert_eq!(empty["has_record"], false);
    set_time(&service, "2023-08-01T12:00:00Z");
    start_battle(&service, viewer, "rush-regional-inflight");
    set_time(&service, "2025-03-20T12:00:00Z");
    assert_eq!(
        finish_battle(&service, viewer, "rush-regional-inflight")["rush_event"]
            ["endless_battle_max_round"],
        1
    );
    assert_eq!(
        rush(&service, viewer, "summary")["endless_battle_max_round"],
        3
    );
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        2
    );
    set_time(&service, "2023-08-15T12:00:00Z");
    let current = competition(&service, slot);
    assert_eq!(current["summary"]["best_round"], 1);
    let request = json!({"expected_round":0,"expected_period_start_ms":PERIOD});
    assert_eq!(
        manage(
            &service,
            "POST",
            &format!("{management}/settle"),
            request.clone()
        )["status"],
        "delivered"
    );
    assert_eq!(
        manage(&service, "POST", &format!("{management}/settle"), request)["status"],
        "already_received"
    );
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        2
    );
    let before = snapshot(root.path());
    manage(
        &service,
        "POST",
        &history_path(&service, "rush:700006", "reset"),
        json!({"expected_round":0}),
    );
    let after = snapshot(root.path());
    assert_eq!(after["rush_event_progress"], before["rush_event_progress"]);
    for field in ["user_info", "item_list", "degree_ids"] {
        assert_eq!(after[field], before[field]);
    }
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        3
    );
    set_time(&service, "2025-03-20T12:00:00Z");
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        2
    );
    assert_eq!(
        rush(&service, viewer, "summary")["endless_battle_max_round"],
        3
    );
    set_time(&service, "2023-08-01T12:00:00Z");
    start_battle(&service, viewer, "rush-regional-reset");
    finish_battle(&service, viewer, "rush-regional-reset");
    set_time(&service, "2023-08-15T12:00:00Z");
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        1
    );
    assert_eq!(
        rush(&service, viewer, "reward")["ranking_reward"]["status"],
        2
    );
    let history = manage(
        &service,
        "GET",
        &history_path(&service, "rush:700006", "history"),
        json!({}),
    );
    let entries = history["entries"].as_array().unwrap();
    let claims = entries
        .iter()
        .filter(|entry| entry["kind"] == "claim")
        .collect::<Vec<_>>();
    assert_eq!(claims.len(), 3);
    assert_eq!(
        claims
            .iter()
            .filter(|entry| entry["details"]["period_start_ms"] == PERIOD)
            .count(),
        2
    );
    let original = claims
        .iter()
        .find(|entry| entry["details"]["period_start_ms"].is_null())
        .unwrap();
    assert_eq!(original["participation_round"], 0);
    let inflight = entries
        .iter()
        .find(|entry| entry["details"]["play_id"] == "rush-regional-inflight")
        .unwrap();
    assert_eq!(inflight["details"]["period_start_ms"], PERIOD);
    assert_eq!(
        inflight["details"]["event_statistics"]["endless_battle_max_round"],
        1
    );
    service.stop().unwrap();
}
