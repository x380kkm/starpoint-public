// audience: internal
// # personal-service-cn-shop-tests
//
// 该文件验证 CN 商店目录与体力恢复的扣费, 上限, 失败原子性和载入持久化.
// 每个测试使用独立临时数据库, 不修改共享账号或进程环境.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::{fs, path::Path};
use support::request_with_headers;
use tempfile::TempDir;

const STAMINA_TEST_TIME: &str = "2030-01-01T12:00:00.000Z";
const STAMINA_TEST_TIME_SECONDS: i64 = 1_893_499_200;

#[derive(Serialize)]
struct RecoverStaminaRequest {
    api_count: i64,
    viewer_id: i64,
}

#[derive(Serialize)]
struct SalesListRequest {
    api_count: i64,
    viewer_id: i64,
    shop_types: Vec<i64>,
    boss_coin_shop_category_ids: Vec<i64>,
    equipment_enhancement_shop_category_ids: Vec<i64>,
    browse_treasure_flag: bool,
    event_list: Vec<Value>,
}

#[derive(Serialize)]
struct BuyRequest {
    viewer_id: i64,
    shop_type: i64,
    shop_item_id: i64,
    number: i64,
}

// //// 准备并读取体力恢复测试账号 [@x380kkm 2026-08-04] ////
fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
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

fn set_resources(root: &Path, stamina: i64, stamina_heal_time: i64, free_vmoney: i64, vmoney: i64) {
    update_player_snapshot(root, |player_data| {
        player_data["user_info"]["stamina"] = Value::from(stamina);
        player_data["user_info"]["stamina_heal_time"] = Value::from(stamina_heal_time);
        player_data["user_info"]["free_vmoney"] = Value::from(free_vmoney);
        player_data["user_info"]["vmoney"] = Value::from(vmoney);
    });
}

fn recover_stamina(port: u16, viewer_id: i64) -> String {
    cn_support::send_request(
        port,
        "/api/index.php/shop/recover_stamina",
        &encode_request(&RecoverStaminaRequest {
            api_count: 1,
            viewer_id,
        }),
    )
}

fn load(port: u16, viewer_id: i64) -> cn_support::Envelope<Value> {
    decode_response(&cn_support::send_request(
        port,
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ))
}

fn set_virtual_time(service: &PersonalService, iso: &str) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let body = format!(r#"{{"enabled":true,"iso":"{iso}","rate":1.0}}"#);
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        body.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
}
// //// /准备并读取体力恢复测试账号 ////

// //// 验证 CN 商店目录返回可浏览的合法响应 [@x380kkm 2026-08-22] ////
#[test]
fn returns_available_sales_for_valid_viewer() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 85);
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 1,
            viewer_id,
            shop_types: vec![1, 2, 3],
            boss_coin_shop_category_ids: Vec::new(),
            equipment_enhancement_shop_category_ids: Vec::new(),
            browse_treasure_flag: true,
            event_list: Vec::new(),
        }),
    ));
    let sales = response.data["sales_list"]
        .as_array()
        .expect("sales list is an array");
    assert!(!sales.is_empty());
    assert!(sales
        .iter()
        .all(|sale| sale["shop_type"] == 2 && sale["shop_item_id"].is_i64()));
    assert!(sales.iter().all(|sale| {
        sale["group_info"]["group_total_stock_quantity"].is_i64()
            && sale["group_info"]["group_total_purchase_num"].is_i64()
            && sale["group_info"]["multi_stage"] == false
            && sale["group_info"]["other_group_items"].is_null()
            && sale["discount_id"].is_null()
            && sale["discount_rate"].is_null()
            && sale["discounted_price"].is_null()
    }));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 商店目录返回可浏览的合法响应 ////

