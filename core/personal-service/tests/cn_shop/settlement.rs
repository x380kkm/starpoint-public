// audience: internal
// # personal-service-cn-shop-settlement-tests
// 此文件核对实际商品的开放时刻、逐级强化、费用原子性和请求重试.

use super::*;

// //// 准备具有强化材料的装备 [@x380kkm 2026-09-21] ////
fn prepare_equipment(root: &Path, level: i64, enhancement_level: i64) {
    update_player_snapshot(root, |data| {
        data["user_equipment_list"]["5020035"] = json!({"equipment_id": 5020035, "level": level, "enhancement_level": enhancement_level, "stack": 0, "protection": false});
        for item_id in [14033, 14040, 14041, 30028] {
            data["item_list"][item_id.to_string()] = json!(500);
        }
    });
}
// //// /准备具有强化材料的装备 ////

// //// 在 CN 中午开放时同时验证目录和购买 [@x380kkm 2026-09-21] ////
#[test]
fn uses_cn_master_time_for_sales_and_purchases() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 220);
    prepare_equipment(root.path(), 5, 0);
    let request = json!({"viewer_id": viewer_id, "shop_type": 10, "shop_item_id": 1, "number": 1});
    let sales_request = json!({"viewer_id": viewer_id, "shop_types": [10], "boss_coin_shop_category_ids": [], "event_list": []});
    set_virtual_time(&service, "2024-05-23T03:59:00.000Z");
    let before = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    );
    assert_eq!(
        decode_response::<Value>(&before).data_headers.result_code,
        2053
    );
    let sales = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&sales_request),
    ));
    assert!(sales.data["sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .all(|sale| sale["shop_item_id"] != 1));
    set_virtual_time(&service, "2024-05-23T04:00:00.000Z");
    let sales = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&sales_request),
    ));
    assert!(sales.data["sales_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|sale| sale["shop_item_id"] == 1));
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(bought.data["equipment_list"][0]["enhancement_level"], 1);
    assert_eq!(bought.data["item_list"]["14033"], 490);
    assert_eq!(bought.data["item_list"]["14040"], 499);
    service.stop().unwrap();
}
// //// /在 CN 中午开放时同时验证目录和购买 ////

// //// 按数量逐级强化并在跨阶段兑换失败时保留资产 [@x380kkm 2026-09-21] ////
#[test]
fn validates_equipment_stage_and_bulk_costs() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 221);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    prepare_equipment(root.path(), 4, 70);
    let request = json!({"viewer_id": viewer_id, "shop_type": 10, "shop_item_id": 4, "number": 1});
    let before = read_player_snapshot(root.path());
    let failed = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    );
    assert_eq!(
        decode_response::<Value>(&failed).data_headers.result_code,
        3010
    );
    assert_eq!(read_player_snapshot(root.path()), before);
    prepare_equipment(root.path(), 5, 70);
    let before = read_player_snapshot(root.path());
    let invalid = json!({"viewer_id": viewer_id, "shop_type": 10, "shop_item_id": 2, "number": 1});
    assert_eq!(
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&invalid)
        ))
        .data_headers
        .result_code,
        3010
    );
    let overflow =
        json!({"viewer_id": viewer_id, "shop_type": 10, "shop_item_id": 4, "number": 29});
    assert_eq!(
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&overflow)
        ))
        .data_headers
        .result_code,
        3010
    );
    assert_eq!(read_player_snapshot(root.path()), before);
    let failed_bulk =
        json!({"viewer_id": viewer_id, "shop_type": 10, "buy_item_list": {"4": 27, "5": 1}});
    assert_eq!(
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/bulk_buy",
            &encode_request(&failed_bulk)
        ))
        .data_headers
        .result_code,
        3010
    );
    assert_eq!(read_player_snapshot(root.path()), before);
    let bulk = json!({"viewer_id": viewer_id, "shop_type": 10, "buy_item_list": {"4": 28, "5": 1}});
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&bulk),
    ));
    assert_eq!(bought.data["equipment_list"][0]["enhancement_level"], 99);
    assert_eq!(bought.data["item_list"]["14033"], 408);
    assert_eq!(bought.data["item_list"]["14040"], 498);
    assert_eq!(bought.data["item_list"]["14041"], 499);
    assert_eq!(bought.data["item_list"]["30028"], 496);
    service.stop().unwrap();
}
// //// /按数量逐级强化并在跨阶段兑换失败时保留资产 ////

