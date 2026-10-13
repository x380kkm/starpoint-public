// audience: internal
// # personal-service-rescue-policy-tests
// 活动改期决定救援可用性, 活动结束与救援关闭保持已有选择.

use super::*;
use crate::database::{ActivityMode, ActivityPeriod};
use tempfile::TempDir;
use serde_json::{json, Value};

fn unlocked_player(database: &mut ServiceDatabase) -> (i64, Value) {
    let data = crate::cn_player::create_default_player_data(1_790_000_000, "2026-09-25 12:00:00").unwrap();
    let mut player: Value = serde_json::from_str(&data).unwrap();
    crate::cn_player::finish_quest_progress(player.as_object_mut().unwrap(), 1, 1_008_004).unwrap();
    let account = database
        .get_or_create_account_and_rotate_viewer_session(923941, &player.to_string())
        .ok()
        .unwrap();
    (account.account_id, player)
}

// //// 救援选择随活动窗口启停并在重启后保留 [@x380kkm 2026-09-23] ////
#[test]
fn retains_selection_across_activity_expiry_disable_and_restart() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    unlocked_player(&mut database);
    let now = 1_800_000_000_000;
    database.set_virtual_time(true, now, 1.0).unwrap();
    database
        .upsert_activity_rule(
            "hard-multi:1",
            true,
            ActivityMode::Window,
            ActivityPeriod::Once,
            None,
            now - 60_000,
            now + 60_000,
        )
        .unwrap();
    let quest = RescueQuest {
        category_id: 26,
        quest_id: 1001,
    };
    let mut settings = RescuePolicy {
        enabled: true,
        quests: vec![quest],
        ..RescuePolicy::default()
    };
    database.set_rescue_settings(&settings).unwrap();
    assert_eq!(policy(&database).unwrap().quests, vec![quest]);

    database.set_virtual_time(true, now + 61_000, 1.0).unwrap();
    assert!(policy(&database).unwrap().quests.is_empty());
    assert_eq!(database.rescue_settings().unwrap().quests, vec![quest]);

    database.upsert_always_activity("hard-multi:1").unwrap();
    assert_eq!(policy(&database).unwrap().quests, vec![quest]);
    settings.enabled = false;
    database.set_rescue_settings(&settings).unwrap();
    assert!(policy(&database).unwrap().quests.is_empty());
    drop(database);

    let database = ServiceDatabase::open(root.path()).unwrap();
    let restored = database.rescue_settings().unwrap();
    assert!(!restored.enabled);
    assert_eq!(restored.quests, vec![quest]);
}
// //// /救援选择随活动窗口启停并在重启后保留 ////

// //// 救援目录遵守晚开关卡和独立关卡关闭 [@x380kkm 2026-09-25] ////
#[test]
fn rescue_policy_respects_delayed_quests_and_individual_overrides() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let (_, player) = unlocked_player(&mut database);
    let start = 1_800_000_000_000;
    let day = 86_400_000;
    database
        .set_virtual_time(true, start + day, 0.0001)
        .unwrap();
    database
        .upsert_activity_schedule("world-story:100400", true, start, start + 30 * day)
        .unwrap();
    let quest = RescueQuest {
        category_id: 19,
        quest_id: 100400003,
    };
    let settings = RescuePolicy {
        enabled: true,
        quests: vec![quest],
        ..RescuePolicy::default()
    };
    database.set_rescue_settings(&settings).unwrap();
    assert!(policy(&database).unwrap().quests.is_empty());
    database
        .set_virtual_time(true, start + 8 * day, 0.0001)
        .unwrap();
    assert_eq!(policy(&database).unwrap().quests, vec![quest]);
    database
        .upsert_manual_activity("quest:19:100400003", false)
        .unwrap();
    assert!(policy(&database).unwrap().quests.is_empty());
    assert!(!can_enter_rescue_quest(&database, &player, &quest).unwrap());
    assert_eq!(database.rescue_settings().unwrap().quests, vec![quest]);
}

// //// 铃铛目录与新入场共用主线解锁和关卡时窗 [@x380kkm 2026-09-25] ////
#[test]
fn requires_attention_unlock_without_boss_host_progress_or_rank() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let quest = RescueQuest { category_id: 2, quest_id: 1_001_003 };
    database.upsert_always_activity("quest:2:1001003").unwrap();
    assert!(eligible_quest_keys(&database).unwrap().is_empty());
    let (account, mut player) = unlocked_player(&mut database);
    player["user_info"]["rank"] = json!(1);
    player["quest_progress"]["2"] = json!([]);
    database.save_player_snapshot(account, &player.to_string()).unwrap();
    assert!(eligible_quest_keys(&database).unwrap().contains(&(quest.category_id, quest.quest_id)));
    assert!(can_enter_rescue_quest(&database, &player, &quest).unwrap());

    player["quest_progress"]["1"] = json!([{"quest_id":1_008_004,"finished":false}]);
    database.save_player_snapshot(account, &player.to_string()).unwrap();
    assert!(eligible_quest_keys(&database).unwrap().is_empty());
    assert!(!can_enter_rescue_quest(&database, &player, &quest).unwrap());

    player["quest_progress"]["1"][0]["finished"] = json!(true);
    database.upsert_manual_activity("quest:2:1001003", false).unwrap();
    assert!(!can_enter_rescue_quest(&database, &player, &quest).unwrap());
}
