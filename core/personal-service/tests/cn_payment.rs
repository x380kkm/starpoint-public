// audience: internal
// # personal-service-cn-payment-tests
// 商品领取通过客户端请求格式核对到账资产, 订单恢复, 账号隔离和事务回滚.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
#[path = "cn_payment/subscriptions.rs"]
mod subscriptions;
#[path = "cn_payment/availability.rs"]
mod availability;
#[path = "cn_payment/item_selection.rs"]
mod item_selection;
#[path = "cn_payment/compatibility.rs"]
mod compatibility;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn request(service: &PersonalService, route: &str, body: Value) -> String {
    cn_support::send_request(service.port(), &format!("/api/index.php/{route}"), &encode_request(&body))
}

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn snapshot(root: &Path) -> Value {
    let data: String = database(root).query_row("SELECT data_json FROM player_snapshots ORDER BY account_id LIMIT 1", [], |row| row.get(0)).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn update_snapshot(root: &Path, update: impl FnOnce(&mut Value)) {
    let mut data = snapshot(root);
    update(&mut data);
    database(root).execute("UPDATE player_snapshots SET data_json=?1 WHERE account_id=(SELECT MIN(account_id) FROM player_snapshots)", [data.to_string()]).unwrap();
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&request(service, "tool/signup", json!({"device_id":47}))).data_headers.viewer_id
}

fn set_time(service: &PersonalService, iso: &str) {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(service.port(), "PUT", "/v1/time", "application/json", &[("Authorization", &token)],
        json!({"enabled":true,"iso":iso,"rate":1}).to_string().as_bytes());
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn start(service: &PersonalService, viewer: i64, count: i64, product: &str) -> Value {
    let response = request(service, "payment/start", json!({"viewer_id":viewer,"api_count":count,"payment":{"product_id":product}}));
    let response = decode_response::<Value>(&response);
    assert_eq!(response.data_headers.result_code, 1, "{product}: {}", response.data);
    response.data
}

fn finish(service: &PersonalService, viewer: i64, product: &str, order: &str) -> Value {
    let response = request(service, "payment/finish", json!({"viewer_id":viewer,"payment":{
        "transaction_id":"","signature":"","original_receipt":format!("{product}:{order}"),"currency_code":"CNY","price_number":6}}));
    let response = decode_response::<Value>(&response);
    assert_eq!(response.data_headers.result_code, 1);
    response.data
}

fn history_count(root: &Path) -> i64 {
    database(root).query_row("SELECT COUNT(*) FROM player_receive_history WHERE event_key LIKE 'payment:%'", [], |row| row.get(0)).unwrap()
}

// //// 点击即到账并使客户端重试保持一次领取 [@x380kkm 2026-09-22] ////
#[test]
fn grants_stones_on_click_and_replays_start_and_finish() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let before = snapshot(root.path());
    let product = "com.leiting.wf.stone_50";
    let order = start(&service, viewer, 1, product);
    assert_eq!(order["orderId"], order["order_id"]);
    let delivered = snapshot(root.path());
    assert_eq!(delivered["user_info"]["vmoney"], before["user_info"]["vmoney"].as_i64().unwrap() + 50);
    assert_eq!(history_count(root.path()), 1);
    assert_eq!(start(&service, viewer, 1, product), order);
    assert_eq!(snapshot(root.path()), delivered);
    let finished = finish(&service, viewer, product, order["order_id"].as_str().unwrap());
    assert_eq!(finished["after_vmoney"], delivered["user_info"]["vmoney"]);
    assert_eq!(finished["after_free_vmoney"], delivered["user_info"]["free_vmoney"]);
    assert_eq!(finished["first_payment"], true);
    assert_eq!(finished["first_time"], true);
    assert!(finished["monthly_payment_total"].is_i64());
    assert_eq!(finished["purchased_times_list"][product], 1);
    assert_eq!(finished["user_info"]["renewal_gift_remain_days"], json!({}));
    assert!(finished["user_info"].get("vmoney").is_none());
    assert!(finished["user_info"].get("free_vmoney").is_none());
    assert_eq!(finish(&service, viewer, product, order["order_id"].as_str().unwrap()), finished);
    assert_eq!(history_count(root.path()), 1);
    let next = start(&service, viewer, 2, product);
    assert_ne!(next["order_id"], order["order_id"]);
    assert_eq!(snapshot(root.path())["user_info"]["vmoney"], delivered["user_info"]["vmoney"].as_i64().unwrap() + 50);
    service.stop().unwrap();
}