// //// 验证装备强化目录选择当前阶段并购买同一商品 [@x380kkm 2026-08-24] ////
#[test]
fn selects_current_equipment_enhancement_stage_and_buys_same_item() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 86);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["user_equipment_list"]["5020035"] = json!({
            "equipment_id": 5_020_035,
            "level": 1,
            "enhancement_level": 70,
            "protection": false,
            "stack": 1,
        });
        player_data["item_list"]["14033"] = Value::from(100);
    });

    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 1,
            viewer_id,
            shop_types: vec![10],
            boss_coin_shop_category_ids: Vec::new(),
            equipment_enhancement_shop_category_ids: vec![1],
            browse_treasure_flag: false,
            event_list: Vec::new(),
        }),
    ));
    let sales = response.data["sales_list"]
        .as_array()
        .expect("sales list is an array");
    let group_sales = sales
        .iter()
        .filter(|sale| {
            sale["shop_item_id"]
                .as_i64()
                .is_some_and(|shop_item_id| (1..=5).contains(&shop_item_id))
        })
        .collect::<Vec<_>>();
    assert_eq!(group_sales.len(), 1);
    let sale = group_sales[0];
    assert_eq!(sale["shop_item_id"], 4);
    assert_eq!(sale["stock_quantity"], 28);
    assert_eq!(sale["total_purchase_num"], 70);
    assert_eq!(sale["group_info"]["group_total_stock_quantity"], 29);
    assert_eq!(sale["group_info"]["multi_stage"], true);
    let other_group_items = sale["group_info"]["other_group_items"]
        .as_array()
        .expect("other equipment enhancement stages are an array");
    let stage_states = other_group_items
        .iter()
        .map(|item| {
            (
                item["shop_item_id"].as_i64(),
                item["stock_quantity"].as_i64(),
                item["total_purchase_num"].as_i64(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stage_states,
        vec![
            (Some(1), Some(0), Some(1)),
            (Some(2), Some(0), Some(68)),
            (Some(3), Some(0), Some(1)),
            (Some(5), Some(1), Some(0)),
        ]
    );
    assert!(other_group_items.iter().all(|item| {
        item["today_purchase_num"] == 0
            && item["this_month_purchase_num"].is_null()
            && item["discount_id"].is_null()
            && item["discount_rate"].is_null()
            && item["discounted_price"].is_null()
    }));

    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 10,
            shop_item_id: sale["shop_item_id"]
                .as_i64()
                .expect("shop item id is an integer"),
            number: 1,
        }),
    ));
    assert_eq!(bought.data["equipment_list"][0]["equipment_id"], 5_020_035);
    assert_eq!(bought.data["equipment_list"][0]["enhancement_level"], 98);
    service.stop().expect("service stops cleanly");
}
// //// /验证装备强化目录选择当前阶段并购买同一商品 ////

// //// 验证复刻活动目录商品沿用同一购买标识 [@x380kkm 2026-08-24] ////
#[test]
fn returns_and_buys_rush_rerun_shop_items() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2023-11-01T00:00:00Z",
            "activities": [{
                "activity_id": "rush:700011",
                "name": "Rush 700011",
                "kind": "rush",
                "tags": ["test"],
                "description": "rush rerun shop",
                "default_start_at_ms": 1698796800000,
                "default_end_at_ms": 1704067200000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let viewer_id = signup(&service, 87);
    set_virtual_time(&service, "2023-12-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["2370001"] = Value::from(1_000);
    });

    let event_list = vec![json!({
        "event_type": 11,
        "event_ids": [700_011],
    })];
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 1,
            viewer_id,
            shop_types: vec![4],
            boss_coin_shop_category_ids: Vec::new(),
            equipment_enhancement_shop_category_ids: Vec::new(),
            browse_treasure_flag: false,
            event_list,
        }),
    ));
    let shop_item_id = response.data["sales_list"]
        .as_array()
        .expect("sales list is an array")
        .iter()
        .find_map(|sale| (sale["shop_item_id"] == 700_000).then_some(700_000))
        .expect("rerun shop item is returned");

    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 4,
            shop_item_id,
            number: 1,
        }),
    ));
    assert_eq!(bought.data["item_list"]["49100"], 1);
    service.stop().expect("service stops cleanly");
}
// //// /验证复刻活动目录商品沿用同一购买标识 ////

