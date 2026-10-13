// audience: internal
// # personal-service-cn-equipment-dissolve-api-tests
// 装备分解按真实主表验证最后一件, 副本, 批量产出, 容量, 任务, 领取记录和失败恢复.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "cn_equipment_dissolve/support.rs"]
mod fixture;
#[allow(dead_code)]
mod support;

use cn_support::decode_response;
use fixture::*;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 扭蛋装备分解按稀有度产生星粒并限制实际入库量 [@x380kkm 2026-09-23] ////
#[test]
fn grants_star_grains_for_gacha_equipment_and_caps_the_balance() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(&service, 923506, json!({"3010007":1,"4010010":3}));
    let sold = data(
        &service,
        "equipment/sell_equipment",
        &request(viewer, 3_010_007, None, 1),
    );
    assert_eq!(sold["item_list"]["990008"], 1);
    assert_eq!(sold["item_list"]["100000"], 3);
    edit_player(root.path(), |player| {
        player["item_list"]["990008"] = json!(999995);
    });
    let sold = data(
        &service,
        "equipment/sell_stack",
        &request(viewer, 4_010_010, Some(2), 2),
    );
    assert_eq!(sold["item_list"]["990008"], 999999);
    assert_eq!(sold["item_list"]["100000"], 11);
    assert_eq!(sold["item_list"]["4010010"], 2);
    assert_eq!(
        history_amount(root.path(), 1, Some(990008), "sell-stack"),
        4
    );
    service.stop().unwrap();
}
// //// /扭蛋装备分解按稀有度产生星粒并限制实际入库量 ////

// //// 最后一件装备产生一份奖励并在跨重启重放中保留新获得装备 [@x380kkm 2026-09-23] ////
#[test]
fn sells_last_equipment_once_and_replays_after_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(&service, 923501, json!({FIRST.to_string():1}));
    let first = request(viewer, FIRST, None, 1);
    let response = send(&service, "equipment/sell_equipment", &first);
    let rewards = decode_response::<Value>(&response).data;
    assert_eq!(rewards["item_list"]["100000"], 5);
    assert_eq!(rewards["item_list"][FIRST.to_string()], 1);
    assert!(player(root.path())["user_equipment_list"]
        .get(FIRST.to_string())
        .is_none());
    assert_eq!(
        history_amount(root.path(), 1, Some(100000), "sell-equipment"),
        5
    );
    grant(&service, viewer, json!({FIRST.to_string():1}), json!({}));
    let before = state(root.path());
    assert_eq!(send(&service, "equipment/sell_equipment", &first), response);
    assert_eq!(state(root.path()), before);
    service.stop().unwrap();

    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(send(&service, "equipment/sell_equipment", &first), response);
    assert_eq!(state(root.path()), before);
    let new_viewer = signup(&service, 923501);
    assert_ne!(new_viewer, viewer);
    data(
        &service,
        "equipment/sell_equipment",
        &request(new_viewer, FIRST, None, 1),
    );
    assert_eq!(
        history_amount(root.path(), 1, Some(100000), "sell-equipment"),
        10
    );
    service.stop().unwrap();
}
// //// /最后一件装备产生一份奖励并在跨重启重放中保留新获得装备 ////

