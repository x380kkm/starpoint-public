// audience: internal
// # personal-service-cn-payment-compatibility-tests
// 跨实例存档迁移保留商品订单, 月卡权益和已领日期, 当前设备继续确认订单与领取日奖.

use super::*;

#[path = "../support/local_saves.rs"]
#[allow(dead_code)]
mod local_saves;

struct PaymentArchive {
    package: Value,
    payment_state: Value,
    viewer_id: i64,
    product_id: String,
    finished_order: String,
    pending_order: String,
}

struct ImportedSave {
    root: TempDir,
    service: PersonalService,
    slot_id: i64,
    stranger_slot_id: i64,
    stranger_viewer_id: i64,
}

// //// 按槽位读取当前账号的玩家快照 [@x380kkm 2026-09-28] ////
fn slot_snapshot(root: &Path, slot_id: i64) -> Value {
    let data: String = database(root).query_row(
        "SELECT player_snapshots.data_json FROM player_snapshots JOIN local_save_slots ON local_save_slots.account_id=player_snapshots.account_id WHERE local_save_slots.id=?1",
        [slot_id], |row| row.get(0),
    ).unwrap();
    serde_json::from_str(&data).unwrap()
}

// //// 提取订单确认前后应保持不变的资产 [@x380kkm 2026-09-28] ////
fn holdings(player: &Value) -> Value {
    json!({
        "vmoney":player["user_info"]["vmoney"],
        "free_vmoney":player["user_info"]["free_vmoney"],
        "items":player["item_list"],
        "purchases":player["purchased_times_list"],
        "payment_counts":player["payment_purchase_counts"],
    })
}

// //// 查询当前账号的未完成订单 [@x380kkm 2026-09-28] ////
fn pending(service: &PersonalService, viewer: i64) -> Value {
    let response = decode_response::<Value>(&request(
        service,
        "channels/channel_leiting_pay/query_unfinish_order",
        json!({"viewer_id":viewer}),
    ));
    assert_eq!(response.data_headers.result_code, 1);
    response.data
}

// //// 购买月卡并导出包含已确认与未确认订单的真实存档包 [@x380kkm 2026-09-28] ////
fn export_month_card_orders() -> PaymentArchive {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2026-09-22T12:00:00.000Z");
    let viewer = local_saves::signup(service.port(), 981)
        .data_headers
        .viewer_id;
    local_saves::load(service.port(), viewer, "1.4.99");
    let catalog: Value =
        serde_json::from_str(include_str!("../../assets/cn-payment-catalog.json")).unwrap();
    let product = catalog["products"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, product)| {
            product["direct_purchase"] == true
                && product["entitlements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| {
                        entry["entitlement"] == "subscription" && entry["id"] == "month_card"
                    })
        })
        .unwrap()
        .0
        .clone();
    let finished = start(&service, viewer, 7, "com.leiting.wf.stone_50")["order_id"]
        .as_str()
        .unwrap()
        .to_owned();
    finish(&service, viewer, "com.leiting.wf.stone_50", &finished);
    let pending = start(&service, viewer, 8, &product)["order_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let slot_id = local_saves::list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    local_saves::update_slot_player_snapshot(root.path(), slot_id, |player| {
        player["payment_state"]["store_context"] = json!({"region":"cn","channel":"local"});
        for order in player["payment_state"]["orders"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            order["receipt_context"] = json!({"format":"local-store","attributes":["retained",3]});
        }
        player["user_info"]["boost_point"] = json!(0);
        player["user_info"]["boss_boost_point"] = json!(0);
    });
    let source = slot_snapshot(root.path(), slot_id);
    let mut package = local_saves::export_local_save(&service, slot_id);
    package["sourceClient"] = json!({"platform":"android","version":"1.8.1"});
    assert!(package["data"]["payment_state"]["orders"][&pending]
        .get("viewer_id")
        .is_none());
    assert_eq!(source["user_info"]["month_card_remain_days"], 30);
    let month = &source["payment_subscription_state"]["entitlements"]["month_card"];
    assert_eq!(month["store_product_id"], product);
    assert_eq!(month["received_days"].as_array().unwrap().len(), 1);
    service.stop().unwrap();
    PaymentArchive {
        package,
        payment_state: source["payment_state"].clone(),
        viewer_id: viewer,
        product_id: product,
        finished_order: finished,
        pending_order: pending,
    }
}
// //// /购买月卡并导出包含已确认与未确认订单的真实存档包 ////

