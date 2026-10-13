// audience: internal
// # personal-service-cn-mission-battle-window-tests
//
// 隔离数据库验证入场收据跨活动截止后的任务奖励, 普通收集与日周周期分别使用结算时间.

use super::*;
use crate::cn_battle_state::FinishBattleInput;
use crate::database::{ActiveSingleQuest, MultiplayerBattleIdentity, MultiplayerBattleStart};
use tempfile::TempDir;

const VIEWER_ID: i64 = 923931;

fn player() -> (TempDir, ServiceDatabase, i64, Map<String, Value>) {
    let storage = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(storage.path()).unwrap();
    let data = crate::cn_player::create_default_player_data(1_790_000_000, "2026-09-23 12:00:00").unwrap();
    let account = database.get_or_create_account_and_rotate_viewer_session(VIEWER_ID, &data).ok().unwrap();
    let root = serde_json::from_str::<Value>(&data).unwrap().as_object().unwrap().clone();
    (storage, database, account.account_id, root)
}

fn mission(category: i64, id: i64) -> &'static MissionDefinition {
    mission_catalog().unwrap().categories[&category].iter().find(|mission| mission.id == id).unwrap()
}

fn set_time(database: &mut ServiceDatabase, time: i64) {
    database.set_virtual_time(true, time * 1000, 1.0).unwrap();
}

fn input() -> FinishBattleInput<'static> {
    FinishBattleInput {
        party_id: 1, elapsed_time_ms: 90_000, score: 1000, add_mana: 0,
        is_accomplished: true, is_lose: false, character_ids: &[],
        main_character_ids: &[], unison_character_ids: &[], equipment_ids: &[], ability_soul_ids: &[],
        is_multi: false, power_flip_count: None, dash_count: None, skill_count: None,
        max_skill_chain_count: None, max_combo_count: None, is_host: None, is_mvp: None,
    }
}

fn start_single(database: &mut ServiceDatabase, account: i64, root: &Map<String, Value>, quest: (i64, i64), time: i64) {
    database.start_active_single_quest_with_receipt(account, &serde_json::to_string(root).unwrap(), &ActiveSingleQuest {
        play_id: "window-single".to_owned(), category: quest.0, quest_id: quest.1,
        use_boss_boost_point: false, use_boost_point: false, is_auto_start_mode: false,
    }, &json!({"start_time": time}).to_string()).unwrap();
}

fn finish(root: &mut Map<String, Value>, database: &mut ServiceDatabase, account: i64, quest: (i64, i64), input: &FinishBattleInput<'_>) -> Vec<Value> {
    let quest = &conditions::battle_fixture().unwrap().quests[&format!("{}:{}", quest.0, quest.1)];
    record_completed_battle_actions(root, database, account, quest, input, &json!({}), 5).unwrap()
}