// //// 本体保护覆盖普通与展示编队且副本允许在装备强化后分解 [@x380kkm 2026-09-23] ////
#[test]
fn preserves_protected_equipped_enhanced_and_unsellable_equipment() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(
        &service,
        923502,
        json!({FIRST.to_string():2,SECOND.to_string():1,FIXED.to_string():1}),
    );
    data(
        &service,
        "equipment/set_protection",
        &json!({"viewer_id":viewer,"protection":true,"equipment_ids":[FIRST]}),
    );
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_stack",
        &request(viewer, FIRST, Some(1), 1),
        "equipment_protected",
    );
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_equipment",
        &request(viewer, FIRST, None, 1),
        "equipment_protected",
    );
    let bulk = data(
        &service,
        "equipment/bulk_sell_stack",
        &json!({"viewer_id":viewer,"api_count":2,"equipment_ids":[FIRST,FIXED,9999999]}),
    );
    assert_eq!(bulk["item_list"], json!({}));
    assert_eq!(
        player(root.path())["user_equipment_list"][FIRST.to_string()]["stack"],
        1
    );
    data(
        &service,
        "equipment/set_protection",
        &json!({"viewer_id":viewer,"protection":false,"equipment_ids":[FIRST]}),
    );
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_equipment",
        &request(viewer, FIRST, None, 3),
        "equipment_has_stack",
    );
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_equipment",
        &request(viewer, FIXED, None, 3),
        "equipment_not_sellable",
    );
    for field in ["user_party_group_list", "profile_party_group_list"] {
        edit_player(root.path(), |player| {
            player[field]["1"]["list"]["1"]["equipment_ids"] = json!([SECOND, null, null]);
        });
        assert_rejected(
            &service,
            root.path(),
            "equipment/sell_equipment",
            &request(viewer, SECOND, None, 3),
            "equipment_is_equipped",
        );
        edit_player(root.path(), |player| {
            player[field]["1"]["list"]["1"]["equipment_ids"] = json!([null, null, null]);
        });
    }
    edit_player(root.path(), |player| {
        player["cn_activity_state"]["event_families"]["raid"]["1"]["party_group_list"] =
            json!([{"party_group_id":1,"party_list":[{"equipment_ids":[SECOND,null,null]}]}]);
    });
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_equipment",
        &request(viewer, SECOND, None, 3),
        "equipment_is_equipped",
    );
    edit_player(root.path(), |player| {
        player["cn_activity_state"]["event_families"]["raid"]["1"]["party_group_list"] = json!([]);
        player["user_equipment_list"][SECOND.to_string()]["enhancement_level"] = json!(1);
        player["user_equipment_list"][FIRST.to_string()]["enhancement_level"] = json!(1);
        player["user_party_group_list"]["1"]["list"]["1"]["equipment_ids"] =
            json!([FIRST, null, null]);
    });
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_equipment",
        &request(viewer, SECOND, None, 3),
        "equipment_is_enhanced",
    );
    data(
        &service,
        "equipment/sell_stack",
        &request(viewer, FIRST, Some(1), 3),
    );
    assert_eq!(
        player(root.path())["user_equipment_list"][FIRST.to_string()]["stack"],
        0
    );
    service.stop().unwrap();
}
// //// /本体保护覆盖普通与展示编队且副本允许在装备强化后分解 ////

// //// 副本合并数量遵守魂珠容量且独立点击与请求重放分别计数 [@x380kkm 2026-09-23] ////
#[test]
fn sells_requested_copies_without_repeating_and_checks_soul_capacity() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(&service, 923503, json!({FIRST.to_string():7}));
    edit_player(root.path(), |player| {
        player["item_list"][FIRST.to_string()] = json!(9998);
    });
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_stack",
        &request(viewer, FIRST, Some(2), 1),
        "ability_soul_max",
    );
    edit_player(root.path(), |player| {
        player["item_list"][FIRST.to_string()] = json!(0);
    });
    let duplicates = json!({"viewer_id":viewer,"api_count":1,"equipment_list":[{"equipment_id":FIRST,"number":1},{"equipment_id":FIRST,"number":2}]});
    let response = send(&service, "equipment/sell_stack", &duplicates);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(
        player(root.path())["user_equipment_list"][FIRST.to_string()]["stack"],
        3
    );
    assert_eq!(history_amount(root.path(), 1, Some(FIRST), "sell-stack"), 3);
    let after = state(root.path());
    assert_eq!(
        send(&service, "equipment/sell_stack", &duplicates),
        response
    );
    assert_eq!(state(root.path()), after);
    assert_rejected(
        &service,
        root.path(),
        "equipment/sell_stack",
        &request(viewer, FIRST, Some(1), 1),
        "equipment_request_conflict",
    );
    let legacy = json!({"viewer_id":viewer,"equipment_list":[{"equipment_id":FIRST,"number":1}]});
    data(&service, "equipment/sell_stack", &legacy);
    data(&service, "equipment/sell_stack", &legacy);
    assert_eq!(
        player(root.path())["user_equipment_list"][FIRST.to_string()]["stack"],
        1
    );
    assert_eq!(history_amount(root.path(), 1, Some(FIRST), "dissolve"), 2);
    service.stop().unwrap();
}
// //// /副本合并数量遵守魂珠容量且独立点击与请求重放分别计数 ////

