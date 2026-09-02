// audience: internal
// # personal-service-cn-reference-bulk-tests
//
// 该文件验证 CN 批量养成和编队外围接口的精确空结果容器.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

#[derive(Serialize)]
struct BulkEquipmentRequest {
    viewer_id: i64,
    api_count: i64,
    equipment_ids: Vec<i64>,
}

#[derive(Serialize)]
struct BulkExpRequest {
    viewer_id: i64,
    api_count: i64,
}

#[derive(Serialize)]
struct PartyWordRequest {
    viewer_id: i64,
    word: String,
}

#[derive(Serialize)]
struct PartyPublishRequest {
    viewer_id: i64,
    party_name: String,
    battle_party: Value,
}

// //// 验证批量空结果和编队外围响应类型 [@x380kkm 2026-08-22] ////
#[test]
fn returns_typed_bulk_and_party_responses() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 74 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    for path in [
        "/api/index.php/equipment/bulk_upgrade",
        "/api/index.php/equipment/bulk_sell_stack",
    ] {
        let response = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            path,
            &encode_request(&BulkEquipmentRequest {
                viewer_id,
                api_count: 1,
                equipment_ids: vec![9_999_999],
            }),
        ));
        assert_eq!(response.data["equipment_list"], Value::Array(Vec::new()));
        assert_eq!(response.data["item_list"], serde_json::json!({}));
    }
    let converted = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/expod/bulk_stack_to_exp",
        &encode_request(&BulkExpRequest {
            viewer_id,
            api_count: 2,
        }),
    ));
    assert_eq!(converted.data["character_list"], Value::Array(Vec::new()));
    assert_eq!(converted.data["converted_exp_info"]["add_exp"], 0);

    let checked = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/check_word",
        &encode_request(&PartyWordRequest {
            viewer_id,
            word: "Party A".to_owned(),
        }),
    ));
    assert_eq!(checked.data["check_passed"], true);
    let published = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/party/publish",
        &encode_request(&PartyPublishRequest {
            viewer_id,
            party_name: "Party A".to_owned(),
            battle_party: serde_json::json!({}),
        }),
    ));
    assert_eq!(
        published.data["party_code"],
        "https://www.howLongCanThisBe?=+-.comhttps://www.howLongCanThisBe?=+-.comhttps://www.howLongCanThisBe?=+-.com"
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证批量空结果和编队外围响应类型 ////
