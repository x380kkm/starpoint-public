// audience: internal
// # personal-service-cn-shop-activity-tests
//
// 该文件验证活动日历控制 CN 活动商店目录和购买入口.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::{fs, path::Path};
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct SalesListRequest {
    viewer_id: i64,
    shop_types: Vec<i64>,
    boss_coin_shop_category_ids: Vec<i64>,
    equipment_enhancement_shop_category_ids: Vec<i64>,
    browse_treasure_flag: bool,
    event_list: Vec<EventShopRequest>,
}

#[derive(Serialize)]
struct EventShopRequest {
    event_type: i64,
    event_ids: Vec<i64>,
}

#[derive(Serialize)]
struct BuyRequest {
    viewer_id: i64,
    shop_type: i64,
    shop_item_id: i64,
    number: i64,
}

#[derive(Serialize)]
struct HowToGetRequest {
    viewer_id: i64,
    item_id: Option<i64>,
    equipment_id: Option<i64>,
}

fn update_player_snapshot(root: &Path, update: impl FnOnce(&mut Value)) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let (account_id, serialized) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("one player snapshot exists");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    update(&mut player_data);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id
            ],
        )
        .expect("player snapshot is updated");
}

// //// 读取测试账号快照 [@x380kkm 2026-08-29] ////
fn read_player_snapshot(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let serialized = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("one player snapshot exists");
    serde_json::from_str(&serialized).expect("player snapshot is JSON")
}
// //// /读取测试账号快照 ////

// //// 验证永久规则和临时租约共同控制活动商店 [@x380kkm 2026-08-24] ////
#[test]
fn hides_disabled_event_shop_inventory() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2023-02-10T00:00:00Z",
            "activities": [{
                "activity_id": "carnival:1",
                "name": "Carnival 1",
                "kind": "carnival",
                "tags": ["test"],
                "description": "event shop activity",
                "default_start_at_ms": 1677628800000,
                "default_end_at_ms": 1681516800000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 70 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let time = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        br#"{"enabled":true,"iso":"2023-03-20T12:00:00.000Z","rate":1.0}"#,
    );
    assert!(time.starts_with("HTTP/1.1 200 OK"));
    let request = SalesListRequest {
        viewer_id,
        shop_types: vec![4],
        boss_coin_shop_category_ids: Vec::new(),
        equipment_enhancement_shop_category_ids: Vec::new(),
        browse_treasure_flag: false,
        event_list: vec![EventShopRequest {
            event_type: 10,
            event_ids: vec![1],
        }],
    };
    let open = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    let sales = open.data["sales_list"]
        .as_array()
        .expect("sales list is an array");
    assert!(!sales.is_empty());
    let shop_item_id = sales[0]["shop_item_id"]
        .as_i64()
        .expect("shop item id is an integer");

    let disabled = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/calendar/carnival:1",
        "application/json",
        &authorization,
        br#"{"enabled":false,"start_at_ms":1677628800000,"end_at_ms":1681516800000}"#,
    );
    assert!(disabled.starts_with("HTTP/1.1 200 OK"));
    let closed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    assert_eq!(closed.data["sales_list"], Value::Array(Vec::new()));
    let buy = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 4,
            shop_item_id,
            number: 1,
        }),
    );
    assert!(buy.ends_with("{\"error\":\"activity_disabled\"}"));

    let temporary_open = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/carnival%3A1/temporary-open",
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(temporary_open.starts_with("HTTP/1.1 200 OK"));
    let temporarily_visible = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    assert!(!temporarily_visible.data["sales_list"]
        .as_array()
        .expect("sales list is an array")
        .is_empty());

    let temporary_close = request_with_headers(
        service.port(),
        "DELETE",
        "/v1/activities/carnival%3A1/temporary-open",
        "application/json",
        &authorization,
        b"",
    );
    assert!(temporary_close.starts_with("HTTP/1.1 200 OK"));
    let restored = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    assert_eq!(restored.data["sales_list"], Value::Array(Vec::new()));
    service.stop().expect("service stops cleanly");
}
// //// /验证永久规则和临时租约共同控制活动商店 ////