// //// 在截止后发放本局限时任务与关联完成奖励并保留当前日周周期 [@x380kkm 2026-09-23] ////
#[test]
fn single_battle_keeps_event_chain_and_current_daily_weekly_scopes() {
    let (_storage, mut database, account, mut root) = player();
    let completed_mission = mission(3, 1453);
    let started_at = completed_mission.end_time.unwrap() - 30;
    let completed_at = completed_mission.end_time.unwrap() + 60;
    root.insert("mission_window_progress".to_owned(), json!({}));
    for id in 1448..=1452 {
        let key = MissionKey { category: 3, mission_id: id };
        let scope = window::mission_scope_at(key, mission(3, id), &database, started_at).unwrap().unwrap();
        root.get_mut("mission_window_progress").unwrap()[scope.key] = json!(1);
    }
    start_single(&mut database, account, &root, (13, 1017), started_at);
    set_time(&mut database, completed_at);
    assert!(mission_scope(MissionKey { category: 3, mission_id: 1453 }, completed_mission, &database).unwrap().is_none());
    let rewards = finish(&mut root, &mut database, account, (13, 1017), &input());
    for id in [1453, 1454] {
        assert!(rewards.iter().any(|entry| entry["mission_category_id"] == 3 && entry["mission_id"] == id));
        let scope = window::mission_scope_at(MissionKey { category: 3, mission_id: id }, mission(3, id), &database, started_at).unwrap().unwrap();
        assert!(root["mission_stage_receipts"][scope.receipt(&root, 1)].is_object());
    }
    let mut catalog = build_mission_catalog().unwrap();
    let dependent = catalog.categories.get_mut(&3).unwrap().iter_mut().find(|mission| mission.id == 1454).unwrap();
    dependent.need_mission_id = Some(1453);
    dependent.need_mission_stage = Some(1);
    let dependent_key = MissionKey { category: 3, mission_id: 1454 };
    assert!(dependency_received_at(&root, dependent_key, dependent, &database, started_at).unwrap());
    assert!(!dependency_received(&root, dependent_key, dependent, &database).unwrap());
    let reset = crate::database::parse_iso_timestamp("2026-09-27T21:00:00.000Z").unwrap() / 1000;
    database.upsert_always_activity("pass-card:1").unwrap();
    for (category, id) in [(2, 14), (10, 2), (6, 1), (7, 1)] {
        let key = MissionKey { category, mission_id: id };
        let definition = mission(category, id);
        let previous = window::mission_scope_at(key, definition, &database, reset - 1).unwrap().unwrap();
        let current = window::mission_scope_at(key, definition, &database, reset + 1).unwrap().unwrap();
        let captured = battle_time::BattleMissionWindow { started_at: reset - 1, activities: None };
        let battle = window::mission_scope_for_battle(key, definition, &database, &captured, reset + 1).unwrap().unwrap();
        assert_ne!(previous.key, current.key);
        assert_eq!(battle.key, current.key);
    }
}
// //// /在截止后发放本局限时任务与关联完成奖励并保留当前日周周期 ////

// //// 通过带请求计数的多人入场收据保留限时任务窗口 [@x380kkm 2026-09-23] ////
#[test]
fn multiplayer_battle_uses_persisted_start_response_time() {
    let (_storage, mut database, account, mut root) = player();
    let definition = mission(3, 1224);
    let started_at = definition.end_time.unwrap() - 30;
    let completed_at = definition.end_time.unwrap() + 60;
    let scope = window::mission_scope_at(MissionKey { category: 3, mission_id: 1224 }, definition, &database, started_at).unwrap().unwrap();
    root.insert("mission_window_progress".to_owned(), json!({scope.key.clone(): 2}));
    let room = database.create_multiplayer_room(account, VIEWER_ID, 1, 1, 2, 1_001_001, started_at * 1000, started_at * 1000).unwrap();
    database.start_multiplayer_battle_member(MultiplayerBattleStart {
        identity: MultiplayerBattleIdentity {
            account_id: account, room_number: &room.room_number, play_id: "window-multi",
            category_id: 2, quest_id: 1_001_001, api_count: Some(73),
        },
        snapshot: &serde_json::to_string(&root).unwrap(), response_time: started_at, response_json: "{}",
        use_boss_boost_point: false, use_boost_point: false, is_auto_start_mode: false,
    }).unwrap().unwrap();
    set_time(&mut database, completed_at);
    let mut battle = input();
    battle.is_multi = true;
    battle.is_host = Some(true);
    let rewards = finish(&mut root, &mut database, account, (2, 1_001_001), &battle);
    assert!(rewards.iter().any(|entry| entry["mission_category_id"] == 3 && entry["mission_id"] == 1224));
    assert_eq!(root["mission_window_progress"][&scope.key], 3);
    assert!(root["mission_stage_receipts"][scope.receipt(&root, 1)].is_object());
    let pass_mission = mission(8, 3);
    let pass_end = pass_mission.end_time.unwrap();
    let key = MissionKey { category: 8, mission_id: 3 };
    assert!(window::mission_scope_at(key, pass_mission, &database, pass_end + 60).unwrap().is_none());
    let captured = battle_time::BattleMissionWindow { started_at: pass_end - 30, activities: None };
    assert!(window::mission_scope_for_battle(key, pass_mission, &database, &captured, pass_end + 60).unwrap().is_some());
}
// //// /通过带请求计数的多人入场收据保留限时任务窗口 ////

