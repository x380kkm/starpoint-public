// audience: internal
// # personal-service-cn-equipment-tests
//
// 该文件验证 CN 装备养成、保护、出售的资源扣除和快照持久化.

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
struct UpgradeRequest {
    viewer_id: i64,
    equipment_id: i64,
    upgrade_count: i64,
    use_stack: bool,
    item_id: Option<i64>,
    api_count: i64,
}

#[derive(Serialize)]
struct SetProtectionRequest {
    viewer_id: i64,
    protection: bool,
    equipment_ids: Vec<i64>,
    api_count: i64,
}

#[derive(Serialize)]
struct SellEquipmentRequest {
    viewer_id: i64,
    api_count: i64,
    equipment_list: Vec<SellEquipmentItem>,
}

#[derive(Serialize)]
struct SellEquipmentItem {
    equipment_id: i64,
    number: Option<i64>,
}

// //// 验证 CN 装备生命周期资源和快照持久化 [@x380kkm 2026-07-24] ////
#[test]
fn upgrades_equipment_and_persists_resources() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 43 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Equipment\",\"body\":\"Upgrade test\",\"sender\":\"Admin\",\"rewards\":{{\"equipmentList\":{{\"5030037\":3,\"5040028\":4}},\"itemList\":{{\"100000\":25}}}}}}"
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

    let upgraded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/equipment/upgrade",
        &encode_request(&UpgradeRequest {
            viewer_id,
            equipment_id: 5_030_037,
            upgrade_count: 1,
            use_stack: true,
            item_id: None,
            api_count: 1,
        }),
    ));
    assert_eq!(upgraded.data["equipment_list"][0]["level"], 2);
    assert_eq!(upgraded.data["equipment_list"][0]["stack"], 1);
    assert_eq!(upgraded.data["item_list"]["100000"], 0);
    assert_eq!(upgraded.data["item_list"]["5030037"], 1);

    let protected = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/equipment/set_protection",
        &encode_request(&SetProtectionRequest {
            viewer_id,
            protection: true,
            equipment_ids: vec![5_030_037, 5_040_028],
            api_count: 2,
        }),
    ));
    assert_eq!(protected.data, serde_json::json!({}));

    let sold_stack = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/equipment/sell_stack",
        &encode_request(&SellEquipmentRequest {
            viewer_id,
            api_count: 3,
            equipment_list: vec![SellEquipmentItem {
                equipment_id: 5_030_037,
                number: Some(1),
            }],
        }),
    ));
    assert_eq!(sold_stack.data["equipment_list"][0]["stack"], 0);
    assert_eq!(sold_stack.data["item_list"]["100000"], 5);
    assert_eq!(sold_stack.data["item_list"]["5030037"], 2);

    let sold_equipment = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/equipment/sell_equipment",
        &encode_request(&SellEquipmentRequest {
            viewer_id,
            api_count: 4,
            equipment_list: vec![SellEquipmentItem {
                equipment_id: 5_040_028,
                number: None,
            }],
        }),
    ));
    assert!(sold_equipment.data["equipment_list"].is_array());
    assert_eq!(
        sold_equipment.data["equipment_list"][0]["equipment_id"],
        5_030_037
    );
    assert_eq!(sold_equipment.data["equipment_list"][0]["stack"], 0);
    assert_eq!(sold_equipment.data["item_list"]["100000"], 20);
    assert_eq!(sold_equipment.data["item_list"]["5040028"], 3);

    let missing_equipment = cn_support::send_request(
        service.port(),
        "/api/index.php/equipment/sell_equipment",
        &encode_request(&SellEquipmentRequest {
            viewer_id,
            api_count: 5,
            equipment_list: vec![SellEquipmentItem {
                equipment_id: 9_999_999,
                number: None,
            }],
        }),
    );
    assert!(missing_equipment.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(missing_equipment.contains("equipment_not_owned"));

    for (api_count, number) in [(6, None), (7, Some(0))] {
        let invalid_quantity = cn_support::send_request(
            service.port(),
            "/api/index.php/equipment/sell_stack",
            &encode_request(&SellEquipmentRequest {
                viewer_id,
                api_count,
                equipment_list: vec![SellEquipmentItem {
                    equipment_id: 5_030_037,
                    number,
                }],
            }),
        );
        assert!(invalid_quantity.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(invalid_quantity.contains("invalid_equipment_sell_count"));
    }

    let excessive_quantity = cn_support::send_request(
        service.port(),
        "/api/index.php/equipment/sell_stack",
        &encode_request(&SellEquipmentRequest {
            viewer_id,
            api_count: 8,
            equipment_list: vec![SellEquipmentItem {
                equipment_id: 5_030_037,
                number: Some(1),
            }],
        }),
    );
    assert!(excessive_quantity.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(excessive_quantity.contains("not_enough_equipment_stack"));

    let insufficient_stack = cn_support::send_request(
        service.port(),
        "/api/index.php/equipment/upgrade",
        &encode_request(&UpgradeRequest {
            viewer_id,
            equipment_id: 5_030_037,
            upgrade_count: 2,
            use_stack: true,
            item_id: None,
            api_count: 2,
        }),
    );
    assert!(insufficient_stack.starts_with("HTTP/1.1 400 Bad Request"));

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    let equipment = &loaded.data["user_equipment_list"]["5030037"];
    assert_eq!(equipment["level"], 2);
    assert_eq!(equipment["stack"], 0);
    assert_eq!(equipment["protection"], true);
    assert!(loaded.data["user_equipment_list"].get("5040028").is_none());
    assert_eq!(loaded.data["item_list"]["100000"], 20);
    assert_eq!(loaded.data["item_list"]["5030037"], 2);
    assert_eq!(loaded.data["item_list"]["5040028"], 3);
    let history = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/history/receive",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    let history = history.data["history"]
        .as_array()
        .expect("history is an array");
    let received = |type_id| {
        history
            .iter()
            .filter(|entry| entry["type"] == 1 && entry["type_id"] == type_id)
            .filter_map(|entry| entry["number"].as_i64())
            .sum::<i64>()
    };
    assert_eq!(received(100_000), 45);
    assert_eq!(received(5_030_037), 2);
    assert_eq!(received(5_040_028), 3);
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let restarted_load = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(restarted_load.data["item_list"]["100000"], 20);
    assert!(restarted_load.data["user_equipment_list"]
        .get("5040028")
        .is_none());
    let restarted_history = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/history/receive",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(restarted_history.data["total_count"], history.len());
    restarted.stop().expect("service stops cleanly");
}
// //// /验证 CN 装备生命周期资源和快照持久化 ////
