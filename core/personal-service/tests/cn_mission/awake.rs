// audience: internal
// # personal-service-cn-awake-mission-api-tests
// 任务完成响应同步觉醒资格, 已领取记录在载入与重复请求中保持一致.

use super::*;

// //// 查询角色觉醒任务进度 [@x380kkm 2026-09-29] ////
fn query(service: &PersonalService, viewer_id: i64) -> Value {
    decode_response::<Value>(&send(
        service,
        "/api/index.php/mission/get_mission_progress",
        json!({
            "viewer_id":viewer_id,"api_count":1,"category_list":[{"category":9}],
        }),
    ))
    .data
}

// //// 读取角色响应中的觉醒资格 [@x380kkm 2026-09-29] ////
fn qualification(response: &Value, character_id: i64) -> Value {
    response["character_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|character| character["character_id"] == character_id)
        .map(|character| {
            for field in ["join_time", "update_time"] {
                assert!(
                    character[field].is_string(),
                    "{character_id}.{field}: {character}"
                );
            }
            character["mana_board_awake"].clone()
        })
        .unwrap_or(Value::Null)
}

// //// 完成最后一段角色剧情后立即回传资格并保持重复领奖的资产不变 [@x380kkm 2026-09-29] ////
#[test]
fn finishing_last_story_returns_awake_qualification_before_reloading() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 929101);
    set_time(&service, "2026-09-29T12:00:00.000Z");
    update_player(root.path(), |player| {
        player["mission_window_progress"] = json!({"9:12":100,"9:13":97,"9:14":0});
        player["quest_progress"]["3"] = json!([
            {"quest_id":101,"finished":true,"unlocked":true},
            {"quest_id":102,"finished":true,"unlocked":true},
            {"quest_id":103,"finished":false,"unlocked":true},
        ]);
        player["user_info"]["free_mana"] = json!(100000);
        player["item_list"]["99"] = json!(100);
        player["item_list"]["70047"] = json!(100);
        player["user_character_mana_node_list"]["1"] = json!([2201]);
    });
    assert!(qualification(&query(&service, viewer_id), 1).is_null());
    let finished = decode_response::<Value>(&send(
        &service,
        "/api/index.php/story_quest/finish",
        json!({
            "viewer_id":viewer_id,"party_id":1,"category":3,"quest_id":103,
        }),
    ))
    .data;
    assert_eq!(qualification(&finished, 1), json!({"1":1}));
    let character = finished["character_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|character| character["character_id"] == 1)
        .unwrap();
    assert!(character["update_time"].is_string());
    assert_eq!(
        qualification(&query(&service, viewer_id), 1),
        json!({"1":1})
    );
    let before = read_player(root.path())["item_list"].clone();
    let received = decode_response::<Value>(&send(
        &service,
        "/api/index.php/active_mission/receive",
        json!({
            "viewer_id":viewer_id,"active_mission_list":[
                {"mission_id":11,"stages":[1]}, {"mission_id":12,"stages":[1]},
                {"mission_id":13,"stages":[1]}, {"mission_id":14,"stages":[1]},
            ],
        }),
    ))
    .data;
    assert_eq!(qualification(&received, 1), json!({"1":1}));
    assert_eq!(read_player(root.path())["item_list"], before);
    let awoken = decode_response::<Value>(&send(&service, "/api/index.php/character/awake_mana_node", json!({
        "viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201],"awake_level":1,
    }))).data;
    assert_eq!(qualification(&awoken, 1), json!({"1":1}));
    assert!(awoken["character_list"][0]["bond_token_list"].is_array());
    assert_eq!(
        awoken["user_character_mana_node_list"]["1"][0]["awake_level"],
        1
    );
    assert_eq!(
        load(&service, viewer_id)["user_character_list"]["1"]["mana_board_awake"],
        json!({"1":1})
    );
    service.stop().unwrap();
}

// //// 首页交互返回觉醒资格并保持角色加入时间与最后使用时间 [@x380kkm 2026-09-30] ////
#[test]
fn home_interactions_return_complete_awake_deltas_without_changing_saved_characters() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 930101);
    update_player(root.path(), |player| {
        player["mission_stage_receipts"] = json!({
            "9:11:1":true,"9:12:1":true,"9:13:1":true,"9:14:1":true,
        });
        player["user_character_list"]["1"]["join_time"] = json!(1_700_000_000);
        player["user_character_list"]["1"]["update_time"] = json!(1_700_000_060);
    });
    let characters_before = read_player(root.path())["user_character_list"].clone();
    for (index, pattern) in ["home_tap_town_character_count", "home_change_voice_count"]
        .iter()
        .enumerate()
    {
        let response = decode_response::<Value>(&send(
            &service,
            "/api/index.php/mission/update_mission_progress",
            json!({
                "viewer_id":viewer_id,"api_count":index + 1,
                "mission_param_list":[{"progress_value":1,"mission_pattern":pattern}],
            }),
        ));
        assert_eq!(response.data_headers.result_code, 1);
        assert_eq!(qualification(&response.data, 1), json!({"1":1}));
        let character = response.data["character_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|character| character["character_id"] == 1)
            .unwrap();
        assert_eq!(character["join_time"], "2023-11-14 22:13:20");
        assert_eq!(character["update_time"], "2023-11-14 22:14:20");
        assert_eq!(
            read_player(root.path())["user_character_list"],
            characters_before
        );
    }
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let reloaded = load(&service, viewer_id);
    assert_eq!(
        reloaded["user_character_list"]["1"]["join_time"],
        1_700_000_000
    );
    assert_eq!(
        qualification(&query(&service, viewer_id), 1),
        json!({"1":1})
    );
    service.stop().unwrap();
}

