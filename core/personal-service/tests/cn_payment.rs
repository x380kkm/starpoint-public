// audience: internal
// # personal-service-cn-payment-tests
//
// 该文件验证 CN 支付兼容查询、商品确认和 viewer 校验.

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
struct PaymentRequest {
    viewer_id: i64,
    api_count: i64,
}

#[derive(Serialize)]
struct PaymentStartRequest {
    viewer_id: i64,
    api_count: i64,
    payment: PaymentReference,
}

#[derive(Serialize)]
struct PaymentReference {
    product_id: String,
}

#[derive(Serialize)]
struct PaymentFinishRequest {
    viewer_id: i64,
    api_count: i64,
    product_id: String,
}

// //// 验证 CN 支付兼容空状态 [@x380kkm 2026-07-24] ////
#[test]
fn returns_empty_payment_state_for_valid_viewer() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 47 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let item_list = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/payment/item_list",
        &encode_request(&PaymentRequest {
            viewer_id,
            api_count: 1,
        }),
    ));
    assert_eq!(item_list.data["payment_item_list"], serde_json::json!([]));

    let order = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/channels/channel_leiting_pay/query_unfinish_order",
        &encode_request(&PaymentRequest {
            viewer_id,
            api_count: 2,
        }),
    ));
    assert_eq!(order.data["order_id"], "");

    let product_id = "com.leiting.wf.stone_50".to_owned();
    let started = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/payment/start",
        &encode_request(&PaymentStartRequest {
            viewer_id,
            api_count: 3,
            payment: PaymentReference {
                product_id: product_id.clone(),
            },
        }),
    ));
    assert_eq!(started.data, serde_json::json!({}));
    let finished = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/payment/finish",
        &encode_request(&PaymentFinishRequest {
            viewer_id,
            api_count: 4,
            product_id,
        }),
    ));
    assert_eq!(finished.data["after_vmoney"], 50);
    assert_eq!(finished.data["first_payment"], true);
    assert_eq!(
        finished.data["purchased_times_list"]["com.leiting.wf.stone_50"],
        1
    );
    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["vmoney"], 50);
    assert_eq!(
        loaded.data["user_info"]["free_vmoney"],
        finished.data["after_free_vmoney"]
    );

    for response in [
        cn_support::send_request(
            service.port(),
            "/api/index.php/payment/start",
            &encode_request(&PaymentStartRequest {
                viewer_id,
                api_count: 5,
                payment: PaymentReference {
                    product_id: "unknown-product".to_owned(),
                },
            }),
        ),
        cn_support::send_request(
            service.port(),
            "/api/index.php/payment/start",
            &encode_request(&PaymentStartRequest {
                viewer_id: 999_999_999,
                api_count: 6,
                payment: PaymentReference {
                    product_id: "com.leiting.wf.stone_50".to_owned(),
                },
            }),
        ),
        cn_support::send_request(
            service.port(),
            "/api/index.php/payment/finish",
            "not-base64-messagepack",
        ),
        cn_support::send_request(
            service.port(),
            "/api/index.php/payment/finish",
            &encode_request(&PaymentRequest {
                viewer_id,
                api_count: 7,
            }),
        ),
    ] {
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(
            decode_response::<Value>(&response).data,
            serde_json::json!({})
        );
    }

    let stateless_order = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/channels/channel_leiting_pay/query_unfinish_order",
        &encode_request(&PaymentRequest {
            viewer_id: 999_999_999,
            api_count: 8,
        }),
    ));
    assert_eq!(stateless_order.data_headers.viewer_id, 0);
    assert_eq!(stateless_order.data["order_id"], "");
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 支付兼容空状态 ////
