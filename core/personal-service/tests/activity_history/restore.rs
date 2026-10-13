// audience: internal
// # personal-service-activity-history-restore-tests
// 活动重置在恢复存档后继续分配新轮次, 历史邮件和领取记录保持独立.

use super::*;

// //// 通过存档恢复和活动重置核对新轮次发奖 [@x380kkm 2026-09-22] ////
#[test]
fn allocates_new_rounds_after_restoring_an_older_save() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start_service(root.path(), cdn.path());
    let viewer = signup(&service);
    management_value(&service, "PUT", "/v1/activities/ranking:1/mode", json!({"mode":"always"}));
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mut initial = snapshot(root.path());
    initial["user_info"]["stamina"] = json!(1000);
    db.execute("UPDATE player_snapshots SET data_json=?1", [initial.to_string()]).unwrap();
    let saves = management_value(&service, "GET", "/v1/local-saves", json!({}));
    let slot_id = saves["slots"][0]["id"].as_i64().unwrap();
    let saved = management_value(&service, "GET", &format!("/v1/local-saves/{slot_id}/export"), json!({}));
    let revision = saved["source"]["revisionId"].as_str().unwrap();
    let restore_path = format!("/v1/local-saves/{slot_id}/revisions/{revision}/restore");
    let reset_path = activity_path(&service, "ranking:1", "reset");
    for round in 0..=2 {
        if round == 2 {
            management_value(&service, "POST", &restore_path, json!({}));
        }
        if round > 0 {
            let result = management_value(&service, "POST", &reset_path, json!({"expected_round":0}));
            assert_eq!(result["next_round"], round, "restored save receives an unused round");
        }
        let play_id = format!("restored-ranking-{round}");
        start_battle(&service, viewer, 11, &play_id);
        finish_battle(&service, viewer, 11, &play_id, 40_000, true);
        assert_eq!(send(&service, "/api/index.php/ranking_event/receive_reward", json!({
            "viewer_id":viewer,"ranking_event_id":1,"quest_kind":1,"api_count":1,
        }))["status"], 1);
    }
    let count: i64 = db.query_row("SELECT COUNT(*) FROM player_mails WHERE title LIKE '%排名奖励%'", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 3);
    let records = history(&service, "ranking:1");
    let deliveries = records["entries"].as_array().unwrap().iter().filter(|entry| entry["kind"] == "delivery")
        .map(|entry| entry["participation_round"].as_i64().unwrap()).collect::<Vec<_>>();
    assert_eq!(deliveries, vec![2, 1, 0]);
    for next_round in 3..=4 {
        management_value(&service, "POST", &restore_path, json!({}));
        let result = management_value(&service, "POST", &reset_path, json!({"expected_round":0}));
        assert_eq!(result["next_round"], next_round);
    }
    service.stop().unwrap();
}
// //// /通过存档恢复和活动重置核对新轮次发奖 ////
