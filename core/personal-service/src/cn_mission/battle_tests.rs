// audience: internal
// # personal-service-cn-mission-battle-tests
//
// 隔离数据库覆盖主表关卡, 队长, 限时与队伍条件, 并核对阶段奖励和重启后的进度.

use super::*;
use crate::cn_battle_state::FinishBattleInput;
use tempfile::TempDir;

fn player() -> (TempDir, ServiceDatabase, i64, Map<String, Value>) {
    let storage = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(storage.path()).unwrap();
    let data = crate::cn_player::create_default_player_data(1_790_000_000, "2026-09-22 12:00:00").unwrap();
    let account = database.get_or_create_account_and_rotate_viewer_session(922091, &data).ok().unwrap();
    let root = serde_json::from_str::<Value>(&data).unwrap().as_object().unwrap().clone();
    (storage, database, account.account_id, root)
}

fn input(main: &[Option<i64>]) -> FinishBattleInput<'_> {
    FinishBattleInput {
        party_id:1, elapsed_time_ms:90_000, score:1000, add_mana:0, is_accomplished:true, is_lose:false,
        character_ids:&[], main_character_ids:main, unison_character_ids:&[], equipment_ids:&[], ability_soul_ids:&[],
        is_multi:false, power_flip_count:None, dash_count:None, skill_count:None, max_skill_chain_count:None,
        max_combo_count:None, is_host:None, is_mvp:None,
    }
}

fn finish(root: &mut Map<String, Value>, database: &mut ServiceDatabase, account:i64, quest:(i64,i64), input:&FinishBattleInput<'_>, stats:Value, rank:i64) -> Vec<Value> {
    let quest = conditions::battle_fixture().unwrap().quests.values().find(|q| (q.category,q.quest_id) == quest).unwrap();
    record_completed_battle_actions(root, database, account, quest, input, &stats, rank).unwrap()
}

fn value(root: &Map<String, Value>, category:i64, id:i64) -> i64 {
    root.get("mission_window_progress").and_then(|values| values.get(format!("{category}:{id}"))).and_then(Value::as_i64).unwrap_or_default()
}

// //// 组合关卡与队长限时条件并记录真实阶段奖励 [@x380kkm 2026-09-22] ////
#[test]
fn timed_awake_mission_uses_expert_stage_and_same_battle_leader() {
    let (storage, mut database, account, mut root) = player();
    let mut battle = input(&[Some(231001)]);
    finish(&mut root, &mut database, account, (2,1010004), &battle, json!({}), 5);
    assert_eq!(value(&root,9,2310013),0);
    battle.elapsed_time_ms = 90_001;
    finish(&mut root, &mut database, account, (21,1006), &battle, json!({}), 5);
    assert_eq!(value(&root,9,2310013),0);
    battle.elapsed_time_ms = 90_000;
    battle.main_character_ids = &[Some(1)];
    finish(&mut root, &mut database, account, (21,1006), &battle, json!({}), 5);
    assert_eq!(value(&root,9,2310013),0);
    battle.main_character_ids = &[Some(231001)];
    let rewarded = finish(&mut root, &mut database, account, (21,1006), &battle, json!({}), 5);
    assert_eq!(value(&root,9,2310013),1);
    assert!(rewarded.iter().any(|entry| entry["mission_category_id"] == 9 && entry["mission_id"] == 2310013));
    assert_eq!(root["mission_stage_receipts"]["9:2310013:1"]["rewards"][0]["amount"],3);
    let rewarded = finish(&mut root, &mut database, account, (21,1006), &battle, json!({}), 5);
    assert!(!rewarded.iter().any(|entry| entry["mission_category_id"] == 9 && entry["mission_id"] == 2310013));
    drop(database);
    let database = ServiceDatabase::open(storage.path()).unwrap();
    assert_eq!(database.mission_progress(account).unwrap()[&(9,2310013)],2);
}
// //// /组合关卡与队长限时条件并记录真实阶段奖励 ////

// //// 按单次战斗核对 S+ 与完整队伍和零阵亡统计 [@x380kkm 2026-09-22] ////
#[test]
fn records_s_plus_full_party_and_explicit_zero_statistics() {
    let (_storage, mut database, account, mut root) = player();
    let mut battle = input(&[Some(161002)]);
    finish(&mut root,&mut database,account,(1,10001001),&battle,json!({"zones":[{}]}),4);
    assert_eq!(value(&root,1,2),0);
    assert_eq!(value(&root,9,1610022),0);
    finish(&mut root,&mut database,account,(1,10001001),&battle,json!({"zones":[{"encoffinment_count":0},{}]}),5);
    assert_eq!(value(&root,1,2),1);
    assert_eq!(value(&root,9,1610022),0);
    finish(&mut root,&mut database,account,(1,10001001),&battle,json!({"zones":[{"encoffinment_count":0}]}),5);
    assert_eq!(value(&root,9,1610022),1);
    battle.main_character_ids = &[Some(241063),Some(243007)];
    finish(&mut root,&mut database,account,(1,10001001),&battle,json!({}),5);
    assert_eq!(value(&root,9,2410633),0);
    battle.main_character_ids = &[Some(241063),Some(243007),Some(361009)];
    finish(&mut root,&mut database,account,(1,10001001),&battle,json!({}),5);
    assert_eq!(value(&root,9,2410633),1);
    let context = build_context(&root,mission_catalog().unwrap(),&BTreeMap::new()).unwrap();
    assert_eq!(awake_progress(2410633,&context,mission_catalog().unwrap()),1);
}
// //// /按单次战斗核对 S+ 与完整队伍和零阵亡统计 ////

// //// 按指定角色二板节点计算觉醒任务并过滤关卡档位 [@x380kkm 2026-09-22] ////
#[test]
fn character_board_and_scoped_rank_use_master_conditions() {
    let (_storage, _database, _account, mut root) = player();
    root.insert("character_clear_counts".to_owned(),json!({"141003":100}));
    root.insert("user_character_list".to_owned(),json!({"141003":{},"1":{}}));
    root.insert("quest_progress".to_owned(),json!({"2":[{"quest_id":1028003,"finished":true,"leader_character_id":111001}]}));
    let catalog = mission_catalog().unwrap();
    let context = build_context(&root,catalog,&BTreeMap::new()).unwrap();
    assert_eq!(awake_progress(1410033,&context,catalog),0);
    assert_eq!(awake_progress(1110013,&context,catalog),0);
    let nodes:Value=serde_json::from_str(include_str!("../../../../assets/mana_node.json")).unwrap();
    let ids=nodes["141003"]["2"].as_object().unwrap().keys().map(|id|id.parse::<i64>().unwrap()).collect::<Vec<_>>();
    root.insert("user_character_mana_node_list".to_owned(),json!({"141003":ids}));
    root["quest_progress"]["2"][0]["quest_id"]=json!(1028004);
    let context = build_context(&root,catalog,&BTreeMap::new()).unwrap();
    assert_eq!(awake_progress(1410033,&context,catalog),1);
    assert_eq!(awake_progress(1110013,&context,catalog),1);
}
// //// /按指定角色二板节点计算觉醒任务并过滤关卡档位 ////