// //// 验证复刻活动沿用原活动的开放窗口 ////
#[test]
fn lists_rush_rerun_shop_when_original_activity_window_is_open() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2023-11-01T00:00:00Z",
            "activities": [{
                "activity_id": "rush:700001",
                "name": "Rush 700001",
                "kind": "rush",
                "tags": ["test"],
                "description": "rush original shop",
                "default_start_at_ms": 1698796800000,
                "default_end_at_ms": 1704067200000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let viewer_id = signup(&service, 103);
    set_virtual_time(&service, "2023-12-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["2370001"] = Value::from(1_000);
    });

    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 1,
            viewer_id,
            shop_types: vec![4],
            boss_coin_shop_category_ids: Vec::new(),
            equipment_enhancement_shop_category_ids: Vec::new(),
            browse_treasure_flag: false,
            event_list: vec![json!({
                "event_type": 11,
                "event_ids": [700_011],
            })],
        }),
    ));
    assert!(response.data["sales_list"]
        .as_array()
        .expect("sales list is an array")
        .iter()
        .any(|sale| sale["shop_item_id"] == 700_000));
    service.stop().expect("service stops cleanly");
}
// //// /验证复刻活动沿用原活动的开放窗口 ////

// //// 验证商店目录和购买只使用客户端 master 商品 [@x380kkm 2026-08-23] ////
#[test]
fn filters_sales_and_purchases_by_client_shop_master() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 88);

    set_virtual_time(&service, "2019-12-02T12:00:00.000Z");
    let star_grain = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 1,
            viewer_id,
            shop_types: vec![9],
            boss_coin_shop_category_ids: Vec::new(),
            equipment_enhancement_shop_category_ids: Vec::new(),
            browse_treasure_flag: false,
            event_list: Vec::new(),
        }),
    ));
    let star_grain_ids = star_grain.data["sales_list"]
        .as_array()
        .expect("star grain sales list is an array")
        .iter()
        .filter_map(|sale| sale["shop_item_id"].as_i64())
        .collect::<Vec<_>>();
    assert!(star_grain_ids.contains(&100_000));
    assert!([
        100_003, 100_023, 100_024, 100_025, 100_026, 100_027, 100_028, 100_029, 100_030, 100_031,
        100_032, 100_033, 100_034, 100_035,
    ]
    .iter()
    .all(|shop_item_id| !star_grain_ids.contains(shop_item_id)));

    set_virtual_time(&service, "2022-01-15T12:00:00.000Z");
    let boss_coin = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 2,
            viewer_id,
            shop_types: Vec::new(),
            boss_coin_shop_category_ids: vec![9],
            equipment_enhancement_shop_category_ids: Vec::new(),
            browse_treasure_flag: false,
            event_list: Vec::new(),
        }),
    ));
    let boss_coin_ids = boss_coin.data["sales_list"]
        .as_array()
        .expect("boss coin sales list is an array")
        .iter()
        .filter_map(|sale| sale["shop_item_id"].as_i64())
        .collect::<Vec<_>>();
    assert!(boss_coin_ids.contains(&300_085));
    assert!(!boss_coin_ids.contains(&300_084));

    let rejected = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 7,
            shop_item_id: 300_084,
            number: 1,
        }),
    );
    assert!(rejected.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(rejected.ends_with("{\"error\":\"shop_item_not_found\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证商店目录和购买只使用客户端 master 商品 ////

// //// 验证商店奖励写入库存和领取记录 [@x380kkm 2026-08-22] ////
#[test]
fn persists_shop_rewards_and_receive_history() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 87);
    set_virtual_time(&service, "2020-01-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["user_info"]["free_mana"] = Value::from(1_000);
        player_data["all_active_mission_list"] = json!({
            "12010": {"progress": 145, "stages": {"1": false}}
        });
    });
    let initial = load(service.port(), viewer_id);
    let initial_free_mana = initial.data["user_info"]["free_mana"]
        .as_i64()
        .expect("initial free mana is an integer");
    let initial_exp_pool = initial.data["user_info"]["exp_pool"]
        .as_i64()
        .expect("initial experience pool is an integer");
    let initial_item_count = initial.data["item_list"]["1"].as_i64().unwrap_or_default();
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 2,
            shop_item_id: 200_001,
            number: 1,
        }),
    ));
    assert_eq!(bought.data_headers.result_code, 1);
    assert_eq!(bought.data["item_list"]["1"], initial_item_count + 1);
    assert_eq!(
        bought.data["user_info"]["free_mana"],
        initial_free_mana - 300
    );
    assert_eq!(bought.data["user_info"]["exp_pool"], initial_exp_pool);
    assert_eq!(
        bought.data["active_mission_list"],
        json!([{"mission_id": 12010, "progress_value": 146, "stages": []}])
    );
    let history = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/history/receive",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(history.data["total_count"], 1);
    assert_eq!(history.data["history"][0]["type"], 1);
    assert_eq!(history.data["history"][0]["type_id"], 1);

    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = load(restarted.port(), viewer_id);
    assert_eq!(
        loaded.data["user_info"]["free_mana"],
        initial_free_mana - 300
    );
    assert_eq!(loaded.data["user_info"]["exp_pool"], initial_exp_pool);
    assert_eq!(loaded.data["item_list"]["1"], initial_item_count + 1);
    assert_eq!(
        loaded.data["all_active_mission_list"]["12010"]["progress"],
        146
    );
    let restarted_history = decode_response::<Value>(&cn_support::send_request(
        restarted.port(),
        "/api/index.php/history/receive",
        &encode_request(&serde_json::json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(restarted_history.data["total_count"], 1);
    restarted.stop().expect("service stops cleanly");
}
// //// /验证商店奖励写入库存和领取记录 ////

// //// 验证星之粒商店直接发放道具并持久化 [@x380kkm 2026-08-24] ////
#[test]
fn persists_star_grain_shop_item_reward() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 97);
    set_virtual_time(&service, "2022-04-20T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["990008"] = Value::from(200);
        player_data["item_list"]["101"] = Value::from(2);
        for item_id in ["1", "2", "3", "4", "99", "10001"] {
            player_data["item_list"][item_id] = Value::from(0);
        }
    });

    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 9,
            shop_item_id: 100_005,
            number: 1,
        }),
    ));
    assert_eq!(bought.data_headers.result_code, 1);
    assert_eq!(bought.data["item_list"]["990008"], 195);
    assert_eq!(bought.data["item_list"]["101"], 3);

    let character = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 9,
            shop_item_id: 100_000,
            number: 1,
        }),
    ));
    assert_eq!(character.data["item_list"]["990008"], 155);
    assert_eq!(character.data["joined_character_id_list"], json!([243_013]));
    assert_eq!(character.data["character_list"][0]["character_id"], 243_013);

    let equipment = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 9,
            shop_item_id: 100_011,
            number: 1,
        }),
    ));
    assert_eq!(equipment.data["item_list"]["990008"], 125);
    assert_eq!(
        equipment.data["equipment_list"][0]["equipment_id"],
        5_010_020
    );

    let material_box = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            viewer_id,
            shop_type: 9,
            shop_item_id: 100_017,
            number: 1,
        }),
    ));
    assert_eq!(material_box.data["item_list"]["990008"], 95);
    for (item_id, amount) in [
        ("10001", 1),
        ("1", 175),
        ("2", 140),
        ("3", 75),
        ("4", 25),
        ("99", 25),
    ] {
        assert_eq!(material_box.data["item_list"][item_id], amount);
    }

    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = load(restarted.port(), viewer_id);
    assert_eq!(loaded.data["item_list"]["990008"], 95);
    assert_eq!(loaded.data["item_list"]["101"], 3);
    for (item_id, amount) in [
        ("10001", 1),
        ("1", 175),
        ("2", 140),
        ("3", 75),
        ("4", 25),
        ("99", 25),
    ] {
        assert_eq!(loaded.data["item_list"][item_id], amount);
    }
    assert!(loaded.data["user_character_list"].get("243013").is_some());
    assert!(loaded.data["user_equipment_list"].get("5010020").is_some());
    restarted.stop().expect("service stops cleanly");
}
// //// /验证星之粒商店直接发放道具并持久化 ////

