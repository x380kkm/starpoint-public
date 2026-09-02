// audience: internal
// # personal-service-cn-auxiliary-tests
//
// 该文件验证 CN 辅助页面的响应类型和生日存档更新.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 验证 CN 辅助页面契约和生日持久化 [@x380kkm 2026-08-24] ////
#[test]
fn returns_typed_auxiliary_responses_and_saves_birth() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 135 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;

    let detail = request(
        service.port(),
        "/api/index.php/payment/detail_items",
        json!({"viewer_id": viewer_id}),
    );
    assert!(detail["mana_detail_items"].is_object());
    assert!(detail["vmoney_detail_items"].is_object());

    let agreement = request(
        service.port(),
        "/api/index.php/tool/agreement",
        json!({"viewer_id": viewer_id}),
    );
    assert_eq!(agreement["terms_text"], "");

    request(
        service.port(),
        "/api/index.php/payment/update_birth",
        json!({"viewer_id": viewer_id, "birth": 19991231}),
    );
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["birth"], 19991231);

    let debug = request(
        service.port(),
        "/api/index.php/debug/get_characters",
        json!({"viewer_id": viewer_id}),
    );
    assert!(debug["user_character_list"].is_array());

    let follow = request(
        service.port(),
        "/api/index.php/follow/search_id",
        json!({"viewer_id": viewer_id, "search_id": viewer_id.to_string()}),
    );
    assert_eq!(follow["search_result"]["viewer_id"], viewer_id);
    assert!(follow["search_result"]["name"].is_string());

    let takeover = request(
        service.port(),
        "/api/index.php/take_over_register/get_take_over_setting",
        json!({"viewer_id": viewer_id}),
    );
    assert_eq!(takeover["exists_user_take_over_data"], false);
    assert_eq!(takeover["social_account"]["is_apple_linked"], false);

    let exchange = request(
        service.port(),
        "/api/index.php/special_exchange/enter_campaign",
        json!({"viewer_id": viewer_id, "campaign_id": 1}),
    );
    assert!(exchange["special_exchange_campaign_list"].is_array());

    let character_exchange = request(
        service.port(),
        "/api/index.php/special_exchange/exchange_character",
        json!({"viewer_id": viewer_id, "campaign_id": 1, "character_id": 1}),
    );
    assert!(character_exchange["special_exchange_campaign_list"].is_array());
    assert!(character_exchange["character_list"].is_array());
    assert!(character_exchange["equipment_list"].is_array());
    assert!(character_exchange["item_list"].is_object());
    assert!(character_exchange["mail_arrived"].is_boolean());
    assert!(character_exchange["user_info"].is_null());
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 辅助页面契约和生日持久化 ////

fn request(port: u16, path: &str, body: Value) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        port,
        path,
        &encode_request(&body),
    ))
    .data
}
