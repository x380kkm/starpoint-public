// audience: internal
// # personal-service-cn-payment-subscription-tests
//
// 客户端支付与载入请求验证日领奖励, 资源消费, 续购和补领邮件的持久化.

use super::*;

fn product_for(subscription_id: &str) -> String {
    let catalog: Value = serde_json::from_str(include_str!("../../assets/cn-payment-catalog.json")).unwrap();
    catalog["products"].as_object().unwrap().iter().find(|(_, product)| {
        product["direct_purchase"] == true && product["entitlements"].as_array().unwrap().iter()
            .any(|entitlement| entitlement["entitlement"] == "subscription" && entitlement["id"] == subscription_id)
    }).unwrap().0.clone()
}

fn load(service: &PersonalService, viewer: i64) -> Value {
    decode_response::<Value>(&request(service, "load", json!({"viewer_id":viewer,"keychain":viewer}))).data
}

fn points(player: &Value, id: i64) -> i64 {
    player["user_daily_challenge_point_list"].as_array().unwrap().iter().find(|entry| entry["id"] == id).unwrap()["point"].as_i64().unwrap()
}

// //// 保留午夜前已消费资源并在五点领取下一日奖励 [@x380kkm 2026-09-22] ////
#[test]
fn keeps_spent_subscription_resources_until_five_and_extends_on_purchase() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2026-09-22T12:00:00.000Z");
    let viewer = signup(&service);
    load(&service, viewer);
    let product = product_for("month_card");
    let initial = snapshot(root.path());
    let stones = initial["user_info"]["free_vmoney"].as_i64().unwrap();
    start(&service, viewer, 1, &product);
    let purchased = snapshot(root.path());
    assert_eq!(purchased["user_info"]["month_card_remain_days"], 30);
    assert_eq!(purchased["monthly_charge_bonus_info"]["bonus_days"], 1);
    assert_eq!(purchased["user_info"]["free_vmoney"], stones + 100);
    assert_eq!(purchased["user_info"]["boost_point"], initial["user_info"]["boost_point"].as_i64().unwrap() + 1);
    start(&service, viewer, 1, &product);
    assert_eq!(snapshot(root.path()), purchased);
    start(&service, viewer, 2, &product);
    assert_eq!(snapshot(root.path())["user_info"]["month_card_remain_days"], 60);
    assert_eq!(snapshot(root.path())["user_info"]["free_vmoney"], stones + 100);
    update_snapshot(root.path(), |player| {
        player["user_info"]["boost_point"] = json!(0);
        player["user_info"]["boss_boost_point"] = json!(0);
    });
    set_time(&service, "2026-09-22T16:01:00.000Z");
    let midnight = load(&service, viewer);
    assert_eq!(midnight["user_info"]["boost_point"], 0);
    assert_eq!(midnight["user_info"]["boss_boost_point"], 0);
    assert_eq!(midnight["user_info"]["free_vmoney"], stones + 100);
    set_time(&service, "2026-09-22T21:00:01.000Z");
    let morning = load(&service, viewer);
    assert_eq!(morning["user_info"]["boost_point"], 4);
    assert_eq!(morning["user_info"]["boss_boost_point"], 4);
    assert_eq!(morning["user_info"]["free_vmoney"], stones + 200);
    assert_eq!(morning["monthly_charge_bonus_info"]["bonus_days"], 2);
    assert_eq!(morning["user_info"]["month_card_remain_days"], 59);
    assert!(morning.get("payment_subscription_state").is_none());
    let history = database(root.path()).query_row::<i64, _, _>("SELECT COUNT(*) FROM player_receive_history", [], |row| row.get(0)).unwrap();
    load(&service, viewer);
    assert_eq!(database(root.path()).query_row::<i64, _, _>("SELECT COUNT(*) FROM player_receive_history", [], |row| row.get(0)).unwrap(), history);
    set_time(&service, "2026-09-22T12:00:00.000Z");
    assert_eq!(load(&service, viewer)["user_info"]["free_vmoney"], stones + 200);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(load(&service, viewer)["user_info"]["free_vmoney"], stones + 200);
    service.stop().unwrap();
}
// //// /保留午夜前已消费资源并在五点领取下一日奖励 ////