// //// 验证管理窗口跨越商品历史日期并同步来源与购买 [@x380kkm 2026-08-29] ////
#[test]
fn temporary_open_overrides_event_item_period_for_all_shop_entries() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2023-02-10T00:00:00Z",
            "activities": [{
                "activity_id": "carnival:1",
                "name": "Carnival 1",
                "kind": "carnival",
                "tags": ["test"],
                "description": "historical event shop",
                "default_start_at_ms": 1677628800000,
                "default_end_at_ms": 1681516800000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 72 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["90030"] = Value::from(150);
    });
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    set_virtual_time(&service, &authorization, "2023-04-01T12:00:00.000Z");
    let request = SalesListRequest {
        viewer_id,
        shop_types: vec![4],
        boss_coin_shop_category_ids: Vec::new(),
        equipment_enhancement_shop_category_ids: Vec::new(),
        browse_treasure_flag: false,
        event_list: vec![EventShopRequest {
            event_type: 10,
            event_ids: vec![1],
        }],
    };
    let closed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    assert!(closed.data["sales_list"]
        .as_array()
        .expect("sales list is an array")
        .is_empty());

    let temporary_open = request_with_headers(
        service.port(),
        "POST",
        "/v1/activities/carnival%3A1/temporary-open",
        "application/json",
        &authorization,
        br#"{}"#,
    );
    assert!(
        temporary_open.starts_with("HTTP/1.1 200 OK"),
        "{temporary_open}"
    );
    let opened = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    assert!(opened.data["sales_list"]
        .as_array()
        .expect("sales list is an array")
        .iter()
        .any(|sale| sale["shop_item_id"] == 1));

    let sources = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/how_to_get/get_list",
        &encode_request(&HowToGetRequest {
            viewer_id,
            item_id: None,
            equipment_id: Some(5_030_034),
        }),
    ));
    assert!(sources.data["shop_sales_list"]
        .as_array()
        .expect("shop sources are an array")
        .iter()
        .any(|sale| sale["shop_type"] == 4 && sale["shop_item_id"] == 1));

    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 4,
            shop_item_id: 1,
            number: 1,
        }),
    ));
    assert_eq!(bought.data_headers.result_code, 1);
    assert_eq!(bought.data["equipment_list"][0]["equipment_id"], 5_030_034);
    service.stop().expect("service stops cleanly");
}
// //// /验证管理窗口跨越商品历史日期并同步来源与购买 ////

// //// 验证每周活动规则按同一窗口重新开放商店 [@x380kkm 2026-08-25] ////
#[test]
fn reopens_event_shop_on_weekly_activity_window() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2023-03-20T00:00:00Z",
            "activities": [{
                "activity_id": "carnival:1",
                "name": "Carnival 1",
                "kind": "carnival",
                "tags": ["test"],
                "description": "weekly event shop activity",
                "default_start_at_ms": 1679313600000,
                "default_end_at_ms": 1679400000000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 71 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["90030"] = Value::from(300);
    });
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let weekly = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/carnival%3A1/period",
        "application/json",
        &authorization,
        br#"{"period":"weekly","interval_days":1}"#,
    );
    assert!(weekly.starts_with("HTTP/1.1 200 OK"), "{weekly}");
    let request = SalesListRequest {
        viewer_id,
        shop_types: vec![4],
        boss_coin_shop_category_ids: Vec::new(),
        equipment_enhancement_shop_category_ids: Vec::new(),
        browse_treasure_flag: false,
        event_list: vec![EventShopRequest {
            event_type: 10,
            event_ids: vec![1],
        }],
    };

    set_virtual_time(&service, &authorization, "2023-03-20T18:00:00.000Z");
    assert_eq!(
        event_shop_sale(service.port(), &request, 1)["stock_quantity"],
        2
    );
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 4,
            shop_item_id: 1,
            number: 1,
        }),
    ));
    assert_eq!(bought.data_headers.result_code, 1);
    assert_eq!(
        event_shop_sale(service.port(), &request, 1)["stock_quantity"],
        1
    );
    set_virtual_time(&service, &authorization, "2023-03-22T18:00:00.000Z");
    assert!(event_shop_sales(service.port(), &request).is_empty());
    set_virtual_time(&service, &authorization, "2023-03-27T18:00:00.000Z");
    let next_week = event_shop_sale(service.port(), &request, 1);
    assert_eq!(next_week["stock_quantity"], 2);
    assert_eq!(next_week["total_purchase_num"], 0);
    set_virtual_time(&service, &authorization, "2023-04-03T18:00:00.000Z");
    assert_eq!(
        event_shop_sale(service.port(), &request, 1)["stock_quantity"],
        2
    );
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["shop_purchase_counts"]["4:1"], 1);
    assert!(stored["shop_purchase_windows"]["4:1"]["activity_key"]
        .as_str()
        .is_some_and(|key| key.starts_with("schedule:carnival:1:")));
    service.stop().expect("service stops cleanly");
}
// //// /验证每周活动规则按同一窗口重新开放商店 ////