// //// 验证批量购买按客户端请求格式发放全部奖励 [@x380kkm 2026-08-24] ////
#[test]
fn persists_bulk_shop_rewards() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 98);
    set_virtual_time(&service, "2022-04-20T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["990008"] = Value::from(100);
        player_data["all_active_mission_list"] = json!({
            "12010": {"progress": 145, "stages": {"1": false}}
        });
    });

    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&json!({
            "viewer_id": viewer_id,
            "shop_type": 9,
            "buy_item_list": {
                "100005": 1,
                "100006": 1,
            },
        })),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let bought = decode_response::<Value>(&response);
    assert_eq!(bought.data_headers.result_code, 1);
    assert_eq!(bought.data["item_list"]["990008"], 85);
    assert_eq!(bought.data["item_list"]["101"], 1);
    assert_eq!(bought.data["item_list"]["102"], 1);
    assert_eq!(
        bought.data["active_mission_list"],
        json!([{"mission_id": 12010, "progress_value": 147, "stages": []}])
    );

    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let loaded = load(restarted.port(), viewer_id);
    assert_eq!(loaded.data["item_list"]["990008"], 85);
    assert_eq!(loaded.data["item_list"]["101"], 1);
    assert_eq!(loaded.data["item_list"]["102"], 1);
    assert_eq!(
        loaded.data["all_active_mission_list"]["12010"]["progress"],
        147
    );
    restarted.stop().expect("service stops cleanly");
}
// //// /验证批量购买按客户端请求格式发放全部奖励 ////

