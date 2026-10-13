// audience: internal
// # personal-service-cn-shop-lifecycle-tests
// 活动商店按主表真实商品检查重放、管理窗口变化和参与轮次重置, 历史与资产在同一临时存档验证.

use super::*;

struct EventProduct {
    activity_id: String,
    item_id: i64,
    count: i64,
    item: Value,
}

// //// 为每种活动商店选取具有累计限购的实际商品 [@x380kkm 2026-09-21] ////
fn event_products() -> Vec<EventProduct> {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../assets/event_item_shop.json")).unwrap();
    let limits: Value =
        serde_json::from_str(include_str!("../../../../assets/cn-shop-limits.json")).unwrap();
    [
        (0, "advent"),
        (2, "story"),
        (4, "challenge-dungeon"),
        (6, "world-story"),
        (9, "collect-item"),
        (10, "carnival"),
        (11, "rush"),
        (12, "score-attack"),
    ]
    .into_iter()
    .map(|(kind, family)| {
        let events = catalog[kind.to_string()].as_object().unwrap();
        events
            .iter()
            .find_map(|(event_id, items)| {
                items
                    .as_object()
                    .unwrap()
                    .iter()
                    .find_map(|(item_id, item)| {
                        let count = limits["4"][item_id]["maxFrequency"].as_i64()?;
                        if !(1..=3).contains(&count) {
                            return None;
                        }
                        Some(EventProduct {
                            activity_id: format!("{family}:{event_id}"),
                            item_id: item_id.parse().unwrap(),
                            count,
                            item: item.clone(),
                        })
                    })
            })
            .unwrap_or_else(|| panic!("no limited event product for {family}"))
    })
    .collect()
}
// //// /为每种活动商店选取具有累计限购的实际商品 ////

// //// 通过管理接口操作临时存档活动 [@x380kkm 2026-09-21] ////
fn manage(service: &PersonalService, method: &str, path: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", token.as_str())],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}
// //// /通过管理接口操作临时存档活动 ////

// //// 按实际商品主表建立自选活动的购买前置条件 [@x380kkm 2026-09-23] ////
fn select_product_lineup(service: &PersonalService, viewer_id: i64, item_id: i64) {
    let catalog: Value =
        serde_json::from_str(include_str!("../../assets/cn-shop-selection.json")).unwrap();
    let Some(product) = catalog["products"].get(format!("4:{item_id}")) else {
        return;
    };
    let key = product["campaign"].as_str().unwrap();
    let activity_id = catalog["campaigns"][key]["activity_id"].as_str().unwrap();
    manage(
        service,
        "PUT",
        &format!("/v1/activities/{activity_id}/mode"),
        json!({"mode":"always"}),
    );
    if let Some(lineup_id) = product["lineup_id"].as_i64() {
        let campaign_id: i64 = key.strip_prefix("4:").unwrap().parse().unwrap();
        let selected = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/set_campaign_lineup_id",
            &encode_request(
                &json!({"viewer_id":viewer_id,"shop_type":4,"campaign_id":campaign_id,"lineup_id":lineup_id}),
            ),
        ));
        assert_eq!(selected.data_headers.result_code, 1);
    }
}
// //// /按实际商品主表建立自选活动的购买前置条件 ////

// //// 活动商店重置库存时保留资产和历史并隔离请求重放 [@x380kkm 2026-09-21] ////
#[test]
fn reopens_each_event_shop_without_replaying_previous_rounds() {
    let products = event_products();
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let activities = products.iter().map(|product| json!({"activity_id":product.activity_id,"name":product.activity_id,"kind":product.activity_id.split(':').next().unwrap()})).collect::<Vec<_>>();
    fs::write(
        cdn.path().join("activity-catalog.json"),
        json!({"format_version":1,"activities":activities}).to_string(),
    )
    .unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let viewer_id = signup(&service, 232);
    set_virtual_time(&service, "2030-06-01T12:00:00.000Z");
    let saves = manage(&service, "GET", "/v1/local-saves", json!({}));
    let slot_id = saves["slots"][0]["id"].as_i64().unwrap();
    for (index, product) in products.iter().enumerate() {
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/{}/mode", product.activity_id),
            json!({"mode":"always"}),
        );
        update_player_snapshot(root.path(), |data| {
            for cost in product.item["costs"].as_array().unwrap() {
                data["item_list"][cost["id"].as_i64().unwrap().to_string()] =
                    json!(cost["amount"].as_i64().unwrap() * product.count * 2);
            }
        });
        let request = json!({"viewer_id":viewer_id,"api_count":600+index,"shop_type":4,"shop_item_id":product.item_id,"number":product.count});
        select_product_lineup(&service, viewer_id, product.item_id);
        let first = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request),
        ));
        let before = read_player_snapshot(root.path());
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/calendar/{}", product.activity_id),
            json!({"enabled":false,"start_at_ms":1906502400000_i64,"end_at_ms":1906588800000_i64}),
        );
        let replay = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request),
        ));
        assert_eq!(replay.data, first.data, "{}", product.activity_id);
        assert_eq!(
            read_player_snapshot(root.path()),
            before,
            "{}",
            product.activity_id
        );
        let mut another = request.clone();
        another["api_count"] = json!(700 + index);
        let closed = cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&another),
        );
        assert_eq!(
            decode_response::<Value>(&closed).data_headers.result_code,
            2053,
            "{}",
            product.activity_id
        );
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/{}/mode", product.activity_id),
            json!({"mode":"always"}),
        );
        let exhausted = cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&another),
        );
        assert_eq!(
            decode_response::<Value>(&exhausted)
                .data_headers
                .result_code,
            3010,
            "{}",
            product.activity_id
        );
        let reset_path = format!(
            "/v1/local-saves/{slot_id}/activities/{}/reset",
            product.activity_id
        );
        let reset = manage(&service, "POST", &reset_path, json!({"expected_round":0}));
        assert_eq!(reset["next_round"], 1, "{}", product.activity_id);
        let after_reset = read_player_snapshot(root.path());
        for field in [
            "item_list",
            "user_info",
            "user_character_list",
            "user_equipment_list",
        ] {
            assert_eq!(
                after_reset[field], before[field],
                "{}:{field}",
                product.activity_id
            );
        }
        assert_eq!(
            after_reset["shop_purchase_counts"],
            before["shop_purchase_counts"]
        );
        select_product_lineup(&service, viewer_id, product.item_id);
        let second = cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request),
        );
        assert!(
            second.starts_with("HTTP/1.1 200 OK"),
            "{}: {second}",
            product.activity_id
        );
        let stored = read_player_snapshot(root.path());
        assert_eq!(
            stored["shop_purchase_counts"][format!("4:{}", product.item_id)],
            product.count * 2
        );
        assert_ne!(
            stored["item_list"], after_reset["item_list"],
            "{}",
            product.activity_id
        );
        let replay = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request),
        ));
        assert_eq!(replay.data, decode_response::<Value>(&second).data);
        assert_eq!(read_player_snapshot(root.path()), stored);
        let history = manage(
            &service,
            "GET",
            &format!(
                "/v1/local-saves/{slot_id}/activities/{}/history",
                product.activity_id
            ),
            json!({}),
        );
        let exchanges = history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["kind"] == "exchange")
            .collect::<Vec<_>>();
        assert_eq!(exchanges.len(), 2, "{}", product.activity_id);
        assert_eq!(exchanges[0]["participation_round"], 1);
        assert_eq!(exchanges[1]["participation_round"], 0);
    }
    service.stop().unwrap();
}
// //// /活动商店重置库存时保留资产和历史并隔离请求重放 ////
