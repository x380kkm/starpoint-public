// audience: internal
// # personal-service-cn-option-tests
//
// 该文件验证 CN 用户选项更新和 load 持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

#[derive(Serialize)]
struct UpdateRequest {
    viewer_id: i64,
    api_count: i64,
    option_params: Value,
}

// //// 验证 CN 用户选项更新和持久化 [@x380kkm 2026-07-24] ////
#[test]
fn updates_options_in_normal_and_battle_routes() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 51 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/option/update",
        &encode_request(&UpdateRequest {
            viewer_id,
            api_count: 1,
            option_params: serde_json::json!({
                "auto_play": true,
                "attention_vibration": true,
            }),
        }),
    ));
    assert_eq!(first.data["user_option"]["auto_play"], true);
    assert_eq!(first.data["user_option"]["attention_vibration"], true);

    let second = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/option/update_in_battle",
        &encode_request(&UpdateRequest {
            viewer_id,
            api_count: 1,
            option_params: serde_json::json!({"auto_play": false}),
        }),
    ));
    assert_eq!(second.data["user_option"]["auto_play"], false);

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_option"]["auto_play"], false);
    assert_eq!(loaded.data["user_option"]["attention_vibration"], true);
    assert_eq!(loaded.data["user_option"]["payment_alert"], true);
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 用户选项更新和持久化 ////
