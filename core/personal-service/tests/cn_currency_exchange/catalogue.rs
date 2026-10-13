// audience: internal
// # personal-service-cn-currency-exchange-catalogue-tests
// 每个实际兑换目标核对主表费用, 保存状态和图鉴, 羁绊证装备保留既有养成状态.

use super::*;
use std::collections::BTreeMap;

// //// 兑换全部不同星屑奖励并核对任务和领取记录 [@x380kkm 2026-09-23] ////
#[test]
fn exchanges_every_star_crumb_reward() {
    let definitions = star_catalog();
    let costs: Value = serde_json::from_str(include_str!(
        "../../../../assets/star_crumb_exchange_cost.json"
    ))
    .unwrap();
    let encyclopedia: Value =
        serde_json::from_str(include_str!("../../assets/cn-character-encyclopedia.json")).unwrap();
    let mut targets = BTreeMap::new();
    for (id, rows) in definitions.as_object().unwrap() {
        let row = &rows[0];
        let key = (
            row[0].as_str().unwrap().parse::<i64>().unwrap(),
            row[1].as_str().unwrap().parse::<i64>().unwrap(),
        );
        if row[6] == "(None)" && row[7] == "(None)" {
            targets
                .entry(key)
                .or_insert((id.parse::<i64>().unwrap(), row.clone()));
        }
    }
    assert_eq!(targets.len(), 399);
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 101);
    update(root.path(), viewer, |data| {
        data["user_info"]["star_crumb"] = json!(1_000_000);
        data["user_character_list"] = json!({});
        data["user_equipment_list"] = json!({});
        data["item_list"]["10002"] = json!(0);
        data["item_list"]["10003"] = json!(0);
    });
    let mut expected = 1_000_000;
    for (index, ((kind, id), (exchange_id, row))) in targets.iter().enumerate() {
        let now = local_time(row[3].as_str().unwrap()) + 30;
        set_time(&service, now);
        let cost = costs[kind.to_string()][0][usize::from(row[8] == "5")]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        expected -= cost;
        let response = send(
            &service,
            "exchange/star_crumb",
            &star_request(viewer, *exchange_id, index as i64 + 1),
        );
        assert_eq!(
            response["user_info"]["star_crumb"], expected,
            "{exchange_id}"
        );
        let saved = stored(root.path(), viewer);
        assert_eq!(saved["user_info"]["star_crumb"], expected);
        match kind {
            0 => {
                assert!(
                    saved["user_character_list"][id.to_string()].is_object(),
                    "{exchange_id}"
                );
                assert_eq!(response["character_list"][0]["character_id"], *id);
                if let Some(encyclopedia_id) = encyclopedia[id.to_string()].as_i64() {
                    assert!(
                        saved["encyclopedia_list"][encyclopedia_id.to_string()].is_object(),
                        "{exchange_id}"
                    );
                    assert!(
                        response["encyclopedia_info"][encyclopedia_id.to_string()].is_object(),
                        "{exchange_id}"
                    );
                }
            }
            1 => assert_eq!(saved["item_list"][id.to_string()], 1),
            2 => {
                assert_eq!(saved["user_equipment_list"][id.to_string()]["stack"], 0);
                assert_eq!(response["equipment_list"][0]["equipment_id"], *id);
            }
            _ => unreachable!(),
        }
        let before = stored(root.path(), viewer);
        assert_eq!(
            send(
                &service,
                "exchange/star_crumb",
                &star_request(viewer, *exchange_id, index as i64 + 1)
            ),
            response
        );
        assert_eq!(stored(root.path(), viewer), before, "{exchange_id}");
    }
    assert_eq!(history_count(root.path(), viewer), 399);
    let progress = send(
        &service,
        "mission/get_mission_progress",
        &json!({
            "viewer_id":viewer,"api_count":10000,"category_list":[{"category":1}]
        }),
    );
    for (id, expected) in [(32, 357), (33, 40)] {
        let entry = progress["mission_progress_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["mission_category"] == 1 && entry["mission_id"] == id)
            .unwrap();
        assert_eq!(entry["progress_value"], expected);
    }
    service.stop().unwrap();
}
// //// /兑换全部不同星屑奖励并核对任务和领取记录 ////

// //// 全部羁绊证装备按各自库存兑换并保留已有强化 [@x380kkm 2026-09-23] ////
#[test]
fn exchanges_all_bond_equipment_and_preserves_owned_state() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 102);
    let definitions: Value =
        serde_json::from_str(include_str!("../../../../assets/bond_token_exchange.json")).unwrap();
    let owned = json!({"stack":3,"level":4,"enhancement_level":2,"protection":true});
    update(root.path(), viewer, |data| {
        data["user_info"]["bond_token"] = json!(100);
        data["user_equipment_list"] = json!({"5010005":owned});
    });
    for (sequence, (id, row)) in definitions.as_object().unwrap().iter().enumerate() {
        let id = id.parse::<i64>().unwrap();
        set_time(&service, local_time(row[2].as_str().unwrap()) + 30);
        let body = bond_request(viewer, id, sequence as i64 + 1);
        let response = send(&service, "exchange/bond_token", &body);
        let equipment = &response["equipment_list"][0];
        assert_eq!(equipment["equipment_id"], id);
        assert_eq!(
            response["user_info"]["bond_token"],
            50 - (sequence as i64) * 50
        );
        if id == 5010005 {
            for field in ["level", "enhancement_level", "protection"] {
                assert_eq!(equipment[field], owned[field]);
            }
            assert_eq!(equipment["stack"], 4);
        } else {
            assert_eq!(equipment["stack"], 0);
        }
        let before = stored(root.path(), viewer);
        assert_eq!(send(&service, "exchange/bond_token", &body), response);
        assert_eq!(stored(root.path(), viewer), before);
        let exhausted = request(
            &service,
            "exchange/bond_token",
            &bond_request(viewer, id, 100 + sequence as i64),
        );
        assert!(exhausted.contains("shop_out_of_stock"), "{exhausted}");
        assert_eq!(stored(root.path(), viewer), before);
    }
    let list = send(
        &service,
        "exchange/get_bond_token_exchange_list",
        &json!({"viewer_id":viewer}),
    );
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["exchange_count"] == 1));
    assert_eq!(history_count(root.path(), viewer), 2);
    service.stop().unwrap();
}
// //// /全部羁绊证装备按各自库存兑换并保留已有强化 ////
