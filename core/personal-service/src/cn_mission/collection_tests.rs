// audience: internal
// # personal-service-cn-mission-collection-tests
// 隔离快照验证任务奖励继续触发收集时的实际计数与外层库存结算去重.

use super::*;
use tempfile::TempDir;

// //// 将收集任务的物品奖励计入关联任务并保持嵌套结算幂等 [@x380kkm 2026-09-28] ////
#[test]
fn counts_reward_chains_without_recounting_outer_inventory_changes() {
    let storage = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(storage.path()).unwrap();
    let now = crate::database::parse_iso_timestamp("2020-02-22T06:00:00.000Z").unwrap();
    database.set_virtual_time(true, now, 1.0).unwrap();
    let data = crate::cn_player::create_default_player_data(now / 1000, "2020-02-22 14:00:00").unwrap();
    let mut root = serde_json::from_str::<Value>(&data).unwrap().as_object().unwrap().clone();
    root.insert("mission_window_progress".to_owned(), json!({}));
    root.insert("mission_stage_receipts".to_owned(), json!({}));
    let before = capture_collect_item_counts(&root).unwrap();
    let source_before = root["item_list"]["80001"].as_i64().unwrap_or_default();
    let reward_before = root["item_list"]["100000"].as_i64().unwrap_or_default();
    root["item_list"]["80001"] = json!(source_before + 100);
    let plan = plan_collect_item_missions(&database).unwrap();
    let completed = record_collected_item_rewards(&mut root, &plan, &BTreeMap::from([(80001,100)]), now / 1000).unwrap();
    assert_eq!(root["mission_window_progress"]["1:66"], 100);
    assert_eq!(root["item_list"]["100000"], reward_before + 100);
    assert!(completed.iter().any(|entry| entry["mission_category_id"] == 4 && entry["mission_id"] == 1501));
    assert!(completed.iter().any(|entry| entry["mission_category_id"] == 1 && entry["mission_id"] == 66));
    let settled = root.clone();
    assert!(record_collected_items(&mut root, &database, 1, &before).unwrap().is_empty());
    assert!(record_collected_items(&mut root, &database, 1, &before).unwrap().is_empty());
    assert_eq!(root, settled);
}
// //// /将收集任务的物品奖励计入关联任务并保持嵌套结算幂等 ////

// //// 按物品, 玛纳和 EXP 的剩余容量投递任务奖励并复用阶段凭据 [@x380kkm 2026-09-28] ////
#[test]
fn splits_mission_inventory_rewards_between_balances_and_mail() {
    let capacities = crate::cn_reward_capacity::capacities().unwrap();
    let item_limit = capacities.items[&100000];
    let mut root = json!({
        "user_info":{"free_mana":capacities.mana - 40,"paid_mana":10,"exp_pool":capacities.exp_pool - 20,"exp_pooled_time":100},
        "item_list":{"100000":item_limit - 50}
    }).as_object().unwrap().clone();
    let rewards = vec![
        MissionReward { kind:1,amount:100,item_id:Some(100000),character_id:None,equipment_id:None,degree_id:None,pass_card_event_id:None },
        MissionReward { kind:3,amount:100,item_id:None,character_id:None,equipment_id:None,degree_id:None,pass_card_event_id:None },
        MissionReward { kind:5,amount:100,item_id:None,character_id:None,equipment_id:None,degree_id:None,pass_card_event_id:None },
    ];
    assert!(settle_rewards(&mut root, "mission_stage_receipts", "4:1501:event:1:1", &rewards, &[], 100).unwrap().is_some());
    let deliveries: Vec<crate::cn_reward_overflow::Delivery> = serde_json::from_value(root["reward_deliveries"].clone()).unwrap();
    assert_eq!(deliveries.len(), 1);
    let credited = deliveries[0].credited_reward();
    assert_eq!(credited.item_list["100000"], 50);
    assert_eq!(credited.free_mana, 30);
    assert_eq!(credited.exp_pool, 20);
    assert_eq!(deliveries[0].mail.item_list["100000"], 50);
    assert_eq!(deliveries[0].mail.free_mana, 70);
    assert_eq!(deliveries[0].mail.exp_pool, 80);
    assert_eq!(root["item_list"]["100000"], item_limit);
    assert_eq!(root["user_info"]["free_mana"], capacities.mana - 10);
    assert_eq!(root["user_info"]["exp_pool"], capacities.exp_pool);
    let settled = root.clone();
    assert!(settle_rewards(&mut root, "mission_stage_receipts", "4:1501:event:1:1", &rewards, &[], 100).unwrap().is_none());
    assert_eq!(root, settled);
}
// //// /按物品, 玛纳和 EXP 的剩余容量投递任务奖励并复用阶段凭据 ////