// //// 批量产出按主表封顶并将锻造玉溢出收入计入任务和历史 [@x380kkm 2026-09-23] ////
#[test]
fn bulk_sale_converts_craft_overflow_and_caps_actual_soul_rewards() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(
        &service,
        923504,
        json!({FIRST.to_string():401,SECOND.to_string():3}),
    );
    edit_player(root.path(), |player| {
        player["item_list"]["100000"] = json!(999999);
        player["item_list"][FIRST.to_string()] = json!(9998);
        player["user_equipment_list"][SECOND.to_string()]["protection"] = json!(true);
    });
    let before_mana = player(root.path())["user_info"]["free_mana"]
        .as_i64()
        .unwrap();
    let request = json!({"viewer_id":viewer,"api_count":1,"equipment_ids":[FIRST,SECOND,FIRST]});
    let raw = send(&service, "equipment/bulk_sell_stack", &request);
    let response = decode_response::<Value>(&raw).data;
    assert_eq!(response["item_list"]["100000"], 999999);
    assert_eq!(response["item_list"][FIRST.to_string()], 9999);
    assert_eq!(response["user_info"]["free_mana"], before_mana + 10000);
    assert_eq!(
        player(root.path())["user_equipment_list"][FIRST.to_string()]["stack"],
        0
    );
    assert_eq!(
        player(root.path())["user_equipment_list"][SECOND.to_string()]["stack"],
        2
    );
    assert_eq!(
        history_amount(root.path(), 8, None, "bulk-sell-stack"),
        10000
    );
    assert_eq!(
        history_amount(root.path(), 1, Some(100000), "bulk-sell-stack"),
        0
    );
    assert_eq!(
        history_amount(root.path(), 1, Some(FIRST), "bulk-sell-stack"),
        1
    );
    assert_eq!(
        player(root.path())["mission_economic_counters"]["mana_addition_count"],
        10000
    );
    assert!(response["mission_info"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mission| mission["mission_category_id"] == 1 && mission["mission_id"] == 40));
    let after = state(root.path());
    assert_eq!(send(&service, "equipment/bulk_sell_stack", &request), raw);
    assert_eq!(state(root.path()), after);
    service.stop().unwrap();
}
// //// /批量产出按主表封顶并将锻造玉溢出收入计入任务和历史 ////

// //// 发奖历史写入失败时回滚装备消耗任务和请求凭据 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_rewards_missions_inventory_and_receipts_on_failure() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = prepare(&service, 923505, json!({FIRST.to_string():401}));
    edit_player(root.path(), |player| {
        player["item_list"]["100000"] = json!(999999);
    });
    let request = json!({"viewer_id":viewer,"api_count":1,"equipment_ids":[FIRST]});
    let before = state(root.path());
    database(root.path()).execute_batch("CREATE TRIGGER reject_dissolve BEFORE INSERT ON player_receive_history WHEN NEW.event_key LIKE 'equipment:%' BEGIN SELECT RAISE(ABORT,'reject dissolve history'); END;").unwrap();
    let failed = send(&service, "equipment/bulk_sell_stack", &request);
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(state(root.path()), before);
    database(root.path())
        .execute_batch("DROP TRIGGER reject_dissolve;")
        .unwrap();
    data(&service, "equipment/bulk_sell_stack", &request);
    assert_eq!(
        history_amount(root.path(), 8, None, "bulk-sell-stack"),
        10000
    );
    assert_eq!(
        player(root.path())["mission_economic_counters"]["mana_addition_count"],
        10000
    );
    service.stop().unwrap();
}
// //// /发奖历史写入失败时回滚装备消耗任务和请求凭据 ////
