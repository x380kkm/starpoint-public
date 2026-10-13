// audience: internal
// # personal-service-cn-equipment-overflow-tests
// 正式抽取和兑换在容量边界保存装备状态, 转换物品与邮件, 请求重放复用同一结算.

use super::overflow::{mails, save};
use super::*;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::Path;

// //// 创建具有装备积分和抽取货币的账号 [@x380kkm 2026-09-26] ////
fn prepare(root: &Path) -> (PersonalService, i64) {
    let service = PersonalService::start(root, 0).unwrap();
    set_virtual_time(&service, "2025-01-02T12:00:00.000Z");
    open_gacha(&service, 1);
    open_gacha(&service, 3);
    let viewer = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 814 }),
    ))
    .data_headers
    .viewer_id;
    let mut player = replay::stored_state(root).0;
    player["gacha_info_list"] = json!([{"gacha_id":1,"gacha_exchange_point":1000}]);
    player["user_info"]["free_vmoney"] = json!(3000);
    player["user_info"]["vmoney"] = json!(50);
    player["user_equipment_list"]["5030037"] =
        json!({"stack":999,"level":5,"enhancement_level":2,"protection":true});
    player["item_list"]["100000"] = json!(999997);
    player["item_list"]["5030037"] = json!(9999);
    save(root, &player);
    (service, viewer)
}
// //// /创建具有装备积分和抽取货币的账号 ////

// //// 积分兑换与邮件投递原子提交且跨重启重放只结算一次 [@x380kkm 2026-09-26] ////
#[test]
fn equipment_overflow_preserves_exchange_mail_and_replay() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let request = encode_request(
        &json!({"viewer_id":viewer,"api_count":1,"gacha_id":1,"equipment_id":5030037}),
    );
    let endpoint = "/api/index.php/gacha/exchange_equipment";
    let before = replay::stored_state(root.path());
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_equipment_mail BEFORE INSERT ON player_mails BEGIN SELECT RAISE(ABORT,'write unavailable'); END;").unwrap();
    let failed = cn_support::send_request(service.port(), endpoint, &request);
    assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
    assert_eq!(replay::stored_state(root.path()), before);
    assert!(mails(root.path()).is_empty());
    db.execute_batch("DROP TRIGGER reject_equipment_mail")
        .unwrap();

    let response = cn_support::send_request(service.port(), endpoint, &request);
    let result = decode_response::<Value>(&response).data;
    assert_eq!(result["equipment_list"][0]["stack"], 999);
    assert_eq!(result["equipment_list"][0]["protection"], true);
    assert_eq!(result["equipment_list"][0]["level"], 5);
    assert_eq!(result["equipment_list"][0]["enhancement_level"], 2);
    assert_eq!(result["gacha_info_list"][0]["gacha_exchange_point"], 750);
    assert_eq!(result["item_list"]["100000"], 999999);
    assert_eq!(result["item_list"]["5030037"], 9999);
    assert_eq!(
        result["over_max"][0],
        json!({"process_type":6,"equipment":{"equipment_id":5030037,"number":1},"converted_craft_point":5,"converted_ability_soul_id":5030037})
    );
    assert_eq!(result["mail_arrived"], true);
    let delivered = mails(root.path());
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0]["itemList"], json!({"100000":3,"5030037":1}));
    let recorded: i64 = db.query_row("SELECT SUM(number) FROM player_receive_history WHERE subject='装备转换奖励' AND kind=1", [], |row| row.get(0)).unwrap();
    assert_eq!(recorded, 2);
    let settled = replay::stored_state(root.path());
    assert!(settled.0.get("reward_deliveries").is_none());
    assert_eq!(
        cn_support::send_request(service.port(), endpoint, &request),
        response
    );
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(
        cn_support::send_request(service.port(), endpoint, &request),
        response
    );
    assert_eq!(replay::stored_state(root.path()), settled);
    assert_eq!(mails(root.path()), delivered);

    let mut player = settled.0;
    player["item_list"]["100000"] = json!(999990);
    player["item_list"]["5030037"] = json!(9998);
    save(root.path(), &player);
    let request = encode_request(&json!({"viewer_id":viewer}));
    let claimed = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mail/receive_all",
        &request,
    ))
    .data;
    assert_eq!(claimed["item_list"]["100000"], 999993);
    assert_eq!(claimed["item_list"]["5030037"], 9999);
    let settled = replay::stored_state(root.path());
    cn_support::send_request(service.port(), "/api/index.php/mail/receive_all", &request);
    assert_eq!(replay::stored_state(root.path()), settled);
    service.stop().unwrap();
}
// //// /积分兑换与邮件投递原子提交且跨重启重放只结算一次 ////