// //// 战斗掉落沿用入场活动窗口而普通入库使用当前时间 [@x380kkm 2026-09-23] ////
#[test]
fn collects_battle_drops_after_end_without_backdating_other_acquisitions() {
    let (_storage, mut database, account, mut root) = player();
    let end = 1_790_000_000;
    database.upsert_activity_schedule("collect-item:1", true, (end - 300) * 1000, end * 1000).unwrap();
    root.get_mut("item_list").unwrap()["80001"] = json!(0);
    let before = capture_collect_item_counts(&root).unwrap();
    start_single(&mut database, account, &root, (14, 1001), end - 30);
    set_time(&mut database, end + 60);
    root.get_mut("item_list").unwrap()["80001"] = json!(50);
    let mana = root["user_info"]["free_mana"].as_i64().unwrap();
    assert!(record_collected_items(&mut root, &database, account, &before).unwrap().is_empty());
    let rewards = record_battle_collected_items(&mut root, &database, account, &before).unwrap();
    assert!(rewards.iter().any(|entry| entry["mission_category_id"] == 4 && entry["mission_id"] == 1500));
    let key = MissionKey { category: 4, mission_id: 1500 };
    let scope = window::mission_scope_at(key, mission(4, 1500), &database, end - 30).unwrap().unwrap();
    assert_eq!(root["mission_window_progress"][&scope.key], 50);
    assert_eq!(root["user_info"]["free_mana"], mana + 10_000);
    let before_purchase = capture_collect_item_counts(&root).unwrap();
    root.get_mut("item_list").unwrap()["80001"] = json!(100);
    assert!(record_collected_items(&mut root, &database, account, &before_purchase).unwrap().is_empty());
    assert_eq!(root["mission_window_progress"][&scope.key], 50);
    assert_eq!(root["user_info"]["free_mana"], mana + 10_000);
}
// //// /战斗掉落沿用入场活动窗口而普通入库使用当前时间 ////

// //// 恢复临时开放期间的当前局资格并按新入场更新活动资格 [@x380kkm 2026-09-24] ////
#[test]
fn temporary_activity_window_survives_restart_and_stays_with_current_battle() {
    let (storage, mut database, account, mut root) = player();
    let started_at = 1_500_000_000;
    set_time(&mut database, started_at);
    for activity in ["collect-item:1", "active-mission:1", "pass-card:1"] {
        database.upsert_activity_schedule(activity, false, started_at * 1000, (started_at + 60) * 1000).unwrap();
        database.create_activity_temporary_open_lease(activity).ok().unwrap();
    }
    root.get_mut("item_list").unwrap()["80001"] = json!(0);
    root.insert("all_active_mission_list".to_owned(), json!({"12080":{"progress":2,"stages":{}}}));
    for id in [12010, 12020, 12030, 12040, 12050, 12060, 12070, 12090, 12100] {
        root.get_mut("all_active_mission_list").unwrap()[id.to_string()] = json!({"progress":0,"stages":{"1":true}});
    }
    let before = capture_collect_item_counts(&root).unwrap();
    capture_battle_mission_window(&mut root, &database, "window-single", started_at).unwrap();
    start_single(&mut database, account, &root, (2, 1_001_001), started_at);
    for activity in ["collect-item:1", "active-mission:1", "pass-card:1"] {
        assert!(database.delete_activity_temporary_open_lease(activity).unwrap());
    }
    drop(database);
    let mut database = ServiceDatabase::open(storage.path()).unwrap();
    root = serde_json::from_str::<Value>(&database.activity_player_snapshot(account).unwrap()).unwrap().as_object().unwrap().clone();
    set_time(&mut database, started_at + 120);
    let captured = battle_time::battle_mission_window(&root, &database, account, started_at + 120).unwrap();
    for (category, id) in [(6, 1), (7, 1), (8, 3)] {
        let key = MissionKey { category, mission_id: id };
        assert!(mission_scope(key, mission(category, id), &database).unwrap().is_none());
        assert!(window::mission_scope_for_battle(key, mission(category, id), &database, &captured, started_at + 120).unwrap().is_some());
    }
    root.get_mut("item_list").unwrap()["80001"] = json!(50);
    let mana = root["user_info"]["free_mana"].as_i64().unwrap();
    assert!(record_collected_items(&mut root, &database, account, &before).unwrap().is_empty());
    let rewards = record_battle_collected_items(&mut root, &database, account, &before).unwrap();
    assert!(rewards.iter().any(|entry| entry["mission_category_id"] == 4 && entry["mission_id"] == 1500));
    assert_eq!(root["user_info"]["free_mana"], mana + 10_000);
    let quest = &conditions::battle_fixture().unwrap().quests["2:1001001"];
    let mut battle = input();
    battle.is_multi = true;
    let updates = record_active_battle(&mut root, &database, account, quest, &battle, &json!({}), 5).unwrap();
    assert!(updates.iter().any(|entry| entry["mission_id"] == 12080 && entry["progress_value"] == 3));
    assert!(updates.iter().any(|entry| entry["mission_id"] == 12110 && entry["progress_value"] == 10));
    root.get_mut("user_info").unwrap()["total_stamina_used"] = json!(mission(7, 1).stages[0].target.unwrap());
    let completed = finish(&mut root, &mut database, account, (2, 1_001_001), &battle);
    assert!(completed.iter().any(|entry| entry["mission_category_id"] == 7 && entry["mission_id"] == 1));

    capture_battle_mission_window(&mut root, &database, "next-battle", started_at + 120).unwrap();
    database.start_active_single_quest_with_receipt(account, &serde_json::to_string(&root).unwrap(), &ActiveSingleQuest {
        play_id: "next-battle".to_owned(), category: 2, quest_id: 1_001_001,
        use_boss_boost_point: false, use_boost_point: false, is_auto_start_mode: false,
    }, &json!({"start_time": started_at + 120}).to_string()).unwrap();
    let before_next = capture_collect_item_counts(&root).unwrap();
    root.get_mut("item_list").unwrap()["80001"] = json!(100);
    assert!(record_battle_collected_items(&mut root, &database, account, &before_next).unwrap().is_empty());
    assert!(record_active_battle(&mut root, &database, account, quest, &battle, &json!({}), 5).unwrap().is_empty());
    assert_eq!(root["all_active_mission_list"]["12080"]["progress"], 3);
}
// //// /恢复临时开放期间的当前局资格并按新入场更新活动资格 ////