// //// 旧支付回执补齐角色日期并保持同一订单的奖励只到账一次 [@x380kkm 2026-09-30] ////
#[test]
fn payment_replay_repairs_awake_character_dates_without_regranting_rewards() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 930102);
    set_time(&service, "2024-06-05T12:00:00.000Z");
    update_player(root.path(), |player| {
        player["mission_stage_receipts"] = json!({
            "9:11:1":true,"9:12:1":true,"9:13:1":true,"9:14:1":true,
        });
        player["user_character_list"]["1"]["join_time"] = json!(1_700_000_000);
    });
    let order = decode_response::<Value>(&send(
        &service,
        "/api/index.php/payment/start",
        json!({
            "viewer_id":viewer_id,"api_count":1,"payment":{"product_id":"com.leiting.wf.stone_50"},
        }),
    ));
    assert_eq!(order.data_headers.result_code, 1);
    let order_id = order.data["order_id"].as_str().unwrap();
    update_player(root.path(), |player| {
        let characters = player["payment_state"]["orders"][order_id]["response"]["character_list"]
            .as_array_mut()
            .unwrap();
        assert!(!characters.is_empty());
        for character in characters {
            character.as_object_mut().unwrap().remove("join_time");
        }
    });
    let granted = read_player(root.path());
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    for _ in 0..2 {
        let finished = decode_response::<Value>(&send(
            &service,
            "/api/index.php/payment/finish",
            json!({
                "viewer_id":viewer_id,"payment":{"order_id":order_id},
            }),
        ));
        assert_eq!(finished.data_headers.result_code, 1);
        assert_eq!(qualification(&finished.data, 1), json!({"1":1}));
        assert_eq!(
            finished.data["character_list"][0]["join_time"],
            "2023-11-14 22:13:20"
        );
        let after = read_player(root.path());
        for field in [
            "user_info",
            "item_list",
            "user_character_list",
            "purchased_times_list",
        ] {
            assert_eq!(after[field], granted[field], "{field}");
        }
    }
    service.stop().unwrap();
}

// //// 从全角色的各类领取记录恢复资格并保持资产与领取状态 [@x380kkm 2026-09-29] ////
#[test]
fn restores_all_awake_characters_from_received_stages_without_regranting_rewards() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../assets/cn-mission-master.json")).unwrap();
    let missions = catalog["categories"]["9"].as_array().unwrap();
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 929102);
    set_time(&service, "2026-09-29T12:00:00.000Z");
    update_player(root.path(), |player| {
        let character = player["user_character_list"]["1"].clone();
        player["mission_stage_receipts"] = json!({});
        player["active_mission_receipts"] = json!({});
        player["all_active_mission_list"] = json!({});
        player["active_mission_list"] = json!([]);
        for (index, mission) in missions.iter().enumerate() {
            let id = mission["id"].as_i64().unwrap();
            player["user_character_list"][(id / 10).to_string()] = character.clone();
            match index % 4 {
                0 => player["mission_stage_receipts"][format!("9:{id}:1")] = json!(true),
                1 => player["active_mission_receipts"][format!("{id}:1")] = json!(true),
                2 => player["all_active_mission_list"][id.to_string()] = json!({"progress":0,"stages":{"1":true}}),
                _ => player["active_mission_list"].as_array_mut().unwrap()
                    .push(json!({"mission_id":id,"progress_value":0,"stages":[{"stage":1,"received":true}]})),
            }
        }
    });
    let before = read_player(root.path())["item_list"].clone();
    let response = query(&service, viewer_id);
    let received = decode_response::<Value>(&send(
        &service,
        "/api/index.php/active_mission/receive",
        json!({
            "viewer_id":viewer_id,"active_mission_list":missions.iter().map(|mission| {
                json!({"mission_id":mission["id"],"stages":[1]})
            }).collect::<Vec<_>>(),
        }),
    ))
    .data;
    for mission in missions {
        let id = mission["id"].as_i64().unwrap();
        let target = mission["stages"][0]["target"].as_i64().unwrap();
        let progress = response["mission_progress_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["mission_id"] == id)
            .unwrap()["progress_value"]
            .as_i64()
            .unwrap();
        assert!(progress >= target, "{id}: {progress} < {target}");
        assert_eq!(qualification(&response, id / 10), json!({"1":1}), "{id}");
        assert_eq!(qualification(&received, id / 10), json!({"1":1}), "{id}");
    }
    assert_eq!(read_player(root.path())["item_list"], before);
    let loaded = load(&service, viewer_id);
    for mission in missions {
        let id = mission["id"].as_i64().unwrap();
        assert_eq!(
            loaded["user_character_list"][(id / 10).to_string()]["mana_board_awake"],
            json!({"1":1}),
            "{id}"
        );
        let stage = loaded["active_mission_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["mission_id"] == id)
            .unwrap();
        assert_eq!(stage["stages"][0]["received"], true, "{id}");
    }
    service.stop().unwrap();
}
