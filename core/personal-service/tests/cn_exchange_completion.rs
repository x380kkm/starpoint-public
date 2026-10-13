// audience: internal
// # personal-service-cn-exchange-tests
// 隔离存档覆盖全部自选活动目录, 真实资产消耗, 候选限制和失败后的重试.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "cn_exchange_completion/reset.rs"]
mod reset;
#[path = "cn_exchange_completion/overflow.rs"]
mod overflow;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

fn request(service: &PersonalService, route: &str, body: &Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(body),
    )
}

fn send(service: &PersonalService, route: &str, body: &Value) -> Value {
    let response = decode_response::<Value>(&request(service, route, body));
    assert_eq!(
        response.data_headers.result_code, 1,
        "{route}: {}",
        response.data
    );
    response.data
}

fn denied(service: &PersonalService, route: &str, body: &Value) {
    let response = decode_response::<Value>(&request(service, route, body));
    assert_ne!(
        response.data_headers.result_code, 1,
        "{route}: {}",
        response.data
    );
}

fn signup(service: &PersonalService) -> i64 {
    decode_response::<SignupData>(&request(
        service,
        "tool/signup",
        &json!({"device_id":923110}),
    ))
    .data_headers
    .viewer_id
}

fn db(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn stored(root: &Path) -> Value {
    let text: String = db(root)
        .query_row(
            "SELECT data_json FROM player_snapshots LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn update(root: &Path, change: impl FnOnce(&mut Value)) {
    let mut data = stored(root);
    change(&mut data);
    db(root)
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [data.to_string()],
        )
        .unwrap();
}

fn set_time(service: &PersonalService, seconds: i64) {
    let iso: String = Connection::open_in_memory()
        .unwrap()
        .query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1, 'unixepoch')",
            [seconds],
            |row| row.get(0),
        )
        .unwrap();
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", &token)],
        json!({"enabled":true,"iso":iso,"rate":1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

fn catalog() -> Value {
    serde_json::from_str(include_str!("../assets/cn-optional-exchange.json")).unwrap()
}

fn count(data: &Value, ticket: i64) -> i64 {
    data["item_list"][ticket.to_string()].as_i64().unwrap_or(0)
}

fn load(service: &PersonalService, viewer: i64) -> Value {
    send(
        service,
        "load",
        &json!({"viewer_id":viewer,"keychain":viewer}),
    )
}

// //// 全部普通自选活动消耗正确货币并限制各期候选和重复领取 [@x380kkm 2026-09-23] ////
#[test]
fn all_special_campaigns_deliver_their_ticket_and_selected_reward_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    update(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(100000)
    });
    for (id, campaign) in catalog()["special"].as_object().unwrap() {
        let id: i64 = id.parse().unwrap();
        set_time(&service, campaign["start_at"].as_i64().unwrap() + 60);
        let ticket = campaign["ticket_id"].as_i64().unwrap();
        let target = campaign["candidates"][0].as_i64().unwrap();
        let price = campaign["price"].as_i64().unwrap();
        let before = stored(root.path());
        let route = if campaign["product_source"] == 1 {
            "special_exchange/enter_campaign"
        } else {
            "special_exchange/buy_ticket"
        };
        let buy = json!({"viewer_id":viewer,"campaign_id":id,"api_count":id*10});
        let response = send(&service, route, &buy);
        assert_eq!(response["special_exchange_campaign_list"][0]["status"], 2);
        assert_eq!(count(&response, ticket), count(&before, ticket) + 1);
        assert_eq!(
            stored(root.path())["user_info"]["vmoney"].as_i64().unwrap(),
            before["user_info"]["vmoney"].as_i64().unwrap() - price
        );
        if let Some(gacha_ticket) = campaign["gacha_ticket_id"].as_i64() {
            assert_eq!(
                count(&response, gacha_ticket),
                count(&before, gacha_ticket) + 1
            );
        }
        assert_eq!(send(&service, route, &buy), response);
        if id == 1 {
            set_time(&service, campaign["end_at"].as_i64().unwrap() + 60);
        }
        let (action, target_field, inventory) = if campaign["kind"] == 0 {
            ("exchange_character", "character_id", "user_character_list")
        } else {
            ("exchange_equipment", "equipment_id", "user_equipment_list")
        };
        let route = format!("special_exchange/{action}");
        let mut exchange = json!({"viewer_id":viewer,"campaign_id":id,"api_count":id*10+1});
        exchange[target_field] = json!(2147483647);
        let before_exchange = stored(root.path());
        denied(&service, &route, &exchange);
        assert_eq!(stored(root.path()), before_exchange);
        exchange[target_field] = json!(target);
        let response = send(&service, &route, &exchange);
        assert_eq!(response["special_exchange_campaign_list"][0]["status"], 3);
        assert_eq!(count(&response, ticket), count(&before, ticket));
        let settled = stored(root.path());
        assert!(settled[inventory][target.to_string()].is_object());
        let previous = &before_exchange[inventory][target.to_string()];
        let expected_stack = if previous.is_object() {
            previous["stack"].as_i64().unwrap() + 1
        } else {
            0
        };
        assert_eq!(
            settled[inventory][target.to_string()]["stack"],
            expected_stack
        );
        assert_eq!(send(&service, &route, &exchange), response);
        assert_eq!(stored(root.path()), settled);
        exchange["api_count"] = json!(id * 10 + 2);
        denied(&service, &route, &exchange);
        assert_eq!(stored(root.path()), settled);
    }
    let total: i64 = db(root.path()).query_row("SELECT COUNT(*) FROM player_receive_history WHERE event_key LIKE 'optional-exchange:%'", [], |row| row.get(0)).unwrap();
    assert!(total >= 64);
    service.stop().unwrap();
}
// //// /全部普通自选活动消耗正确货币并限制各期候选和重复领取 ////

