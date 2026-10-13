// audience: internal
// # personal-service-cn-periodic-reward-tests

use super::*;

// //// 验证限时降临战发放主表道具并扣除周期点数 [@x380kkm 2026-09-28] ////
#[test]
fn grants_limited_event_rewards_and_consumes_one_point() {
    let catalog = catalog().unwrap();
    let expected = [
        ("26:1001", 1, 40402),
        ("26:2001", 2, 40401),
        ("26:3001", 3, 40403),
        ("26:100000001", 10000000, 40406),
        ("26:100001001", 10000001, 40404),
        ("26:100002001", 10000002, 40405),
    ];
    for (quest_id, point_id, item_id) in expected {
        let mut root = json!({
            "user_periodic_reward_point_list": [{"id":point_id,"point":2}],
            "item_list": {item_id.to_string(): 7},
        })
        .as_object()
        .unwrap()
        .clone();
        let result = grant_periodic_rewards(
            &mut root,
            &catalog.quests[quest_id],
            catalog,
            0,
            1,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(root["item_list"][item_id.to_string()], 16);
        assert_eq!(points::stored_point(&root, point_id), Some(1));
        assert_eq!(
            result.drop_ids,
            vec![json!({"group_id":point_id,"index":1,"number":9})]
        );
        assert_eq!(
            result.point_list,
            Some(vec![json!({"id":point_id,"point":1})])
        );
    }
}

// //// 验证常驻降临战和耗尽点数保持奖励余额 [@x380kkm 2026-09-28] ////
#[test]
fn permanent_events_and_empty_points_do_not_grant_periodic_rewards() {
    let catalog = catalog().unwrap();
    for quest in catalog
        .quests
        .values()
        .filter(|quest| quest.point_id.is_none())
    {
        let mut root =
            json!({"item_list":{},"user_periodic_reward_point_list":[{"id":1,"point":2}]})
                .as_object()
                .unwrap()
                .clone();
        let before = root.clone();
        let result =
            grant_periodic_rewards(&mut root, quest, catalog, 0, 1, &BTreeMap::new()).unwrap();
        assert!(result.drop_ids.is_empty());
        assert_eq!(root, before);
    }
    let mut root = json!({"item_list":{},"user_periodic_reward_point_list":[{"id":1,"point":0}]})
        .as_object()
        .unwrap()
        .clone();
    let result = grant_periodic_rewards(
        &mut root,
        &catalog.quests["26:1001"],
        catalog,
        0,
        1,
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(result.drop_ids.is_empty());
    assert_eq!(points::stored_point(&root, 1), Some(0));
    assert_eq!(root["item_list"], json!({}));
}

// //// 验证概率筛选与候选加权选择使用各自抽样 [@x380kkm 2026-09-28] ////
#[test]
fn chooses_only_among_successful_probability_candidates() {
    let mut group: Vec<PeriodicReward> = serde_json::from_value(json!([
        {"position":1,"probability":0.25,"reward":{"type":0,"id":1,"count":9}},
        {"position":2,"probability":0.75,"reward":{"type":0,"id":2,"count":9}},
    ]))
    .unwrap();
    let mut rolls = [0.1, 0.5, 0.9].into_iter();
    assert_eq!(
        choose_reward(&group, || Ok(rolls.next().unwrap()))
            .unwrap()
            .unwrap()
            .position,
        2
    );
    assert!(choose_reward(&group, || Ok(0.9)).unwrap().is_none());
    group[0].probability = 0.0;
    group[1].probability = 1.0;
    assert_eq!(
        choose_reward(&group, || Ok(0.0)).unwrap().unwrap().position,
        2
    );
}
