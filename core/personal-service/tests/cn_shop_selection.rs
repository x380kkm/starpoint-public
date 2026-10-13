// audience: internal
// # personal-service-cn-shop-selection-tests
// 自选商店验证客户端目录, 选择隔离, 购买资产与管理重开记录.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "cn_shop_selection/lifecycle.rs"]
mod lifecycle;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tempfile::TempDir;

fn catalog() -> Value {
    serde_json::from_str(include_str!("../assets/cn-shop-selection.json")).unwrap()
}

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn stored(root: &Path) -> Value {
    let serialized: String = database(root)
        .query_row(
            "SELECT data_json FROM player_snapshots ORDER BY account_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&serialized).unwrap()
}

fn update(root: &Path, change: impl FnOnce(&mut Value)) {
    let mut data = stored(root);
    change(&mut data);
    database(root).execute("UPDATE player_snapshots SET data_json=?1 WHERE account_id=(SELECT MIN(account_id) FROM player_snapshots)", [data.to_string()]).unwrap();
}

fn request(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    )
}

fn send(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = decode_response::<Value>(&request(service, route, body));
    assert_eq!(
        response.data_headers.result_code, 1,
        "{route} {body}: {}",
        response.data
    );
    response.data
}

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&request(
        service,
        "tool/signup",
        &json!({"device_id":device_id}),
    ))
    .data_headers
    .viewer_id
}

