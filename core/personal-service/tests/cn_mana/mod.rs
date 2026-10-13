// audience: internal
// # personal-service-cn-mana-lifecycle-tests
//
// 学习结果分别驱动角色进化和每块板的羁绊状态. 节点重试读取存档等级并保留资产和任务计数.

use super::*;
use rusqlite::{params, Connection};
use serde_json::json;
use std::path::Path;

mod safety;

// //// 读取节点测试的玩家存档 [@x380kkm 2026-09-23] ////
fn read_player(root: &Path) -> Value {
    let connection = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let serialized: String = connection
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&serialized).unwrap()
}
// //// /读取节点测试的玩家存档 ////

// //// 设置节点流程需要的角色和素材 [@x380kkm 2026-09-23] ////
fn setup_player(root: &Path, update: impl FnOnce(&mut Value)) {
    let mut player = read_player(root);
    player["user_info"]["free_mana"] = json!(200_000);
    for id in ["1", "2", "3", "4", "50", "51", "52", "99", "70047"] {
        player["item_list"][id] = json!(1_000);
    }
    update(&mut player);
    Connection::open(root.join("personal-service.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            params![player.to_string()],
        )
        .unwrap();
}
// //// /设置节点流程需要的角色和素材 ////

// //// 创建节点流程玩家 [@x380kkm 2026-09-23] ////
fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}
// //// /创建节点流程玩家 ////

// //// 执行节点 HTTP 操作 [@x380kkm 2026-09-23] ////
fn send(service: &PersonalService, action: &str, request: Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/character/{action}"),
        &encode_request(&request),
    )
}
// //// /执行节点 HTTP 操作 ////

// //// 在技能进化和各能力解锁时进化角色 [@x380kkm 2026-09-23] ////
#[test]
fn evolves_after_required_abilities_before_board_completion() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 923_401);
    setup_player(root.path(), |_| {});
    let first = decode_response::<Value>(&send(&service, "learn_mana_node", json!({
        "viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201,2202,2203,2204,2207,2213]
    }))).data;
    assert_eq!(first["character_list"][0]["evolution_level"], 0);
    let evolved = decode_response::<Value>(&send(
        &service,
        "learn_mana_node",
        json!({
            "viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2219]
        }),
    ))
    .data;
    assert_eq!(evolved["character_list"][0]["evolution_level"], 1);
    assert_eq!(evolved["evolution"]["level"], 1);
    assert_eq!(
        evolved["character_list"][0]["bond_token_list"][0]["status"],
        0
    );
    let saved = read_player(root.path());
    assert_eq!(saved["user_character_list"]["1"]["evolution_level"], 1);
    assert_eq!(
        saved["user_character_mana_node_list"]["1"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    service.stop().unwrap();
}
// //// /在技能进化和各能力解锁时进化角色 ////

// //// 载入和节点重放按已有节点回填角色进化 [@x380kkm 2026-09-23] ////
#[test]
fn reconciles_stored_evolution_on_load_and_replay_without_spending() {
    for (device_id, use_load, skill_awake_level) in [
        (923_403, true, 0),
        (923_404, false, 0),
        (923_405, true, 1),
        (923_406, false, 1),
    ] {
        let root = TempDir::new().unwrap();
        let service = PersonalService::start(root.path(), 0).unwrap();
        let viewer_id = signup(&service, device_id);
        let load_request = LoadRequest {
            keychain: viewer_id,
            viewer_id,
        };
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/load",
            &encode_request(&load_request),
        ));
        setup_player(root.path(), |player| {
            player["user_character_list"]["1"]["evolution_level"] = json!(0);
            player["user_character_mana_node_list"]["1"] =
                json!([2201, 2202, 2203, 2204, 2207, 2213]
                    .into_iter()
                    .map(Value::from)
                    .chain([json!({"multiplied_id":2219,"awake_level":skill_awake_level})])
                    .collect::<Vec<_>>());
        });
        let before = read_player(root.path());
        let response = if use_load {
            decode_response::<Value>(&cn_support::send_request(
                service.port(),
                "/api/index.php/load",
                &encode_request(&load_request),
            ))
            .data
        } else {
            decode_response::<Value>(&send(
                &service,
                "learn_mana_node",
                json!({
                    "viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2219]
                }),
            ))
            .data
        };
        let character = if use_load {
            &response["user_character_list"]["1"]
        } else {
            &response["character_list"][0]
        };
        assert_eq!(character["evolution_level"], 1 + skill_awake_level);
        let after = read_player(root.path());
        assert_eq!(
            after["user_character_list"]["1"]["evolution_level"],
            1 + skill_awake_level
        );
        for balance in ["free_mana", "paid_mana"] {
            assert_eq!(after["user_info"][balance], before["user_info"][balance]);
        }
        for field in [
            "item_list",
            "user_character_mana_node_list",
            "mission_economic_counters",
        ] {
            assert_eq!(after[field], before[field]);
        }
        if !use_load {
            let mut expected = before;
            expected["user_character_list"]["1"]["evolution_level"] = json!(1 + skill_awake_level);
            assert_eq!(after, expected);
            assert_eq!(response["character_list"][0]["evolution_img_level"], 1);
            assert!(response["active_mission_list"]
                .as_array()
                .unwrap()
                .is_empty());
        }
        service.stop().unwrap();
    }
}
// //// /载入和节点重放按已有节点回填角色进化 ////

