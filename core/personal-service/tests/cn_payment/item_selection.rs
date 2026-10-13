// audience: internal
// # personal-service-cn-payment-item-selection-tests
// 商品赠送的素材箱通过购买, 背包载入和道具使用接口进入实际库存.

use super::*;

// //// 购买目录中的素材箱并从背包选取素材 [@x380kkm 2026-09-28] ////
#[test]
fn uses_every_selection_box_granted_by_payment_products() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    set_time(&service, "2025-01-02T12:00:00.000Z");
    let products: Value =
        serde_json::from_str(include_str!("../../assets/cn-payment-catalog.json")).unwrap();
    let selections: Value =
        serde_json::from_str(include_str!("../../assets/cn-item-selection.json")).unwrap();
    let mut count = 0;
    for (product_id, product) in products["products"].as_object().unwrap() {
        for reward in product["rewards"].as_array().unwrap() {
            if reward["kind"] != "item" {
                continue;
            }
            let item_id = reward["id"].as_i64().unwrap();
            let id = item_id.to_string();
            let Some(selection) = selections.get(&id) else {
                continue;
            };
            count += 1;
            let choice = &selection["choices"][5];
            let material = choice["item_id"].as_i64().unwrap().to_string();
            let before = snapshot(root.path());
            let held_box = before["item_list"][&id].as_i64().unwrap_or_default();
            let held_material = before["item_list"][&material].as_i64().unwrap_or_default();
            let order = start(&service, viewer, count, product_id);
            assert_eq!(
                snapshot(root.path())["item_list"][&id],
                held_box + reward["amount"].as_i64().unwrap()
            );
            assert_eq!(start(&service, viewer, count, product_id), order);
            finish(
                &service,
                viewer,
                product_id,
                order["order_id"].as_str().unwrap(),
            );
            let loaded = decode_response::<Value>(&request(
                &service,
                "load",
                json!({"viewer_id":viewer,"keychain":viewer}),
            ));
            assert_eq!(
                loaded.data["item_list"][&id],
                held_box + reward["amount"].as_i64().unwrap()
            );
            let use_body = json!({"viewer_id":viewer,"api_count":count,"items":[{"id":item_id,"number":1,"selectIndex":6}]});
            let used =
                decode_response::<Value>(&request(&service, "item/use_item", use_body.clone()));
            assert_eq!(used.data_headers.result_code, 1);
            assert_eq!(
                used.data["item_list"][&id],
                held_box + reward["amount"].as_i64().unwrap() - 1
            );
            assert_eq!(
                used.data["item_list"][&material],
                held_material + choice["amount"].as_i64().unwrap()
            );
            let saved = snapshot(root.path());
            let replay = decode_response::<Value>(&request(&service, "item/use_item", use_body));
            assert_eq!(replay.data, used.data);
            assert_eq!(snapshot(root.path()), saved);
        }
    }
    assert_eq!(count, 5);
    service.stop().unwrap();
}
// //// /购买目录中的素材箱并从背包选取素材 ////
