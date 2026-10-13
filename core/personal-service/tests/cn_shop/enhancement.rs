// audience: internal
// # personal-service-cn-shop-enhancement-tests
// 强化目录验证覆盖各类别的完整阶段, HTTP 购买结果与持久化资产共同确定实际扣料和等级增量.

use super::*;

// //// 逐个购买主表强化阶段并核对费用和阶段上限 [@x380kkm 2026-09-21] ////
#[test]
fn purchases_every_equipment_enhancement_stage() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 230);
    set_virtual_time(&service, "2030-06-01T12:00:00.000Z");
    let catalog: Value = serde_json::from_str(include_str!(
        "../../../../assets/equipment_enhancement_shop.json"
    ))
    .unwrap();
    let catalog = catalog.as_object().unwrap();
    for (item_id, item) in catalog {
        let stage = item["stage"].as_i64().unwrap();
        let previous = catalog
            .values()
            .filter(|other| {
                other["shopCategoryId"] == item["shopCategoryId"]
                    && other["groupId"] == item["groupId"]
                    && other["stage"].as_i64().unwrap() < stage
            })
            .filter_map(|other| other["enhancementMaxLevel"].as_i64())
            .max()
            .unwrap_or_default();
        let target = item["enhancementMaxLevel"].as_i64().unwrap();
        let count = target - previous;
        let equipment_id = item["equipmentId"].as_i64().unwrap();
        update_player_snapshot(root.path(), |data| {
            data["item_list"] = json!({});
            data["user_equipment_list"][equipment_id.to_string()] = json!({"equipment_id":equipment_id,"level":item["requireAwakeningLevel"],"enhancement_level":previous,"stack":0,"protection":false});
            for cost in item["costs"].as_array().unwrap() {
                let id = cost["id"].as_i64().unwrap().to_string();
                let amount = data["item_list"][&id].as_i64().unwrap_or_default()
                    + cost["amount"].as_i64().unwrap() * count;
                data["item_list"][id] = json!(amount);
            }
        });
        let mut request = BuyRequest {
            viewer_id,
            shop_type: 10,
            shop_item_id: item_id.parse().unwrap(),
            number: count + 1,
        };
        let before = read_player_snapshot(root.path());
        let over_limit = cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request),
        );
        assert_eq!(
            decode_response::<Value>(&over_limit)
                .data_headers
                .result_code,
            3010,
            "{item_id}"
        );
        assert_eq!(read_player_snapshot(root.path()), before, "{item_id}");
        request.number = count;
        let response = cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request),
        );
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "{item_id}: {response}"
        );
        let response = decode_response::<Value>(&response);
        assert_eq!(
            response.data["equipment_list"][0]["enhancement_level"], target,
            "{item_id}"
        );
        let stored = read_player_snapshot(root.path());
        assert_eq!(
            stored["user_equipment_list"][equipment_id.to_string()]["enhancement_level"],
            target,
            "{item_id}"
        );
        assert!(
            stored["item_list"]
                .as_object()
                .unwrap()
                .values()
                .all(|value| value == 0),
            "{item_id}"
        );
        assert_eq!(
            stored["shop_purchase_counts"][format!("10:{item_id}")],
            count,
            "{item_id}"
        );
        request.number = 1;
        let completed = cn_support::send_request(
            service.port(),
            "/api/index.php/shop/buy",
            &encode_request(&request),
        );
        assert_eq!(
            decode_response::<Value>(&completed)
                .data_headers
                .result_code,
            3010,
            "{item_id}"
        );
        assert_eq!(read_player_snapshot(root.path()), stored, "{item_id}");
    }
    service.stop().unwrap();
}
// //// /逐个购买主表强化阶段并核对费用和阶段上限 ////

// //// 不同强化类别分别展示相同组号的装备 [@x380kkm 2026-09-21] ////
#[test]
fn keeps_enhancement_groups_scoped_to_their_shop_category() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 231);
    set_virtual_time(&service, "2024-09-28T12:00:00.000Z");
    update_player_snapshot(root.path(), |data| {
        for id in [5010070, 5100011] {
            data["user_equipment_list"][id.to_string()] = json!({"equipment_id":id,"level":1,"enhancement_level":0,"stack":0,"protection":false});
        }
    });
    let request = json!({"viewer_id":viewer_id,"shop_types":[10],"equipment_enhancement_shop_category_ids":[2,4],"boss_coin_shop_category_ids":[],"event_list":[]});
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/shop/get_sales_list",
        &encode_request(&request),
    ));
    let entries = response.data["sales_list"].as_array().unwrap();
    let first = entries
        .iter()
        .find(|entry| entry["shop_item_id"] == 1001)
        .unwrap();
    let second = entries
        .iter()
        .find(|entry| entry["shop_item_id"] == 3001)
        .unwrap();
    assert_eq!(first["group_info"]["group_total_stock_quantity"], 99);
    assert_eq!(second["group_info"]["group_total_stock_quantity"], 120);
    service.stop().unwrap();
}
// //// /不同强化类别分别展示相同组号的装备 ////