// //// 跨开放板学习并保持觉醒节点重试一致 [@x380kkm 2026-09-23] ////
#[test]
fn learns_each_open_board_and_replays_without_cost_or_losing_awake_level() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 923_402);
    setup_player(root.path(), |player| {
        player["user_character_list"]["1"]["mana_board_index"] = json!(2);
        player["user_character_list"]["1"]["evolution_level"] = json!(1);
        player["user_character_mana_node_list"]["1"] = json!((2201..2223)
            .chain(2401..2418)
            .map(|id| json!({"multiplied_id":id,"awake_level":i64::from(id==2201)}))
            .collect::<Vec<_>>());
    });
    let request = json!({"viewer_id":viewer_id,"character_id":1,"api_count":7,"mana_node_multiplied_id_list":[2418,2201,2223,2223]});
    let response =
        decode_response::<Value>(&send(&service, "learn_mana_node", request.clone())).data;
    assert_eq!(response["user_info"]["free_mana"], 160_800);
    assert_eq!(
        response["user_character_mana_node_list"]["1"][1]["awake_level"],
        1
    );
    assert_eq!(
        response["character_list"][0]["bond_token_list"][0]["status"],
        1
    );
    assert_eq!(
        response["character_list"][0]["bond_token_list"][1]["status"],
        1
    );
    let ability_mission = response["active_mission_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mission| mission["mission_id"] == 11_070)
        .unwrap();
    assert_eq!(ability_mission["progress_value"], 1);
    let saved = read_player(root.path());
    assert_eq!(
        saved["user_character_mana_node_list"]["1"]
            .as_array()
            .unwrap()
            .len(),
        41
    );
    let replay = decode_response::<Value>(&send(&service, "learn_mana_node", request)).data;
    assert_eq!(
        replay["user_character_mana_node_list"]["1"][1]["awake_level"],
        1
    );
    assert_eq!(replay["user_info"]["free_mana"], 160_800);
    assert_eq!(read_player(root.path()), saved);
    service.stop().unwrap();
}
// //// /跨开放板学习并保持觉醒节点重试一致 ////

// //// 觉醒各类节点按主表收费并限制档位 [@x380kkm 2026-09-23] ////
#[test]
fn awakens_all_cost_categories_once_and_rejects_unsupported_level() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 923_501);
    let nodes = [2201, 2202, 2207, 2208, 2213, 2214, 2219, 2220];
    setup_player(root.path(), |player| {
        player["user_character_list"]["1"]["exp"] = json!(76_272);
        player["user_character_list"]["1"]["evolution_level"] = json!(1);
        player["user_character_mana_node_list"]["1"] = json!((2201..=2223)
            .map(|id| json!({"multiplied_id":id,"awake_level":0}))
            .collect::<Vec<_>>());
        player["mission_window_progress"] = json!({"9:11":3,"9:12":100,"9:13":97});
    });
    let request = json!({"viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":nodes,"awake_level":1});
    let response =
        decode_response::<Value>(&send(&service, "awake_mana_node", request.clone())).data;
    assert_eq!(response["user_info"]["free_mana"], 186_500);
    assert_eq!(response["item_list"]["70047"], 712);
    assert_eq!(response["item_list"]["99"], 988);
    assert_eq!(response["character_list"][0]["evolution_level"], 2);
    assert_eq!(response["character_list"][0]["evolution_img_level"], 1);
    assert_eq!(
        response["character_list"][0]["mana_board_awake"],
        json!({"1":1})
    );
    assert_eq!(response["evolution"]["level"], 2);
    assert_eq!(response["evolution"]["img_level"], 1);
    let saved = read_player(root.path());
    assert_eq!(saved["user_character_list"]["1"]["evolution_level"], 2);
    decode_response::<Value>(&send(&service, "awake_mana_node", request.clone()));
    assert_eq!(read_player(root.path()), saved);
    let mut unsupported = request;
    unsupported["awake_level"] = json!(2);
    let rejected = send(&service, "awake_mana_node", unsupported);
    assert!(rejected.starts_with("HTTP/1.1 400"), "{rejected}");
    assert_eq!(read_player(root.path()), saved);
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ))
    .data;
    assert_eq!(loaded["user_character_list"]["1"]["evolution_level"], 2);
    assert_eq!(
        loaded["user_character_list"]["1"]["mana_board_awake"],
        json!({"1":1})
    );
    service.stop().unwrap();
}
// //// /觉醒各类节点按主表收费并限制档位 ////
