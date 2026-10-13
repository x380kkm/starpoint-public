// audience: internal
// # personal-service-cn-mana-safety-tests
// 载入保留旧节点记录, 客户端只接收有效状态. 跨板和未取得资格的请求保持存档与资产一致.

use super::*;

// //// 载入异常节点并保留有效学习与原始记录 [@x380kkm 2026-09-24] ////
#[test]
fn projects_legacy_node_records_and_preserves_unrelated_data_on_learning_and_awaking() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 924_601);
    let records = json!([
        {"multiplied_id":2201,"awake_level":-4},
        {"multiplied_id":2202,"awake_level":9},
        {"mana_node_multiplied_id":2202,"awake_level":1},
        {"multiplied_id":2401,"awake_level":3},
        {"multiplied_id":222004201,"awake_level":1},
        {"legacy":"preserved"}
    ]);
    setup_player(root.path(), |player| {
        player["user_character_list"]["1"]["mana_board_index"] = json!(2);
        player["user_character_list"]["1"]["evolution_level"] = json!(99);
        player["user_character_mana_node_list"]["1"] = records.clone();
        player["mission_window_progress"] = json!({"9:11":3,"9:12":100,"9:13":97});
    });
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ))
    .data;
    assert_eq!(
        loaded["user_character_mana_node_list"]["1"],
        json!([
            {"multiplied_id":2201,"awake_level":0}, {"multiplied_id":2202,"awake_level":1}, {"multiplied_id":2401,"awake_level":0}
        ])
    );
    assert_eq!(loaded["user_character_list"]["1"]["evolution_level"], 0);
    assert_eq!(
        read_player(root.path())["user_character_mana_node_list"]["1"],
        records
    );
    decode_response::<Value>(&send(
        &service,
        "learn_mana_node",
        json!({
            "viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2203]
        }),
    ));
    let learned = read_player(root.path());
    assert_eq!(
        &learned["user_character_mana_node_list"]["1"]
            .as_array()
            .unwrap()[..6],
        records.as_array().unwrap()
    );
    let request = json!({"viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201],"awake_level":1});
    let response =
        decode_response::<Value>(&send(&service, "awake_mana_node", request.clone())).data;
    assert_eq!(
        response["user_character_mana_node_list"]["1"][0]["awake_level"],
        1
    );
    let saved = read_player(root.path());
    assert_eq!(
        &saved["user_character_mana_node_list"]["1"]
            .as_array()
            .unwrap()[1..6],
        &records.as_array().unwrap()[1..]
    );
    decode_response::<Value>(&send(&service, "awake_mana_node", request));
    assert_eq!(read_player(root.path()), saved);
    service.stop().unwrap();
}
// //// /载入异常节点并保留有效学习与原始记录 ////

// //// 拒绝混入二板的觉醒请求并保留整单资产 [@x380kkm 2026-09-24] ////
#[test]
fn rejects_unqualified_and_second_board_awakening_without_partial_changes() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 924_602);
    setup_player(root.path(), |player| {
        player["user_character_mana_node_list"]["1"] = json!([2201, 2202, 2401]);
    });
    let before = read_player(root.path());
    let request = json!({"viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201],"awake_level":1});
    assert!(send(&service, "awake_mana_node", request.clone()).contains("mana_board_not_awakened"));
    assert_eq!(read_player(root.path()), before);
    setup_player(root.path(), |player| {
        player["mission_window_progress"] = json!({"9:11":3,"9:12":100,"9:13":97});
    });
    let before = read_player(root.path());
    assert!(send(&service, "awake_mana_node", json!({
        "viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201,2401],"awake_level":1
    })).contains("mana_node_not_awakable"));
    assert_eq!(read_player(root.path()), before);
    decode_response::<Value>(&send(&service, "awake_mana_node", request));
    assert_eq!(
        read_player(root.path())["user_character_mana_node_list"]["1"],
        json!([{"mana_node_multiplied_id":2201,"awake_level":1},2202,2401])
    );
    service.stop().unwrap();
}
// //// /拒绝混入二板的觉醒请求并保留整单资产 ////

// //// 学习按当前开放窗口检查并保留历史二板进度 [@x380kkm 2026-09-24] ////
#[test]
fn preserves_learned_boards_while_rejecting_new_nodes_outside_the_time_window() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 924_603);
    setup_player(root.path(), |player| {
        player["user_character_list"]["1"]["mana_board_index"] = json!(2);
        player["user_character_mana_node_list"]["1"] = json!([2401]);
    });
    let authorization = format!("Bearer {}", service.management_token());
    for (iso, accepted) in [
        ("2014-03-01T00:00:00.000Z", false),
        ("2016-03-01T00:00:00.000Z", true),
    ] {
        let response = request_with_headers(
            service.port(),
            "PUT",
            "/v1/time",
            "application/json",
            &[("Authorization", authorization.as_str())],
            json!({"enabled":true,"iso":iso,"rate":1.0})
                .to_string()
                .as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 200"));
        let before = read_player(root.path());
        let response = send(
            &service,
            "learn_mana_node",
            json!({
                "viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201,2402]
            }),
        );
        if accepted {
            decode_response::<Value>(&response);
            assert!(
                read_player(root.path())["user_character_mana_node_list"]["1"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(2401))
            );
        } else {
            assert!(response.contains("mana_board_not_open"));
            assert_eq!(read_player(root.path()), before);
        }
    }
    service.stop().unwrap();
}
// //// /学习按当前开放窗口检查并保留历史二板进度 ////
