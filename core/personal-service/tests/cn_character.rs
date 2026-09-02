// audience: internal
// # personal-service-cn-character-tests
//
// 该文件验证 CN 角色突破、外观设置及 Mana board 开启和觉醒的快照持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::path::Path;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct MailReceiveAllRequest {
    viewer_id: i64,
}

#[derive(Serialize)]
struct OverLimitRequest {
    viewer_id: i64,
    character_id: i64,
    over_limit_count: i64,
    use_stack: bool,
    item_id: Option<i64>,
}

#[derive(Serialize)]
struct SetIllustrationSettingsRequest {
    viewer_id: i64,
    character_id: i64,
    illustration_settings: Vec<i64>,
}

#[derive(Serialize)]
struct SetProtectionRequest {
    viewer_id: i64,
    protect_character_ids: Vec<i64>,
    unprotect_character_ids: Vec<i64>,
}

#[derive(Serialize)]
struct OpenManaBoardRequest {
    viewer_id: i64,
    character_id: i64,
    mana_board_index: i64,
}

#[derive(Serialize)]
struct AddCharacterFromTownRequest {
    viewer_id: i64,
    character_id: i64,
}

#[derive(Serialize)]
struct LearnManaNodeRequest {
    viewer_id: i64,
    character_id: i64,
    api_count: i64,
    mana_node_multiplied_id_list: Vec<i64>,
}

#[derive(Serialize)]
struct AwakeManaNodeRequest {
    viewer_id: i64,
    character_id: i64,
    mana_node_multiplied_id_list: Vec<i64>,
    awake_level: i64,
}

// //// 修改 CN 测试玩家快照 [@x380kkm 2026-08-23] ////
fn update_player_snapshot(root: &Path, update: impl FnOnce(&mut Value)) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let (account_id, serialized): (i64, String) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("player snapshot is read");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    update(&mut player_data);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id,
            ],
        )
        .expect("player snapshot is updated");
}
// //// /修改 CN 测试玩家快照 ////

// //// 验证 CN 角色保护状态持久化 [@x380kkm 2026-08-24] ////
#[test]
fn persists_character_protection() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 43 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;

    let protection = cn_support::send_request(
        service.port(),
        "/api/index.php/character/set_protection",
        &encode_request(&SetProtectionRequest {
            viewer_id,
            protect_character_ids: vec![1],
            unprotect_character_ids: Vec::new(),
        }),
    );
    assert!(protection.starts_with("HTTP/1.1 200 OK"));

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_character_list"]["1"]["protection"], true);
}
// //// /验证 CN 角色保护状态持久化 ////