// //// 验证库存刷新保留历史购买计数并建立新基线 [@x380kkm 2026-08-24] ////
#[test]
fn refreshes_shop_stock_from_purchase_baseline() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 99);
    set_virtual_time(&service, "2022-04-20T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["990008"] = Value::from(500);
    });
    let request = BuyRequest {
        viewer_id,
        shop_type: 9,
        shop_item_id: 100_012,
        number: 1,
    };

    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(first.data["item_list"]["10003"], 1);
    let exhausted = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    );
    assert!(exhausted.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(exhausted.ends_with("{\"error\":\"shop_stock_exceeded\"}"));

    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let refresh = request_with_headers(
        service.port(),
        "POST",
        "/v1/shop-stock/refresh",
        "application/json",
        &authorization,
        serde_json::to_string(&json!({
            "viewer_id": viewer_id,
            "shop_type": 9,
            "shop_item_id": 100_012,
        }))
        .expect("refresh request is encoded")
        .as_bytes(),
    );
    assert!(refresh.starts_with("HTTP/1.1 200 OK"), "{refresh}");

    let second = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(second.data["item_list"]["10003"], 2);
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["shop_purchase_counts"]["9:100012"], 2);
    assert_eq!(stored["shop_purchase_count_baselines"]["9:100012"], 1);
    service.stop().expect("service stops cleanly");
}
// //// /验证库存刷新保留历史购买计数并建立新基线 ////

