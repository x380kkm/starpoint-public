// audience: internal
// # personal-service-cn-payment-availability-tests
// 商品目录与订单通过真实 HTTP 请求共享游戏时间, 已到账订单在停售后仍可恢复.

use super::*;

fn product_ids(service: &PersonalService, viewer: i64) -> Vec<String> {
    let response = decode_response::<Value>(&request(service, "payment/item_list", json!({"viewer_id":viewer})));
    assert_eq!(response.data_headers.result_code, 1);
    assert!(response.data.get("refund_penalty_status").is_some());
    response.data["payment_item_list"].as_array().unwrap().iter()
        .map(|item| item["store_product_id"].as_str().unwrap().to_owned()).collect()
}

fn refuse_purchase(service: &PersonalService, viewer: i64, count: i64, product: &str) {
    let response = decode_response::<Value>(&request(service, "payment/start",
        json!({"viewer_id":viewer,"api_count":count,"payment":{"product_id":product}})));
    assert_eq!(response.data_headers.result_code, 20001);
}

// //// 核对周年礼包的开放边界并恢复停售后的已到账订单 [@x380kkm 2026-09-26] ////
#[test]
fn matches_catalog_and_purchase_windows_without_blocking_order_recovery() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let product = "com.leiting.wf.2anv_pack_2023_01";
    set_time(&service, "2024-07-11T03:59:59.000Z");
    let before = snapshot(root.path());
    assert!(!product_ids(&service, viewer).iter().any(|id| id == product));
    refuse_purchase(&service, viewer, 1, product);
    assert_eq!(snapshot(root.path()), before);

    set_time(&service, "2024-07-11T04:00:00.000Z");
    assert!(product_ids(&service, viewer).iter().any(|id| id == product));
    let order = start(&service, viewer, 2, product);
    let delivered = snapshot(root.path());
    let delivered_entries = history_count(root.path());
    set_time(&service, "2024-07-12T04:00:00.000Z");
    let ids = product_ids(&service, viewer);
    let fixture: Value = serde_json::from_str(include_str!("../../assets/cn-payment-catalog.json")).unwrap();
    for id in ids {
        let definition = &fixture["products"][&id];
        assert!(definition["start_time"].as_i64().unwrap() <= 1_720_756_800, "{id}");
        assert!(definition["end_time"].as_i64().unwrap() >= 1_720_756_800, "{id}");
    }
    set_time(&service, "2024-08-01T15:59:59.000Z");
    assert!(product_ids(&service, viewer).iter().any(|id| id == product));
    set_time(&service, "2024-08-01T16:00:00.000Z");
    assert!(!product_ids(&service, viewer).iter().any(|id| id == product));
    refuse_purchase(&service, viewer, 3, product);
    assert_eq!(snapshot(root.path()), delivered);
    assert_eq!(start(&service, viewer, 2, product), order);
    finish(&service, viewer, product, order["order_id"].as_str().unwrap());
    assert_eq!(history_count(root.path()), delivered_entries);
    service.stop().unwrap();
}
// //// /核对周年礼包的开放边界并恢复停售后的已到账订单 ////

// //// 跨月选择开放修行期并遵循管理日历关闭状态 [@x380kkm 2026-09-26] ////
#[test]
fn binds_pass_purchase_to_open_event_after_clock_moves() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let product = "com.leiting.wf.pass_card";
    set_time(&service, "2024-06-10T00:00:00.000Z");
    decode_response::<Value>(&request(&service, "Pass_card/get_pass_card", json!({"viewer_id":viewer,"pass_card_id":1})));
    set_time(&service, "2024-07-12T04:00:00.000Z");
    start(&service, viewer, 1, product);
    let purchased = snapshot(root.path());
    assert_ne!(purchased["pass_card_state"]["events"]["1"]["is_buy"], true);
    assert_eq!(purchased["pass_card_state"]["events"]["2"]["is_buy"], true);

    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(service.port(), "POST", "/v1/activities/pass-card%3A2/close", "application/json",
        &[("Authorization", &authorization)], b"{}");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(!product_ids(&service, viewer).iter().any(|id| id == product));
    refuse_purchase(&service, viewer, 2, product);
    assert_eq!(snapshot(root.path()), purchased);
    service.stop().unwrap();
}
// //// /跨月选择开放修行期并遵循管理日历关闭状态 ////

// //// 通过商品管理时窗匹配客户端展示与到账并恢复关闭前订单 [@x380kkm 2026-09-27] ////
#[test]
fn manages_product_periods_and_returns_the_effective_client_window() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let product = "com.leiting.wf.2anv_pack_2023_01";
    let activity = format!("payment:{product}");
    let authorization = format!("Bearer {}", service.management_token());
    let manage = |method: &str, path: &str, body: Value| {
        let response = support::request_with_headers(service.port(), method, path, "application/json",
            &[("Authorization", &authorization)], body.to_string().as_bytes());
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let (_, data) = response.split_once("\r\n\r\n").unwrap();
        serde_json::from_str::<Value>(data).unwrap()
    };
    set_time(&service, "2026-09-27T04:00:00.000Z");
    assert!(!product_ids(&service, viewer).iter().any(|id| id == product));
    let catalog = manage("GET", "/v1/activities/catalog?kind=payment", json!({}));
    let fixture: Value = serde_json::from_str(include_str!("../../assets/cn-payment-catalog.json")).unwrap();
    assert_eq!(catalog["activities"].as_array().unwrap().len(), fixture["products"].as_object().unwrap().len());
    let entry = catalog["activities"].as_array().unwrap().iter()
        .find(|entry| entry["activity_id"] == activity).unwrap();
    assert_eq!(entry["name"], fixture["products"][product]["name"]);

    manage("PUT", &format!("/v1/activities/calendar/{activity}"), json!({
        "enabled":true, "start_at":"2026-09-27T03:00:00.000Z", "end_at":"2026-09-27T05:00:00.000Z",
    }));
    let response = decode_response::<Value>(&request(&service, "payment/item_list", json!({"viewer_id":viewer})));
    let listed = response.data["payment_item_list"].as_array().unwrap().iter()
        .find(|item| item["store_product_id"] == product).unwrap();
    assert_eq!(listed["start_time"], 1_790_478_000_i64);
    assert_eq!(listed["end_time"], 1_790_485_199_i64);
    let order = start(&service, viewer, 80, product);
    let delivered = snapshot(root.path());
    manage("POST", &format!("/v1/activities/{activity}/close"), json!({}));
    assert!(!product_ids(&service, viewer).iter().any(|id| id == product));
    refuse_purchase(&service, viewer, 81, product);
    assert_eq!(snapshot(root.path()), delivered);
    assert_eq!(start(&service, viewer, 80, product), order);
    finish(&service, viewer, product, order["order_id"].as_str().unwrap());

    manage("POST", &format!("/v1/activities/{activity}/temporary-open"), json!({}));
    let reopened = decode_response::<Value>(&request(&service, "payment/item_list", json!({"viewer_id":viewer})));
    let listed = reopened.data["payment_item_list"].as_array().unwrap().iter()
        .find(|item| item["store_product_id"] == product).unwrap();
    assert!(listed["start_time"].as_i64().unwrap() <= reopened.data_headers.servertime);
    assert!(listed["end_time"].as_i64().unwrap() >= reopened.data_headers.servertime);
    service.stop().unwrap();
}
// //// /通过商品管理时窗匹配客户端展示与到账并恢复关闭前订单 ////