// //// 重试单买和批量请求时保留一次扣费和发放 [@x380kkm 2026-09-21] ////
#[test]
fn replays_purchase_receipts_after_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 222);
    set_virtual_time(&service, "2020-01-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        data["user_info"]["free_mana"] = json!(1000);
        data["item_list"]["990008"] = json!(100);
    });
    let single = json!({"viewer_id": viewer_id, "api_count": 101, "shop_type": 2, "shop_item_id": 200001, "number": 1});
    let bulk = json!({"viewer_id": viewer_id, "api_count": 102, "shop_type": 9, "buy_item_list": {"100005": 1, "100006": 1}});
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&single),
    ));
    set_virtual_time(&service, "2022-04-20T12:00:00.000Z");
    let second = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&bulk),
    ));
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["user_info"]["free_mana"], 700);
    assert_eq!(stored["item_list"]["990008"], 85);
    assert_eq!(
        stored["shop_request_receipts"].as_object().unwrap().len(),
        2
    );
    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    let replayed = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/shop/buy",
        &encode_request(&single),
    ));
    assert_eq!(replayed.data, first.data);
    let replayed = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&bulk),
    ));
    assert_eq!(replayed.data, second.data);
    let mut conflict = single.clone();
    conflict["number"] = json!(2);
    let conflict = cn_support::send_request(
        restarted.port(),
        "/api/index.php/shop/buy",
        &encode_request(&conflict),
    );
    assert!(conflict.contains("shop_request_conflict"), "{conflict}");
    assert_eq!(read_player_snapshot(root.path()), stored);
    restarted.stop().unwrap();
}
// //// /重试单买和批量请求时保留一次扣费和发放 ////

// //// 在 CN 月界同时切换讨伐币费用和商品库存 [@x380kkm 2026-09-21] ////
#[test]
fn changes_scheduled_costs_and_stock_at_cn_month_reset() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 223);
    set_virtual_time(&service, "2024-01-31T20:59:00.000Z");
    update_player_snapshot(root.path(), |data| {
        for id in [40265, 40277, 40301, 40162] {
            data["item_list"][id.to_string()] = json!(100);
        }
        data["item_list"]["40265"] = json!(74);
        data["item_list"]["13001"] = json!(0);
    });
    let request =
        json!({"viewer_id": viewer_id, "shop_type": 8, "shop_item_id": 220032, "number": 1});
    let before = read_player_snapshot(root.path());
    let failed = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    );
    assert_eq!(
        decode_response::<Value>(&failed).data_headers.result_code,
        3010
    );
    assert_eq!(read_player_snapshot(root.path()), before);
    update_player_snapshot(root.path(), |data| data["item_list"]["40265"] = json!(100));
    let january = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(
        january.data["item_list"],
        json!({"40265": 25, "40277": 25, "13001": 1})
    );
    let before = read_player_snapshot(root.path());
    assert_eq!(
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request)
        ))
        .data_headers
        .result_code,
        3010
    );
    assert_eq!(read_player_snapshot(root.path()), before);
    set_virtual_time(&service, "2024-01-31T21:00:00.000Z");
    let february = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(
        february.data["item_list"],
        json!({"40301": 25, "40162": 25, "13001": 2})
    );
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["item_list"]["40265"], 25);
    assert_eq!(stored["item_list"]["40277"], 25);
    assert_eq!(stored["shop_purchase_counts"]["8:220032"], 2);
    service.stop().unwrap();
}
// //// /在 CN 月界同时切换讨伐币费用和商品库存 ////

