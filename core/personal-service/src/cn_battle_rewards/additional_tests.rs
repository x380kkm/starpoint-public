// audience: internal
// # personal-service-additional-reward-tests
// 原始日历和区域日历都包含结束秒, 有效空集合限制对应关卡范围.

use super::*;
use tempfile::TempDir;

// //// 验证收集活动的原始结束秒和区域结束秒 [@x380kkm 2026-09-28] ////
#[test]
fn includes_original_and_regional_end_seconds() {
    let storage = TempDir::new().unwrap();
    let database = ServiceDatabase::open(storage.path()).unwrap();
    let event = catalog()
        .unwrap()
        .events
        .iter()
        .find(|event| event.id == 100008)
        .unwrap();
    for end in [event.end_time, 1_699_880_399] {
        let mut window = BattleMissionWindow {
            started_at: end,
            activities: None,
        };
        assert!(event_available(event, &window, &database).unwrap());
        window.started_at += 1;
        assert!(!event_available(event, &window, &database).unwrap());
    }
}

// //// 验证空范围与无限制范围使用不同匹配结果 [@x380kkm 2026-09-28] ////
#[test]
fn distinguishes_empty_quest_ranges_from_unrestricted_ranges() {
    let fixture = crate::cn_battle_assets::load_battle_fixture().unwrap();
    let quest = &fixture.quests["18:400016102"];
    let mut relation = QuestRelation {
        quest_kind: Some(9),
        quest_parts: [None, None, None],
        reward_relation_id: 1,
    };
    assert!(matches_quest(&relation, quest).unwrap());
    relation.quest_parts[0] = Some(Vec::new());
    assert!(!matches_quest(&relation, quest).unwrap());
    relation.quest_parts[0] = Some(vec![400016]);
    relation.quest_parts[2] = Some(vec![102]);
    assert!(matches_quest(&relation, quest).unwrap());
    relation.quest_parts[2] = Some(vec![101]);
    assert!(!matches_quest(&relation, quest).unwrap());
}
