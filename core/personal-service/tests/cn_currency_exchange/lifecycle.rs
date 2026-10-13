// audience: internal
// # personal-service-cn-currency-exchange-lifecycle-tests
// 兑换错误保留资产, 重启和请求重放保留原响应, 历史写入失败回滚整个兑换.

use super::*;

// //// 星屑兑换遵循持有资格和两种结晶的真实上限 [@x380kkm 2026-09-23] ////
#[test]
fn enforces_star_crumb_capacity_and_ownership_without_charging() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 103);
    let limits: Value =
        serde_json::from_str(include_str!("../../assets/cn-exchange-item-limits.json")).unwrap();
    for (exchange_id, item_id) in [(9000001, 10002), (9000002, 10003)] {
        let limit = limits[item_id.to_string()].as_i64().unwrap();
        update(root.path(), viewer, |data| {
            data["user_info"]["star_crumb"] = json!(10_000);
            data["item_list"][item_id.to_string()] = json!(limit - 1);
        });
        let response = send(
            &service,
            "exchange/star_crumb",
            &star_request(viewer, exchange_id, exchange_id),
        );
        assert_eq!(response["item_list"][item_id.to_string()], limit);
        let before = stored(root.path(), viewer);
        let rejected = request(
            &service,
            "exchange/star_crumb",
            &star_request(viewer, exchange_id, exchange_id + 10),
        );
        assert!(rejected.contains("exchange_item_limit"), "{rejected}");
        assert_eq!(stored(root.path(), viewer), before);
    }
    let offers = star_catalog();
    for (sequence, kind) in [(1, "0"), (2, "2")] {
        let (id, rows) = offers
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, rows)| {
                rows[0][0] == kind && rows[0][6] == "(None)" && rows[0][7] == "(None)"
            })
            .unwrap();
        let id = id.parse::<i64>().unwrap();
        let target = rows[0][1].as_str().unwrap();
        set_time(&service, local_time(rows[0][3].as_str().unwrap()) + 30);
        update(root.path(), viewer, |data| {
            data["user_info"]["star_crumb"] = json!(10000);
            let field = if kind == "0" {
                "user_character_list"
            } else {
                "user_equipment_list"
            };
            data[field][target] = json!({"stack":5});
        });
        let before = stored(root.path(), viewer);
        let blocked = request(
            &service,
            "exchange/star_crumb",
            &star_request(viewer, id, sequence),
        );
        assert!(blocked.contains("exchange_reward_owned"), "{blocked}");
        assert_eq!(stored(root.path(), viewer), before);
    }
    service.stop().unwrap();
}
// //// /星屑兑换遵循持有资格和两种结晶的真实上限 ////

