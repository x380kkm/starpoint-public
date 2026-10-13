// audience: internal
// # personal-service-cn-mission-lifecycle-tests
//
// 该模块通过隔离数据库验证任务周期, 活动物品范围和领取凭据对应的资产变化.

use super::*;
use crate::database::parse_iso_timestamp;
use tempfile::TempDir;

fn player() -> (TempDir, ServiceDatabase, i64, Map<String, Value>) {
    let storage = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(storage.path()).unwrap();
    let data = crate::cn_player::create_default_player_data(1_789_999_000, "2026-09-21 12:00:00").unwrap();
    let account = database.get_or_create_account_and_rotate_viewer_session(921091, &data).ok().unwrap();
    let root = serde_json::from_str::<Value>(&data).unwrap().as_object().unwrap().clone();
    (storage, database, account.account_id, root)
}

fn set_time(database: &mut ServiceDatabase, timestamp: &str) {
    database.set_virtual_time(true, parse_iso_timestamp(&timestamp.replace('Z', ".000Z")).unwrap(), 1.0).unwrap();
}

fn scoped_value(root: &Map<String, Value>, database: &ServiceDatabase, category: i64, id: i64) -> (String, i64) {
    let mission = mission_catalog().unwrap().categories[&category].iter().find(|mission| mission.id == id).unwrap();
    let key = MissionKey { category, mission_id: id };
    let scope = mission_scope(key, mission, database).unwrap().unwrap();
    let value = scoped_progress(root, key, mission, &scope, database).unwrap();
    (scope.key, value)
}

// //// 按北京时间日界和周界分别累计动作 [@x380kkm 2026-09-21] ////
#[test]
fn separates_daily_statistics_and_weekly_counts_at_cn_reset() {
    let (_storage, mut database, account, mut root) = player();
    set_time(&mut database, "2026-09-21T20:59:00Z");
    action::record_pattern_progress(&mut root, &mut database, account, "battle_zone_statistics_count:1", 30).unwrap();
    assert_eq!(scoped_value(&root, &database, 2, 14).1, 0);
    let completed = action::record_pattern_progress(&mut root, &mut database, account, "battle_zone_statistics_count:2", 10).unwrap();
    assert!(completed.iter().any(|info| info["mission_category_id"] == 2 && info["mission_id"] == 14));
    let (first_scope, first_progress) = scoped_value(&root, &database, 2, 14);
    assert_eq!(first_progress, 10);
    assert!(root["mission_stage_receipts"][format!("{first_scope}:1")].is_object());
    set_time(&mut database, "2026-09-21T21:00:00Z");
    assert_eq!(scoped_value(&root, &database, 2, 14).1, 0);
    action::record_pattern_progress(&mut root, &mut database, account, "battle_zone_statistics_count:2", 11).unwrap();
    assert_eq!(scoped_value(&root, &database, 2, 14).1, 1);
    action::record_pattern_progress(&mut root, &mut database, account, "battle_zone_statistics_count:2", 20).unwrap();
    let (second_scope, second_progress) = scoped_value(&root, &database, 2, 14);
    assert_ne!(first_scope, second_scope);
    assert_eq!(second_progress, 10);
    assert!(root["mission_stage_receipts"][format!("{second_scope}:1")].is_object());
    action::record_pattern_progress(&mut root, &mut database, account, "multi_battle_clear_count", 3).unwrap();
    assert_eq!(scoped_value(&root, &database, 10, 2).1, 3);
    set_time(&mut database, "2026-09-27T21:00:00Z");
    action::record_pattern_progress(&mut root, &mut database, account, "multi_battle_clear_count", 4).unwrap();
    assert_eq!(scoped_value(&root, &database, 10, 2).1, 1);
}
// //// /按北京时间日界和周界分别累计动作 ////

