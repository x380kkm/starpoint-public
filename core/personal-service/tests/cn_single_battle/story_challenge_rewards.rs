// audience: internal
// # personal-service-story-challenge-rewards-tests
// 常驻剧情挑战的首通和 S+ 星导石各领取一次, 限时版本按独立掉落表结算.

use super::*;

fn snapshot(database: &Connection) -> Value {
    let data: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

// //// 补充隔离玩家的单场体力并保留库存与关卡进度 [@x380kkm 2026-09-28] ////
fn refill_battle_stamina(database: &Connection) {
    database
        .execute(
            "UPDATE player_snapshots SET data_json=json_set(data_json, '$.user_info.stamina', 100)",
            [],
        )
        .unwrap();
}

// //// 按完整剧情挑战集合验证首通评级奖励及重复结算 [@x380kkm 2026-09-28] ////
#[test]
fn grants_story_challenge_clear_rewards_and_talos_event_drops() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_virtual_time(&service, "2030-01-01T12:00:00.000Z");
    let viewer = decode_response::<SignupData>(&send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest {
            device_id: 928400016,
        }),
    ))
    .data_headers
    .viewer_id;
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../../assets/cn-single-battle.json")).unwrap();
    let challenges: Vec<_> = fixture["quests"]
        .as_object()
        .unwrap()
        .values()
        .filter(|quest| {
            quest["category"] == 18
                && quest["stamina_cost"].as_i64().unwrap_or(0) > 0
                && quest["score_reward_group_id"].is_null()
                && quest["clear_reward_id"] == 55
                && quest["s_plus_reward_id"] == 55
        })
        .collect();
    assert_eq!(challenges.len(), 23);
    assert!(challenges
        .iter()
        .any(|quest| quest["quest_id"] == 400016102));
    let mut player = snapshot(&database);
    player["quest_progress"]["18"] = json!([]);
    database
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
    for quest in challenges {
        let id = quest["quest_id"].as_i64().unwrap();
        set_activity_always(&service, quest["activity_id"].as_str().unwrap());
        for (attempt, elapsed, expected) in [(0, 9_999_999, 75), (1, 1_000, 75), (2, 1_000, 0)] {
            let play = format!("story-challenge-{id}-{attempt}");
            refill_battle_stamina(&database);
            let started = start_battle_for(service.port(), viewer, 18, id, &play, false);
            assert!(started.starts_with("HTTP/1.1 200 OK"), "{id}: {started}");
            let before = snapshot(&database);
            let finished = finish_battle_for(service.port(), viewer, 18, id, &play, elapsed, 1000);
            assert!(finished.starts_with("HTTP/1.1 200 OK"), "{id}: {finished}");
            let result = decode_response::<Value>(&finished).data;
            let saved = snapshot(&database);
            assert_eq!(
                saved["user_info"]["free_vmoney"].as_i64().unwrap()
                    - before["user_info"]["free_vmoney"].as_i64().unwrap(),
                expected + mission_currency(&result, 0),
                "{id}: {attempt}"
            );
            assert!(
                result["drop_score_reward_ids"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{id}"
            );
            assert!(
                result["drop_rare_reward_ids"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{id}"
            );
            let replay = decode_response::<Value>(&finish_battle_for(
                service.port(),
                viewer,
                18,
                id,
                &play,
                elapsed,
                1000,
            ))
            .data;
            assert_eq!(replay, result, "{id}");
            assert_eq!(snapshot(&database), saved, "{id}");
        }
    }
    for (id, coin) in [(500009101, 30132), (500014101, 30136)] {
        let quest = &fixture["quests"][format!("18:{id}")];
        set_activity_always(&service, quest["activity_id"].as_str().unwrap());
        let play = format!("talos-event-{id}");
        refill_battle_stamina(&database);
        let started = start_battle_for(service.port(), viewer, 18, id, &play, false);
        assert!(started.starts_with("HTTP/1.1 200 OK"), "{started}");
        let before = snapshot(&database);
        let result = decode_response::<Value>(&finish_battle_for(
            service.port(),
            viewer,
            18,
            id,
            &play,
            1000,
            1000,
        ))
        .data;
        let saved = snapshot(&database);
        let mut amounts =
            std::collections::BTreeMap::from([(coin, 0_i64), (62, 0), (63, 0), (64, 0), (45, 0)]);
        let rewards = quest["score_rewards"].as_array().unwrap();
        assert_eq!(
            result["drop_score_reward_ids"].as_array().unwrap().len(),
            12,
            "{id}"
        );
        for drop in result["drop_score_reward_ids"].as_array().unwrap() {
            assert_eq!(drop["group_id"], quest["score_reward_group_id"], "{id}");
            let reward = rewards
                .iter()
                .find(|reward| reward["position"] == drop["index"])
                .unwrap();
            assert_eq!(reward["type"], 0, "{id}");
            assert_eq!(reward["reward_type"], 0, "{id}");
            assert_eq!(drop["number"], reward["count"], "{id}");
            *amounts.get_mut(&reward["id"].as_i64().unwrap()).unwrap() +=
                drop["number"].as_i64().unwrap();
        }
        let mut rare_counts = std::collections::BTreeMap::<i64, usize>::new();
        for drop in result["drop_rare_reward_ids"].as_array().unwrap() {
            let group = drop["group_id"].as_i64().unwrap();
            assert!(
                rewards
                    .iter()
                    .any(|reward| reward["type"] == 1 && reward["id"] == group),
                "{id}: {group}"
            );
            let reward = fixture["rare_reward_groups"][group.to_string()]
                .as_array()
                .unwrap()
                .iter()
                .find(|reward| reward["position"] == drop["index"])
                .unwrap();
            assert_eq!(reward["type"], 0, "{id}: {group}");
            assert_eq!(drop["number"], reward["count"], "{id}: {group}");
            *amounts.get_mut(&reward["id"].as_i64().unwrap()).unwrap() +=
                drop["number"].as_i64().unwrap();
            *rare_counts.entry(group).or_default() += 1;
        }
        let mut guaranteed_counts = std::collections::BTreeMap::<i64, usize>::new();
        for reward in rewards
            .iter()
            .filter(|reward| reward["type"] == 1 && reward["rarity"] == 1)
        {
            *guaranteed_counts
                .entry(reward["id"].as_i64().unwrap())
                .or_default() += 1;
        }
        for (group, count) in guaranteed_counts {
            assert_eq!(rare_counts.get(&group), Some(&count), "{id}: {group}");
        }
        for (item, minimum) in [(62, 3), (63, 2), (64, 1)] {
            assert!(amounts[&item] >= minimum, "{id}: {item}");
        }
        for (item, amount) in amounts {
            let key = item.to_string();
            let old = before["item_list"][&key].as_i64().unwrap_or(0);
            let total = saved["item_list"][&key].as_i64().unwrap_or(0);
            assert_eq!(total - old, amount, "{id}: {item}");
            if amount > 0 {
                assert_eq!(result["item_list"][&key], total, "{id}: {item}");
            }
        }
        let replay = decode_response::<Value>(&finish_battle_for(
            service.port(),
            viewer,
            18,
            id,
            &play,
            1000,
            1000,
        ))
        .data;
        assert_eq!(replay, result, "{id}");
        assert_eq!(snapshot(&database), saved, "{id}");
    }
    service.stop().unwrap();
}
