// audience: internal
// # personal-service-cn-active-mission-condition-tests
//
// 使用 CN 任务, 角色和装备定义验证任务前置条件与参战条件的组合.

use super::{advance, battle_fixture, progress, record_active_battle, record_active_counters};
use crate::cn_battle_state::FinishBattleInput;
use crate::cn_mission::conditions::{has_complete_second_board, has_second_board_abilities};
use crate::database::ServiceDatabase;
use serde_json::{json, Map, Value};
use tempfile::TempDir;

fn database(event_id: i64) -> (TempDir, ServiceDatabase) {
    let temporary = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(temporary.path()).unwrap();
    database
        .upsert_always_activity(&format!("active-mission:{event_id}"))
        .unwrap();
    (temporary, database)
}

fn input(main: &[Option<i64>]) -> FinishBattleInput<'_> {
    FinishBattleInput {
        party_id: 1,
        elapsed_time_ms: 5_000,
        score: 0,
        add_mana: 0,
        is_accomplished: true,
        is_lose: false,
        character_ids: &[],
        main_character_ids: main,
        unison_character_ids: &[],
        equipment_ids: &[],
        ability_soul_ids: &[],
        is_multi: false,
        power_flip_count: Some(0),
        dash_count: Some(0),
        skill_count: Some(0),
        max_skill_chain_count: Some(0),
        max_combo_count: Some(0),
        is_host: Some(true),
        is_mvp: Some(false),
    }
}

fn finish(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    input: &FinishBattleInput<'_>,
    statistics: Value,
) {
    let quest = battle_fixture()
        .unwrap()
        .quests
        .values()
        .find(|quest| quest.category == 1 && quest.b_rank_time > 0)
        .unwrap();
    record_active_battle(root, database, 0, quest, input, &statistics, 5).unwrap();
}

fn received(root: &mut Map<String, Value>, id: i64) {
    root.get_mut("all_active_mission_list")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(id.to_string(), json!({"progress":1,"stages":{"1":true}}));
}

// //// 结合旧存档前置阶段和实际角色武器属性推进指南 [@x380kkm 2026-09-21] ////
#[test]
fn guide_requires_claimed_predecessor_and_equipped_element() {
    let (_temporary, database) = database(2);
    let mut root = json!({
        "all_active_mission_list":{"20010":{"progress":1,"stages":[{"stage":1,"received":true}]}},
        "user_character_list":{"111003":{},"221006":{}},
        "user_equipment_list":{"5010012":{"level":1}},
    })
    .as_object()
    .unwrap()
    .clone();
    finish(&mut root, &database, &input(&[Some(221006)]), json!({}));
    assert_eq!(progress(&root, 20011), 0);
    finish(&mut root, &database, &input(&[Some(111003)]), json!({}));
    assert_eq!(progress(&root, 20011), 1);
    let mut equipped = input(&[Some(111003)]);
    equipped.equipment_ids = &[Some(5010012)];
    finish(&mut root, &database, &equipped, json!({}));
    assert_eq!(progress(&root, 20012), 0);
    received(&mut root, 20011);
    finish(&mut root, &database, &input(&[Some(111003)]), json!({}));
    assert_eq!(progress(&root, 20012), 0);
    finish(&mut root, &database, &equipped, json!({}));
    assert_eq!(progress(&root, 20012), 1);
    assert_eq!(
        root["all_active_mission_list"]["20012"]["stages"]["1"],
        false
    );
}
// //// /结合旧存档前置阶段和实际角色武器属性推进指南 ////

// //// 使用真实减抗技能与辅助位恢复技能完成对应指南 [@x380kkm 2026-09-21] ////
#[test]
fn guide_checks_actual_skill_effects_and_excluded_characters() {
    let (_temporary, database) = database(2);
    let mut root = json!({"all_active_mission_list":{},"user_character_mana_node_list":{}})
        .as_object()
        .unwrap()
        .clone();
    received(&mut root, 20014);
    for id in [1, 151111, 321007] {
        finish(&mut root, &database, &input(&[Some(id)]), json!({}));
        assert_eq!(progress(&root, 20015), 0);
    }
    finish(&mut root, &database, &input(&[Some(111003)]), json!({}));
    assert_eq!(progress(&root, 20015), 1);
    received(&mut root, 20015);
    finish(&mut root, &database, &input(&[Some(111006)]), json!({}));
    assert_eq!(progress(&root, 20016), 0);
    let mut healed = input(&[Some(1)]);
    healed.unison_character_ids = &[Some(131002)];
    finish(&mut root, &database, &healed, json!({}));
    assert_eq!(progress(&root, 20016), 1);
}
// //// /使用真实减抗技能与辅助位恢复技能完成对应指南 ////

