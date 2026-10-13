// audience: internal
// # personal-service-cn-shop-selection-lifecycle-tests
// 选择状态跨活动和账号隔离, 显式重开保留资产与旧轮次记录.

use super::*;

fn open(service: &PersonalService, id: i64) {
    manage(
        service,
        "PUT",
        &format!("/v1/activities/event-shop-select-item-campaign:{id}/mode"),
        json!({"mode":"always"}),
    );
}

fn start(root: &Path, cdn: &Path) -> PersonalService {
    let definitions = catalog();
    let entries: Vec<_> = definitions["campaigns"].as_object().unwrap().values().map(|entry| {
        json!({"activity_id":entry["activity_id"],"name":"幽玄域自选商店","kind":"event-shop"})
    }).collect();
    std::fs::write(
        cdn.join("activity-catalog.json"),
        json!({"format_version":1,"activities":entries}).to_string(),
    )
    .unwrap();
    PersonalService::start_with_cdn_root(root, 0, cdn).unwrap()
}

fn activity_path(service: &PersonalService, action: &str) -> String {
    let saves = manage(service, "GET", "/v1/local-saves", json!({}));
    format!(
        "/v1/local-saves/{}/activities/event-shop-select-item-campaign:10/{action}",
        saves["slots"][0]["id"]
    )
}

// //// 保留活动隔离和重启选择并安全处理空配置 [@x380kkm 2026-09-23] ////
#[test]
fn scopes_choices_and_preserves_them_across_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    open(&service, 10);
    open(&service, 11);
    let viewer = signup(&service, 1);
    update(root.path(), |data| {
        data["shop_campaign_lineup_id"] = json!(1010);
    });
    send(
        &service,
        "shop/set_campaign_lineup_id",
        &selection(viewer, 10, Some(1010)),
    );
    assert!(send(
        &service,
        "shop/get_campaign_lineup_id",
        &selection(viewer, 11, None)
    )["lineup_id"]
        .is_null());
    send(
        &service,
        "shop/set_campaign_lineup_id",
        &selection(viewer, 11, Some(1040)),
    );
    let before = stored(root.path());
    for body in [
        selection(viewer, 10, Some(1040)),
        selection(viewer, 10, Some(1020)),
        selection(viewer, 10, None),
    ] {
        let response =
            decode_response::<Value>(&request(&service, "shop/set_campaign_lineup_id", &body));
        assert_eq!(response.data_headers.result_code, 20001);
        assert_eq!(stored(root.path()), before);
    }
    let stranger = signup(&service, 2);
    assert!(send(
        &service,
        "shop/get_campaign_lineup_id",
        &selection(stranger, 10, None)
    )["lineup_id"]
        .is_null());
    let boss = decode_response::<Value>(&request(
        &service,
        "shop/get_campaign_lineup_id",
        &json!({"viewer_id":viewer,"shop_type":7,"campaign_id":10}),
    ));
    assert_eq!(boss.data_headers.result_code, 1652);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(
        send(
            &service,
            "shop/get_campaign_lineup_id",
            &selection(viewer, 10, None)
        )["lineup_id"],
        1010
    );
    assert_eq!(
        send(
            &service,
            "shop/get_campaign_lineup_id",
            &selection(viewer, 11, None)
        )["lineup_id"],
        1040
    );
    let loaded = send(
        &service,
        "load",
        &json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert!(loaded.get("shop_campaign_lineups").is_none());
    service.stop().unwrap();
}