// //// 验证静态活动商店跨虚拟日期沿用购买窗口 [@x380kkm 2026-08-30] ////
#[test]
fn keeps_static_event_shop_purchase_window_across_virtual_dates() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2023-03-20T00:00:00Z",
            "activities": [{
                "activity_id": "carnival:1",
                "name": "Carnival 1",
                "kind": "carnival",
                "tags": ["test"],
                "description": "static event shop activity",
                "default_start_at_ms": 1679313600000,
                "default_end_at_ms": 1679400000000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 74 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["90030"] = Value::from(300);
    });
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let request = SalesListRequest {
        viewer_id,
        shop_types: vec![4],
        boss_coin_shop_category_ids: Vec::new(),
        equipment_enhancement_shop_category_ids: Vec::new(),
        browse_treasure_flag: false,
        event_list: vec![EventShopRequest {
            event_type: 10,
            event_ids: vec![1],
        }],
    };

    set_virtual_time(&service, &authorization, "2023-03-25T18:00:00.000Z");
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 4,
            shop_item_id: 1,
            number: 1,
        }),
    ));
    assert_eq!(bought.data_headers.result_code, 1);
    let outside_default_window = event_shop_sale(service.port(), &request, 1);
    assert_eq!(outside_default_window["stock_quantity"], 1);
    assert_eq!(outside_default_window["total_purchase_num"], 1);

    set_virtual_time(&service, &authorization, "2023-03-20T18:00:00.000Z");
    let inside_default_window = event_shop_sale(service.port(), &request, 1);
    assert_eq!(inside_default_window["stock_quantity"], 1);
    assert_eq!(inside_default_window["total_purchase_num"], 1);
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["shop_purchase_counts"]["4:1"], 1);
    assert_eq!(
        stored["shop_purchase_windows"]["4:1"]["activity_key"],
        "static:carnival:1:persistent"
    );
    assert_eq!(
        stored["shop_purchase_windows"]["4:1"]["activity_baseline"],
        0
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证静态活动商店跨虚拟日期沿用购买窗口 ////

// //// 验证长期活动商店跨自然周保持同一购买窗口 [@x380kkm 2026-08-29] ////
#[test]
fn keeps_persistent_event_shop_purchase_window_across_weeks() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2023-03-20T00:00:00Z",
            "activities": [{
                "activity_id": "carnival:1",
                "name": "Carnival 1",
                "kind": "carnival",
                "tags": ["test"],
                "description": "persistent event shop activity",
                "default_start_at_ms": 1679313600000,
                "default_end_at_ms": 1679400000000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 73 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["90030"] = Value::from(300);
    });
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let always = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/carnival%3A1/mode",
        "application/json",
        &authorization,
        br#"{"mode":"always"}"#,
    );
    assert!(always.starts_with("HTTP/1.1 200 OK"), "{always}");
    let request = SalesListRequest {
        viewer_id,
        shop_types: vec![4],
        boss_coin_shop_category_ids: Vec::new(),
        equipment_enhancement_shop_category_ids: Vec::new(),
        browse_treasure_flag: false,
        event_list: vec![EventShopRequest {
            event_type: 10,
            event_ids: vec![1],
        }],
    };

    set_virtual_time(&service, &authorization, "2023-03-20T12:00:00.000Z");
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 4,
            shop_item_id: 1,
            number: 1,
        }),
    ));
    assert_eq!(bought.data_headers.result_code, 1);

    set_virtual_time(&service, &authorization, "2023-03-27T12:00:00.000Z");
    let next_week = event_shop_sale(service.port(), &request, 1);
    assert_eq!(next_week["stock_quantity"], 1);
    assert_eq!(next_week["total_purchase_num"], 1);
    let stored = read_player_snapshot(root.path());
    assert_eq!(
        stored["shop_purchase_windows"]["4:1"]["activity_key"],
        "schedule:carnival:1:persistent"
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证长期活动商店跨自然周保持同一购买窗口 ////

fn event_shop_sales(port: u16, request: &SalesListRequest) -> Vec<Value> {
    decode_response::<Value>(&cn_support::send_request(
        port,
        "/api/index.php/shop/get_sales_list",
        &encode_request(request),
    ))
    .data["sales_list"]
        .as_array()
        .expect("sales list is an array")
        .clone()
}

fn event_shop_sale(port: u16, request: &SalesListRequest, shop_item_id: i64) -> Value {
    event_shop_sales(port, request)
        .into_iter()
        .find(|sale| sale["shop_item_id"] == shop_item_id)
        .expect("event shop item is listed")
}

fn set_virtual_time(service: &PersonalService, authorization: &[(&str, &str)], iso: &str) {
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        authorization,
        format!(r#"{{"enabled":true,"iso":"{iso}","rate":1.0}}"#).as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}
