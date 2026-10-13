// audience: internal
// # personal-service-cn-shop-entry-tests
// 客户端目录请求和购买请求共同验证商店入口, 付费币范围和批量奖励.

use super::*;

fn send(service: &PersonalService, route: &str, body: Value) -> Value {
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        &format!("/api/index.php/shop/{route}"),
        &encode_request(&body),
    ));
    assert_eq!(
        response.data_headers.result_code, 1,
        "{route}: {}",
        response.data
    );
    response.data
}

// //// 从讨伐币全部分类入口连续兑换多件素材和装备 [@x380kkm 2026-09-24] ////
#[test]
fn opens_all_boss_categories_and_buys_multiple_products() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 241);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    let sales = send(
        &service,
        "get_sales_list",
        json!({"viewer_id":viewer,"shop_types":[7],"boss_coin_shop_category_ids":[],"event_list":[],"equipment_enhancement_shop_category_ids":[]}),
    );
    let entries = sales["sales_list"].as_array().unwrap();
    assert!(entries.iter().any(|sale| sale["shop_item_id"] == 200103));
    assert!(entries.iter().any(|sale| sale["shop_item_id"] == 201015));
    let filtered = send(
        &service,
        "get_sales_list",
        json!({"viewer_id":viewer,"shop_types":[7],"boss_coin_shop_category_ids":[1]}),
    );
    assert!(entries.len() > filtered["sales_list"].as_array().unwrap().len());
    update_player_snapshot(root.path(), |data| {
        data["item_list"]["40000"] = json!(100);
        data["item_list"]["13"] = json!(0);
        data["user_equipment_list"]
            .as_object_mut()
            .unwrap()
            .remove("4010014");
    });
    let request = json!({"viewer_id":viewer,"api_count":1200,"shop_type":7,"buy_item_list":{"200103":12,"200110":3}});
    let bought = send(&service, "bulk_buy", request.clone());
    assert_eq!(bought["item_list"], json!({"40000":13,"13":12}));
    assert_eq!(bought["equipment_list"][0]["stack"], 2);
    let stored = read_player_snapshot(root.path());
    assert_eq!(send(&service, "bulk_buy", request), bought);
    assert_eq!(read_player_snapshot(root.path()), stored);
    service.stop().unwrap();
}
// //// /从讨伐币全部分类入口连续兑换多件素材和装备 ////

// //// 从特殊礼包和玛纳目录购买并分别扣除可用星导石 [@x380kkm 2026-09-24] ////
#[test]
fn opens_special_pack_and_mana_shops_with_exact_currency_costs() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 242);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    let sales = send(
        &service,
        "get_sales_list",
        json!({"viewer_id":viewer,"shop_types":[3,5]}),
    );
    let entries = sales["sales_list"].as_array().unwrap();
    assert!(entries
        .iter()
        .any(|sale| sale["shop_type"] == 3 && sale["shop_item_id"] == 200001));
    assert_eq!(
        entries.iter().filter(|sale| sale["shop_type"] == 5).count(),
        3
    );
    update_player_snapshot(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(2000);
        data["user_info"]["free_vmoney"] = json!(60);
        data["user_info"]["free_mana"] = json!(0);
        data["item_list"]["999010"] = json!(0);
        data["item_list"]["999001"] = json!(0);
    });
    let pack = send(
        &service,
        "buy",
        json!({"viewer_id":viewer,"api_count":1201,"shop_type":3,"shop_item_id":200001,"number":1}),
    );
    assert_eq!(pack["user_info"]["vmoney"], 500);
    assert_eq!(pack["user_info"]["free_vmoney"], 60);
    assert_eq!(pack["item_list"], json!({"999010":1,"999001":1}));
    let mana = send(
        &service,
        "bulk_buy",
        json!({"viewer_id":viewer,"api_count":1202,"shop_type":5,"buy_item_list":{"200001":1,"200002":1}}),
    );
    assert_eq!(mana["user_info"]["vmoney"], 260);
    assert_eq!(mana["user_info"]["free_vmoney"], 10);
    assert_eq!(mana["user_info"]["free_mana"], 35000);
    let stored = read_player_snapshot(root.path());
    let receipt = stored["shop_request_receipts"]
        .as_object()
        .unwrap()
        .values()
        .find(|receipt| receipt["request"]["shop_type"] == 5)
        .unwrap();
    assert_eq!(
        receipt["costs"],
        json!([
            {"kind":"free_vmoney","count":50},
            {"kind":"free_vmoney","count":10},
            {"kind":"vmoney","count":240}
        ])
    );
    assert_eq!(
        read_player_snapshot(root.path())["shop_purchase_counts"]["3:200001"],
        1
    );
    service.stop().unwrap();
}
// //// /从特殊礼包和玛纳目录购买并分别扣除可用星导石 ////

