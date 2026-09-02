// audience: internal
// # personal-service-cn-mana-tests
//
// 该文件验证 CN Mana node 学习的混合 Mana 扣费和整板完成状态持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct MailReceiveAllRequest {
    viewer_id: i64,
}

#[derive(Serialize)]
struct LearnManaNodeRequest {
    viewer_id: i64,
    character_id: i64,
    api_count: i64,
    mana_node_multiplied_id_list: Vec<i64>,
}

// //// 验证 CN Mana node 整板完成语义 [@x380kkm 2026-08-23] ////
#[test]
fn persists_board_completion_after_free_and_paid_mana_deduction() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 73 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Mana board materials\",\"body\":\"Mana board test\",\"sender\":\"Admin\",\"rewards\":{{\"paidMana\":112240,\"itemList\":{{\"1\":212,\"2\":179,\"3\":90,\"4\":29,\"99\":31}}}}}}"
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

    let partial = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/learn_mana_node",
        &encode_request(&LearnManaNodeRequest {
            viewer_id,
            character_id: 1,
            api_count: 1,
            mana_node_multiplied_id_list: (2201..=2207).collect(),
        }),
    ));
    assert_eq!(partial.data["user_info"]["free_mana"], 0);
    assert_eq!(partial.data["user_info"]["paid_mana"], 111_900);
    assert_eq!(
        partial.data["user_character_mana_node_list"]["1"][0],
        serde_json::json!({"mana_node_multiplied_id": 2201})
    );
    assert_eq!(partial.data["character_list"][0]["evolution_level"], 0);
    assert_eq!(
        partial.data["character_list"][0]["bond_token_list"],
        serde_json::json!([
            {"mana_board_index": 1, "status": 0},
            {"mana_board_index": 2, "status": 0},
        ])
    );
    assert_eq!(partial.data["evolution"], serde_json::json!([]));
    assert_eq!(partial.data["active_mission_list"][0]["mission_id"], 11_070);
    assert_eq!(partial.data["active_mission_list"][0]["progress_value"], 7);

    let completed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/learn_mana_node",
        &encode_request(&LearnManaNodeRequest {
            viewer_id,
            character_id: 1,
            api_count: 2,
            mana_node_multiplied_id_list: (2208..=2223).collect(),
        }),
    ));
    assert_eq!(completed.data["user_info"]["free_mana"], 0);
    assert_eq!(completed.data["user_info"]["paid_mana"], 0);
    assert_eq!(completed.data["character_list"][0]["evolution_level"], 1);
    assert_eq!(
        completed.data["character_list"][0]["bond_token_list"][0]["status"],
        1
    );
    assert_eq!(completed.data["evolution"]["character_id"], 1);
    assert_eq!(completed.data["evolution"]["level"], 1);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["free_mana"], 0);
    assert_eq!(loaded.data["user_info"]["paid_mana"], 0);
    assert_eq!(
        loaded.data["user_character_list"]["1"]["evolution_level"],
        1
    );
    assert_eq!(
        loaded.data["user_character_list"]["1"]["bond_token_list"][0]["status"],
        1
    );
    assert_eq!(
        loaded.data["user_character_mana_node_list"]["1"]
            .as_array()
            .map(Vec::len),
        Some(23)
    );
    assert!(loaded.data["user_character_mana_node_list"]["1"]
        .as_array()
        .is_some_and(|nodes| nodes.iter().all(|node| {
            node.get("multiplied_id").and_then(Value::as_i64).is_some()
                && node.get("awake_level").and_then(Value::as_i64).is_some()
        })));
    assert_eq!(
        loaded.data["all_active_mission_list"]["11070"]["progress"],
        23
    );
    assert_eq!(
        loaded.data["all_active_mission_list"]["11110"]["progress"],
        1
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN Mana node 整板完成语义 ////

// //// 请求内重复 Mana node 只学习一次 [@x380kkm 2026-08-28] ////
#[test]
fn learns_duplicate_requested_mana_node_once() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 74 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Mana node material\",\"body\":\"Mana node test\",\"sender\":\"Admin\",\"rewards\":{{\"itemList\":{{\"1\":3}}}}}}"
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

    let learned = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/character/learn_mana_node",
        &encode_request(&LearnManaNodeRequest {
            viewer_id,
            character_id: 1,
            api_count: 1,
            mana_node_multiplied_id_list: vec![2_201, 2_201],
        }),
    ));
    assert_eq!(learned.data["user_info"]["free_mana"], 940);
    assert_eq!(learned.data["item_list"]["1"], 0);
    assert_eq!(
        learned.data["user_character_mana_node_list"]["1"],
        serde_json::json!([{"mana_node_multiplied_id": 2_201}])
    );
    assert_eq!(learned.data["active_mission_list"][0]["progress_value"], 1);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["free_mana"], 940);
    assert_eq!(
        loaded.data["user_character_mana_node_list"]["1"],
        serde_json::json!([{"multiplied_id": 2_201, "awake_level": 0}])
    );
    assert_eq!(
        loaded.data["all_active_mission_list"]["11070"]["progress"],
        1
    );
    service.stop().expect("service stops cleanly");
}
// //// /请求内重复 Mana node 只学习一次 ////