// //// 装备十连和每日抽取按实际结果完整投递每份转换物品 [@x380kkm 2026-09-26] ////
#[test]
fn equipment_overflow_preserves_bulk_and_daily_draw_quantities() {
    let definitions: Value =
        serde_json::from_str(include_str!("../../../../assets/equipment_dissolve.json")).unwrap();
    for (payment_type, draw_type, pulls, free_cost, paid_cost) in
        [(1, 2, 10, 750, 0), (2, 5, 1, 0, 25)]
    {
        let root = TempDir::new().unwrap();
        let (service, viewer) = prepare(root.path());
        let mut player = replay::stored_state(root.path()).0;
        for (id, definition) in definitions.as_object().unwrap() {
            player["user_equipment_list"][id] =
                json!({"stack":999,"level":1,"enhancement_level":0,"protection":false});
            let soul_id = definition["ability_soul_id"].as_i64().unwrap().to_string();
            player["item_list"][soul_id] = definition["ability_soul_max_count"].clone();
        }
        player["item_list"]["100000"] = json!(999999);
        player["item_list"]["990008"] = json!(999999);
        save(root.path(), &player);
        let request = encode_request(
            &json!({"viewer_id":viewer,"api_count":10,"gacha_id":3,"payment_type":payment_type,"type":draw_type,"number_of_exec":1}),
        );
        let response =
            cn_support::send_request(service.port(), "/api/index.php/gacha/exec", &request);
        let decoded = decode_response::<Value>(&response);
        assert_eq!(decoded.data_headers.result_code, 1, "{}", decoded.data);
        let result = decoded.data;
        assert_eq!(result["draw_equipment"].as_array().unwrap().len(), pulls);
        assert_eq!(result["user_info"]["free_vmoney"], 3000 - free_cost);
        assert_eq!(result["user_info"]["vmoney"], 50 - paid_cost);
        assert!(result["equipment_list"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["stack"] == 999));
        let mut expected = BTreeMap::<String, i64>::new();
        for draw in result["draw_equipment"].as_array().unwrap() {
            let id = draw["equipment_id"].as_i64().unwrap().to_string();
            let definition = &definitions[&id];
            for (item, amount) in [
                (
                    "100000".to_owned(),
                    definition["craft_points"].as_i64().unwrap(),
                ),
                (
                    "990008".to_owned(),
                    definition["star_grains"].as_i64().unwrap(),
                ),
                (
                    definition["ability_soul_id"].as_i64().unwrap().to_string(),
                    i64::from(definition["generate_ability_soul"].as_bool().unwrap()),
                ),
            ] {
                if amount > 0 {
                    *expected.entry(item).or_default() += amount;
                }
            }
        }
        let delivered = mails(root.path());
        let mut actual = BTreeMap::<String, i64>::new();
        for mail in &delivered {
            for (id, amount) in mail["itemList"].as_object().unwrap() {
                *actual.entry(id.clone()).or_default() += amount.as_i64().unwrap();
            }
        }
        assert_eq!(actual, expected);
        assert_eq!(
            result["over_max"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["process_type"] == 6)
                .count(),
            pulls
        );
        let settled = replay::stored_state(root.path());
        assert_eq!(
            cn_support::send_request(service.port(), "/api/index.php/gacha/exec", &request),
            response
        );
        assert_eq!(replay::stored_state(root.path()), settled);
        assert_eq!(mails(root.path()), delivered);
        service.stop().unwrap();
    }
}
// //// /装备十连和每日抽取按实际结果完整投递每份转换物品 ////
