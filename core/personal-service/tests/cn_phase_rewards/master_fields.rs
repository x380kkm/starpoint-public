// audience: internal
// # personal-service-cn-master-reward-tests
// 真实战斗接口按关卡主表结算独立首通奖励, 评级奖励和基础收益.

use super::*;

// //// 首通与最高评级奖励各发放一次并保留基础收益 [@x380kkm 2026-09-26] ////
#[test]
fn settles_master_rewards_once_per_quest() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    manage(
        &service,
        "/v1/activities/hard-multi:1/mode",
        json!({"mode":"always"}),
    );
    manage(
        &service,
        "/v1/time",
        json!({"enabled":true,"iso":"2030-01-01T12:00:00.000Z","rate":1}),
    );
    refill_stamina(root.path());
    for (category, quest, stones, rp, mana, exp) in [
        (1, 8014001, 30, 216, 320, 848),
        (1, 10014001, 30, 195, 745, 310),
        (4, 10001001, 50, 120, 220, 430),
        (26, 1001, 60, 1590, 2790, 2900),
    ] {
        for attempt in 0..2 {
            let play = format!("master-{category}-{quest}-{attempt}");
            start(&service, viewer, category, quest, &play);
            let before = snapshot(root.path());
            let mut body = finish_body(viewer, category, quest, &play);
            body["is_accomplished"] = json!(true);
            let response = finish(&service, body.clone());
            let after = snapshot(root.path());
            assert_eq!(response["clear_rank"], 5);
            assert_eq!(response["rewards"]["reward_mana"], mana);
            assert_eq!(response["rewards"]["reward_pool_exp"], exp);
            assert_eq!(
                after["user_info"]["rank_point"].as_i64().unwrap()
                    - before["user_info"]["rank_point"].as_i64().unwrap(),
                rp
            );
            assert_eq!(
                after["user_info"]["free_vmoney"].as_i64().unwrap()
                    - before["user_info"]["free_vmoney"].as_i64().unwrap()
                    - mission_currency(&response, 0),
                if attempt == 0 { stones } else { 0 }
            );
            assert_eq!(finish(&service, body), response);
            assert_eq!(snapshot(root.path()), after);
        }
    }
    service.stop().unwrap();
}
// //// /首通与最高评级奖励各发放一次并保留基础收益 ////

// //// 练习评级保留半秒边界且首通只结算主表奖励 [@x380kkm 2026-09-26] ////
#[test]
fn practice_uses_fractional_rank_times_without_clear_reward() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    for (elapsed, rank) in [(173_250, 3), (173_501, 2), (70_000, 5)] {
        let play = format!("practice-{elapsed}");
        start(&service, viewer, 15, 1, &play);
        let before = snapshot(root.path());
        let mut body = finish_body(viewer, 15, 1, &play);
        body["elapsed_time_ms"] = json!(elapsed);
        body["is_accomplished"] = json!(true);
        let response = finish(&service, body);
        let after = snapshot(root.path());
        assert_eq!(response["clear_rank"], rank);
        assert_eq!(
            after["user_info"]["free_vmoney"].as_i64().unwrap()
                - before["user_info"]["free_vmoney"].as_i64().unwrap(),
            mission_currency(&response, 0)
        );
    }
    service.stop().unwrap();
}
// //// /练习评级保留半秒边界且首通只结算主表奖励 ////