// //// 按所选商品限制购买并保留批购失败前状态 [@x380kkm 2026-09-23] ////
#[test]
fn rejects_unselected_purchases_and_projects_item_sources() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    open(&service, 10);
    let viewer = signup(&service, 1);
    update(root.path(), |data| {
        data["item_list"]["30005"] = json!(1000);
    });
    let sources = send(
        &service,
        "how_to_get/get_list",
        &json!({"viewer_id":viewer,"equipment_id":5020025}),
    );
    assert!(sources["unselected_lineup_shop_sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|sale| sale["shop_item_id"] == 100010));
    let before = stored(root.path());
    let blocked = request(
        &service,
        "shop/buy",
        &json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":100010,"number":1,"api_count":1}),
    );
    assert_eq!(decode_response::<Value>(&blocked).data_headers.result_code, 2053);
    assert_eq!(stored(root.path()), before);
    send(
        &service,
        "shop/set_campaign_lineup_id",
        &selection(viewer, 10, Some(1010)),
    );
    let definitions = catalog();
    let wrong: i64 = definitions["products"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, product)| product["campaign"] == "4:10" && product["lineup_id"] == 1020)
        .unwrap()
        .0
        .strip_prefix("4:")
        .unwrap()
        .parse()
        .unwrap();
    let before = stored(root.path());
    let blocked = request(
        &service,
        "shop/bulk_buy",
        &json!({"viewer_id":viewer,"shop_type":4,"api_count":2,"buy_item_list":{"100010":1,wrong.to_string():1}}),
    );
    assert_eq!(decode_response::<Value>(&blocked).data_headers.result_code, 2053);
    assert_eq!(stored(root.path()), before);
    let sources = send(
        &service,
        "how_to_get/get_list",
        &json!({"viewer_id":viewer,"equipment_id":5020025}),
    );
    assert!(sources["shop_sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|sale| sale["shop_item_id"] == 100010));
    assert!(!sources["unselected_lineup_shop_sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|sale| sale["shop_item_id"] == 100010));
    manage(
        &service,
        "POST",
        "/v1/activities/challenge-dungeon:1/close",
        json!({}),
    );
    let before = stored(root.path());
    for (route, body) in [
        ("shop/buy", json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":100010,"number":1,"api_count":3})),
        ("shop/bulk_buy", json!({"viewer_id":viewer,"shop_type":4,"api_count":4,"buy_item_list":{"100010":1}})),
    ] {
        let closed = decode_response::<Value>(&request(&service, route, &body));
        assert_eq!(closed.data_headers.result_code, 2053, "{route}");
        assert_eq!(stored(root.path()), before, "{route}");
    }
    service.stop().unwrap();
}

// //// 显式重开清理选择和限购并保留两轮领取记录 [@x380kkm 2026-09-23] ////
#[test]
fn resets_choices_and_stock_while_retaining_history() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    open(&service, 10);
    open(&service, 11);
    let viewer = signup(&service, 1);
    update(root.path(), |data| {
        data["item_list"]["30005"] = json!(1000);
    });
    let choice = selection(viewer, 10, Some(1010));
    send(&service, "shop/set_campaign_lineup_id", &choice);
    send(
        &service,
        "shop/set_campaign_lineup_id",
        &selection(viewer, 11, Some(1040)),
    );
    let body =
        json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":100010,"number":1,"api_count":1});
    send(&service, "shop/buy", &body);
    let old = manage(
        &service,
        "GET",
        &activity_path(&service, "history"),
        json!({}),
    );
    assert!(old["can_reset"].as_bool().unwrap());
    let before = stored(root.path());
    manage(
        &service,
        "POST",
        &activity_path(&service, "reset"),
        json!({"expected_round":0}),
    );
    let reset = stored(root.path());
    assert_eq!(reset["item_list"], before["item_list"]);
    assert_eq!(reset["user_equipment_list"], before["user_equipment_list"]);
    assert!(send(&service, "shop/get_campaign_lineup_id", &choice)["lineup_id"].is_null());
    assert_eq!(
        send(
            &service,
            "shop/get_campaign_lineup_id",
            &selection(viewer, 11, None)
        )["lineup_id"],
        1040
    );
    send(&service, "shop/set_campaign_lineup_id", &choice);
    send(&service, "shop/buy", &body);
    assert_eq!(
        quantity(&stored(root.path()), "user_equipment_list", 5020025),
        quantity(&before, "user_equipment_list", 5020025) + 1
    );
    let history = manage(
        &service,
        "GET",
        &activity_path(&service, "history"),
        json!({}),
    );
    for round in [0, 1] {
        assert!(history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["participation_round"] == round && entry["kind"] == "exchange"));
    }
    assert!(
        history["entries"].as_array().unwrap().len() > old["entries"].as_array().unwrap().len()
    );
    service.stop().unwrap();
}

// //// 活动结束后按兑换期继续选择购买并保留回拨后的选择 [@x380kkm 2026-09-23] ////
#[test]
fn follows_campaign_exchange_window_and_management_status() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let definitions = catalog();
    let campaign = &definitions["campaigns"]["4:10"];
    let viewer = signup(&service, 1);
    let choice = selection(viewer, 10, Some(1010));
    set_time(&service, campaign["start_time"].as_i64().unwrap() - 60);
    let before =
        decode_response::<Value>(&request(&service, "shop/get_campaign_lineup_id", &choice));
    assert_eq!(before.data_headers.result_code, 1652);
    set_time(&service, campaign["end_time"].as_i64().unwrap() + 60);
    send(&service, "shop/set_campaign_lineup_id", &choice);
    update(root.path(), |data| {
        data["item_list"]["30005"] = json!(1000);
    });
    send(
        &service,
        "shop/buy",
        &json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":100010,"number":1,"api_count":1}),
    );
    set_time(
        &service,
        campaign["exchange_end_time"].as_i64().unwrap() + 60,
    );
    let ended =
        decode_response::<Value>(&request(&service, "shop/get_campaign_lineup_id", &choice));
    assert_eq!(ended.data_headers.result_code, 1652);
    assert!(!sales(&service, viewer)
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["shop_item_id"] == 100010));
    set_time(&service, campaign["end_time"].as_i64().unwrap() + 60);
    assert_eq!(
        send(&service, "shop/get_campaign_lineup_id", &choice)["lineup_id"],
        1010
    );
    manage(
        &service,
        "POST",
        "/v1/activities/event-shop-select-item-campaign:10/close",
        json!({}),
    );
    let closed =
        decode_response::<Value>(&request(&service, "shop/get_campaign_lineup_id", &choice));
    assert_eq!(closed.data_headers.result_code, 1652);
    open(&service, 10);
    assert_eq!(
        send(&service, "shop/get_campaign_lineup_id", &choice)["lineup_id"],
        1010
    );
    service.stop().unwrap();
}