// //// 购买多份修行点数时按所选活动容量完整扣发 [@x380kkm 2026-09-24] ////
#[test]
fn purchases_pass_card_points_without_truncating_quantity() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 243);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(1000);
        data["user_info"]["free_vmoney"] = json!(5000);
        data["pass_card_state"] = json!({"current_event_id":1,"events":{"1":{"point":5700,"is_buy":false,"all_received_record":[]}}});
    });
    let request =
        json!({"viewer_id":viewer,"api_count":1203,"shop_type":3,"shop_item_id":220040,"number":3});
    let bought = send(&service, "buy", request.clone());
    assert_eq!(bought["user_info"]["vmoney"], 850);
    assert_eq!(bought["user_info"]["free_vmoney"], 5000);
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["pass_card_state"]["events"]["1"]["point"], 6000);
    assert_eq!(send(&service, "buy", request), bought);
    let rejected = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(
            &json!({"viewer_id":viewer,"shop_type":3,"shop_item_id":220040,"number":1}),
        ),
    ));
    assert_eq!(rejected.data_headers.result_code, 3010);
    assert_eq!(read_player_snapshot(root.path()), stored);
    service.stop().unwrap();
}
// //// /购买多份修行点数时按所选活动容量完整扣发 ////

// //// 积分购买保留合法尾期并隔离过期活动和整单失败 [@x380kkm 2026-09-25] ////
#[test]
fn pass_points_use_a_redeemable_event_and_preserve_failed_bundles() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 245);
    set_virtual_time(&service, "2025-01-02T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(10000);
        data["pass_card_state"] = json!({"current_event_id":1,"events":{
            "1":{"point":100,"is_buy":false,"all_received_record":[]},
            "7":{"point":300,"is_buy":false,"all_received_record":[]}
        }});
    });
    let purchase = json!({"viewer_id":viewer,"shop_type":3,"shop_item_id":220040,"number":2});
    let bought = send(&service, "buy", purchase.clone());
    assert_eq!(bought["user_info"]["vmoney"], 9900);
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["pass_card_state"]["events"]["1"]["point"], 100);
    assert_eq!(stored["pass_card_state"]["events"]["8"]["point"], 200);

    update_player_snapshot(root.path(), |data| {
        data["pass_card_state"]["current_event_id"] = json!(7);
    });
    send(&service, "buy", purchase.clone());
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["pass_card_state"]["events"]["7"]["point"], 500);
    assert_eq!(stored["pass_card_state"]["events"]["8"]["point"], 200);

    set_virtual_time(&service, "2030-01-02T12:00:00.000Z");
    let sales = send(&service, "get_sales_list", json!({"viewer_id":viewer,"shop_types":[3]}));
    assert!(sales["sales_list"].as_array().unwrap().iter().all(|sale| sale["shop_item_id"] != 220040));
    let before = read_player_snapshot(root.path());
    let failed = decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/shop/bulk_buy",
        &encode_request(&json!({"viewer_id":viewer,"shop_type":3,"buy_item_list":{"200001":1,"220040":2}})),
    ));
    assert_ne!(failed.data_headers.result_code, 1);
    assert_eq!(read_player_snapshot(root.path()), before);
    service.stop().unwrap();
}
// //// /积分购买保留合法尾期并隔离过期活动和整单失败 ////

// //// 临时开放结束后的兑换期沿用同一库存和购买记录 [@x380kkm 2026-09-24] ////
#[test]
fn keeps_temporary_event_shop_stock_during_reward_tail() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    fs::write(cdn.path().join("activity-catalog.json"), json!({"format_version":1,"activities":[{"activity_id":"carnival:1","name":"嘉年华","kind":"carnival"}]}).to_string()).unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let viewer = signup(&service, 244);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    let token = format!("Bearer {}", service.management_token());
    let opened = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/carnival:1/temporary-open",
        "application/json",
        &[("Authorization", &token)],
        b"{}",
    );
    assert!(opened.starts_with("HTTP/1.1 200"), "{opened}");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    connection.execute(
        "UPDATE activity_temporary_open_leases SET opened_at_ms=?1 WHERE activity_id='carnival:1'",
        [now_ms - 86_400_000],
    ).unwrap();
    update_player_snapshot(root.path(), |data| data["item_list"]["90030"] = json!(300));
    send(
        &service,
        "buy",
        json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":1,"number":1}),
    );
    connection.execute(
        "UPDATE activity_temporary_open_leases SET expires_at_ms=?1 WHERE activity_id='carnival:1'",
        [now_ms - 60_000],
    ).unwrap();
    let sales = send(
        &service,
        "get_sales_list",
        json!({"viewer_id":viewer,"shop_types":[4],"event_list":[{"event_type":10,"event_ids":[1]}]}),
    );
    let sale = sales["sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sale| sale["shop_item_id"] == 1)
        .unwrap();
    assert_eq!(sale["stock_quantity"], 1);
    assert_eq!(sale["total_purchase_num"], 1);
    send(
        &service,
        "buy",
        json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":1,"number":1}),
    );
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["shop_purchase_counts"]["4:1"], 2);
    assert_eq!(stored["item_list"]["90030"], 0);
    service.stop().unwrap();
}
// //// /临时开放结束后的兑换期沿用同一库存和购买记录 ////