// //// 保持实际物品入库量和活动范围一致 [@x380kkm 2026-09-21] ////
#[test]
fn collects_only_acquired_items_in_open_event() {
    let (_storage, mut database, _account, mut root) = player();
    set_time(&mut database, "2026-09-21T12:00:00Z");
    let before = root["user_info"]["free_mana"].as_i64().unwrap();
    let closed = plan_collect_item_missions(&database).unwrap();
    record_collected_item_rewards(&mut root, &closed, &BTreeMap::from([(80_001, 50)]), 1_800_000_000).unwrap();
    assert_eq!(root["user_info"]["free_mana"], before);
    database.create_activity_temporary_open_lease("collect-item:1").unwrap();
    let plan = plan_collect_item_missions(&database).unwrap();
    root.get_mut("item_list").unwrap()["80001"] = json!(5_000);
    record_collected_item_rewards(&mut root, &plan, &BTreeMap::from([(80_002, 100)]), 1_800_000_000).unwrap();
    assert_eq!(scoped_value(&root, &database, 4, 1500).1, 0);
    record_collected_item_rewards(&mut root, &plan, &BTreeMap::from([(80_001, 20)]), 1_800_000_000).unwrap();
    assert_eq!(scoped_value(&root, &database, 4, 1500).1, 20);
    let completed = record_collected_item_rewards(&mut root, &plan, &BTreeMap::from([(80_001, 30)]), 1_800_000_000).unwrap();
    assert!(completed.iter().any(|info| info["mission_id"] == 1500));
    assert_eq!(root["user_info"]["free_mana"], before + 10_000);
    let mut response = json!({});
    apply_mission_reward_response(&mut response, &mut root, &completed, 1, 1_800_000_000).unwrap();
    assert_eq!(response["user_info"]["free_mana"], before + 10_000);
}
// //// /保持实际物品入库量和活动范围一致 ////

// //// 根据明确领取凭据补齐奖励并保持重复载入幂等 [@x380kkm 2026-09-21] ////
#[test]
fn recovers_recorded_rewards_without_repaying_received_items() {
    let (_storage, _database, _account, mut root) = player();
    root.insert("mission_stage_receipts".to_owned(), json!({"1:1:1":true,"5:49000:1":true,"2:11:1":true}));
    root.insert("active_mission_receipts".to_owned(), json!({"20003:1":true,"20002:1":true}));
    let items = root["item_list"].clone();
    let stones = root["user_info"]["free_vmoney"].as_i64().unwrap();
    recover_recorded_mission_rewards(&mut root, 1_800_000_000).unwrap();
    assert_eq!(root["user_info"]["free_vmoney"], stones + 255);
    assert_eq!(root["degree_ids"], json!([49000]));
    assert_eq!(root["item_list"], items);
    assert_eq!(root["mission_stage_receipts"]["2:11:1"], true);
    recover_recorded_mission_rewards(&mut root, 1_800_000_000).unwrap();
    assert_eq!(root["user_info"]["free_vmoney"], stones + 255);
    assert_eq!(root["item_list"], items);
}
// //// /根据明确领取凭据补齐奖励并保持重复载入幂等 ////

// //// 按活动隔离重置范围并给新轮次保留独立领取凭据 [@x380kkm 2026-09-21] ////
#[test]
fn resets_only_selected_activity_and_grants_new_round_rewards() {
    let (_storage, mut database, _account, mut root) = player();
    database.create_activity_temporary_open_lease("collect-item:1").unwrap();
    database.create_activity_temporary_open_lease("collect-item:10003").unwrap();
    let plan = plan_collect_item_missions(&database).unwrap();
    let mana = root["user_info"]["free_mana"].as_i64().unwrap();
    record_collected_item_rewards(&mut root, &plan, &BTreeMap::from([(80001, 50), (50200, 100)]), 1_800_000_000).unwrap();
    let first_receipts = root["mission_stage_receipts"].clone();
    let untouched_progress = scoped_value(&root, &database, 4, 1761);
    root.insert("all_active_mission_list".to_owned(), json!({
        "21010":{"progress":1,"stages":{"1":true}},"20003":{"progress":1,"stages":{"1":true}},
    }));
    root.insert("active_mission_receipts".to_owned(), json!({"21010:1":{"rewards":[]},"20003:1":{"rewards":[]}}));
    root.insert("active_mission_incentive_receipts".to_owned(), json!({"21010":1001,"20003":1002}));
    let assets = root["item_list"].clone();
    let active_state = root["all_active_mission_list"].clone();
    let cleared = reset_activity_missions(&mut root, "collect-item:1", None).unwrap();
    assert_eq!(cleared.len(), 74);
    assert!(cleared.contains(&(4, 1500)));
    assert_eq!(scoped_value(&root, &database, 4, 1500).1, 0);
    assert_eq!(scoped_value(&root, &database, 4, 1761), untouched_progress);
    assert_eq!(root["all_active_mission_list"], active_state);
    assert_eq!(root["item_list"], assets);
    assert_eq!(root["user_info"]["free_mana"], mana + 10_000);
    root.insert("activity_participation_rounds".to_owned(), json!({"collect-item:1":1}));
    record_collected_item_rewards(&mut root, &plan, &BTreeMap::from([(80001, 50)]), 1_800_000_001).unwrap();
    assert_eq!(root["user_info"]["free_mana"], mana + 20_000);
    assert!(root["mission_stage_receipts"].as_object().unwrap().keys().all(|key| first_receipts.get(key).is_none()));
    for key in root["mission_stage_receipts"].as_object().unwrap().keys() {
        assert_eq!(receipt_activity_id("mission_stage_receipts", key).unwrap().as_deref(), Some("collect-item:1"));
    }
    reset_activity_missions(&mut root, "active-mission:3", None).unwrap();
    assert!(root["all_active_mission_list"].get("21010").is_none());
    assert!(root["active_mission_receipts"].get("21010:1").is_none());
    assert!(root["active_mission_incentive_receipts"].get("21010").is_none());
    assert_eq!(root["all_active_mission_list"]["20003"], active_state["20003"]);
    assert!(root["active_mission_receipts"]["20003:1"].is_object());
    assert_eq!(root["active_mission_incentive_receipts"]["20003"], 1002);
    assert_eq!(root["item_list"], assets);
    assert_eq!(root["user_info"]["free_mana"], mana + 20_000);
}
// //// /按活动隔离重置范围并给新轮次保留独立领取凭据 ////