// //// 宝库每券发放五件装备且请求重放保持库存和记录 [@x380kkm 2026-09-23] ////
#[test]
fn treasure_vault_grants_five_equipment_per_ticket_across_campaigns() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    for (id, campaign) in catalog()["treasure"].as_object().unwrap() {
        let id: i64 = id.parse().unwrap();
        let ticket = campaign["ticket_id"].as_i64().unwrap();
        let target = campaign["candidates"][0].as_i64().unwrap();
        update(root.path(), |data| {
            data["item_list"][ticket.to_string()] = json!(2);
            data["user_equipment_list"]
                .as_object_mut()
                .unwrap()
                .remove(&target.to_string());
        });
        let mut body =
            json!({"viewer_id":viewer,"campaign_id":id,"equipment_id":target,"api_count":id*100});
        set_time(&service, campaign["start_at"].as_i64().unwrap() - 10);
        denied(
            &service,
            "treasure_vault_exchange/exchange_equipment",
            &body,
        );
        set_time(&service, campaign["start_at"].as_i64().unwrap() + 60);
        let response = send(
            &service,
            "treasure_vault_exchange/exchange_equipment",
            &body,
        );
        assert_eq!(response["equipment_list"][0]["stack"], 4);
        assert_eq!(count(&response, ticket), 1);
        assert_eq!(
            send(
                &service,
                "treasure_vault_exchange/exchange_equipment",
                &body
            ),
            response
        );
        body["equipment_id"] = campaign["candidates"][1].clone();
        denied(
            &service,
            "treasure_vault_exchange/exchange_equipment",
            &body,
        );
        body["equipment_id"] = json!(target);
        body["api_count"] = json!(id * 100 + 1);
        let response = send(
            &service,
            "treasure_vault_exchange/exchange_equipment",
            &body,
        );
        assert_eq!(response["equipment_list"][0]["stack"], 9);
        assert_eq!(count(&response, ticket), 0);
        body["api_count"] = json!(id * 100 + 2);
        denied(
            &service,
            "treasure_vault_exchange/exchange_equipment",
            &body,
        );
    }
    let count: i64 = db(root.path())
        .query_row(
            "SELECT SUM(number) FROM player_receive_history WHERE kind=6",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 20);
    service.stop().unwrap();
}
// //// /宝库每券发放五件装备且请求重放保持库存和记录 ////

