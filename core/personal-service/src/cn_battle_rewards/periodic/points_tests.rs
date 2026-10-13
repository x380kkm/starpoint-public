// audience: internal
// # personal-service-cn-periodic-point-tests

use super::*;

// //// 验证五点日切, 累积上限和时钟回退共用恢复检查点 [@x380kkm 2026-09-28] ////
#[test]
fn recovers_at_five_with_cap_and_monotonic_checkpoint() {
    let catalog = catalog().unwrap();
    let point = &catalog.points[&1];
    let window = PointWindow {
        start_time: point.start_time,
        end_time: point.end_time,
    };
    let reset = (point.start_time + 3 * 3_600).div_euclid(86_400) * 86_400 + 86_400 - 3 * 3_600;
    let mut root = Map::new();
    recover_point(&mut root, 1, point, Some(&window), point.start_time, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(2));
    recover_point(&mut root, 1, point, Some(&window), reset - 1, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(2));
    recover_point(&mut root, 1, point, Some(&window), reset, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(4));
    recover_point(&mut root, 1, point, Some(&window), reset, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(4));
    recover_point(&mut root, 1, point, Some(&window), reset - 10, 5).unwrap();
    assert_eq!(root[RECOVERY_STATE_FIELD]["1"]["last_recovery_time"], reset);
    recover_point(&mut root, 1, point, Some(&window), reset + 3 * 86_400, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(10));
    store_point(&mut root, 1, 98).unwrap();
    recover_point(&mut root, 1, point, Some(&window), reset + 4 * 86_400, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(99));
}

// //// 验证旧余额建立检查点和关闭区间排除每日恢复 [@x380kkm 2026-09-28] ////
#[test]
fn preserves_imported_points_and_excludes_closed_days() {
    let catalog = catalog().unwrap();
    let point = &catalog.points[&1];
    let mut root = json!({"user_periodic_reward_point_list":[{"id":1,"point":22}]})
        .as_object()
        .unwrap()
        .clone();
    let window = PointWindow {
        start_time: point.start_time,
        end_time: point.end_time,
    };
    let at = point.start_time + 4 * 86_400;
    recover_point(&mut root, 1, point, Some(&window), at, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(22));
    recover_point(&mut root, 1, point, None, at + 2 * 86_400, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(22));
    recover_point(&mut root, 1, point, Some(&window), at + 2 * 86_400 + 60, 5).unwrap();
    assert_eq!(stored_point(&root, 1), Some(22));
    let mut missing = Map::new();
    recover_point(&mut missing, 1, point, None, at, 5).unwrap();
    assert!(missing.is_empty());
}

// //// 验证管理开放创建周期点并在关闭时保留余额 [@x380kkm 2026-09-28] ////
#[test]
fn manual_activity_controls_periodic_point_recovery() {
    let catalog = catalog().unwrap();
    let directory = tempfile::TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(directory.path()).unwrap();
    database
        .upsert_manual_activity("hard-multi:1", true)
        .unwrap();
    let mut root = json!({
        "item_list":{},
        "user_periodic_reward_point_list":[],
    })
    .as_object()
    .unwrap()
    .clone();
    let at = catalog.points[&1].end_time + 86_400;
    recover_periodic_reward_points(&mut root, &database, at).unwrap();
    assert_eq!(stored_point(&root, 1), Some(2));
    database
        .upsert_manual_activity("hard-multi:1", false)
        .unwrap();
    let window = BattleMissionWindow {
        started_at: at,
        activities: None,
    };
    assert!(!recover_battle_points(&mut root, &database, catalog, 1, &window).unwrap());
    assert_eq!(stored_point(&root, 1), Some(2));
}
