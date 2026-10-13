// audience: internal
// # personal-service-cn-shop-quantities
//
// 该模块使用 CN 活动商店商品验证经验, 玛纳和素材的批量兑换与持久化.

use super::*;

// //// 创建剧情活动商店测试账号 [@x380kkm 2026-09-20] ////
fn create_event_shop() -> (TempDir, TempDir, PersonalService, i64) {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("temporary CDN directory is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.56",
            "generated_at": "2021-12-01T00:00:00Z",
            "activities": [{
                "activity_id": "story:100003",
                "name": "Story 100003",
                "kind": "story",
                "tags": [],
                "description": "",
                "default_start_at_ms": 1639008000000,
                "default_end_at_ms": 1640908800000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    set_virtual_time(&service, "2021-12-15T12:00:00.000Z");
    let viewer_id = signup(&service, 117);
    update_player_snapshot(root.path(), |data| {
        data["item_list"]["50101"] = json!(10_000);
        data["item_list"]["13"] = json!(5);
        data["user_info"]["exp_pool"] = json!(7);
        data["user_info"]["free_mana"] = json!(11);
    });
    (root, cdn_root, service, viewer_id)
}
// //// /创建剧情活动商店测试账号 ////

// //// 验证活动经验按商品上限兑换并在重启后保留 [@x380kkm 2026-09-20] ////
#[test]
fn exchanges_experience_up_to_the_master_limit() {
    let (root, cdn_root, service, viewer_id) = create_event_shop();
    let request = BuyRequest {
        viewer_id,
        shop_type: 4,
        shop_item_id: 310_126,
        number: 1_000,
    };
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(bought.data["user_info"]["exp_pool"], 1_007);
    assert_eq!(bought.data["item_list"], json!({"50101": 9_000}));
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["shop_purchase_counts"]["4:310126"], 1_000);
    let exhausted = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&BuyRequest {
            number: 1,
            ..request
        }),
    );
    assert_eq!(
        decode_response::<Value>(&exhausted)
            .data_headers
            .result_code,
        3010
    );
    assert_eq!(read_player_snapshot(root.path()), stored);
    let history = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/history/receive",
        &encode_request(&json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(history.data["history"][0]["type"], 9);
    assert_eq!(history.data["history"][0]["number"], 1_000);

    service.stop().expect("service stops cleanly");
    let restarted = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service restarts");
    let loaded = load(restarted.port(), viewer_id);
    assert_eq!(loaded.data["user_info"]["exp_pool"], 1_007);
    assert_eq!(loaded.data["item_list"]["50101"], 9_000);
    restarted.stop().expect("service stops cleanly");
}
// //// /验证活动经验按商品上限兑换并在重启后保留 ////

// //// 验证批量兑换按商品分别扣费发放并原子拒绝超额 [@x380kkm 2026-09-20] ////
#[test]
fn bulk_exchanges_experience_mana_and_materials_at_master_limits() {
    let (root, _cdn_root, service, viewer_id) = create_event_shop();
    let request = json!({
        "viewer_id": viewer_id,
        "shop_type": 4,
        "buy_item_list": {"310112": 450, "310122": 120, "310126": 1_000}
    });
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&request),
    ));
    assert_eq!(bought.data["user_info"]["exp_pool"], 1_007);
    assert_eq!(bought.data["user_info"]["free_mana"], 120_011);
    assert_eq!(bought.data["item_list"], json!({"13": 455, "50101": 7_350}));
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["user_info"]["exp_pool"], 1_007);
    assert_eq!(stored["user_info"]["free_mana"], 120_011);
    assert_eq!(stored["item_list"]["13"], 455);
    assert_eq!(stored["shop_purchase_counts"]["4:310126"], 1_000);

    let rejected = cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&json!({
            "viewer_id": viewer_id,
            "shop_type": 4,
            "buy_item_list": {"310113": 101, "310126": 1_001}
        })),
    );
    assert_eq!(
        decode_response::<Value>(&rejected).data_headers.result_code,
        3010
    );
    assert_eq!(read_player_snapshot(root.path()), stored);
    service.stop().expect("service stops cleanly");
}
// //// /验证批量兑换按商品分别扣费发放并原子拒绝超额 ////

