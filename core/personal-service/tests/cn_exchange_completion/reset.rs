// audience: internal
// # personal-service-cn-exchange-reset-tests
// 管理操作核对活动轮次, 领取历史和客户端入口; 旧轮次资产随存档保留.

use super::*;

fn manage(service: &PersonalService, method: &str, route: &str, body: Value) -> Value {
    let token = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        method,
        route,
        "application/json",
        &[("Authorization", &token)],
        body.to_string().as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn start(root: &Path, cdn: &Path) -> PersonalService {
    std::fs::write(cdn.join("activity-catalog.json"), json!({"format_version":1,"activities":[
        {"activity_id":"special-exchange:11","name":"自选","kind":"special-exchange"},
        {"activity_id":"start-dash-exchange:1","name":"新手自选","kind":"start-dash-exchange"},
        {"activity_id":"treasure-vault-exchange:1","name":"星见宝库","kind":"treasure-vault-exchange"}
    ]}).to_string()).unwrap();
    PersonalService::start_with_cdn_root(root, 0, cdn).unwrap()
}

fn activity_path(service: &PersonalService, activity: &str, action: &str) -> String {
    let saves = manage(service, "GET", "/v1/local-saves", json!({}));
    format!(
        "/v1/local-saves/{}/activities/{activity}/{action}",
        saves["slots"][0]["id"]
    )
}

fn history(service: &PersonalService, activity: &str) -> Value {
    manage(
        service,
        "GET",
        &activity_path(service, activity, "history"),
        json!({}),
    )
}

fn reset(service: &PersonalService, activity: &str) {
    manage(
        service,
        "POST",
        &activity_path(service, activity, "reset"),
        json!({"expected_round":0}),
    );
}

// //// 自选重置保留资产和旧记录并允许新轮次再次购买兑换 [@x380kkm 2026-09-23] ////
#[test]
fn special_exchange_reset_keeps_assets_and_records_both_rounds() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    set_time(&service, 1717560000);
    let viewer = signup(&service);
    update(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(3000)
    });
    let target = catalog()["special"]["11"]["candidates"][0].clone();
    let buy = json!({"viewer_id":viewer,"campaign_id":11,"api_count":1});
    let exchange = json!({"viewer_id":viewer,"campaign_id":11,"character_id":target,"api_count":2});
    send(&service, "special_exchange/buy_ticket", &buy);
    send(&service, "special_exchange/exchange_character", &exchange);
    let before = stored(root.path());
    let old = history(&service, "special-exchange:11");
    assert!(old["can_reset"].as_bool().unwrap());
    assert!(old["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 0));
    reset(&service, "special-exchange:11");
    assert_eq!(
        stored(root.path())["user_character_list"],
        before["user_character_list"]
    );
    assert_eq!(stored(root.path())["item_list"], before["item_list"]);
    assert_eq!(
        send(&service, "special_exchange/buy_ticket", &buy)["user_info"]["vmoney"],
        0
    );
    let reward = send(&service, "special_exchange/exchange_character", &exchange);
    assert_eq!(reward["special_exchange_campaign_list"][0]["status"], 3);
    let history = history(&service, "special-exchange:11");
    assert_eq!(history["participation_round"], 1);
    assert!(
        history["entries"].as_array().unwrap().len() > old["entries"].as_array().unwrap().len()
    );
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 0));
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 1 && entry["kind"] != "reset"));
    service.stop().unwrap();
}
// //// /自选重置保留资产和旧记录并允许新轮次再次购买兑换 ////

// //// 新手管理开关只过滤入口并以重置时间重建个人窗口 [@x380kkm 2026-09-23] ////
#[test]
fn start_dash_management_preserves_qualification_and_renews_explicitly() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    set_time(&service, 1717560000);
    let viewer = signup(&service);
    update(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(3000)
    });
    let first = load(&service, viewer)["start_dash_exchange_campaign_list"][0].clone();
    let gacha = first["gacha_id"].as_i64().unwrap();
    let target = catalog()["gachas"][gacha.to_string()]["candidates"][0].clone();
    let buy = json!({"viewer_id":viewer,"campaign_id":1,"api_count":1});
    let exchange = json!({"viewer_id":viewer,"campaign_id":1,"character_id":target,"api_count":2});
    send(&service, "start_dash_exchange/buy_ticket", &buy);
    send(
        &service,
        "start_dash_exchange/exchange_character",
        &exchange,
    );
    manage(
        &service,
        "POST",
        "/v1/activities/start-dash-exchange:1/close",
        json!({}),
    );
    assert_eq!(
        load(&service, viewer)["start_dash_exchange_campaign_list"],
        json!([])
    );
    assert_eq!(
        stored(root.path())["start_dash_exchange_campaign_list"][0]["status"],
        3
    );
    manage(
        &service,
        "PUT",
        "/v1/activities/start-dash-exchange:1/mode",
        json!({"mode":"always"}),
    );
    set_time(&service, 1717560000 + 20 * 86400);
    reset(&service, "start-dash-exchange:1");
    let reopened = load(&service, viewer)["start_dash_exchange_campaign_list"][0].clone();
    assert!(reopened["period_start_time"].as_i64().unwrap() >= 1717560000 + 20 * 86400);
    assert_eq!(
        reopened["period_end_time"].as_i64().unwrap()
            - reopened["period_start_time"].as_i64().unwrap(),
        7 * 86400
    );
    send(&service, "start_dash_exchange/buy_ticket", &buy);
    let gacha = reopened["gacha_id"].as_i64().unwrap();
    let target = catalog()["gachas"][gacha.to_string()]["candidates"][0].clone();
    send(
        &service,
        "start_dash_exchange/exchange_character",
        &json!({"viewer_id":viewer,"campaign_id":1,"character_id":target,"api_count":2}),
    );
    let history = history(&service, "start-dash-exchange:1");
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 0));
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 1 && entry["kind"] != "reset"));
    service.stop().unwrap();
}
// //// /新手管理开关只过滤入口并以重置时间重建个人窗口 ////

// //// 宝库重置保留装备与剩余券并分开记录后续兑换 [@x380kkm 2026-09-23] ////
#[test]
fn treasure_reset_keeps_ticket_costs_and_prior_rewards() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    let service = start(root.path(), cdn.path());
    set_time(&service, 1717560000);
    let viewer = signup(&service);
    update(root.path(), |data| data["item_list"]["70031"] = json!(2));
    let target = catalog()["treasure"]["1"]["candidates"][0].clone();
    let body = json!({"viewer_id":viewer,"campaign_id":1,"equipment_id":target,"api_count":1});
    send(
        &service,
        "treasure_vault_exchange/exchange_equipment",
        &body,
    );
    let before = stored(root.path());
    reset(&service, "treasure-vault-exchange:1");
    assert_eq!(
        stored(root.path())["user_equipment_list"],
        before["user_equipment_list"]
    );
    assert_eq!(count(&stored(root.path()), 70031), 1);
    let reward = send(
        &service,
        "treasure_vault_exchange/exchange_equipment",
        &body,
    );
    assert_eq!(reward["equipment_list"][0]["stack"], 9);
    assert_eq!(count(&stored(root.path()), 70031), 0);
    let history = history(&service, "treasure-vault-exchange:1");
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 0));
    assert!(history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["participation_round"] == 1 && entry["kind"] != "reset"));
    service.stop().unwrap();
}
// //// /宝库重置保留装备与剩余券并分开记录后续兑换 ////