// //// 按入场资格结算租约结束后的团队战任务 [@x380kkm 2026-09-26] ////
#[test]
fn settles_raid_missions_after_temporary_access_expires() {
    let (storage, mut database, account, mut root) = player();
    let started_at = 1_500_000_000;
    set_time(&mut database, started_at);
    database.upsert_activity_schedule("raid:1", false, started_at * 1000, (started_at + 60) * 1000).unwrap();
    database.create_activity_temporary_open_lease("raid:1").ok().unwrap();
    capture_battle_mission_window(&mut root, &database, "window-single", started_at).unwrap();
    start_single(&mut database, account, &root, (23, 1001), started_at);
    database.delete_activity_temporary_open_lease("raid:1").unwrap();
    drop(database);

    let mut database = ServiceDatabase::open(storage.path()).unwrap();
    set_time(&mut database, started_at + 120);
    root = serde_json::from_str::<Value>(&database.activity_player_snapshot(account).unwrap()).unwrap().as_object().unwrap().clone();
    let item_before = root["item_list"]["999003"].as_i64().unwrap_or_default();
    let rewards = finish(&mut root, &mut database, account, (23, 1001), &input());
    assert!(rewards.iter().any(|entry| entry["mission_category_id"] == 3 && entry["mission_id"] == 400001));
    assert_eq!(root["item_list"]["999003"], item_before + 1);

    let key = MissionKey { category: 3, mission_id: 400002 };
    let captured = battle_time::battle_mission_window(&root, &database, account, started_at + 120).unwrap();
    let scope = window::mission_scope_for_battle(key, mission(3, 400002), &database, &captured, started_at + 120).unwrap().unwrap();
    assert_eq!(root["mission_window_progress"][&scope.key], 1);
    capture_battle_mission_window(&mut root, &database, "window-single", started_at + 120).unwrap();
    let rewards = finish(&mut root, &mut database, account, (23, 1001), &input());
    assert!(!rewards.iter().any(|entry| entry["mission_category_id"] == 3 && entry["mission_id"].as_i64().is_some_and(|id| (400001..400013).contains(&id))));
    assert_eq!(root["mission_window_progress"][&scope.key], 1);
}
// //// /按入场资格结算租约结束后的团队战任务 ////