// //// 验证城镇角色响应包含客户端图鉴增量 [@x380kkm 2026-08-25] ////
#[test]
fn returns_town_character_encyclopedia_delta() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 44 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;

    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/add_character_from_town",
        &encode_request(&AddCharacterFromTownRequest {
            viewer_id,
            character_id: 512_001,
        }),
    ));

    assert_eq!(response.data["character_list"][0]["character_id"], 512_001);
    assert_eq!(
        response.data["encyclopedia_info"],
        serde_json::json!({"151200101": {"read": false}})
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证城镇角色响应包含客户端图鉴增量 ////

// //// 验证 CN 角色养成快照持久化 [@x380kkm 2026-07-24] ////
#[test]
fn persists_character_growth_operations() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 42 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Character duplicate\",\"body\":\"Character test\",\"sender\":\"Admin\",\"rewards\":{{\"characterList\":[1,1,1,1]}}}}"
    );
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let item_mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Mana materials\",\"body\":\"Mana test\",\"sender\":\"Admin\",\"rewards\":{{\"itemList\":{{\"1\":3,\"99\":2,\"10001\":4,\"70047\":30}}}}}}"
    );
    let item_created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        item_mail.as_bytes(),
    );
    assert!(item_created.starts_with("HTTP/1.1 201 Created"));
    let received = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    );
    assert!(received.starts_with("HTTP/1.1 200 OK"));

    let over_limit = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/over_limit",
        &encode_request(&OverLimitRequest {
            viewer_id,
            character_id: 1,
            over_limit_count: 4,
            use_stack: false,
            item_id: Some(10_001),
        }),
    ));
    assert_eq!(over_limit.data["character_list"][0]["over_limit_step"], 4);
    assert_eq!(over_limit.data["character_list"][0]["stack"], 0);

    let illustration = cn_support::send_request(
        service.port(),
        "/api/index.php/character/set_illustration_settings",
        &encode_request(&SetIllustrationSettingsRequest {
            viewer_id,
            character_id: 1,
            illustration_settings: vec![1, 2],
        }),
    );
    assert!(illustration.starts_with("HTTP/1.1 200 OK"));

    update_player_snapshot(root.path(), |player_data| {
        player_data["user_character_list"]["1"]["exp"] = Value::from(76_272);
        player_data["user_info"]["free_mana"] = Value::from(2_000);
    });

    let board = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/open_mana_board",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 1,
        }),
    ));
    assert_eq!(board.data["character_list"][0]["mana_board_index"], 1);

    let before_learn = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let free_mana_before = before_learn.data["user_info"]["free_mana"]
        .as_i64()
        .expect("free mana is an integer");
    let paid_mana_before = before_learn.data["user_info"]["paid_mana"]
        .as_i64()
        .expect("paid mana is an integer");
    assert_eq!(before_learn.data["item_list"]["99"], 2);
    assert_eq!(before_learn.data["item_list"]["70047"], 30);
    let learned = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/learn_mana_node",
        &encode_request(&LearnManaNodeRequest {
            viewer_id,
            character_id: 1,
            api_count: 1,
            mana_node_multiplied_id_list: vec![2201],
        }),
    ));
    assert_eq!(
        learned.data["user_info"]["free_mana"],
        free_mana_before - 60
    );
    assert_eq!(learned.data["item_list"]["1"], 0);
    assert_eq!(
        learned.data["user_character_mana_node_list"]["1"][0],
        serde_json::json!({"mana_node_multiplied_id": 2201})
    );
    assert_eq!(learned.data["character_list"][0]["evolution_level"], 0);

    update_player_snapshot(root.path(), |player_data| {
        player_data["user_character_mana_node_list"]["1"][0] =
            serde_json::json!({"multiplied_id": 2201});
    });

    let duplicate_node = cn_support::send_request(
        service.port(),
        "/api/index.php/character/learn_mana_node",
        &encode_request(&LearnManaNodeRequest {
            viewer_id,
            character_id: 1,
            api_count: 2,
            mana_node_multiplied_id_list: vec![2201],
        }),
    );
    assert!(duplicate_node.starts_with("HTTP/1.1 400 Bad Request"));

    let awakened_response = cn_support::send_request(
        service.port(),
        "/api/index.php/character/awake_mana_node",
        &encode_request(&AwakeManaNodeRequest {
            viewer_id,
            character_id: 1,
            mana_node_multiplied_id_list: vec![2201],
            awake_level: 1,
        }),
    );
    assert!(
        awakened_response.starts_with("HTTP/1.1 200 OK"),
        "unexpected awake response: {awakened_response}"
    );
    let awakened = decode_response::<Value>(&awakened_response);
    let free_mana_after_awake = free_mana_before - 60 - 1_600;
    assert_eq!(
        awakened.data["user_info"]["free_mana"],
        free_mana_after_awake
    );
    assert_eq!(awakened.data["user_info"]["paid_mana"], paid_mana_before);
    assert_eq!(awakened.data["item_list"]["99"], 0);
    assert_eq!(awakened.data["item_list"]["70047"], 0);
    assert_eq!(
        awakened.data["user_character_mana_node_list"]["1"][0],
        serde_json::json!({"mana_node_multiplied_id": 2201, "awake_level": 1})
    );
    assert_eq!(
        awakened.data["character_list"][0]["mana_board_awake"]["1"],
        1
    );
    assert_eq!(
        awakened.data["character_list"][0]["bond_token_list"],
        before_learn.data["user_character_list"]["1"]["bond_token_list"]
    );

    let repeated_awake = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/awake_mana_node",
        &encode_request(&AwakeManaNodeRequest {
            viewer_id,
            character_id: 1,
            mana_node_multiplied_id_list: vec![2201, 2201],
            awake_level: 1,
        }),
    ));
    assert_eq!(
        repeated_awake.data["user_info"]["free_mana"],
        free_mana_after_awake
    );
    assert_eq!(
        repeated_awake.data["user_character_mana_node_list"]["1"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let awakened_again = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/awake_mana_node",
        &encode_request(&AwakeManaNodeRequest {
            viewer_id,
            character_id: 1,
            mana_node_multiplied_id_list: vec![2201],
            awake_level: 1,
        }),
    ));
    assert_eq!(
        awakened_again.data["user_info"]["free_mana"],
        free_mana_after_awake
    );
    assert_eq!(
        awakened_again.data["user_character_mana_node_list"]["1"][0]["awake_level"],
        1
    );
    assert_eq!(
        awakened_again.data["character_list"][0]["bond_token_list"],
        before_learn.data["user_character_list"]["1"]["bond_token_list"]
    );

    let locked_board = cn_support::send_request(
        service.port(),
        "/api/index.php/character/open_mana_board",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 2,
        }),
    );
    assert!(locked_board.starts_with("HTTP/1.1 400 Bad Request"));

    let bond_token = cn_support::send_request(
        service.port(),
        "/api/index.php/character/receive_bond_token",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 1,
        }),
    );
    assert!(bond_token.starts_with("HTTP/1.1 400 Bad Request"));

    let invalid = cn_support::send_request(
        service.port(),
        "/api/index.php/character/over_limit",
        &encode_request(&OverLimitRequest {
            viewer_id,
            character_id: 1,
            over_limit_count: -1,
            use_stack: true,
            item_id: None,
        }),
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let character = &loaded.data["user_character_list"]["1"];
    assert_eq!(character["over_limit_step"], 4);
    assert_eq!(character["stack"], 0);
    assert_eq!(character["mana_board_index"], 1);
    assert_eq!(
        character["illustration_settings"],
        serde_json::json!([1, 2])
    );
    assert_eq!(loaded.data["user_info"]["free_mana"], free_mana_after_awake);
    assert_eq!(loaded.data["item_list"]["1"], 0);
    assert_eq!(loaded.data["item_list"]["99"], 0);
    assert_eq!(loaded.data["item_list"]["70047"], 0);
    assert_eq!(
        loaded.data["user_character_mana_node_list"]["1"],
        serde_json::json!([{"multiplied_id": 2201, "awake_level": 1}])
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 角色养成快照持久化 ////

// //// 验证 CN 角色主表 Mana board 与羁绊代币协议 [@x380kkm 2026-08-23] ////
#[test]
fn follows_character_master_for_mana_boards_and_bond_tokens() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 84 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Board characters\",\"body\":\"Character master test\",\"sender\":\"Admin\",\"rewards\":{{\"characterList\":[512001,111129]}}}}"
    );
    let created = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let received = cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &encode_request(&MailReceiveAllRequest { viewer_id }),
    );
    assert!(received.starts_with("HTTP/1.1 200 OK"));

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(
        loaded.data["user_character_list"]["512001"]["bond_token_list"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        loaded.data["user_character_list"]["111129"]["bond_token_list"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    update_player_snapshot(root.path(), |player_data| {
        player_data["user_info"]["bond_token"] = Value::from(5);
        player_data["user_character_list"]["1"]["bond_token_list"][0]["status"] = Value::from(1);
        player_data["all_active_mission_list"] = serde_json::json!({
            "14110": {"progress": 1, "stages": []}
        });
        player_data["user_character_list"]["1"]["exp"] = Value::from(76_271);
        player_data["user_character_list"]["1"]["over_limit_step"] = Value::from(4);
        player_data["user_character_list"]["111129"]["bond_token_list"] = serde_json::json!([
            {"mana_board_index": 1, "status": 0},
            {"mana_board_index": 2, "status": 0},
        ]);
        player_data["user_character_list"]["111129"]["exp"] = Value::from(153_988);
        player_data["user_character_list"]["111129"]["over_limit_step"] = Value::from(2);
        player_data["user_character_list"]["512001"]["over_limit_step"] = Value::from(10);
    });

    let claimed_response = cn_support::send_request(
        service.port(),
        "/api/index.php/character/receive_bond_token",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 1,
        }),
    );
    assert!(
        claimed_response.starts_with("HTTP/1.1 200 OK"),
        "unexpected receive bond token response: {claimed_response}"
    );
    let claimed = decode_response::<Value>(&claimed_response);
    assert_eq!(claimed.data["user_info"]["bond_token"], 6);
    assert_eq!(
        claimed.data["character_list"][0]["bond_token_list"][0]["status"],
        2
    );
    assert_eq!(
        claimed.data["user_character_mana_node_list"],
        serde_json::json!({})
    );
    assert_eq!(claimed.data["item_list"], serde_json::json!({}));
    assert_eq!(claimed.data["evolution"], serde_json::json!([]));
    assert_eq!(
        claimed.data["mission_info"],
        serde_json::json!([{
            "mission_category_id": 1,
            "mission_id": 39,
            "mission_reward_id": 39001,
        }])
    );
    assert_eq!(
        claimed.data["active_mission_list"],
        serde_json::json!([
            {
                "mission_id": 14070,
                "progress_value": 1,
                "stages": [{"stage": 1, "received": false}],
            },
            {"mission_id": 14110, "progress_value": 2, "stages": []},
        ])
    );

    let claimed_again = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/receive_bond_token",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 1,
        }),
    ));
    assert_eq!(claimed_again.data["user_info"]["bond_token"], 6);
    assert_eq!(
        claimed_again.data["character_list"][0]["bond_token_list"][0]["status"],
        2
    );
    assert_eq!(claimed_again.data["mission_info"], serde_json::json!([]));
    assert_eq!(
        claimed_again.data["active_mission_list"],
        serde_json::json!([])
    );

    let level_locked = cn_support::send_request(
        service.port(),
        "/api/index.php/character/open_mana_board",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 2,
        }),
    );
    assert!(level_locked.starts_with("HTTP/1.1 400 Bad Request"));
    update_player_snapshot(root.path(), |player_data| {
        player_data["user_character_list"]["1"]["exp"] = Value::from(76_272);
        player_data["user_character_list"]["1"]["over_limit_step"] = Value::from(3);
    });
    let uncap_locked = cn_support::send_request(
        service.port(),
        "/api/index.php/character/open_mana_board",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 2,
        }),
    );
    assert!(uncap_locked.starts_with("HTTP/1.1 400 Bad Request"));
    update_player_snapshot(root.path(), |player_data| {
        player_data["user_character_list"]["1"]["over_limit_step"] = Value::from(4);
    });
    let opened = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/open_mana_board",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 1,
            mana_board_index: 2,
        }),
    ));
    assert_eq!(opened.data["character_list"][0]["mana_board_index"], 2);
    assert!(opened.data["user_info"]["free_vmoney"].is_i64());
    assert_eq!(
        opened.data["mission_info"],
        serde_json::json!([{
            "mission_category_id": 1,
            "mission_id": 95,
            "mission_reward_id": 95001,
        }])
    );

    let one_board_response = cn_support::send_request(
        service.port(),
        "/api/index.php/character/open_mana_board",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 512001,
            mana_board_index: 1,
        }),
    );
    assert!(
        one_board_response.starts_with("HTTP/1.1 200 OK"),
        "unexpected one-board character response: {one_board_response}"
    );
    let one_board = decode_response::<Value>(&one_board_response);
    assert_eq!(one_board.data["character_list"][0]["mana_board_index"], 1);
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["bond_token"], 6);
    assert_eq!(loaded.data["cleared_regular_mission_list"]["39"], 1);
    assert_eq!(
        loaded.data["all_active_mission_list"]["14070"]["progress"],
        1
    );
    assert_eq!(
        loaded.data["all_active_mission_list"]["14110"]["progress"],
        2
    );
    assert_eq!(
        loaded.data["user_character_list"]["512001"]["bond_token_list"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    update_player_snapshot(root.path(), |player_data| {
        player_data["user_character_list"]["111129"]["bond_token_list"] = serde_json::json!([
            {"mana_board_index": 1, "status": 0},
            {"mana_board_index": 2, "status": 1},
        ]);
    });
    let preserved = cn_support::send_request(
        service.port(),
        "/api/index.php/character/open_mana_board",
        &encode_request(&OpenManaBoardRequest {
            viewer_id,
            character_id: 111129,
            mana_board_index: 1,
        }),
    );
    assert!(preserved.starts_with("HTTP/1.1 200 OK"));
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(
        loaded.data["user_character_list"]["111129"]["bond_token_list"][1]["status"],
        1
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 角色主表 Mana board 与羁绊代币协议 ////