// //// 批量兑换多个讨伐币商品时完整提交资产和领取记录 [@x380kkm 2026-09-21] ////
#[test]
fn records_a_large_bulk_boss_coin_purchase() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 225);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../assets/boss_coin_shop.json")).unwrap();
    let items = catalog
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_object)
        .flat_map(|items| items.iter())
        .filter(|(_, item)| {
            item["availableFrom"]
                .as_str()
                .map_or(true, |time| time <= "2024-06-01 20:00:00")
                && item["availableUntil"]
                    .as_str()
                    .map_or(true, |time| time >= "2024-06-01 20:00:00")
        })
        .take(35)
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 35);
    update_player_snapshot(root.path(), |data| {
        for (_, item) in &items {
            for cost in item["costs"].as_array().unwrap() {
                let id = cost["id"].as_i64().unwrap().to_string();
                let held = data["item_list"][&id].as_i64().unwrap_or_default();
                data["item_list"][id] = json!(held + cost["amount"].as_i64().unwrap());
            }
        }
    });
    let buy_items = items
        .iter()
        .map(|(id, _)| ((*id).clone(), json!(1)))
        .collect::<serde_json::Map<_, _>>();
    let request = json!({"viewer_id": viewer_id, "api_count": 301, "shop_type": 7, "buy_item_list": buy_items});
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&request),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert_eq!(decode_response::<Value>(&response).data_headers.result_code, 1);
    let stored = read_player_snapshot(root.path());
    for (id, _) in &items {
        assert_eq!(stored["shop_purchase_counts"][format!("7:{id}")], 1);
    }
    let history = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let count: i64 = history
        .query_row(
            "SELECT COUNT(*) FROM player_receive_history WHERE event_key LIKE 'shop:bulk_buy:%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count,
        items
            .iter()
            .map(|(_, item)| item["rewards"].as_array().unwrap().len() as i64)
            .sum::<i64>()
    );
    let retry = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&request),
    );
    assert_eq!(
        decode_response::<Value>(&retry).data,
        decode_response::<Value>(&response).data
    );
    assert_eq!(read_player_snapshot(root.path()), stored);
    service.stop().unwrap();
}
// //// /批量兑换多个讨伐币商品时完整提交资产和领取记录 ////

// //// 无累计限购的商品分别约束每次兑换数量 [@x380kkm 2026-09-21] ////
#[test]
fn keeps_unlimited_stock_separate_from_single_purchase_limit() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 226);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        data["item_list"]["11001"] = json!(2000);
        data["item_list"]["10001"] = json!(0);
    });
    let mut request =
        json!({"viewer_id": viewer_id, "shop_type": 8, "shop_item_id": 200001, "number": 100});
    assert_eq!(
        decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request)
        ))
        .data_headers
        .result_code,
        3010
    );
    request["number"] = json!(99);
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(first.data["item_list"], json!({"11001": 1010, "10001": 99}));
    request["number"] = json!(1);
    let second = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(
        second.data["item_list"],
        json!({"11001": 1000, "10001": 100})
    );
    service.stop().unwrap();
}
// //// /无累计限购的商品分别约束每次兑换数量 ////

// //// 商店任务只累计对应店铺交易和实际玛纳支出 [@x380kkm 2026-09-21] ////
#[test]
fn advances_treasure_and_mana_missions_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 227);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        data["user_info"]["free_mana"] = json!(2000);
        data["item_list"]["11001"] = json!(100);
        data["all_active_mission_list"] = json!({});
    });
    let treasure = json!({"viewer_id": viewer_id, "api_count": 501, "shop_type": 2, "shop_item_id": 200031, "number": 2});
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&treasure),
    ));
    assert_eq!(first.data["user_info"]["free_mana"], 1600);
    let retry = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&treasure),
    ));
    assert_eq!(retry.data, first.data);
    let other = json!({"viewer_id": viewer_id, "api_count": 502, "shop_type": 8, "shop_item_id": 200001, "number": 1});
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&other),
    ));
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["all_active_mission_list"]["12010"]["progress"], 2);
    assert_eq!(stored["all_active_mission_list"]["25005"]["progress"], 400);
    assert_eq!(stored["user_info"]["free_mana"], 1600);
    service.stop().unwrap();
}
// //// /商店任务只累计对应店铺交易和实际玛纳支出 ////
