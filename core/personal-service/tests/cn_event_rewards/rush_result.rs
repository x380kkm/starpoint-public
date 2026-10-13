// audience: internal
// # personal-service-cn-rush-result-tests
// 狂热激战的无限关卡成绩按最高层数归属, 普通关卡结算保留活动中的无限挑战状态.

use super::*;

// //// 从真实开战结算读取狂热激战成绩并验证重试保持存档 [@x380kkm 2026-09-27] ////
fn finish_round(
    service: &PersonalService,
    root: &Path,
    viewer: i64,
    play: &str,
    score: i64,
    elapsed: i64,
) -> Value {
    let started = decode_response::<Value>(&send(
        service,
        "/api/index.php/single_battle_quest/start",
        json!({"viewer_id":viewer,"category":24,"quest_id":700006008,"party_id":1,
            "play_id":play,"use_boss_boost_point":false,"use_boost_point":false,
            "is_auto_start_mode":false}),
    ));
    assert_eq!(started.data_headers.result_code, 1);
    let mut body = finish_body(viewer, 24, 700006008, play, true);
    body["score"] = json!(score);
    body["elapsed_time_ms"] = json!(elapsed);
    let response = request(
        service,
        "/api/index.php/single_battle_quest/finish",
        body.clone(),
    );
    let saved = snapshot(root);
    assert_eq!(
        request(service, "/api/index.php/single_battle_quest/finish", body),
        response
    );
    assert_eq!(snapshot(root), saved);
    response
}

fn reset_round(service: &PersonalService, viewer: i64, round: i64) {
    let response = decode_response::<Value>(&send(
        service,
        "/api/index.php/event/rush/reset",
        json!({"viewer_id":viewer,"event_id":700006,"quest_type":99,
            "is_reset_after_target_round":true,"reset_target_id":round}),
    ));
    assert_eq!(response.data_headers.result_code, 1);
}

fn progress(player: &Value) -> &Value {
    player["quest_progress"]["24"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["quest_id"] == 700006008)
        .unwrap()
}
// //// /从真实开战结算读取狂热激战成绩并验证重试保持存档 ////

// //// 跨层和重置后成绩归属最高层且普通关保留无限挑战上下文 [@x380kkm 2026-09-27] ////
#[test]
fn rush_result_keeps_the_highest_round_record_and_context() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    manage(
        &service,
        "PUT",
        "/v1/activities/rush:700006/mode",
        json!({"mode":"always"}),
    );
    update_snapshot(root.path(), |player| {
        player["user_info"]["stamina"] = json!(1000);
        player["cn_activity_state"] = json!({"event_families":{"rush":{"700006":{
            "legacy_state_imported":true,"endless_battle_max_round":0
        }}}});
    });
    let first = finish_round(
        &service,
        root.path(),
        viewer,
        "rush-first",
        9_000_000,
        20_000,
    );
    assert_eq!(first["rush_event"]["high_score"], 9_000_000);
    assert_eq!(
        first["rush_event"]["old_endless_battle_max_round"],
        Value::Null
    );
    assert_eq!(first["rush_event"]["endless_battle_max_round"], 1);
    assert_eq!(first["rush_event"]["endless_battle_next_round"], 2);

    let higher = finish_round(
        &service,
        root.path(),
        viewer,
        "rush-higher",
        4_000_000,
        500_000,
    );
    assert_eq!(higher["rush_event"]["high_score"], 4_000_000);
    assert_eq!(higher["rush_event"]["best_elapsed_time_ms"], 500_000);
    assert_eq!(higher["rush_event"]["old_best_elapsed_time_ms"], 20_000);
    assert_eq!(higher["rush_event"]["endless_battle_max_round"], 2);
    let record = progress(&snapshot(root.path())).clone();
    assert_eq!(record["high_score"], 4_000_000);
    assert_eq!(record["best_elapsed_time_ms"], 500_000);
    assert_eq!(record["clear_rank"], 5);

    let before_failure = snapshot(root.path());
    let failed = battle(&service, viewer, 24, 700006008, "rush-failed", false);
    assert!(failed["rush_event"].is_null());
    let after_failure = snapshot(root.path());
    assert_eq!(progress(&after_failure), &record);
    assert_eq!(
        after_failure["cn_activity_state"],
        before_failure["cn_activity_state"]
    );

    for (play, round, score, elapsed, best_score, best_time) in [
        ("rush-lower", 1, 10_000_000, 10_000, 4_000_000, 500_000),
        ("rush-same-score", 2, 5_000_000, 550_000, 5_000_000, 500_000),
        ("rush-same-time", 2, 2_000_000, 400_000, 5_000_000, 400_000),
    ] {
        reset_round(&service, viewer, round);
        if round == 1 {
            update_snapshot(root.path(), |player| {
                player["cn_activity_state"]["event_families"]["rush"]["700006"]
                    ["endless_battle_next_round"] = json!(0);
            });
        }
        let result = finish_round(&service, root.path(), viewer, play, score, elapsed);
        assert_eq!(result["rush_event"]["high_score"], best_score, "{play}");
        assert_eq!(
            result["rush_event"]["best_elapsed_time_ms"], best_time,
            "{play}"
        );
        assert_eq!(
            result["rush_event"]["endless_battle_max_round"], 2,
            "{play}"
        );
        let saved = snapshot(root.path());
        assert_eq!(progress(&saved)["clear_rank"], 5, "{play}");
        assert_eq!(progress(&saved)["high_score"], best_score, "{play}");
        assert_eq!(
            progress(&saved)["best_elapsed_time_ms"],
            best_time,
            "{play}"
        );
    }

    let before_folder = snapshot(root.path());
    let ordinary = battle(&service, viewer, 24, 700006004, "rush-ordinary", true);
    assert_eq!(ordinary["rush_event"]["endless_battle_max_round"], 2);
    assert_eq!(ordinary["rush_event"]["endless_battle_next_round"], 3);
    assert_eq!(
        ordinary["rush_event"]["endless_battle_played_party_list"],
        before_folder["cn_activity_state"]["event_families"]["rush"]["700006"]
            ["endless_played_party_list"]
    );
    assert_eq!(progress(&snapshot(root.path())), progress(&before_folder));
    service.stop().unwrap();
}
// //// /跨层和重置后成绩归属最高层且普通关保留无限挑战上下文 ////
