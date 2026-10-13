// audience: internal
// # personal-service-multiplayer-periodic-rewards-tests
// 隔离服务验证降临战周期奖励的库存, 点数和战斗收据使用同一次结算.

use super::*;

// //// 设置隔离服务的活动开放状态与游戏时间 [@x380kkm 2026-09-28] ////
fn manage(service: &PersonalService, method: &str, path: &str, body: Value) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", token.as_str())],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{path}: {response}");
}

// //// 验证失败保留周期点和成功收据跨关闭重启重放 [@x380kkm 2026-09-28] ////
#[test]
fn settles_periodic_points_and_replays_rewards_across_restart() {
    let storage = TempDir::new().unwrap();
    let service = start_service(storage.path(), 0);
    manage(
        &service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"iso":"2030-01-01T12:00:00Z","rate":1}),
    );
    let viewer = signup(&service, 928771);
    let before = stored_player_snapshots(storage.path()).remove(0);
    assert_eq!(before["user_periodic_reward_point_list"], json!([]));
    let database = Connection::open(storage.path().join("personal-service.sqlite3")).unwrap();
    database.execute(
        "UPDATE player_snapshots SET data_json=json_set(data_json, '$.user_info.stamina', 1000)",
        [],
    ).unwrap();
    manage(
        &service,
        "PUT",
        "/v1/activities/hard-multi:1/mode",
        json!({"mode":"always"}),
    );
    set_drop_multiplier(&service, 3);
    let mut last_finish = Value::Null;
    let mut last_result = Value::Null;
    for (play, accomplished) in [("periodic-failed", false), ("periodic-clear", true)] {
        let room = send(
            &service,
            "/api/index.php/multi_battle_quest/create_room",
            json!({
                "viewer_id":viewer,"category":26,"quest_id":1001,"party_id":1,
            }),
        );
        let mut start = start_body(viewer, room["room_number"].as_str().unwrap(), play, 1);
        start["category"] = json!(26);
        start["quest_id"] = json!(1001);
        let started = send(&service, "/api/index.php/multi_battle_quest/start", start);
        let before = stored_player_snapshots(storage.path()).remove(0);
        if accomplished {
            manage(
                &service,
                "POST",
                "/v1/activities/hard-multi%3A1/close",
                json!({}),
            );
            manage(
                &service,
                "PUT",
                "/v1/time",
                json!({"enabled":true,"iso":"2030-01-02T12:00:00Z","rate":1}),
            );
        }
        let mut finish = finish_body(viewer, play, 2);
        finish["category"] = json!(26);
        finish["quest_id"] = json!(1001);
        finish["is_accomplished"] = json!(accomplished);
        let result = send(
            &service,
            "/api/index.php/multi_battle_quest/finish",
            finish.clone(),
        );
        let saved = stored_player_snapshots(storage.path()).remove(0);
        if accomplished {
            assert_eq!(result["start_time"], started["start_time"]);
            assert_eq!(
                result["drop_periodic_reward_ids"],
                json!([{"group_id":1,"index":1,"number":9}])
            );
            assert_eq!(
                result["user_periodic_reward_point_list"],
                json!([{"id":1,"point":1}])
            );
            let expected = before["item_list"]["40402"].as_i64().unwrap_or_default() + 9;
            assert_eq!(saved["item_list"]["40402"], expected);
            assert_eq!(result["item_list"]["40402"], expected);
        } else {
            assert_eq!(result["drop_periodic_reward_ids"], json!([]));
            assert_eq!(
                saved["user_periodic_reward_point_list"],
                before["user_periodic_reward_point_list"]
            );
            assert_eq!(saved["item_list"]["40402"], before["item_list"]["40402"]);
        }
        assert_eq!(
            send(
                &service,
                "/api/index.php/multi_battle_quest/finish",
                finish.clone()
            ),
            result
        );
        assert_eq!(stored_player_snapshots(storage.path())[0], saved);
        last_finish = finish;
        last_result = result;
    }
    let saved = stored_player_snapshots(storage.path()).remove(0);
    service.stop().unwrap();
    let service = start_service(storage.path(), 0);
    assert_eq!(
        send(
            &service,
            "/api/index.php/multi_battle_quest/finish",
            last_finish
        ),
        last_result
    );
    assert_eq!(stored_player_snapshots(storage.path())[0], saved);
}
