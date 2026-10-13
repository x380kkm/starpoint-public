// audience: internal
// # personal-service-cn-active-rank-tests
// 主动任务的 S+ 次数使用实际战斗评级, 请求重放保持任务进度和资产原值.

use super::*;

// //// 按计分和计时评级累计主动任务并重放结算 [@x380kkm 2026-09-26] ////
#[test]
fn counts_active_s_plus_from_settled_battle_ranks() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    open_activity(&service, "score-attack:1");
    open_activity(&service, "active-mission:150");
    refill_stamina(root.path());
    let initial = snapshot(root.path())["all_active_mission_list"]["25016"]["progress"]
        .as_i64().unwrap_or_default();
    for (play, category, quest, elapsed, score, accomplished, rank, increase) in [
        ("score-s", 27, 1001, 100_000, 1_170_999_999, false, 4, 0),
        ("score-s-plus", 27, 1001, 100_000, 1_171_000_000, false, 5, 1),
        ("timed-s", 1, 10002002, 180_001, 5000, true, 4, 1),
        ("timed-s-plus", 1, 10002002, 180_000, 5000, true, 5, 2),
    ] {
        start(&service, viewer, category, quest, play);
        let mut body = finish_body(viewer, category, quest, play);
        body["elapsed_time_ms"] = json!(elapsed);
        body["score"] = json!(score);
        body["is_accomplished"] = json!(accomplished);
        let response = finish(&service, body.clone());
        let after = snapshot(root.path());
        assert_eq!(response["clear_rank"], rank, "{play}");
        assert_eq!(
            after["all_active_mission_list"]["25016"]["progress"].as_i64().unwrap_or_default(),
            initial + increase,
            "{play}",
        );
        assert_eq!(finish(&service, body), response, "{play}");
        assert_eq!(snapshot(root.path()), after, "{play}");
    }
    service.stop().unwrap();
}
// //// /按计分和计时评级累计主动任务并重放结算 ////