// //// 将到期漏领按期邮寄一次并保持回拨后的到期状态 [@x380kkm 2026-09-22] ////
#[test]
fn mails_expired_month_rewards_once_and_prevents_clock_replay() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2026-01-01T12:00:00.000Z");
    let viewer = signup(&service);
    let initial = load(&service, viewer);
    let stones = initial["user_info"]["free_vmoney"].as_i64().unwrap();
    let potions = initial["item_list"]["102"].as_i64().unwrap_or_default();
    start(&service, viewer, 1, &product_for("month_card"));
    set_time(&service, "2026-01-04T12:00:00.000Z");
    assert_eq!(load(&service, viewer)["user_info"]["free_vmoney"], stones + 200);
    set_time(&service, "2026-01-31T12:00:00.000Z");
    let before_expiry = snapshot(root.path());
    database(root.path()).execute_batch("CREATE TRIGGER reject_subscription_mail BEFORE INSERT ON player_mails WHEN NEW.title='每日奖励补领' BEGIN SELECT RAISE(ABORT,'subscription mail failure'); END;").unwrap();
    assert!(request(&service, "load", json!({"viewer_id":viewer,"keychain":viewer})).starts_with("HTTP/1.1 500"));
    assert_eq!(snapshot(root.path()), before_expiry);
    database(root.path()).execute_batch("DROP TRIGGER reject_subscription_mail;").unwrap();
    let expired = load(&service, viewer);
    assert_eq!(expired["user_info"]["month_card_remain_days"], 0);
    assert_eq!(expired["user_info"]["free_vmoney"], stones + 200);
    let mailed = || database(root.path()).query_row::<i64, _, _>("SELECT COUNT(*) FROM player_mails WHERE title='每日奖励补领'", [], |row| row.get(0)).unwrap();
    assert_eq!(mailed(), 2);
    load(&service, viewer);
    assert_eq!(mailed(), 2);
    set_time(&service, "2026-01-02T12:00:00.000Z");
    let rolled_back = load(&service, viewer);
    assert_eq!(rolled_back["user_info"]["month_card_remain_days"], 0);
    assert_eq!(rolled_back["user_info"]["free_vmoney"], stones + 200);
    decode_response::<Value>(&request(&service, "mail/receive_all", json!({"viewer_id":viewer})));
    let claimed = snapshot(root.path());
    assert_eq!(claimed["user_info"]["free_vmoney"], stones + 3000);
    assert_eq!(claimed["item_list"]["102"], potions + 30);
    decode_response::<Value>(&request(&service, "mail/receive_all", json!({"viewer_id":viewer})));
    assert_eq!(snapshot(root.path())["user_info"]["free_vmoney"], stones + 3000);
    assert_eq!(mailed(), 2);
    service.stop().unwrap();
}
// //// /将到期漏领按期邮寄一次并保持回拨后的到期状态 ////

// //// 按单人挑战资源发放周卡权益并保持其他挑战类型 [@x380kkm 2026-09-22] ////
#[test]
fn grants_weekly_expert_challenge_without_refilling_other_modes() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2026-02-01T12:00:00.000Z");
    let viewer = signup(&service);
    let initial = load(&service, viewer);
    let product = product_for("weekly_bonus2");
    start(&service, viewer, 1, &product);
    let purchased = snapshot(root.path());
    assert_eq!(points(&purchased, 1), points(&initial, 1) + 1);
    for id in [251, 5001, 10008] { assert_eq!(points(&purchased, id), points(&initial, id)); }
    assert_eq!(purchased["user_info"]["renewal_gift_remain_days"][&product], 7);
    update_snapshot(root.path(), |player| {
        player["user_daily_challenge_point_list"].as_array_mut().unwrap().iter_mut()
            .find(|entry| entry["id"] == 1).unwrap()["point"] = json!(0);
    });
    set_time(&service, "2026-02-01T16:01:00.000Z");
    assert_eq!(points(&load(&service, viewer), 1), 0);
    update_snapshot(root.path(), |player| {
        player["user_daily_challenge_point_list"].as_array_mut().unwrap().iter_mut()
            .find(|entry| entry["id"] == 1).unwrap()["campaign_list"][0]["additional_point"] = json!(4);
    });
    set_time(&service, "2026-02-01T21:00:01.000Z");
    assert_eq!(points(&load(&service, viewer), 1), 5);
    set_time(&service, "2026-02-08T12:00:00.000Z");
    let expired = load(&service, viewer);
    assert_eq!(points(&expired, 1), 4);
    assert_eq!(expired["user_info"]["renewal_gift_remain_days"][&product], 0);
    service.stop().unwrap();
}
// //// /按单人挑战资源发放周卡权益并保持其他挑战类型 ////

// //// 累计未激活期的消费并在达到金额时发放小月卡 [@x380kkm 2026-09-22] ////
#[test]
fn activates_mini_month_card_at_cumulative_payment_and_excludes_active_period() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2026-03-01T12:00:00.000Z");
    let viewer = signup(&service);
    let initial = load(&service, viewer)["user_info"]["free_vmoney"].as_i64().unwrap();
    for count in 1..=4 {
        start(&service, viewer, count, "com.leiting.wf.stone_50");
        assert_eq!(snapshot(root.path())["user_info"]["free_vmoney"], initial);
    }
    assert_eq!(snapshot(root.path())["payment_subscription_state"]["payment_total"], 24);
    start(&service, viewer, 5, "com.leiting.wf.stone_50");
    assert_eq!(snapshot(root.path())["monthly_charge_bonus_info"]["bonus_days"], 1);
    assert_eq!(snapshot(root.path())["user_info"]["free_vmoney"], initial + 50);
    start(&service, viewer, 6, "com.leiting.wf.stone_50");
    assert_eq!(snapshot(root.path())["payment_subscription_state"]["payment_total"], 0);
    set_time(&service, "2026-03-31T12:00:00.000Z");
    start(&service, viewer, 7, "com.leiting.wf.stone_50");
    assert_eq!(snapshot(root.path())["payment_subscription_state"]["payment_total"], 6);
    assert_eq!(snapshot(root.path())["user_info"]["free_vmoney"], initial + 50);
    service.stop().unwrap();
}
// //// /累计未激活期的消费并在达到金额时发放小月卡 ////