// //// 重启后恢复已到账订单并隔离其他账号 [@x380kkm 2026-09-22] ////
#[test]
fn recovers_unfinished_orders_without_external_receipts() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let product = "com.leiting.wf.stone_50";
    let order = start(&service, viewer, 1, product)["order_id"].as_str().unwrap().to_owned();
    let delivered = snapshot(root.path());
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let pending = decode_response::<Value>(&request(&service, "channels/channel_leiting_pay/query_unfinish_order", json!({"viewer_id":viewer}))).data;
    assert_eq!(pending["order_id"], order);
    assert_eq!(pending["product_key"], product);
    assert_eq!(pending["payment_money"], 6);
    assert_eq!(pending["exchangeRate"], 1);
    let mut restored = delivered.clone();
    restored["payment_state"]["orders"][&order]["viewer_id"] = json!(viewer);
    restored["payment_state"]["orders"][&order]["api_count"] = Value::Null;
    assert_eq!(snapshot(root.path()), restored);
    let stranger = decode_response::<SignupData>(&request(&service, "tool/signup", json!({"device_id":48}))).data_headers.viewer_id;
    let response = decode_response::<Value>(&request(&service, "payment/finish", json!({"viewer_id":stranger,"payment":{"transaction_id":order}})));
    assert_ne!(response.data_headers.result_code, 1);
    assert_eq!(snapshot(root.path()), restored);
    let query = decode_response::<Value>(&request(&service, "channels/channel_leiting_pay/query_purcharge", json!({"viewer_id":viewer,"payment":{"order_id":order}}))).data;
    assert_eq!(query["status"], 3);
    finish(&service, viewer, product, &order);
    let pending = decode_response::<Value>(&request(&service, "channels/channel_leiting_pay/query_unfinish_order", json!({"viewer_id":viewer}))).data;
    assert_eq!(pending["order_id"], "");
    assert_eq!(history_count(root.path()), 1);
    assert_eq!(snapshot(root.path())["user_info"]["vmoney"], delivered["user_info"]["vmoney"]);
    service.stop().unwrap();
}

// //// 领取记录失败时回滚资产和订单并允许同一点击重试 [@x380kkm 2026-09-22] ////
#[test]
fn rolls_back_the_order_when_reward_history_fails() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    let before = snapshot(root.path());
    database(root.path()).execute_batch("CREATE TRIGGER reject_payment_history BEFORE INSERT ON player_receive_history WHEN NEW.event_key LIKE 'payment:%' BEGIN SELECT RAISE(ABORT,'test payment history failure'); END;").unwrap();
    let body = json!({"viewer_id":viewer,"api_count":1,"payment":{"product_id":"com.leiting.wf.stone_50"}});
    let response = request(&service, "payment/start", body.clone());
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(history_count(root.path()), 0);
    database(root.path()).execute_batch("DROP TRIGGER reject_payment_history").unwrap();
    let response = decode_response::<Value>(&request(&service, "payment/start", body));
    assert_eq!(response.data_headers.result_code, 1);
    assert_eq!(history_count(root.path()), 1);
    let delivered = snapshot(root.path());
    let unknown = decode_response::<Value>(&request(&service, "payment/start", json!({"viewer_id":viewer,"api_count":2,"payment":{"product_id":"unknown"}})));
    assert_ne!(unknown.data_headers.result_code, 1);
    assert_eq!(snapshot(root.path()), delivered);
    service.stop().unwrap();
}

// //// 逐项验证目录中商品的实际资产和资格发放 [@x380kkm 2026-09-22] ////
#[test]
fn delivers_all_catalog_product_contents() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2025-06-05T02:00:00.000Z");
    let viewer = signup(&service);
    let fixture: Value = serde_json::from_str(include_str!("../assets/cn-payment-catalog.json")).unwrap();
    let products = fixture["products"].as_object().unwrap();
    for (index, (id, product)) in products.iter().enumerate() {
        if product["direct_purchase"] == false { continue; }
        let iso: String = database(root.path()).query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1, 'unixepoch')",
            [product["start_time"].as_i64().unwrap()], |row| row.get(0)).unwrap();
        set_time(&service, &iso);
        let before = snapshot(root.path());
        let order = start(&service, viewer, index as i64 + 1, id);
        let after = snapshot(root.path());
        assert_eq!(after["purchased_times_list"][id], 1, "{id}");
        assert_eq!(after["user_info"]["vmoney"].as_i64().unwrap() - before["user_info"]["vmoney"].as_i64().unwrap(), product["charge_vmoney_num"].as_i64().unwrap(), "{id}");
        assert!(after["user_info"]["free_vmoney"].as_i64().unwrap() - before["user_info"]["free_vmoney"].as_i64().unwrap() >= product["free_vmoney_num"].as_i64().unwrap(), "{id}");
        for reward in product["rewards"].as_array().unwrap() {
            let amount = reward["amount"].as_i64().unwrap();
            match reward["kind"].as_str().unwrap() {
                "item" => {
                    let key = reward["id"].to_string();
                    assert!(after["item_list"][&key].as_i64().unwrap_or_default() - before["item_list"][&key].as_i64().unwrap_or_default() >= amount, "{id}:{key}");
                }
                "experience" | "mana" | "star_crumb" => {
                    let field = match reward["kind"].as_str().unwrap() { "experience" => "exp_pool", "mana" => "free_mana", _ => "star_crumb" };
                    assert!(after["user_info"][field].as_i64().unwrap() - before["user_info"][field].as_i64().unwrap() >= amount, "{id}:{field}");
                }
                "character" => assert!(after["user_character_list"].get(reward["id"].to_string()).is_some(), "{id}"),
                "free_vmoney" | "pass_card_point" => {}
                kind => panic!("uncovered payment reward: {kind}"),
            }
        }
        finish(&service, viewer, id, order["order_id"].as_str().unwrap());
    }
    service.stop().unwrap();
}