// //// 按区域期隔离任务进度和重置范围并保留原期领奖身份 [@x380kkm 2026-09-28] ////
#[test]
fn preserves_original_mission_receipts_across_regional_resets() {
    for (category, mission_id, activity_id, regional_date) in [
        (3, 400001, "raid:1", "2021-11-28T04:00:00Z"),
        (4, 1761, "collect-item:10003", "2020-09-16T04:00:00Z"),
    ] {
        let (_storage, mut database, _account, mut root) = player();
        let mission = mission_catalog().unwrap().categories[&category].iter()
            .find(|mission| mission.id == mission_id).unwrap();
        let key = MissionKey { category, mission_id };
        let stage = &mission.stages[0];
        let target = stage.target.unwrap();
        let original_at = mission.start_time.unwrap() + 60;
        database.set_virtual_time(true, original_at * 1_000, 1.0).unwrap();
        root.insert("activity_participation_rounds".to_owned(), json!({activity_id:2}));
        let original = mission_scope(key, mission, &database).unwrap().unwrap();
        window::advance_progress(&mut root, &original, "battle_clear_count", target, target).unwrap();
        let original_receipt = original.receipt(&root, stage.stage);
        assert!(settle_mission_stage(&mut root, key, stage, &original_receipt, original_at).unwrap().is_some());

        set_time(&mut database, regional_date);
        let now = database.current_server_time_seconds().unwrap();
        let period = crate::activity_projection::effective_regional_activity_period(
            &database, activity_id, now * 1_000,
        ).unwrap().unwrap().start_at_ms;
        let regional = mission_scope(key, mission, &database).unwrap().unwrap();
        assert_ne!(original.key, regional.key);
        assert_eq!(scoped_progress(&root, key, mission, &regional, &database).unwrap(), 0);
        window::advance_progress(&mut root, &regional, "battle_clear_count", target, target).unwrap();
        let regional_receipt = regional.receipt(&root, stage.stage);
        assert!(settle_mission_stage(&mut root, key, stage, &regional_receipt, now).unwrap().is_some());
        assert!(root["mission_stage_receipts"].get(&original_receipt).is_some());

        assert!(reset_activity_missions(&mut root, activity_id, Some(period)).unwrap().is_empty());
        root["activity_participation_rounds"][activity_id] = json!(3);
        assert_eq!(scoped_progress(&root, key, mission, &regional, &database).unwrap(), 0);
        assert_eq!(scoped_progress(&root, key, mission, &original, &database).unwrap(), target);
        window::advance_progress(&mut root, &regional, "battle_clear_count", target, target).unwrap();
        let repeated_receipt = regional.receipt(&root, stage.stage);
        assert_ne!(regional_receipt, repeated_receipt);
        assert!(settle_mission_stage(&mut root, key, stage, &repeated_receipt, now).unwrap().is_some());

        database.set_virtual_time(true, original_at * 1_000, 1.0).unwrap();
        let returned = mission_scope(key, mission, &database).unwrap().unwrap();
        assert_eq!(returned.receipt(&root, stage.stage), original_receipt);
        assert!(settle_mission_stage(&mut root, key, stage, &original_receipt, original_at).unwrap().is_none());
        assert!(reset_activity_missions(&mut root, activity_id, None).unwrap().contains(&(category, mission_id)));
        assert!(root["mission_stage_receipts"].get(&original_receipt).is_none());
        assert!(root["mission_stage_receipts"].get(&repeated_receipt).is_some());
        assert_eq!(scoped_progress(&root, key, mission, &regional, &database).unwrap(), target);
    }
}
// //// /按区域期隔离任务进度和重置范围并保留原期领奖身份 ////
