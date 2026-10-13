// audience: internal
// # personal-service-cn-item-selection-tests
// 自选箱按客户端序号结算素材, 批量请求及重试共享库存和邮件事务.

use super::*;
use std::collections::BTreeMap;

// //// 在同一批请求中覆盖目录中的全部素材选项 [@x380kkm 2026-09-28] ////
#[test]
fn grants_all_selection_choices_with_client_numeric_values() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let catalog: Value =
        serde_json::from_str(include_str!("../../assets/cn-item-selection.json")).unwrap();
    let mut player = snapshot(root.path());
    player["user_info"]["stamina"] = json!(999);
    let mut entries = Vec::new();
    let mut expected = BTreeMap::<String, i64>::new();
    for (id, item) in catalog.as_object().unwrap() {
        let choices = item["choices"].as_array().unwrap();
        player["item_list"][id] = json!(choices.len());
        expected.insert(id.clone(), 0);
        for (index, choice) in choices.iter().enumerate() {
            let reward_id = choice["item_id"].as_i64().unwrap().to_string();
            player["item_list"][&reward_id] = json!(0);
            *expected.entry(reward_id).or_default() += choice["amount"].as_i64().unwrap();
            entries.push(json!({"id":id.parse::<f64>().unwrap(),"number":1.0,"selectIndex":(index + 1) as f64}));
        }
    }
    save(root.path(), &player);
    let response = decode_response::<Value>(&send(
        &service,
        viewer,
        "item/use_item",
        json!({"api_count":1.0,"items":entries}),
    ));
    assert_eq!(response.data_headers.result_code, 1);
    assert_eq!(response.data["item_list"], json!(expected));
    assert_eq!(response.data["over_max"], json!([]));
    assert_eq!(response.data["mail_arrived"], false);
    let saved = snapshot(root.path());
    for (id, amount) in expected {
        assert_eq!(saved["item_list"][id], amount);
    }
    assert_eq!(saved["user_info"]["stamina"], 999);
    service.stop().unwrap();
}
// //// /在同一批请求中覆盖目录中的全部素材选项 ////

// //// 重启重放保留溢出邮件并拒绝更换选项 [@x380kkm 2026-09-28] ////
#[test]
fn replays_selection_and_overflow_mail_after_restart() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let mut player = snapshot(root.path());
    let capacities: Value =
        serde_json::from_str(include_str!("../../assets/cn-shop-capacities.json")).unwrap();
    let capacity = capacities["items"]["2"].as_i64().unwrap();
    player["item_list"]["999100"] = json!(2);
    player["item_list"]["2"] = json!(capacity - 10);
    save(root.path(), &player);
    let request = json!({"api_count":1,"items":[{"id":999100,"number":1,"selectIndex":1},{"id":999100,"number":1,"selectIndex":1}]});
    let response = decode_response::<Value>(&send(&service, viewer, "item/use_item", request));
    assert_eq!(response.data["item_list"]["999100"], 0);
    assert_eq!(response.data["item_list"]["2"], capacity);
    assert_eq!(
        response.data["over_max"],
        json!([{"process_type":1,"item":{"item_id":2,"number":590}}])
    );
    assert_eq!(response.data["mail_arrived"], true);
    let saved = snapshot(root.path());
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let mail_count: i64 = db
        .query_row("SELECT COUNT(*) FROM player_mails", [], |row| row.get(0))
        .unwrap();
    let mail: String = db
        .query_row(
            "SELECT rewards_json FROM player_mails ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&mail).unwrap()["itemList"]["2"],
        590
    );
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let replay = decode_response::<Value>(&send(
        &service,
        viewer,
        "item/use_item",
        json!({"api_count":1,"items":[{"id":999100,"number":2,"selectIndex":1}]}),
    ));
    assert_eq!(replay.data, response.data);
    assert_eq!(
        replay.data_headers.servertime,
        response.data_headers.servertime
    );
    assert_eq!(snapshot(root.path()), saved);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM player_mails", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        mail_count
    );
    let conflict = send(
        &service,
        viewer,
        "item/use_item",
        json!({"api_count":1,"items":[{"id":999100,"number":2,"selectIndex":2}]}),
    );
    assert!(conflict.contains("item_request_conflict"), "{conflict}");
    assert_eq!(snapshot(root.path()), saved);
    service.stop().unwrap();
}
// //// /重启重放保留溢出邮件并拒绝更换选项 ////

