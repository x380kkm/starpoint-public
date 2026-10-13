// audience: internal
// # personal-service-cn-unlocked-quest-reward-tests
// 门票解锁状态和通关状态独立保存, 首通奖励与 S+ 奖励各发放一次.

use super::*;

// //// 读取隔离玩家快照 [@x380kkm 2026-09-28] ////
fn player_snapshot(database: &Connection) -> Value {
    let data: String = database.query_row(
        "SELECT data_json FROM player_snapshots", [], |row| row.get(0),
    ).unwrap();
    serde_json::from_str(&data).unwrap()
}

// //// 从结算条目汇总指定武器的随机掉落 [@x380kkm 2026-09-28] ////
fn equipment_drops(response: &Value, fixture: &Value, equipment_id: i64) -> i64 {
    response["drop_rare_reward_ids"].as_array().unwrap().iter().filter_map(|drop| {
        let group = drop["group_id"].as_i64()?.to_string();
        let reward = fixture["rare_reward_groups"][group].as_array()?.iter()
            .find(|reward| reward["position"] == drop["index"])?;
        (reward["type"] == 1 && reward["id"] == equipment_id)
            .then(|| drop["number"].as_i64().unwrap())
    }).sum()
}

// //// 结算全部门票解锁关并保留首通和重放语义 [@x380kkm 2026-09-28] ////
#[test]
fn grants_first_clear_rewards_after_unlock_for_every_ticket_quest() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let viewer = decode_response::<SignupData>(&send_request(
        service.port(), "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 928400001 }),
    )).data_headers.viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let fixture: Value = serde_json::from_str(include_str!("../../assets/cn-single-battle.json")).unwrap();
    let unlocks: Value = serde_json::from_str(include_str!("../../assets/cn-quest-unlock-costs.json")).unwrap();
    let mut player = player_snapshot(&database);
    player["user_info"]["stamina"] = json!(5_000);
    player["item_list"]["60001"] = json!(unlocks.as_object().unwrap().len());
    player["quest_progress"]["18"] = json!([]);
    for key in unlocks.as_object().unwrap().keys() {
        if let Some(id) = fixture["quests"][key]["clear_reward"]["id"].as_i64() {
            player["user_equipment_list"].as_object_mut().unwrap().remove(&id.to_string());
        }
    }
    database.execute("UPDATE player_snapshots SET data_json=?1", [player.to_string()]).unwrap();

    for key in unlocks.as_object().unwrap().keys() {
        let quest = fixture["quests"][key]["quest_id"].as_i64().unwrap();
        let unlock = decode_response::<Value>(&send_request(
            service.port(), "/api/index.php/quest/unlock",
            &encode_request(&json!({"viewer_id":viewer, "category":18, "quest_id":quest})),
        ));
        assert_eq!(unlock.data_headers.result_code, 1, "{key}");
        let unlocked = player_snapshot(&database);
        let progress = unlocked["quest_progress"]["18"].as_array().unwrap().iter()
            .find(|entry| entry["quest_id"] == quest).unwrap();
        assert_eq!(progress["finished"], false, "{key}");
        assert_eq!(progress["unlocked"], true, "{key}");
        let play = format!("first-clear-{quest}");
        let started = decode_response::<Value>(&start_battle_for(
            service.port(), viewer, 18, quest, &play, false,
        ));
        assert_eq!(started.data_headers.result_code, 1, "{key}");
        let before = player_snapshot(&database);
        let result = decode_response::<Value>(&finish_battle_for(
            service.port(), viewer, 18, quest, &play, 9_999_999, 10_000,
        ));
        assert_eq!(result.data_headers.result_code, 1, "{key}");
        let saved = player_snapshot(&database);
        let reward = &fixture["quests"][key]["clear_reward"];
        if reward["type"] == 1 {
            let id = reward["id"].as_i64().unwrap();
            assert_eq!(
                saved["user_equipment_list"][id.to_string()]["stack"].as_i64().unwrap() + 1,
                reward["count"].as_i64().unwrap() + equipment_drops(&result.data, &fixture, id),
                "{key}",
            );
        } else {
            assert_eq!(reward["type"], 3, "{key}");
            assert_eq!(
                saved["user_info"]["free_vmoney"].as_i64().unwrap()
                    - before["user_info"]["free_vmoney"].as_i64().unwrap(),
                reward["count"].as_i64().unwrap() + mission_currency(&result.data, 0),
                "{key}",
            );
        }
        let progress = saved["quest_progress"]["18"].as_array().unwrap().iter()
            .find(|entry| entry["quest_id"] == quest).unwrap();
        assert_eq!(progress["finished"], true, "{key}");
        assert_eq!(progress["unlocked"], true, "{key}");
        assert_eq!(progress["clear_rank"], 1, "{key}");
        let replay = decode_response::<Value>(&finish_battle_for(
            service.port(), viewer, 18, quest, &play, 9_999_999, 10_000,
        ));
        assert_eq!(replay.data, result.data, "{key}");
        assert_eq!(player_snapshot(&database), saved, "{key}");
    }

    let quest = 400_006_102;
    for (play, elapsed, expected_reward) in [
        ("cleared-again", 9_999_999, 0),
        ("first-s-plus", 1_000, 120),
        ("repeated-s-plus", 2_000, 0),
    ] {
        let started = decode_response::<Value>(&start_battle_for(
            service.port(), viewer, 18, quest, play, false,
        ));
        assert_eq!(started.data_headers.result_code, 1);
        let before = player_snapshot(&database);
        let result = decode_response::<Value>(&finish_battle_for(
            service.port(), viewer, 18, quest, play, elapsed, 10_000,
        ));
        assert_eq!(result.data_headers.result_code, 1);
        let saved = player_snapshot(&database);
        assert_eq!(
            saved["user_info"]["free_vmoney"].as_i64().unwrap()
                - before["user_info"]["free_vmoney"].as_i64().unwrap(),
            expected_reward + mission_currency(&result.data, 0),
            "{play}",
        );
    }
    let saved = player_snapshot(&database);
    let progress = saved["quest_progress"]["18"].as_array().unwrap().iter()
        .find(|entry| entry["quest_id"] == quest).unwrap();
    assert_eq!(progress["best_elapsed_time_ms"], 1_000);
    assert_eq!(progress["clear_rank"], 5);
    service.stop().unwrap();
}
// //// /结算全部门票解锁关并保留首通和重放语义 ////