// //// 验证单次购买遵守客户端商品数量上限 [@x380kkm 2026-08-29] ////
#[test]
fn rejects_a_purchase_above_the_client_buy_max_count() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 102);
    set_virtual_time(&service, "2020-01-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["user_info"]["free_mana"] = Value::from(20);
    });
    let request = BuyRequest {
        viewer_id,
        shop_type: 2,
        shop_item_id: 200_001,
        number: 2,
    };

    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    );
    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.ends_with("{\"error\":\"shop_stock_exceeded\"}"));
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["user_info"]["free_mana"], 20);
    assert!(stored["shop_purchase_counts"].get("2:200001").is_none());
    service.stop().expect("service stops cleanly");
}
// //// /验证单次购买遵守客户端商品数量上限 ////

// //// 验证每日库存按客户端刷新时刻重置并保留累计购买数 ////
#[test]
fn refreshes_daily_shop_stock_at_the_client_reset_time() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 100);
    set_virtual_time(&service, "2020-01-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["user_info"]["free_mana"] = Value::from(1_000);
    });
    let request = SalesListRequest {
        api_count: 1,
        viewer_id,
        shop_types: vec![2],
        boss_coin_shop_category_ids: Vec::new(),
        equipment_enhancement_shop_category_ids: Vec::new(),
        browse_treasure_flag: false,
        event_list: Vec::new(),
    };
    let buy_request = BuyRequest {
        viewer_id,
        shop_type: 2,
        shop_item_id: 200_001,
        number: 1,
    };
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&buy_request),
    ));
    assert_eq!(first.data_headers.result_code, 1);

    let same_day = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    let same_day_sale = same_day.data["sales_list"]
        .as_array()
        .and_then(|sales| sales.iter().find(|sale| sale["shop_item_id"] == 200_001))
        .expect("daily shop item is listed");
    assert_eq!(same_day_sale["stock_quantity"], 9);
    assert_eq!(same_day_sale["today_purchase_num"], 1);
    assert_eq!(same_day_sale["total_purchase_num"], 1);

    set_virtual_time(&service, "2020-01-01T20:30:00.000Z");
    let before_reset = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    let before_reset_sale = before_reset.data["sales_list"]
        .as_array()
        .and_then(|sales| sales.iter().find(|sale| sale["shop_item_id"] == 200_001))
        .expect("daily shop item remains listed before reset");
    assert_eq!(before_reset_sale["stock_quantity"], 9);
    assert_eq!(before_reset_sale["today_purchase_num"], 1);

    set_virtual_time(&service, "2020-01-01T21:01:00.000Z");
    let next_day = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    let next_day_sale = next_day.data["sales_list"]
        .as_array()
        .and_then(|sales| sales.iter().find(|sale| sale["shop_item_id"] == 200_001))
        .expect("daily shop item remains listed");
    assert_eq!(next_day_sale["stock_quantity"], 10);
    assert_eq!(next_day_sale["today_purchase_num"], 0);
    assert_eq!(next_day_sale["total_purchase_num"], 1);

    let second = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&buy_request),
    ));
    assert_eq!(second.data_headers.result_code, 1);
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["shop_purchase_counts"]["2:200001"], 2);
    service.stop().expect("service stops cleanly");
}
// //// /验证每日库存按客户端刷新时刻重置并保留累计购买数 ////