// //// 新手资格沿用游戏创建时间和固定卡池并跨重启恢复 [@x380kkm 2026-09-23] ////
#[test]
fn start_dash_uses_persisted_game_time_pool_and_entitlement() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let creation = 1658376060;
    set_time(&service, creation);
    let viewer = signup(&service);
    update(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(1500)
    });
    let loaded = load(&service, viewer);
    let campaign = loaded["start_dash_exchange_campaign_list"][0].clone();
    assert_eq!(
        campaign["period_start_time"],
        stored(root.path())["user_character_list"]["1"]["join_time"]
    );
    assert_eq!(
        campaign["period_end_time"].as_i64().unwrap()
            - campaign["period_start_time"].as_i64().unwrap(),
        7 * 86400
    );
    let gacha = campaign["gacha_id"].as_i64().unwrap();
    let target = catalog()["gachas"][gacha.to_string()]["candidates"][0].clone();
    let buy = json!({"viewer_id":viewer,"campaign_id":1,"api_count":1});
    let response = send(&service, "start_dash_exchange/buy_ticket", &buy);
    assert_eq!(response["user_info"]["vmoney"], 0);
    assert_eq!(count(&response, 990007), 1);
    assert_eq!(count(&response, 999001), 1);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(
        send(&service, "start_dash_exchange/buy_ticket", &buy),
        response
    );
    let limited = json!({"viewer_id":viewer,"campaign_id":1,"character_id":151015,"api_count":2});
    let before = stored(root.path());
    denied(&service, "start_dash_exchange/exchange_character", &limited);
    assert_eq!(stored(root.path()), before);
    let body = json!({"viewer_id":viewer,"campaign_id":1,"character_id":target,"api_count":2});
    let response = send(&service, "start_dash_exchange/exchange_character", &body);
    assert_eq!(
        response["start_dash_exchange_campaign_list"][0]["gacha_id"],
        gacha
    );
    assert_eq!(
        response["start_dash_exchange_campaign_list"][0]["status"],
        3
    );
    assert_eq!(count(&response, 990007), 0);
    set_time(&service, creation + 20 * 86400);
    assert_eq!(
        send(&service, "start_dash_exchange/exchange_character", &body),
        response
    );
    let loaded = load(&service, viewer);
    assert_eq!(
        loaded["start_dash_exchange_campaign_list"][0]["period_end_time"],
        campaign["period_end_time"]
    );
    assert_eq!(loaded["start_dash_exchange_campaign_list"][0]["status"], 3);
    service.stop().unwrap();
}
// //// /新手资格沿用游戏创建时间和固定卡池并跨重启恢复 ////

// //// 过期和缺失游戏锚点的旧存档保持原资格边界 [@x380kkm 2026-09-23] ////
#[test]
fn start_dash_does_not_renew_old_or_unknown_registration() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, 1717560000);
    let viewer = signup(&service);
    set_time(&service, 1717560000 + 20 * 86400);
    let loaded = load(&service, viewer);
    assert!(
        loaded["start_dash_exchange_campaign_list"][0]["period_end_time"]
            .as_i64()
            .unwrap()
            < 1717560000 + 20 * 86400
    );
    denied(
        &service,
        "start_dash_exchange/buy_ticket",
        &json!({"viewer_id":viewer,"campaign_id":1,"api_count":1}),
    );
    update(root.path(), |data| {
        data["start_dash_exchange_campaign_list"] = json!([]);
        data["user_character_list"]["1"]["join_time"] = Value::Null;
    });
    assert_eq!(
        load(&service, viewer)["start_dash_exchange_campaign_list"],
        json!([])
    );
    denied(
        &service,
        "start_dash_exchange/buy_ticket",
        &json!({"viewer_id":viewer,"campaign_id":1,"api_count":2}),
    );
    service.stop().unwrap();
}
// //// /过期和缺失游戏锚点的旧存档保持原资格边界 ////

// //// 领取历史写入失败时回滚费用和资格并允许原请求重试 [@x380kkm 2026-09-23] ////
#[test]
fn exchange_failure_rolls_back_inventory_and_receipt_before_retry() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service);
    set_time(&service, 1717560000);
    update(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(1500)
    });
    let before = stored(root.path());
    db(root.path()).execute_batch("CREATE TRIGGER reject_exchange_history BEFORE INSERT ON player_receive_history BEGIN SELECT RAISE(ABORT, 'history unavailable'); END;").unwrap();
    let buy = json!({"viewer_id":viewer,"campaign_id":11,"api_count":1});
    assert!(request(&service, "special_exchange/buy_ticket", &buy).starts_with("HTTP/1.1 500"));
    assert_eq!(stored(root.path()), before);
    db(root.path())
        .execute_batch("DROP TRIGGER reject_exchange_history;")
        .unwrap();
    let response = send(&service, "special_exchange/buy_ticket", &buy);
    assert_eq!(response["user_info"]["vmoney"], 0);
    let settled = stored(root.path());
    assert_eq!(
        send(&service, "special_exchange/buy_ticket", &buy),
        response
    );
    assert_eq!(stored(root.path()), settled);
    service.stop().unwrap();
}
// //// /领取历史写入失败时回滚费用和资格并允许原请求重试 ////