// //// 按免费和付费玛纳合计余额完成多件购买并记录实际消耗 [@x380kkm 2026-09-24] ////
#[test]
fn purchases_multiple_materials_with_combined_mana() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 119);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        data["user_info"]["free_mana"] = json!(50);
        data["user_info"]["paid_mana"] = json!(500);
        data["item_list"]["2"] = json!(0);
        data["all_active_mission_list"] = json!({});
    });
    let request = json!({"viewer_id":viewer_id,"api_count":1100,"shop_type":2,"shop_item_id":200031,"number":2});
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(bought.data["user_info"]["free_mana"], 0);
    assert_eq!(bought.data["user_info"]["paid_mana"], 150);
    assert_eq!(bought.data["item_list"]["2"], 2);
    let stored = read_player_snapshot(root.path());
    assert_eq!(stored["all_active_mission_list"]["25005"]["progress"], 400);
    let retry = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(&request),
    ));
    assert_eq!(retry.data, bought.data);
    let insufficient = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/buy",
        &encode_request(
            &json!({"viewer_id":viewer_id,"shop_type":2,"shop_item_id":200031,"number":1}),
        ),
    ));
    assert_eq!(insufficient.data_headers.result_code, 3010);
    assert_eq!(read_player_snapshot(root.path()), stored);
    service.stop().unwrap();
}
// //// /按免费和付费玛纳合计余额完成多件购买并记录实际消耗 ////

// //// 兑换多件武器材料并按整单容量保持费用和库存一致 [@x380kkm 2026-09-24] ////
#[test]
fn exchanges_weapon_material_quantities_and_recovers_from_capacity_changes() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 120);
    set_virtual_time(&service, "2024-06-01T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        data["item_list"]["13001"] = json!(1000);
        data["item_list"]["12002"] = json!(0);
        data["item_list"]["49100"] = json!(40);
        data["item_list"]["14040"] = json!(0);
        data["item_list"]["1"] = json!(100_000);
    });
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(
            &json!({"viewer_id":viewer_id,"shop_type":8,"buy_item_list":{"210001":99,"300104":2}}),
        ),
    ));
    assert_eq!(
        bought.data["item_list"],
        json!({"13001":10,"12002":99,"49100":0,"14040":2})
    );
    update_player_snapshot(root.path(), |data| {
        data["item_list"]["40000"] = json!(200);
        data["item_list"]["13"] = json!(9998);
        data["user_equipment_list"]["4010014"] =
            json!({"enhancement_level":0,"level":1,"stack":996,"protection":false});
    });
    let request =
        json!({"viewer_id":viewer_id,"shop_type":7,"buy_item_list":{"200103":2,"200110":4}});
    let before = read_player_snapshot(root.path());
    let capacity = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&request),
    ));
    assert_eq!(capacity.data_headers.result_code, 3010);
    assert_eq!(read_player_snapshot(root.path()), before);
    update_player_snapshot(root.path(), |data| data["item_list"]["13"] = json!(9997));
    let bought = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/bulk_buy",
        &encode_request(&request),
    ));
    assert_eq!(
        bought.data["item_list"],
        json!({"40000":98,"13":9999,"100000":4,"4010014":1})
    );
    assert_eq!(bought.data["equipment_list"][0]["stack"], 999);
    assert_eq!(
        bought.data["over_max"][0]["equipment"],
        json!({"equipment_id":4010014,"number":1})
    );
    assert_eq!(read_player_snapshot(root.path())["item_list"]["1"], 100_000);
    service.stop().unwrap();
}
// //// /兑换多件武器材料并按整单容量保持费用和库存一致 ////
