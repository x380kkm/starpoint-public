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

// //// 调用 CN 首页接口并检查业务结果 [@x380kkm 2026-09-23] ////
fn post_homepage_request(port: u16, path: &str, body: Value) -> Value {
    let body = cn_support::encode_request(&body);
    let response = cn_support::decode_response::<Value>(&cn_support::send_request(port, path, &body));
    assert_eq!(
        response.data_headers.result_code, 1,
        "{path}: {}",
        response.data
    );
    response.data
}
// //// /调用 CN 首页接口并检查业务结果 ////

// //// 验证 CN 首页接口的精确响应和路由边界 [@x380kkm 2026-08-21] ////
#[test]
fn returns_homepage_compatibility_data_for_exact_post_routes() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let authorization = format!("Bearer {}", service.management_token());
    let set_time = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", &authorization)],
        br#"{"enabled":true,"iso":"2024-06-05T12:00:00.000Z","rate":1}"#,
    );
    assert!(set_time.starts_with("HTTP/1.1 200 OK"), "{set_time}");
    let signup = cn_support::decode_response::<cn_support::SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &cn_support::encode_request(&cn_support::SignupRequest { device_id: 143 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let product_id = "com.leiting.wf.stone_50";
    let order = post_homepage_request(
        service.port(),
        "/api/index.php/payment/start",
        json!({"viewer_id":viewer_id,"api_count":1,"payment":{"product_id":product_id}}),
    );
    let order_id = order["order_id"]
        .as_str()
        .expect("local order has an identifier");

    assert_eq!(
        post_homepage_request(
            service.port(),
            "/api/index.php/channels/channel_leiting_pay/query_purcharge",
            json!({"viewer_id":viewer_id,"payment":{"order_id":order_id}}),
        ),
        json!({"status":3,"order_id":order_id,"product_id":product_id}),
    );
    assert_eq!(
        post_homepage_request(
            service.port(),
            "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_status",
            json!({"viewer_id":viewer_id,"payment":{"order_id":order_id}}),
        ),
        json!({}),
    );
    let pending = post_homepage_request(
        service.port(),
        "/api/index.php/channels/channel_leiting_pay/query_unfinish_order",
        json!({"viewer_id":viewer_id}),
    );
    assert_eq!(pending["order_id"], "");
    let pass_card = post_homepage_request(
        service.port(),
        "/api/index.php/Pass_card/get_pass_card",
        json!({"viewer_id":viewer_id,"pass_card_id":1}),
    );
    assert_eq!(
        pass_card,
        json!({"point":0,"is_buy":false,"all_received_record":[]})
    );
    let received = post_homepage_request(
        service.port(),
        "/api/index.php/Pass_card/receive_all",
        json!({"viewer_id":viewer_id,"pass_card_id":1,"all_receive":[],"reward1_receive":[],"reward2_receive":[]}),
    );
    assert_eq!(
        received["all_received_record"],
        pass_card["all_received_record"]
    );
    assert_eq!(received["point"], pass_card["point"]);
    assert_eq!(received["is_buy"], pass_card["is_buy"]);
    assert_eq!(
        post_homepage_request(
            service.port(),
            "/api/index.php/episode_trial_reading/finish",
            json!({"viewer_id":viewer_id,"character_id":1,"quest_id":1001003}),
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