// //// 无效选项和写入失败保持整批资产与邮件不变 [@x380kkm 2026-09-28] ////
#[test]
fn rolls_back_selection_batches_and_retries_after_storage_failure() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let mut player = snapshot(root.path());
    player["item_list"]["999100"] = json!(3);
    save(root.path(), &player);
    for invalid in [
        json!({"id":999100,"number":1,"selectIndex":0}),
        json!({"id":999100,"number":1,"selectIndex":7}),
        json!({"id":999100,"number":1,"selectIndex":1.5}),
        json!({"id":999100,"number":3,"selectIndex":2}),
        json!({"id":999100,"number":-1,"selectIndex":1}),
        json!({"id":2,"number":1,"selectIndex":1}),
    ] {
        let failed = send(
            &service,
            viewer,
            "item/use_item",
            json!({"api_count":1,"items":[{"id":999100,"number":1,"selectIndex":1},invalid]}),
        );
        assert!(failed.starts_with("HTTP/1.1 400"), "{failed}");
        assert_eq!(snapshot(root.path()), player);
    }
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_item_snapshot BEFORE UPDATE ON player_snapshots BEGIN SELECT RAISE(ABORT, 'snapshot unavailable'); END;").unwrap();
    let request = json!({"api_count":1,"items":[{"id":999100,"number":1,"selectIndex":1},{"id":999100,"number":2,"selectIndex":2}]});
    let failed = send(&service, viewer, "item/use_item", request.clone());
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(snapshot(root.path()), player);
    db.execute_batch("DROP TRIGGER reject_item_snapshot")
        .unwrap();
    let response = decode_response::<Value>(&send(&service, viewer, "item/use_item", request));
    assert_eq!(response.data_headers.result_code, 1);
    assert_eq!(response.data["item_list"]["999100"], 0);
    service.stop().unwrap();
}
// //// /无效选项和写入失败保持整批资产与邮件不变 ////

// //// 使用期间限定体力道具并在到期后重放原请求 [@x380kkm 2026-09-28] ////
#[test]
fn uses_period_stamina_items_and_replays_after_expiration() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let set_time = |iso: &str| {
        let authorization = format!("Bearer {}", service.management_token());
        let response = support::request_with_headers(
            service.port(),
            "PUT",
            "/v1/time",
            "application/json",
            &[("Authorization", authorization.as_str())],
            json!({"enabled":true,"iso":iso,"rate":1})
                .to_string()
                .as_bytes(),
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    };
    for (index, id) in [
        10000013, 10000015, 10000022, 10000024, 10000028, 10000041, 10000050, 10000052, 10000080,
    ]
    .into_iter()
    .enumerate()
    {
        let iso = if [10000028, 10000041, 10000050, 10000052].contains(&id) {
            "2025-05-28T12:00:00.000Z"
        } else {
            "2025-03-30T12:00:00.000Z"
        };
        set_time(iso);
        let mut player = snapshot(root.path());
        player["user_info"]["stamina"] = json!(950);
        player["item_list"][id.to_string()] = json!(2);
        save(root.path(), &player);
        let body = json!({"api_count":index + 1,"items":[{"id":id,"number":1,"selectIndex":0}]});
        let used = decode_response::<Value>(&send(&service, viewer, "item/use_item", body.clone()));
        assert_eq!(used.data_headers.result_code, 1, "{id}");
        assert_eq!(used.data["user_info"]["stamina"], 975);
        assert_eq!(used.data["item_list"][id.to_string()], 1);
        let saved = snapshot(root.path());
        set_time("2026-09-28T12:00:00.000Z");
        let expired = decode_response::<Value>(&send(
            &service,
            viewer,
            "item/use_item",
            json!({"api_count":100 + index,"items":[{"id":id,"number":1,"selectIndex":0}]}),
        ));
        assert_eq!(expired.data_headers.result_code, 2057, "{id}");
        assert_eq!(snapshot(root.path()), saved);
        let replay = decode_response::<Value>(&send(&service, viewer, "item/use_item", body));
        assert_eq!(replay.data, used.data);
        assert_eq!(snapshot(root.path()), saved);
    }
    service.stop().unwrap();
}
// //// /使用期间限定体力道具并在到期后重放原请求 ////
