// audience: internal
// # personal-service-ranking-temporary-rewards
// 临时比赛到期后继续统计和领奖, 游戏时间回拨保留现实租约边界.

use super::*;

// //// 在临时比赛的统计期后投递一次奖励 [@x380kkm 2026-09-24] ////
#[test]
fn rewards_expired_temporary_event_after_aggregation() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    update_snapshot(root.path(), |player| {
        player["quest_progress"]["11"] = json!([{
            "quest_id": 1001, "finished": true, "best_elapsed_time_ms": 40000, "high_score": 20
        }]);
    });
    set_time(&service, "2030-01-01T12:00:00.000Z");
    manage(&service, "PUT", "/v1/activities/calendar/ranking:1", json!({
        "enabled": false, "start_at": "2030-01-01T00:00:00.000Z", "end_at": "2030-01-02T00:00:00.000Z"
    }));
    manage(&service, "POST", "/v1/activities/ranking:1/temporary-open", json!({}));
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let during_play = send(&service, "/api/index.php/ranking_event/receive_reward", ranking_request(viewer, 1, 0));
    assert!(during_play.contains("ranking_reward_not_ready"), "{during_play}");
    let early_mail_count: i64 = database.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0)).unwrap();
    assert_eq!(early_mail_count, 0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let day = 86_400_000;
    let end_lease = |days_after_end: i64| {
        database.execute(
            "UPDATE activity_temporary_open_leases SET opened_at_ms=?1, expires_at_ms=?2 WHERE activity_id='ranking:1'",
            params![now - (days_after_end + 1) * day, now - days_after_end * day],
        ).unwrap();
    };
    end_lease(1);
    let too_early = send(&service, "/api/index.php/ranking_event/receive_reward", ranking_request(viewer, 1, 1));
    assert!(too_early.contains("ranking_reward_not_ready"), "{too_early}");
    end_lease(4);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2029-01-01T12:00:00.000Z");
    let summary = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/get_summary",
        ranking_request(viewer, 1, 2))).data;
    assert_eq!(summary["best_record"]["elapsed_time_ms"], 40000);
    for (request, expected) in [(3, 1), (4, 2)] {
        let reward = decode_response::<Value>(&send(&service, "/api/index.php/ranking_event/receive_reward",
            ranking_request(viewer, 1, request))).data;
        assert_eq!(reward["status"], expected);
    }
    let count: i64 = database.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 1);
    end_lease(11);
    let expired = send(&service, "/api/index.php/ranking_event/receive_reward", ranking_request(viewer, 1, 5));
    assert!(expired.contains("activity_disabled"), "{expired}");
    service.stop().unwrap();
}
// //// /在临时比赛的统计期后投递一次奖励 ////