// //// 验证每月库存按客户端月初刷新时刻重置并保留累计购买数 ////
#[test]
fn refreshes_monthly_shop_stock_at_the_client_month_boundary() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 101);
    set_virtual_time(&service, "2022-04-30T19:59:00.000Z");
    update_player_snapshot(root.path(), |player_data| {
        player_data["item_list"]["990008"] = Value::from(500);
    });
    let request = SalesListRequest {
        api_count: 1,
        viewer_id,
        shop_types: vec![9],
        boss_coin_shop_category_ids: Vec::new(),
        equipment_enhancement_shop_category_ids: Vec::new(),
        browse_treasure_flag: false,
        event_list: Vec::new(),
    };
    let buy_request = BuyRequest {
        viewer_id,
        shop_type: 9,
        shop_item_id: 100_012,
        number: 1,
    };
    let first = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&buy_request),
    ));
    assert_eq!(first.data_headers.result_code, 1);
    set_virtual_time(&service, "2022-04-30T20:30:00.000Z");
    let before_reset = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    let before_reset_sale = before_reset.data["sales_list"]
        .as_array()
        .and_then(|sales| sales.iter().find(|sale| sale["shop_item_id"] == 100_012))
        .expect("monthly shop item remains listed before reset");
    assert_eq!(before_reset_sale["stock_quantity"], 0);
    assert_eq!(before_reset_sale["this_month_purchase_num"], 1);

    set_virtual_time(&service, "2022-04-30T21:01:00.000Z");
    let next_month = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    let sale = next_month.data["sales_list"]
        .as_array()
        .and_then(|sales| sales.iter().find(|sale| sale["shop_item_id"] == 100_012))
        .expect("monthly shop item is listed");
    assert_eq!(sale["stock_quantity"], 1);
    assert_eq!(sale["this_month_purchase_num"], 0);
    assert_eq!(sale["total_purchase_num"], 1);
    let second = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&buy_request),
    ));
    assert_eq!(second.data_headers.result_code, 1);
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["shop_purchase_counts"]["9:100012"], 2);
    service.stop().expect("service stops cleanly");
}
// //// /验证每月库存按客户端月初刷新时刻重置并保留累计购买数 ////

// //// 验证 CN 商店按请求类型和首领币分类返回目录 [@x380kkm 2026-08-22] ////
#[test]
fn selects_only_requested_boss_coin_categories() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 86);
    let empty = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 1,
            viewer_id,
            shop_types: Vec::new(),
            boss_coin_shop_category_ids: Vec::new(),
            equipment_enhancement_shop_category_ids: Vec::new(),
            browse_treasure_flag: false,
            event_list: Vec::new(),
        }),
    ));
    assert!(empty.data["sales_list"]
        .as_array()
        .is_some_and(Vec::is_empty));

    let boss_coin = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&SalesListRequest {
            api_count: 2,
            viewer_id,
            shop_types: Vec::new(),
            boss_coin_shop_category_ids: vec![1],
            equipment_enhancement_shop_category_ids: Vec::new(),
            browse_treasure_flag: false,
            event_list: Vec::new(),
        }),
    ));
    let sales = boss_coin.data["sales_list"]
        .as_array()
        .expect("boss coin sales list is an array");
    assert!(!sales.is_empty());
    assert!(sales.iter().all(|sale| sale["shop_type"] == 7));
    assert!(sales.iter().any(|sale| sale["shop_item_id"] == 200_103));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 商店按请求类型和首领币分类返回目录 ////

// //// 验证免费余额优先扣除并持久化体力 [@x380kkm 2026-08-04] ////
#[test]
fn does_not_use_paid_vmoney_for_stamina_recovery() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, STAMINA_TEST_TIME);
    let viewer_id = signup(&service, 81);
    service.stop().expect("service stops cleanly");
    set_resources(root.path(), 20, STAMINA_TEST_TIME_SECONDS, 30, 40);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let recovered = decode_response::<Value>(&recover_stamina(service.port(), viewer_id));
    assert_eq!(recovered.data_headers.result_code, 0);
    assert!(recovered
        .data
        .as_object()
        .is_some_and(serde_json::Map::is_empty));

    let loaded = load(service.port(), viewer_id);
    assert_eq!(loaded.data["user_info"]["free_vmoney"], 30);
    assert_eq!(loaded.data["user_info"]["vmoney"], 40);
    service.stop().expect("service stops cleanly");
}
// //// /验证免费余额优先扣除并持久化体力 ////

