// audience: internal
// # personal-service-cn-mission-companion-tests
//
// 临时存档通过战斗接口验证收集任务活动跟随同时开始的主活动开放, 完成后按主表发放奖励.
// 收集任务活动自己的停用规则优先于跟随.

use super::*;

const ANNIVERSARY_STORY: &str = "world-story:100402";
const ANNIVERSARY_MISSIONS: &str = "collect-item:10004";
const FIRST_MISSION: i64 = 1779;
const FIRST_MISSION_CHARACTER: &str = "213001";

fn clear_single_battle(service: &PersonalService, viewer: i64, play: &str) -> Value {
    decode_response::<Value>(&send(service, "/api/index.php/single_battle_quest/start", json!({
        "viewer_id": viewer, "category": 1, "quest_id": 10001001, "play_id": play,
        "party_id": 1, "use_boss_boost_point": false, "use_boost_point": false, "is_auto_start_mode": false,
    })));
    decode_response::<Value>(&send(service, "/api/index.php/single_battle_quest/finish", json!({
        "viewer_id": viewer, "category": 1, "quest_id": 10001001, "play_id": play,
        "is_accomplished": true, "elapsed_time_ms": 100000, "score": 1000, "add_mana": 0,
        "statistics": {"clear_phase": 1, "zones": [{}], "party": {
            "characters": [{"id": 1}, null, null], "unison_characters": [null, null, null],
            "equipments": [null, null, null], "ability_soul_ids": [null, null, null],
        }},
    }))).data
}

fn completes_first_mission(finish: &Value) -> bool {
    finish["mission_info"].as_array().is_some_and(|missions| missions.iter()
        .any(|mission| mission["mission_category_id"] == 4 && mission["mission_id"] == FIRST_MISSION))
}

fn owns_first_mission_character(root: &Path) -> bool {
    read_player(root)["user_character_list"].get(FIRST_MISSION_CHARACTER).is_some()
}

// //// 一周年配套任务跟随世界剧情开放并在通关后发放角色 [@x380kkm 2026-10-05] ////
#[test]
fn opens_anniversary_missions_with_world_story() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 921500);
    set_time(&service, "2026-10-05T04:00:00.000Z");
    let closed = clear_single_battle(&service, viewer, "companion-before-open");
    assert!(!completes_first_mission(&closed));
    assert!(!owns_first_mission_character(root.path()));

    open_activity(&service, ANNIVERSARY_STORY);
    let opened = clear_single_battle(&service, viewer, "companion-after-open");
    assert!(completes_first_mission(&opened), "{}", opened["mission_info"]);
    assert!(owns_first_mission_character(root.path()));
    service.stop().unwrap();
}
// //// /一周年配套任务跟随世界剧情开放并在通关后发放角色 ////

// //// 单独停用的配套任务不随主活动开放 [@x380kkm 2026-10-05] ////
#[test]
fn keeps_disabled_anniversary_missions_closed() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 921501);
    set_time(&service, "2026-10-05T04:00:00.000Z");
    open_activity(&service, ANNIVERSARY_STORY);
    manage(&service, "PUT", &format!("/v1/activities/calendar/{ANNIVERSARY_MISSIONS}"),
        json!({"enabled":false,"start_at":"2020-01-01T00:00:00.000Z","end_at":"2099-01-01T00:00:00.000Z"}));
    let finish = clear_single_battle(&service, viewer, "companion-disabled");
    assert!(!completes_first_mission(&finish));
    assert!(!owns_first_mission_character(root.path()));
    service.stop().unwrap();
}
// //// /单独停用的配套任务不随主活动开放 ////