// //// 区分能力四至六学会与第二板全满并限制开场技能槽 [@x380kkm 2026-09-21] ////
#[test]
fn guide_uses_learned_abilities_and_first_zone_gauges() {
    let (_temporary, database) = database(2);
    let mut root = json!({
        "all_active_mission_list":{},
        "user_character_mana_node_list":{"121033":[242066401,{"mana_node_multiplied_id":242066407},242066413]},
    }).as_object().unwrap().clone();
    assert!(has_second_board_abilities(&root, 121033).unwrap());
    assert!(!has_complete_second_board(&root, 121033).unwrap());
    received(&mut root, 20006);
    let mut battle = input(&[Some(121033)]);
    finish(&mut root, &database, &battle, json!({}));
    assert_eq!(progress(&root, 20007), 0);
    battle.is_multi = true;
    finish(&mut root, &database, &battle, json!({}));
    assert_eq!(progress(&root, 20007), 1);
    received(&mut root, 20016);
    finish(
        &mut root,
        &database,
        &battle,
        json!({"zones":[{"skill_point_over_on_start":1},{"skill_point_over_on_start":2}]}),
    );
    assert_eq!(progress(&root, 20017), 0);
    finish(
        &mut root,
        &database,
        &battle,
        json!({"zones":[{"skill_point_over_on_start":3},{"skill_point_over_on_start":0}]}),
    );
    assert_eq!(progress(&root, 20017), 1);
}
// //// /区分能力四至六学会与第二板全满并限制开场技能槽 ////

// //// 统计任务使用主表类别并以参战编队战力判断门槛 [@x380kkm 2026-09-21] ////
#[test]
fn comeback_counts_real_actions_and_selected_party_power() {
    let (_temporary, database) = database(150);
    let mut root = json!({
        "all_active_mission_list":{},
        "user_party_group_list":{"1":{"list":{
            "1":{"character_ids":[1],"current_battle_power":4999},
            "2":{"character_ids":[111003],"current_battle_power":90000}
        }}},
    })
    .as_object()
    .unwrap()
    .clone();
    let mut battle = input(&[Some(1)]);
    battle.power_flip_count = Some(7);
    battle.dash_count = Some(11);
    battle.skill_count = Some(13);
    finish(
        &mut root,
        &database,
        &battle,
        json!({"zones":[{"enemy_kill_count":17,"weak_point_attack_count":19}]}),
    );
    for (id, count) in [
        (25009, 17),
        (25010, 11),
        (25011, 19),
        (25012, 7),
        (25013, 13),
    ] {
        assert_eq!(progress(&root, id), count, "{id}");
    }
    assert_eq!(progress(&root, 25017), 4999);
    assert!(root["all_active_mission_list"]["25017"]["stages"]
        .get("1")
        .is_none());
    root["user_party_group_list"]["1"]["list"]["1"]["current_battle_power"] = json!(5000);
    finish(&mut root, &database, &battle, json!({"max_power":4999}));
    assert_eq!(progress(&root, 25017), 4999);
    root["user_party_group_list"]["1"]["list"]["1"]["current_battle_power"] = json!(4999);
    finish(&mut root, &database, &battle, json!({"max_power":5000}));
    assert_eq!(
        root["all_active_mission_list"]["25017"]["stages"]["1"],
        false
    );
    let saved = serde_json::to_string(&root).unwrap();
    let mut restored: Map<String, Value> = serde_json::from_str(&saved).unwrap();
    assert!(advance(&mut restored, 25017, 5000).unwrap().is_none());
    restored["all_active_mission_list"]["25017"]["stages"] = json!({});
    assert!(advance(&mut restored, 25017, 5000).unwrap().is_some());
}
// //// /统计任务使用主表类别并以参战编队战力判断门槛 ////

// //// 从明确领奖凭据恢复前置资格和合集完成数 [@x380kkm 2026-09-22] ////
#[test]
fn receipts_preserve_prerequisites_and_collection_progress() {
    let (_temporary, mut database) = database(2);
    let mut root = json!({"active_mission_receipts":{"20010:1":true}})
        .as_object()
        .unwrap()
        .clone();
    finish(&mut root, &database, &input(&[Some(111003)]), json!({}));
    assert_eq!(progress(&root, 20011), 1);
    database
        .upsert_always_activity("active-mission:150")
        .unwrap();
    for id in 25016..=25023 {
        root.get_mut("active_mission_receipts")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(format!("{id}:1"), json!(true));
    }
    record_active_counters(&mut root, &database, &[]).unwrap();
    assert_eq!(progress(&root, 25024), 8);
    assert_eq!(
        root["all_active_mission_list"]["25024"]["stages"]["1"],
        false
    );
}
// //// /从明确领奖凭据恢复前置资格和合集完成数 ////