// //// 将导出的存档导入另一实例的独立账号槽位 [@x380kkm 2026-09-28] ////
fn import_archive(archive: &PaymentArchive) -> ImportedSave {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2026-09-22T12:00:00.000Z");
    let stranger = local_saves::signup(service.port(), 982)
        .data_headers
        .viewer_id;
    let stranger_slot_id = local_saves::list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let response = local_saves::authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&archive.package),
    );
    local_saves::assert_status(&response, "201 Created");
    let slot_id = local_saves::response_body(&response)["id"]
        .as_i64()
        .unwrap();
    assert_ne!(slot_id, stranger_slot_id);
    ImportedSave {
        root,
        service,
        slot_id,
        stranger_slot_id,
        stranger_viewer_id: stranger,
    }
}

// //// 核对订单的持久化业务字段和扩展字段 [@x380kkm 2026-09-28] ////
fn assert_order_contents(retained: &Value, expected: &Value) {
    let mut retained = retained.as_object().unwrap().clone();
    let mut expected = expected.as_object().unwrap().clone();
    for field in ["viewer_id", "api_count", "finished_at"] {
        retained.remove(field);
        expected.remove(field);
    }
    assert_eq!(retained, expected);
}

// //// 在当前设备恢复订单并跨订阅日继续领取月卡奖励 [@x380kkm 2026-09-28] ////
fn continue_imported_month_card(
    archive: PaymentArchive,
    mut imported: ImportedSave,
    expected_orders: Value,
) {
    let source = &archive.package["data"];
    let original_holdings = holdings(source);
    let stranger_before = slot_snapshot(imported.root.path(), imported.stranger_slot_id);
    let activated = local_saves::authorized_request(
        &imported.service,
        "POST",
        &format!("/v1/local-saves/{}/activate", imported.slot_id),
        Some(&json!({"device_id":983})),
    );
    local_saves::assert_status(&activated, "200 OK");
    let viewer = local_saves::signup(imported.service.port(), 983)
        .data_headers
        .viewer_id;
    assert_ne!(viewer, archive.viewer_id);
    let recovery = pending(&imported.service, viewer);
    assert_eq!(recovery["order_id"], archive.pending_order);
    assert_eq!(recovery["product_key"], archive.product_id);
    let restored = slot_snapshot(imported.root.path(), imported.slot_id);
    assert_eq!(holdings(&restored), original_holdings);
    assert_eq!(
        restored["payment_state"]["store_context"],
        expected_orders["store_context"]
    );
    assert_eq!(
        restored["payment_state"]["sequence"],
        expected_orders["sequence"]
    );
    for id in [&archive.finished_order, &archive.pending_order] {
        let order = &restored["payment_state"]["orders"][id];
        assert_order_contents(order, &expected_orders["orders"][id]);
        assert_eq!(order["viewer_id"], viewer);
        assert!(order["api_count"].is_null());
        assert_eq!(
            order["finished_at"],
            expected_orders["orders"][id]["finished_at"]
        );
    }
    let loaded = local_saves::load(imported.service.port(), viewer, "1.4.99").data;
    assert_eq!(holdings(&loaded), original_holdings);
    assert_eq!(loaded["user_info"]["boost_point"], 0);
    assert_eq!(loaded["user_info"]["boss_boost_point"], 0);
    let restored = slot_snapshot(imported.root.path(), imported.slot_id);
    assert_eq!(
        restored["payment_subscription_state"]["entitlements"],
        source["payment_subscription_state"]["entitlements"]
    );
    assert_eq!(
        pending(&imported.service, imported.stranger_viewer_id)["order_id"],
        ""
    );
    let foreign = decode_response::<Value>(&request(
        &imported.service,
        "payment/finish",
        json!({"viewer_id":imported.stranger_viewer_id,"payment":{"transaction_id":archive.pending_order}}),
    ));
    assert_ne!(foreign.data_headers.result_code, 1);
    assert_eq!(
        slot_snapshot(imported.root.path(), imported.stranger_slot_id),
        stranger_before
    );
    finish(
        &imported.service,
        viewer,
        "com.leiting.wf.stone_50",
        &archive.finished_order,
    );
    let confirmed = finish(
        &imported.service,
        viewer,
        &archive.product_id,
        &archive.pending_order,
    );
    assert_eq!(confirmed["after_vmoney"], source["user_info"]["vmoney"]);
    assert_eq!(
        holdings(&slot_snapshot(imported.root.path(), imported.slot_id)),
        original_holdings
    );
    assert_eq!(pending(&imported.service, viewer)["order_id"], "");
    assert_eq!(
        finish(
            &imported.service,
            viewer,
            &archive.product_id,
            &archive.pending_order
        ),
        confirmed
    );
    let new_order = start(&imported.service, viewer, 8, &archive.product_id);
    assert_ne!(new_order["order_id"], archive.pending_order);
    let renewed = slot_snapshot(imported.root.path(), imported.slot_id);
    let catalog: Value =
        serde_json::from_str(include_str!("../../assets/cn-payment-catalog.json")).unwrap();
    let charge = catalog["products"][&archive.product_id]["charge_vmoney_num"]
        .as_i64()
        .unwrap();
    assert_eq!(
        renewed["user_info"]["vmoney"],
        source["user_info"]["vmoney"].as_i64().unwrap() + charge
    );
    assert_eq!(
        renewed["user_info"]["free_vmoney"],
        source["user_info"]["free_vmoney"]
    );
    assert_eq!(
        renewed["purchased_times_list"][&archive.product_id],
        source["purchased_times_list"][&archive.product_id]
            .as_i64()
            .unwrap()
            + 1
    );
    assert_eq!(renewed["user_info"]["month_card_remain_days"], 60);
    assert_eq!(
        renewed["payment_subscription_state"]["entitlements"]["month_card"]["store_product_id"],
        archive.product_id
    );
    assert_eq!(
        renewed["payment_subscription_state"]["entitlements"]["month_card"]["received_days"],
        source["payment_subscription_state"]["entitlements"]["month_card"]["received_days"]
    );
    assert_eq!(
        start(&imported.service, viewer, 8, &archive.product_id),
        new_order
    );
    assert_eq!(
        slot_snapshot(imported.root.path(), imported.slot_id),
        renewed
    );
    imported.service.stop().unwrap();
    imported.service = PersonalService::start(imported.root.path(), 0).unwrap();
    assert_eq!(
        pending(&imported.service, viewer)["order_id"],
        new_order["order_id"]
    );
    assert_eq!(
        start(&imported.service, viewer, 8, &archive.product_id),
        new_order
    );
    assert_eq!(
        holdings(&slot_snapshot(imported.root.path(), imported.slot_id)),
        holdings(&renewed)
    );
    finish(
        &imported.service,
        viewer,
        &archive.product_id,
        new_order["order_id"].as_str().unwrap(),
    );
    set_time(&imported.service, "2026-09-22T21:00:01.000Z");
    let next_day = local_saves::load(imported.service.port(), viewer, "1.4.99").data;
    let entitlements = source["payment_subscription_state"]["entitlements"]
        .as_object()
        .unwrap();
    let daily_stones: i64 = entitlements
        .keys()
        .flat_map(|id| {
            catalog["subscriptions"][id]["daily_rewards"]
                .as_array()
                .unwrap()
        })
        .filter(|reward| reward["kind"] == "free_vmoney")
        .map(|reward| reward["amount"].as_i64().unwrap())
        .sum();
    assert_eq!(
        next_day["user_info"]["free_vmoney"],
        renewed["user_info"]["free_vmoney"].as_i64().unwrap() + daily_stones
    );
    assert_eq!(next_day["user_info"]["month_card_remain_days"], 59);
    let next_saved = slot_snapshot(imported.root.path(), imported.slot_id);
    for (id, entitlement) in entitlements {
        let current = &next_saved["payment_subscription_state"]["entitlements"][id];
        assert_eq!(current["store_product_id"], entitlement["store_product_id"]);
        assert_eq!(
            current["received_days"].as_array().unwrap().len(),
            entitlement["received_days"].as_array().unwrap().len() + 1
        );
    }
    local_saves::load(imported.service.port(), viewer, "1.4.99");
    assert_eq!(
        holdings(&slot_snapshot(imported.root.path(), imported.slot_id)),
        holdings(&next_saved)
    );
    for id in [&archive.finished_order, &archive.pending_order] {
        assert_order_contents(
            &next_saved["payment_state"]["orders"][id],
            &expected_orders["orders"][id],
        );
    }
    assert_eq!(
        next_saved["payment_state"]["store_context"],
        expected_orders["store_context"]
    );
    imported.service.stop().unwrap();
}
// //// /在当前设备恢复订单并跨订阅日继续领取月卡奖励 ////

// //// 导入身份无关的订单并继续原月卡权益 [@x380kkm 2026-09-28] ////
#[test]
fn imports_month_card_orders_without_viewer_and_preserves_daily_claims() {
    let archive = export_month_card_orders();
    let imported = import_archive(&archive);
    let expected = archive.package["data"]["payment_state"].clone();
    continue_imported_month_card(archive, imported, expected);
}

// //// 恢复持久化旧会话订单并继续原月卡权益 [@x380kkm 2026-09-28] ////
#[test]
fn rebinds_persisted_month_card_orders_from_another_viewer() {
    let archive = export_month_card_orders();
    let imported = import_archive(&archive);
    local_saves::update_slot_player_snapshot(imported.root.path(), imported.slot_id, |player| {
        player["payment_state"] = archive.payment_state.clone();
    });
    let expected = archive.payment_state.clone();
    continue_imported_month_card(archive, imported, expected);
}
