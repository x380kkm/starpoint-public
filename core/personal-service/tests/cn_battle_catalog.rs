// audience: internal
// # personal-service-cn-battle-catalog-tests
// 战斗主表中的全部可结算关卡使用隔离存档验证 HTTP 结算和客户端结果字段.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_battle_catalog/client_result.rs"]
mod client_result;

use cn_support::{decode_response, encode_request, send_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

const MASTERS: &[(i64, &str)] = &[
    (1, "main_quest"), (2, "boss_battle_quest"), (4, "ex_quest"),
    (6, "daily_week_event_quest"), (7, "advent_event_quest"),
    (10, "story_event_single_quest"), (11, "ranking_event_single_quest"),
    (13, "challenge_dungeon_event_quest"), (14, "daily_exp_mana_event_quest"),
    (15, "practice_quest"), (18, "world_story_event_quest"),
    (19, "world_story_event_boss_battle_quest"), (20, "tower_dungeon_event_quest"),
    (21, "expert_single_event_quest"), (22, "carnival_event_quest"),
    (23, "raid_event_quest"), (24, "rush_event_quest"),
    (25, "solo_time_attack_event_quest"), (26, "hard_multi_event_quest"),
    (27, "score_attack_event_quest"),
];

// //// 按客户端战斗主表遍历结算并验证收据重放 [@x380kkm 2026-09-27] ////
#[test]
fn finishes_complete_battle_catalog_with_client_result_contract() {
    let fixture: Value = serde_json::from_str(include_str!("../assets/cn-single-battle.json")).unwrap();
    let rules: Value = serde_json::from_str(include_str!("../assets/cn-battle-reward-rules.json")).unwrap();
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = decode_response::<SignupData>(&send_request(
        service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 783 }),
    )).data_headers.viewer_id;
    let mut db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let (account, baseline): (i64, String) = db.query_row(
        "SELECT account_id,data_json FROM player_snapshots", [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    let mut totals = BTreeMap::new();
    for &(category, master) in MASTERS {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets").join(format!("{master}.json"));
        let rows: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for (id, raw) in rows.as_object().unwrap() {
            let key = format!("{category}:{id}");
            if raw.get("bRankTime").is_none() || fixture["quests"].get(&key).is_none() { continue; }
            let quest = &fixture["quests"][&key];
            let quest_id = id.parse::<i64>().unwrap();
            let play = format!("catalog-{key}");
            let tx = db.transaction().unwrap();
            tx.execute("UPDATE player_snapshots SET data_json=?1 WHERE account_id=?2", (&baseline, account)).unwrap();
            tx.execute("INSERT OR REPLACE INTO active_single_quests (account_id,play_id,quest_id,category,use_boss_boost_point,use_boost_point,is_auto_start_mode) VALUES (?1,?2,?3,?4,0,0,0)", (account, &play, quest_id, category)).unwrap();
            tx.commit().unwrap();
            let scripted_loss = rules["lose_quests"].as_array().unwrap().iter().any(|entry| entry == &key);
            let phase = rules["phase_quests"][&key]["phases"].as_array().and_then(|phases| phases.last()).and_then(|phase| phase["phase"].as_i64()).unwrap_or(1);
            let body = json!({
                "viewer_id": viewer, "play_id":play, "quest_id":quest_id, "category":category,
                "elapsed_time_ms":60_000, "score":4_294_967_296_f64, "add_mana":7,
                "is_accomplished":!scripted_loss, "is_lose":scripted_loss,
                "statistics":{"clear_phase":phase, "zones":[], "party":{
                    "characters":[{"id":1},null,null], "unison_characters":[null,null,null],
                    "equipments":[null,null,null], "ability_soul_ids":[null,null,null]}}
            });
            let response = send_request(service.port(), "/api/index.php/single_battle_quest/finish", &encode_request(&body));
            assert!(response.starts_with("HTTP/1.1 200"), "{key}: {response}");
            let result = decode_response::<Value>(&response);
            assert_eq!(result.data_headers.result_code, 1, "{key}");
            client_result::assert_result(&result.data, category, quest, &fixture, &rules, &key);
            let snapshot: String = db.query_row("SELECT data_json FROM player_snapshots WHERE account_id=?1", [account], |row| row.get(0)).unwrap();
            let replay = send_request(service.port(), "/api/index.php/single_battle_quest/finish", &encode_request(&body));
            assert_eq!(decode_response::<Value>(&replay).data, result.data, "{key}: receipt");
            let repeated: String = db.query_row("SELECT data_json FROM player_snapshots WHERE account_id=?1", [account], |row| row.get(0)).unwrap();
            assert_eq!(repeated, snapshot, "{key}: duplicate reward");
            *totals.entry(category).or_insert(0usize) += 1;
        }
        eprintln!("settlement category {category}: {} quests", totals[&category]);
    }
    assert_eq!(totals.len(), MASTERS.len());
    assert_eq!(totals.values().sum::<usize>(), 3140);
    service.stop().unwrap();
}
// //// /按客户端战斗主表遍历结算并验证收据重放 ////