// //// 验证免费余额足额时不扣除付费余额 [@x380kkm 2026-08-04] ////
#[test]
fn spends_only_free_vmoney_when_it_covers_the_cost() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, STAMINA_TEST_TIME);
    let viewer_id = signup(&service, 84);
    service.stop().expect("service stops cleanly");
    set_resources(root.path(), 20, STAMINA_TEST_TIME_SECONDS, 80, 40);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let recovered = decode_response::<Value>(&recover_stamina(service.port(), viewer_id));
    assert_eq!(recovered.data["user_info"]["stamina"], 120);
    assert_eq!(recovered.data["user_info"]["free_vmoney"], 30);
    assert!(recovered.data["user_info"]["vmoney"].is_null());
    let loaded = load(service.port(), viewer_id);
    assert_eq!(loaded.data["user_info"]["free_vmoney"], 30);
    assert_eq!(loaded.data["user_info"]["vmoney"], 40);
    service.stop().expect("service stops cleanly");
}
// //// /验证免费余额足额时不扣除付费余额 ////

// //// 验证余额不足时不修改玩家快照 [@x380kkm 2026-08-04] ////
#[test]
fn rejects_insufficient_vmoney_without_mutation() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, STAMINA_TEST_TIME);
    let viewer_id = signup(&service, 82);
    service.stop().expect("service stops cleanly");
    set_resources(root.path(), 20, STAMINA_TEST_TIME_SECONDS, 20, 20);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let response = decode_response::<Value>(&recover_stamina(service.port(), viewer_id));
    assert_eq!(response.data_headers.result_code, 0);
    assert!(response
        .data
        .as_object()
        .is_some_and(serde_json::Map::is_empty));
    let loaded = load(service.port(), viewer_id);
    assert_eq!(loaded.data["user_info"]["stamina"], 20);
    assert_eq!(
        loaded.data["user_info"]["stamina_heal_time"],
        STAMINA_TEST_TIME_SECONDS
    );
    assert_eq!(loaded.data["user_info"]["free_vmoney"], 20);
    assert_eq!(loaded.data["user_info"]["vmoney"], 20);
    service.stop().expect("service stops cleanly");
}
// //// /验证余额不足时不修改玩家快照 ////

// //// 验证体力上限返回客户端结果码且不扣费 [@x380kkm 2026-08-04] ////
#[test]
fn returns_stamina_limit_result_without_mutation() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    set_virtual_time(&service, STAMINA_TEST_TIME);
    let viewer_id = signup(&service, 83);
    service.stop().expect("service stops cleanly");
    set_resources(root.path(), 999, STAMINA_TEST_TIME_SECONDS, 30, 40);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let capped = decode_response::<Value>(&recover_stamina(service.port(), viewer_id));
    assert_eq!(capped.data_headers.result_code, 2_102);
    assert!(capped
        .data
        .as_object()
        .is_some_and(serde_json::Map::is_empty));
    let loaded = load(service.port(), viewer_id);
    assert_eq!(loaded.data["user_info"]["stamina"], 999);
    assert_eq!(
        loaded.data["user_info"]["stamina_heal_time"],
        STAMINA_TEST_TIME_SECONDS
    );
    assert_eq!(loaded.data["user_info"]["free_vmoney"], 30);
    assert_eq!(loaded.data["user_info"]["vmoney"], 40);
    service.stop().expect("service stops cleanly");
}
// //// /验证体力上限返回客户端结果码且不扣费 ////

// //// 验证无效请求和 viewer 不创建玩家状态 [@x380kkm 2026-08-04] ////
#[test]
fn rejects_invalid_requests_without_creating_player_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let malformed = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/recover_stamina",
        "not-base64-messagepack",
    );
    assert!(malformed.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(malformed.ends_with("{\"error\":\"invalid_request_body\"}"));
    let response = recover_stamina(service.port(), 100_000_001);
    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.ends_with("{\"error\":\"invalid_viewer_session\"}"));
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    let snapshot_count: i64 = database
        .query_row("SELECT COUNT(*) FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .expect("player snapshot count is read");
    assert_eq!(snapshot_count, 0);
}
// //// /验证无效请求和 viewer 不创建玩家状态 ////
