// audience: internal
// # personal-service-ranking-presentation-tests
// 摘要和领取重放遵守客户端的可选成绩与角色主表引用契约.

use super::*;

// //// 未参赛的所有竞速活动返回客户端可选空成绩 [@x380kkm 2026-09-28] ////
#[test]
fn unplayed_events_keep_empty_summary_for_empty_and_default_parties() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    for empty in [false, true] {
        if empty {
            update_snapshot(root.path(), |player| {
                player["user_party_group_list"] = json!({});
                player["user_character_list"] = json!({});
            });
        }
        for event in [1, 2, 3, 4, 5, 1000, 1001] {
            let result = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/get_summary",
                ranking_request(viewer, event, 1))).data;
            assert!(result["best_record"].is_null(), "{event}: {result}");
        }
    }
    service.stop().unwrap();
}

// //// 成绩和领取缓存保留成绩并恢复有效角色 [@x380kkm 2026-09-28] ////
#[test]
fn old_records_and_reward_replays_have_valid_character_references() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    manage(&service, "PUT", "/v1/activities/ranking:1/mode", json!({"mode":"always"}));
    for leader in [Value::Null, json!(0), json!(999999999)] {
        update_snapshot(root.path(), |player| {
            player["user_party_group_list"] = json!({});
            player["user_info"]["leader_character_id"] = json!(0);
            player["user_character_list"] = json!({"111001":{"evolution_img_level":1}});
            player["quest_progress"]["11"] = json!([{"quest_id":1001,"finished":true,
                "best_elapsed_time_ms":108820,"best_elapsed_score":37,"best_leader_character_id":leader}]);
        });
        let result = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/get_summary",
            ranking_request(viewer, 1, 1))).data;
        assert_eq!(result["leader_character_id"], 111001);
        assert_eq!(result["best_record"]["elapsed_time_ms"], 108820);
        assert_eq!(result["best_record"]["score"], 37);
    }
    let claimed = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/receive_reward",
        ranking_request(viewer, 1, 2))).data;
    assert_eq!(claimed["status"], 1);
    update_snapshot(root.path(), |player| {
        player["ranking_reward_receipts"]["1"]["summary"]["leader_character_id"] = json!(0);
    });
    for endpoint in ["get_summary", "receive_reward"] {
        let result = decode_response::<Value>(&send(&service, &format!("/api/index.php/ranking_event/{endpoint}"),
            ranking_request(viewer, 1, 3))).data;
        assert_eq!(result["leader_character_id"], 111001);
        assert_eq!(result["best_record"]["score"], 37);
        if endpoint == "receive_reward" { assert_eq!(result["status"], 2); }
    }
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mail_count: i64 = db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0)).unwrap();
    assert_eq!(mail_count, 1);
    service.stop().unwrap();
}
