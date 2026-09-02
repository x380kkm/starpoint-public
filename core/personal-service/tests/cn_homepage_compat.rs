// audience: internal
// # personal-service-cn-homepage-compat-tests
//
// 该文件验证 CN 首页支付, Pass Card 和章节试读接口的 MessagePack 契约.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 调用无状态的 CN 首页接口 [@x380kkm 2026-08-21] ////
fn post_homepage_request(port: u16, path: &str) -> Value {
    let body = cn_support::encode_request(&json!({}));
    cn_support::decode_response::<Value>(&cn_support::send_request(port, path, &body)).data
}
// //// /调用无状态的 CN 首页接口 ////

// //// 验证 CN 首页接口的精确响应和路由边界 [@x380kkm 2026-08-21] ////
#[test]
fn returns_homepage_compatibility_data_for_exact_post_routes() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    assert_eq!(
        post_homepage_request(
            service.port(),
            "/api/index.php/channels/channel_leiting_pay/query_purcharge",
        ),
        json!({"status": 3}),
    );
    assert_eq!(
        post_homepage_request(
            service.port(),
            "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_status",
        ),
        json!({}),
    );
    assert_eq!(
        post_homepage_request(service.port(), "/api/index.php/Pass_card/get_pass_card"),
        json!({"point": 0, "is_buy": false, "all_received_record": []}),
    );
    assert_eq!(
        post_homepage_request(service.port(), "/api/index.php/Pass_card/receive_all"),
        json!({"all_received_record": []}),
    );
    assert_eq!(
        post_homepage_request(
            service.port(),
            "/api/index.php/episode_trial_reading/finish",
        ),
        json!({}),
    );

    let get_response = support::request(
        service.port(),
        "GET",
        "/api/index.php/Pass_card/get_pass_card",
    );
    assert!(get_response.starts_with("HTTP/1.1 404 Not Found"));
    let unknown_response = cn_support::send_request(
        service.port(),
        "/api/index.php/Pass_card/unknown",
        &cn_support::encode_request(&json!({})),
    );
    assert!(unknown_response.starts_with("HTTP/1.1 404 Not Found"));

    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 首页接口的精确响应和路由边界 ////
