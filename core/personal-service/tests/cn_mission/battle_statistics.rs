// audience: internal
// # personal-service-cn-mission-battle-statistics-api-tests
//
// 临时存档通过战斗接口验证任务统计编码, 战斗种类限制, 奖励发放和请求重放.

use super::*;

fn progress(service: &PersonalService, viewer: i64, category: i64, mission: i64) -> i64 {
    let response = decode_response::<Value>(&send(service, "/api/index.php/mission/get_mission_progress", json!({
        "viewer_id": viewer, "api_count": 1, "category_list": [{"category": category}],
    }))).data;
    response["mission_progress_list"].as_array().unwrap().iter()
        .find(|entry| entry["mission_id"] == mission).unwrap()["progress_value"].as_i64().unwrap()
}

fn battle(service: &PersonalService, viewer: i64, play: &str, accomplished: bool, zone: Value) -> (Value, Value) {
    decode_response::<Value>(&send(service, "/api/index.php/single_battle_quest/start", json!({
        "viewer_id": viewer, "category": 1, "quest_id": 10001001, "play_id": play,
        "party_id": 1, "use_boss_boost_point": false, "use_boost_point": false, "is_auto_start_mode": false,
    })));
    let request = json!({
        "viewer_id": viewer, "category": 1, "quest_id": 10001001, "play_id": play,
        "is_accomplished": accomplished, "elapsed_time_ms": 100000, "score": 1000, "add_mana": 0,
        "statistics": {"clear_phase": 1, "zones": [zone], "party": {
            "characters": [{"id": 1}, null, null], "unison_characters": [null, null, null],
            "equipments": [null, null, null], "ability_soul_ids": [null, null, null],
        }},
    });
    let response = decode_response::<Value>(&send(service, "/api/index.php/single_battle_quest/finish", request.clone())).data;
    (request, response)
}

// //// 成功战斗按主表编码发放收集奖励且失败和重放保持进度 [@x380kkm 2026-09-21] ////
#[test]
fn rewards_actual_battle_statistics_once_and_filters_battle_kind() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 921114);
    set_time(&service, "2026-09-21T12:00:00.000Z");
    open_activity(&service, "collect-item:10003");
    let before = load(&service, viewer);
    let zone = json!({
        "use_power_flip_count": 200, "use_dash_count": 100, "use_skill_count": 50,
        "weak_point_attack_count": 50, "enemy_kill_count": 50, "fever_count": 30,
        "use_buff_to_all_party_members": 50, "clear_debuff_of_self_count": 50, "fever_ms": 90050,
    });
    battle(&service, viewer, "statistics-failed", false, zone.clone());
    for mission in [1769, 1770, 1771, 1772] { assert_eq!(collect_progress(&service, viewer, 10003, mission), 0); }
    assert!(read_player(root.path())["character_clear_counts"]["1"].as_i64().unwrap_or_default() == 0);

    let (request, response) = battle(&service, viewer, "statistics-completed", true, zone);
    for (mission, count) in [(1769, 50), (1770, 50), (1771, 30), (1772, 200)] {
        assert_eq!(collect_progress(&service, viewer, 10003, mission), count);
        assert!(response["mission_info"].as_array().unwrap().iter().any(|entry| entry["mission_category_id"] == 4 && entry["mission_id"] == mission));
    }
    for (mission, count) in [(3, 100), (5, 50), (7, 200), (8, 50), (31, 30), (35, 50)] {
        assert_eq!(progress(&service, viewer, 1, mission), count);
    }
    assert!(read_player(root.path())["cleared_regular_mission_list"]["7"].as_i64().unwrap_or_default() > 0);
    assert_eq!(progress(&service, viewer, 5, 21000), 0);
    assert_eq!(progress(&service, viewer, 5, 20000), 50);
    assert_eq!(progress(&service, viewer, 5, 17000), 90);
    let rewarded = load(&service, viewer);
    assert!(rewarded["item_list"]["202"].as_i64().unwrap() >= before["item_list"]["202"].as_i64().unwrap_or_default() + 20);
    assert!(rewarded["user_info"]["free_mana"].as_i64().unwrap() >= before["user_info"]["free_mana"].as_i64().unwrap() + 150000);
    let replay = decode_response::<Value>(&send(&service, "/api/index.php/single_battle_quest/finish", request)).data;
    assert_eq!(replay, response);
    assert_eq!(load(&service, viewer)["item_list"], rewarded["item_list"]);
    assert_eq!(collect_progress(&service, viewer, 10003, 1772), 200);
    battle(&service, viewer, "statistics-remainder", true, json!({"fever_ms": 950}));
    assert_eq!(progress(&service, viewer, 5, 17000), 91);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(collect_progress(&service, viewer, 10003, 1772), 200);
    assert_eq!(progress(&service, viewer, 5, 17000), 91);
    service.stop().unwrap();
}
// //// /成功战斗按主表编码发放收集奖励且失败和重放保持进度 ////

// //// 内容指南入口产生可领取的主动任务进度 [@x380kkm 2026-09-21] ////
#[test]
fn starts_contents_guide_through_its_real_endpoint() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 921115);
    open_activity(&service, "active-mission:2");
    decode_response::<Value>(&send(&service, "/api/index.php/contents_guide/start", json!({"viewer_id": viewer})));
    assert_eq!(read_player(root.path())["all_active_mission_list"]["20001"]["progress"], 1);
    active_claim(&service, viewer, 20001);
    let rewarded = load(&service, viewer);
    active_claim(&service, viewer, 20001);
    assert_eq!(load(&service, viewer)["item_list"], rewarded["item_list"]);
    assert_eq!(load(&service, viewer)["user_info"]["free_vmoney"], rewarded["user_info"]["free_vmoney"]);
    service.stop().unwrap();
}
// //// /内容指南入口产生可领取的主动任务进度 ////