fn manage(service: &PersonalService, method: &str, route: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        route,
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn set_time(service: &PersonalService, seconds: i64) {
    manage(
        service,
        "PUT",
        "/v1/time",
        json!({"enabled":true,"unix_time_ms":seconds*1000,"rate":1}),
    );
}

fn selection(viewer: i64, campaign: i64, lineup: Option<i64>) -> Value {
    json!({"viewer_id":viewer,"shop_type":4,"campaign_id":campaign,"lineup_id":lineup})
}

fn sales(service: &PersonalService, viewer: i64) -> Value {
    send(
        service,
        "shop/get_sales_list",
        &json!({"viewer_id":viewer,"event_list":[{"event_type":4,"event_ids":[1]}]}),
    )["sales_list"]
        .clone()
}

fn quantity(data: &Value, field: &str, id: i64) -> i64 {
    let value = &data[field][id.to_string()];
    if field == "user_equipment_list" {
        value.as_object().map_or(0, |_| 1 + value["stack"].as_i64().unwrap())
    } else {
        value.as_i64().unwrap_or_default()
    }
}

// //// 比较一次购买的主表费用和实际奖励增量 [@x380kkm 2026-09-23] ////
fn assert_purchase_delta(before: &Value, after: &Value, item: &Value) {
    let mut item_deltas = BTreeMap::<i64, i64>::new();
    for cost in item["costs"].as_array().unwrap() {
        *item_deltas.entry(cost["id"].as_i64().unwrap()).or_default() -=
            cost["amount"].as_i64().unwrap();
    }
    for reward in item["rewards"].as_array().unwrap() {
        let count = reward["count"].as_i64().unwrap();
        match reward["type"].as_i64().unwrap() {
            0 => *item_deltas.entry(reward["id"].as_i64().unwrap()).or_default() += count,
            1 => assert_eq!(
                after["user_info"]["exp_pool"],
                before["user_info"]["exp_pool"].as_i64().unwrap() + count
            ),
            2 => assert_eq!(
                after["user_info"]["free_mana"],
                before["user_info"]["free_mana"].as_i64().unwrap() + count
            ),
            4 => {
                let id = reward["id"].as_i64().unwrap();
                assert_eq!(
                    quantity(after, "user_equipment_list", id),
                    quantity(before, "user_equipment_list", id) + count
                );
            }
            kind => panic!("uncovered selection shop reward kind {kind}"),
        }
    }
    for (id, delta) in item_deltas {
        assert_eq!(
            quantity(after, "item_list", id),
            quantity(before, "item_list", id) + delta,
            "item {id}"
        );
    }
}

// //// 覆盖全部真实选项和关联商品的目录及购买 [@x380kkm 2026-09-23] ////
#[test]
fn selects_and_purchases_every_catalog_product() {
    let definitions = catalog();
    let shops: Value =
        serde_json::from_str(include_str!("../../../assets/event_item_shop.json")).unwrap();
    let items = &shops["4"]["1"];
    let products = definitions["products"].as_object().unwrap();
    let mut purchased = BTreeSet::new();
    for (key, campaign) in definitions["campaigns"].as_object().unwrap() {
        let campaign_id: i64 = key.strip_prefix("4:").unwrap().parse().unwrap();
        for (lineup_index, lineup) in campaign["lineups"].as_array().unwrap().iter().enumerate() {
            let lineup = lineup.as_i64().unwrap();
            let root = TempDir::new().unwrap();
            let service = PersonalService::start(root.path(), 0).unwrap();
            set_time(&service, campaign["start_time"].as_i64().unwrap() + 60);
            let viewer = signup(&service, 1);
            let selection_body = selection(viewer, campaign_id, Some(lineup));
            assert!(
                send(&service, "shop/get_campaign_lineup_id", &selection_body)["lineup_id"]
                    .is_null()
            );
            send(&service, "shop/set_campaign_lineup_id", &selection_body);
            assert_eq!(
                send(&service, "shop/get_campaign_lineup_id", &selection_body)["lineup_id"],
                lineup
            );
            let chosen_state = stored(root.path());
            send(&service, "shop/set_campaign_lineup_id", &selection_body);
            assert_eq!(stored(root.path()), chosen_state);
            let expected: BTreeSet<_> = products
                .iter()
                .filter(|(_, product)| {
                    product["campaign"] == *key
                        && (product["lineup_id"].is_null() || product["lineup_id"] == lineup)
                })
                .map(|(id, _)| id.strip_prefix("4:").unwrap().parse::<i64>().unwrap())
                .collect();
            let listed = sales(&service, viewer);
            let actual: BTreeSet<_> = listed
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|entry| {
                    let id = entry["shop_item_id"].as_i64().unwrap();
                    products.contains_key(&format!("4:{id}")).then_some(id)
                })
                .collect();
            assert_eq!(actual, expected, "campaign {key}, lineup {lineup}");
            let mut required_costs = BTreeMap::<i64, i64>::new();
            for id in &expected {
                for cost in items[id.to_string()]["costs"].as_array().unwrap() {
                    *required_costs.entry(cost["id"].as_i64().unwrap()).or_default() +=
                        cost["amount"].as_i64().unwrap();
                }
            }
            update(root.path(), |data| {
                for (id, amount) in required_costs {
                    data["item_list"][id.to_string()] = json!(amount);
                }
            });
            for id in expected {
                if products[&format!("4:{id}")]["lineup_id"].is_null() && lineup_index != 0 {
                    continue;
                }
                let before = stored(root.path());
                let body = json!({"viewer_id":viewer,"shop_type":4,"shop_item_id":id,"number":1,"api_count":id});
                let response = send(&service, "shop/buy", &body);
                let after = stored(root.path());
                assert_purchase_delta(&before, &after, &items[id.to_string()]);
                assert_eq!(send(&service, "shop/buy", &body), response);
                assert_eq!(stored(root.path()), after);
                let before_stock = listed
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["shop_item_id"] == id)
                    .unwrap()["stock_quantity"]
                    .as_i64()
                    .unwrap();
                let refreshed = sales(&service, viewer);
                let after_stock = refreshed
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["shop_item_id"] == id)
                    .unwrap()["stock_quantity"]
                    .as_i64()
                    .unwrap();
                assert_eq!(
                    after_stock,
                    if before_stock < 0 {
                        -1
                    } else {
                        before_stock - 1
                    }
                );
                purchased.insert(format!("4:{id}"));
            }
            service.stop().unwrap();
        }
    }
    assert_eq!(purchased, products.keys().cloned().collect());
}
