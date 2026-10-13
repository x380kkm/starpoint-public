// audience: internal
// # personal-service-local-raid-tests
// 本地团本的全局次数, 玩家资产和回执共享战斗事务, 领奖通过活动摘要投递邮件.

use super::*;

// //// 回滚失败的团本结算并在重试和重启时保持单次累计 [@x380kkm 2026-09-28] ////
#[test]
fn raid_global_kills_rollback_with_settlement_and_replay_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    manage(
        &service,
        "PUT",
        "/v1/activities/raid:1/mode",
        json!({"mode":"always"}),
    );
    request(
        &service,
        "/api/index.php/event/raid/battle/start",
        json!({
            "viewer_id":viewer,"event_id":1,"quest_id":1001,"play_id":"raid-atomic",
            "party_group_id":1,"is_auto_start_mode":false,
        }),
    );
    let before = snapshot(root.path());
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database.execute_batch(
        "CREATE TRIGGER reject_raid_receipt BEFORE INSERT ON single_battle_finish_receipts
         WHEN NEW.play_id='raid-atomic'
         BEGIN SELECT RAISE(ABORT,'raid receipt failure'); END;",
    ).unwrap();
    let body = finish_body(viewer, 23, 1001, "raid-atomic", true);
    let rejected = send(&service, "/api/index.php/single_battle_quest/finish", body.clone());
    assert!(rejected.starts_with("HTTP/1.1 500"), "{rejected}");
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(database.query_row(
        "SELECT COUNT(*) FROM raid_boss_states WHERE event_id=1",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 0);
    assert_eq!(database.query_row(
        "SELECT COUNT(*) FROM single_battle_finish_receipts WHERE play_id='raid-atomic'",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 0);
    assert_eq!(database.query_row(
        "SELECT COUNT(*) FROM player_mails",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 0);
    database.execute_batch("DROP TRIGGER reject_raid_receipt").unwrap();
    let finished = request(&service, "/api/index.php/single_battle_quest/finish", body.clone());
    assert_eq!(finished["raid_event"]["raid_boss"]["total_kill_count"], 1);
    assert_eq!(finished["raid_event"]["quest_boss"]["kill_count"], 1);
    assert_eq!(
        request(&service, "/api/index.php/single_battle_quest/finish", body.clone()),
        finished,
    );
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(
        request(&service, "/api/index.php/single_battle_quest/finish", body),
        finished,
    );
    assert_eq!(database.query_row(
        "SELECT total_kill_count FROM raid_boss_states WHERE event_id=1",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 1);
    let summary_body = json!({"event_id":1,"viewer_id":viewer});
    let claimed = request(&service, "/api/index.php/event/raid/summary", summary_body.clone());
    assert_eq!(claimed["kill_count_reward_data"]["received_up_to"], 1);
    assert_eq!(claimed["kill_count_reward_data"]["reward_list"], json!([
        {"kind":1,"kind_id":49100,"number":5},
        {"kind":4,"kind_id":null,"number":500},
    ]));
    let repeated = request(&service, "/api/index.php/event/raid/summary", summary_body);
    assert_eq!(repeated["kill_count_reward_data"]["reward_list"], json!([]));
    assert_eq!(database.query_row(
        "SELECT COUNT(*) FROM player_mails",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 1);
    service.stop().unwrap();
}