// //// 兑换窗口和关联卡池管理关闭使用客户端可处理响应 [@x380kkm 2026-09-23] ////
#[test]
fn follows_real_windows_and_every_linked_gacha() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 104);
    update(root.path(), viewer, |data| {
        data["user_info"]["star_crumb"] = json!(1_000_000);
        data["user_info"]["bond_token"] = json!(100);
        data["user_character_list"] = json!({});
    });
    let offers = star_catalog();
    let row = &offers["1"][0];
    for now in [
        local_time(row[3].as_str().unwrap()) - 60,
        local_time(row[4].as_str().unwrap()) + 60,
    ] {
        set_time(&service, now);
        let before = stored(root.path(), viewer);
        let response = decode_response::<Value>(&request(
            &service,
            "exchange/star_crumb",
            &star_request(viewer, 1, 1),
        ));
        assert_eq!(response.data_headers.result_code, 2053);
        assert_eq!(stored(root.path(), viewer), before);
    }
    let bonds: Value =
        serde_json::from_str(include_str!("../../../../assets/bond_token_exchange.json")).unwrap();
    for (id, row) in bonds.as_object().unwrap() {
        for now in [
            local_time(row[2].as_str().unwrap()) - 60,
            local_time(row[3].as_str().unwrap()) + 60,
        ] {
            set_time(&service, now);
            let before = stored(root.path(), viewer);
            let response = decode_response::<Value>(&request(
                &service,
                "exchange/bond_token",
                &bond_request(viewer, id.parse().unwrap(), 2),
            ));
            assert_eq!(response.data_headers.result_code, 2053);
            assert_eq!(stored(root.path(), viewer), before);
        }
    }
    let mut linked = std::collections::BTreeMap::new();
    for (id, rows) in offers.as_object().unwrap() {
        let row = &rows[0];
        for field in [6, 7] {
            if let Some(gacha_id) = row[field].as_str().filter(|id| *id != "(None)") {
                linked.entry(gacha_id.to_owned()).or_insert((
                    id.parse::<i64>().unwrap(),
                    row[1].as_str().unwrap().to_owned(),
                ));
            }
        }
    }
    assert_eq!(linked.len(), 10);
    for (sequence, (gacha, (id, target))) in linked.iter().enumerate() {
        update(root.path(), viewer, |data| {
            data["user_character_list"]
                .as_object_mut()
                .unwrap()
                .remove(target);
        });
        manage(
            &service,
            "POST",
            &format!("/v1/activities/gacha:{gacha}/close"),
            json!({}),
        );
        let before = stored(root.path(), viewer);
        let body = star_request(viewer, *id, sequence as i64 + 10);
        let response = decode_response::<Value>(&request(&service, "exchange/star_crumb", &body));
        assert_eq!(response.data_headers.result_code, 2053);
        assert_eq!(stored(root.path(), viewer), before);
        manage(
            &service,
            "PUT",
            &format!("/v1/activities/gacha:{gacha}/mode"),
            json!({"mode":"always"}),
        );
        let granted = send(&service, "exchange/star_crumb", &body);
        manage(
            &service,
            "POST",
            &format!("/v1/activities/gacha:{gacha}/close"),
            json!({}),
        );
        let before = stored(root.path(), viewer);
        assert_eq!(send(&service, "exchange/star_crumb", &body), granted);
        assert_eq!(stored(root.path(), viewer), before);
    }
    service.stop().unwrap();
}
// //// /兑换窗口和关联卡池管理关闭使用客户端可处理响应 ////

// //// 两种兑换的历史写入失败回滚并在重启后重放原响应 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_failed_history_and_replays_after_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 105);
    let another = signup(&service, 106);
    update(root.path(), viewer, |data| {
        data["user_info"]["star_crumb"] = json!(1000);
        data["user_info"]["bond_token"] = json!(100);
    });
    let requests = [
        ("exchange/star_crumb", star_request(viewer, 9000001, 1)),
        ("exchange/bond_token", bond_request(viewer, 5010005, 1)),
    ];
    let mut responses = Vec::new();
    for (route, body) in &requests {
        let before = stored(root.path(), viewer);
        database(root.path()).execute_batch("CREATE TRIGGER reject_currency_history BEFORE INSERT ON player_receive_history BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;").unwrap();
        let failed = request(&service, route, body);
        assert!(!failed.starts_with("HTTP/1.1 200"), "{failed}");
        assert_eq!(stored(root.path(), viewer), before);
        database(root.path())
            .execute_batch("DROP TRIGGER reject_currency_history;")
            .unwrap();
        responses.push(send(&service, route, body));
        assert_eq!(send(&service, route, body), *responses.last().unwrap());
    }
    assert_eq!(history_count(root.path(), viewer), 2);
    assert_eq!(history_count(root.path(), another), 0);
    let before_other = stored(root.path(), another);
    let insufficient = request(
        &service,
        "exchange/star_crumb",
        &star_request(another, 9000001, 1),
    );
    assert!(insufficient.contains("not_enough_star_crumb"));
    assert_eq!(stored(root.path(), another), before_other);
    let conflict = request(
        &service,
        "exchange/star_crumb",
        &star_request(viewer, 9000002, 1),
    );
    assert!(conflict.contains("exchange_request_conflict"), "{conflict}");
    let saved = stored(root.path(), viewer);
    service.stop().unwrap();
    let restarted = PersonalService::start(root.path(), 0).unwrap();
    for ((route, body), response) in requests.iter().zip(responses) {
        assert_eq!(send(&restarted, route, body), response);
    }
    assert_eq!(stored(root.path(), viewer), saved);
    assert_eq!(history_count(root.path(), viewer), 2);
    let loaded = send(
        &restarted,
        "load",
        &json!({"viewer_id":viewer,"keychain":viewer}),
    );
    assert!(loaded.get("exchange_request_receipts").is_none());
    restarted.stop().unwrap();
}
// //// /两种兑换的历史写入失败回滚并在重启后重放原响应 ////
