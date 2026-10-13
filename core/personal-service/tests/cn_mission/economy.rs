// audience: internal
// # personal-service-cn-mission-economy-api-tests
//
// 隔离存档使用实际领取, 扣费与抽取接口验证经济任务的进度, 资产与领取历史.

use super::*;

fn counter(root: &Path, pattern: &str) -> i64 {
    read_player(root)["mission_economic_counters"][pattern].as_i64().unwrap_or_default()
}

fn progress(service: &PersonalService, viewer_id: i64, category: i64, mission_id: i64) -> i64 {
    let data = decode_response::<Value>(&send(service, "/api/index.php/mission/get_mission_progress",
        json!({"viewer_id":viewer_id,"api_count":1,"category_list":[{"category":category}]}))).data;
    data["mission_progress_list"].as_array().unwrap().iter().find(|entry| entry["mission_id"] == mission_id)
        .unwrap()["progress_value"].as_i64().unwrap()
}

fn mission_history_amount(root: &Path, kind: i64) -> i64 {
    Connection::open(root.join("personal-service.sqlite3")).unwrap().query_row(
        "SELECT COALESCE(SUM(number),0) FROM player_receive_history
         WHERE kind=?1 AND (event_key LIKE 'mission-receipt:%' OR (event_key LIKE 'reward-conversion-%' AND subject='任务奖励'))",
        [kind], |row| row.get(0),
    ).unwrap()
}

// //// 按实收免费和付费玛纳累计任务并保留一次领取记录 [@x380kkm 2026-09-22] ////
#[test]
fn counts_mail_mana_and_records_task_rewards_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 922101);
    progress(&service, viewer_id, 1, 40);
    let initial = load(&service, viewer_id);
    let before = counter(root.path(), "mana_addition_count");
    let history_before = mission_history_amount(root.path(), 4);
    for (rewards, expected) in [(json!({"freeMana":10000}), 10000), (json!({"paidMana":20000}), 30000)] {
        let mail = manage(&service, "POST", "/v1/mails", json!({
            "viewer_id":viewer_id,"title":"玛纳","body":"领取玛纳.","sender":"Starpoint","rewards":rewards,
        }));
        let request = json!({"viewer_id":viewer_id,"mail_id":mail["id"]});
        decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", request.clone()));
        let repeated = decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", request));
        assert_eq!(repeated.data_headers.result_code, 2002);
        assert_eq!(counter(root.path(), "mana_addition_count"), before + expected);
        assert_eq!(progress(&service, viewer_id, 1, 40), before + expected);
    }
    let received = load(&service, viewer_id);
    assert_eq!(received["user_info"]["free_mana"].as_i64().unwrap(), initial["user_info"]["free_mana"].as_i64().unwrap() + 10000);
    assert_eq!(received["user_info"]["paid_mana"].as_i64().unwrap(), initial["user_info"]["paid_mana"].as_i64().unwrap() + 20000);
    assert_eq!(received["user_info"]["free_vmoney"].as_i64().unwrap(), initial["user_info"]["free_vmoney"].as_i64().unwrap() + 10);
    assert_eq!(mission_history_amount(root.path(), 4), history_before + 10);
    let stones = manage(&service, "POST", "/v1/mails", json!({
        "viewer_id":viewer_id,"title":"星导石","body":"领取星导石.","sender":"Starpoint","rewards":{"freeVmoney":300,"vmoney":100},
    }));
    decode_response::<Value>(&send(&service, "/api/index.php/mail/receive", json!({"viewer_id":viewer_id,"mail_id":stones["id"]})));
    assert_eq!(counter(root.path(), "mana_addition_count"), before + 30000);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(progress(&service, viewer_id, 1, 40), before + 30000);
    assert_eq!(mission_history_amount(root.path(), 4), history_before + 10);
    service.stop().unwrap();
}
// //// /按实收免费和付费玛纳累计任务并保留一次领取记录 ////

// //// 从混合玛纳扣费结算消耗任务并累计奖励产生的玛纳 [@x380kkm 2026-09-22] ////
#[test]
fn counts_node_spending_and_bonus_mana_once() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 922102);
    open_activity(&service, "collect-item:10003");
    update_player(root.path(), |player| {
        player["user_info"]["free_mana"] = json!(100);
        player["user_info"]["paid_mana"] = json!(2000);
        for id in ["1", "2", "3", "4", "99"] { player["item_list"][id] = json!(1000); }
        player["item_list"]["70047"] = json!(30);
    });
    let before = counter(root.path(), "mana_addition_count");
    let request = json!({"viewer_id":viewer_id,"character_id":1,"api_count":1,"mana_node_multiplied_id_list":(2201..=2207).collect::<Vec<_>>()});
    let learned = decode_response::<Value>(&send(&service, "/api/index.php/character/learn_mana_node", request.clone())).data;
    assert_eq!(learned["user_info"]["free_mana"], 150000);
    assert_eq!(learned["user_info"]["paid_mana"], 760);
    assert_eq!(collect_progress(&service, viewer_id, 10003, 1761), 1340);
    assert_eq!(counter(root.path(), "total_used_mana_count"), 1340);
    assert_eq!(counter(root.path(), "mana_addition_count"), before + 150000);
    assert_eq!(mission_history_amount(root.path(), 8), 150000);
    assert!(learned["mission_info"].as_array().unwrap().iter().any(|entry| entry["mission_id"] == 1761));
    let after = read_player(root.path());
    decode_response::<Value>(&send(&service, "/api/index.php/character/learn_mana_node", request));
    assert_eq!(read_player(root.path())["mission_economic_counters"], after["mission_economic_counters"]);
    assert_eq!(load(&service, viewer_id)["user_info"]["free_mana"], 150000);
    assert_eq!(mission_history_amount(root.path(), 8), 150000);
    update_player(root.path(), |player| {
        player["mission_window_progress"] = json!({"9:11":3,"9:12":100,"9:13":97});
    });
    let awake = json!({"viewer_id":viewer_id,"character_id":1,"mana_node_multiplied_id_list":[2201],"awake_level":1});
    decode_response::<Value>(&send(&service, "/api/index.php/character/awake_mana_node", awake.clone()));
    decode_response::<Value>(&send(&service, "/api/index.php/character/awake_mana_node", awake));
    assert_eq!(counter(root.path(), "total_used_mana_count"), 2940);
    assert_eq!(counter(root.path(), "mana_addition_count"), before + 150000);
    assert_eq!(load(&service, viewer_id)["user_info"]["free_mana"], 148400);
    service.stop().unwrap();
}
// //// /从混合玛纳扣费结算消耗任务并累计奖励产生的玛纳 ////

// //// 按珍品购买份数和实际费用累计单买与批量购买任务 [@x380kkm 2026-09-22] ////
#[test]
fn counts_treasure_quantities_and_spending_without_replaying_receipts() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 922103);
    set_time(&service, "2026-09-22T00:00:00.000Z");
    update_player(root.path(), |player| {
        player["user_info"]["free_mana"] = json!(100000);
        player["item_list"]["990008"] = json!(5);
    });
    let income_before = counter(root.path(), "mana_addition_count");
    let history_before = mission_history_amount(root.path(), 4);
    let single = json!({"viewer_id":viewer_id,"api_count":1,"shop_type":2,"shop_item_id":200103,"number":2});
    let bought = decode_response::<Value>(&send(&service, "/api/index.php/shop/buy", single.clone())).data;
    assert_eq!(bought["user_info"]["free_mana"], 96000);
    assert_eq!(counter(root.path(), "treasure_shop_bought_item_count"), 2);
    assert_eq!(progress(&service, viewer_id, 1, 41), 4000);
    decode_response::<Value>(&send(&service, "/api/index.php/shop/buy", single));
    let bulk = json!({"viewer_id":viewer_id,"api_count":2,"shop_type":2,"buy_item_list":{"200104":2,"200105":1}});
    let bought = decode_response::<Value>(&send(&service, "/api/index.php/shop/bulk_buy", bulk.clone())).data;
    assert_eq!(bought["user_info"]["free_mana"], 85000);
    decode_response::<Value>(&send(&service, "/api/index.php/shop/bulk_buy", bulk));
    decode_response::<Value>(&send(&service, "/api/index.php/shop/buy",
        json!({"viewer_id":viewer_id,"api_count":3,"shop_type":9,"shop_item_id":100014,"number":1})));
    assert_eq!(counter(root.path(), "treasure_shop_bought_item_count"), 5);
    assert_eq!(counter(root.path(), "treasure_shop_used_mana_count"), 15000);
    assert_eq!(counter(root.path(), "total_used_mana_count"), 15000);
    assert_eq!(counter(root.path(), "mana_addition_count"), income_before);
    assert_eq!(progress(&service, viewer_id, 1, 41), 15000);
    assert_eq!(mission_history_amount(root.path(), 4), history_before + 15);
    service.stop().unwrap();
}
// //// /按珍品购买份数和实际费用累计单买与批量购买任务 ////

// //// 按实际角色抽数累计每日任务并区分装备抽取 [@x380kkm 2026-09-22] ////
#[test]
fn counts_character_draws_across_payments_and_daily_windows() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    set_time(&service, "2023-04-30T00:00:00.000Z");
    let viewer_id = signup(&service, 922104);
    for id in [1, 2, 3, 28] { open_activity(&service, &format!("gacha:{id}")); }
    open_activity(&service, "gacha-campaign:1");
    load(&service, viewer_id);
    update_player(root.path(), |player| {
        player["user_info"]["free_vmoney"] = json!(10000);
        player["user_info"]["vmoney"] = json!(50);
        player["item_list"]["20003"] = json!(1);
    });
    let initial_reward = read_player(root.path())["item_list"]["70001"].as_i64().unwrap_or_default();
    for (api_count, gacha_id, payment_type, draw_type, total) in [(1,28,4,8,10), (2,1,2,5,11), (3,2,3,3,12), (4,1,1,2,22), (5,3,1,1,22)] {
        let request = json!({"viewer_id":viewer_id,"api_count":api_count,"gacha_id":gacha_id,"payment_type":payment_type,"type":draw_type,"number_of_exec":1});
        decode_response::<Value>(&send(&service, "/api/index.php/gacha/exec", request.clone()));
        decode_response::<Value>(&send(&service, "/api/index.php/gacha/exec", request));
        assert_eq!(counter(root.path(), "total_gacha_character_count"), total);
        assert_eq!(progress(&service, viewer_id, 2, 10019), total);
        assert_eq!(read_player(root.path())["item_list"]["70001"], initial_reward + 1000);
    }
    set_time(&service, "2023-05-01T00:00:00.000Z");
    assert_eq!(progress(&service, viewer_id, 2, 10019), 0);
    decode_response::<Value>(&send(&service, "/api/index.php/gacha/exec",
        json!({"viewer_id":viewer_id,"api_count":6,"gacha_id":1,"payment_type":1,"type":1,"number_of_exec":1})));
    assert_eq!(progress(&service, viewer_id, 2, 10019), 1);
    assert_eq!(read_player(root.path())["item_list"]["70001"], initial_reward + 2000);
    assert_eq!(mission_history_amount(root.path(), 1), 2000);
    service.stop().unwrap();
}
// //// /按实际角色抽数累计每日任务并区分装备抽取 ////

// //// 从出售物品, 主动任务和箱池分别累计玛纳入账 [@x380kkm 2026-09-22] ////
#[test]
fn counts_sale_active_reward_and_box_mana() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = signup(&service, 922105);
    open_activity(&service, "box-gacha:1");
    let catalog: Value = serde_json::from_str(include_str!("../../../../assets/box_reward.json")).unwrap();
    let rewards = catalog["1"]["1"].as_object().unwrap();
    let (mana_id, mana_reward) = rewards.iter().find(|(_, reward)| reward["type"] == 3).unwrap();
    let amount = mana_reward["count"].as_i64().unwrap();
    let drawn: serde_json::Map<String, Value> = rewards.iter().map(|(id, reward)| {
        (id.clone(), json!(reward["available"].as_i64().unwrap() - i64::from(id == mana_id)))
    }).collect();
    update_player(root.path(), |player| {
        player["item_list"]["1"] = json!(2000);
        player["item_list"]["30101"] = json!(10);
        player["all_active_mission_list"] = json!({"20002":{"progress":1,"stages":{"1":false}}});
        player["box_gacha_drawn_rewards"] = json!({"1":{"1":drawn}});
    });
    let before = counter(root.path(), "mana_addition_count");
    let sale = json!({"viewer_id":viewer_id,"item_id":1,"sell_number":2000});
    decode_response::<Value>(&send(&service, "/api/index.php/item/sell", sale.clone()));
    assert!(send(&service, "/api/index.php/item/sell", sale).contains("not_enough_items"));
    assert_eq!(counter(root.path(), "mana_addition_count"), before + 10000);
    active_claim(&service, viewer_id, 20002);
    active_claim(&service, viewer_id, 20002);
    assert_eq!(counter(root.path(), "mana_addition_count"), before + 236480);
    let request = json!({"viewer_id":viewer_id,"box_gacha_id":1,"box_id":1,"number":1,"stop_on_featured_rewards":false});
    decode_response::<Value>(&send(&service, "/api/index.php/box_gacha/exec", request.clone()));
    assert!(send(&service, "/api/index.php/box_gacha/exec", request).starts_with("HTTP/1.1 400"));
    assert_eq!(counter(root.path(), "mana_addition_count"), before + 236480 + amount);
    assert_eq!(mission_history_amount(root.path(), 8), 226480);
    service.stop().unwrap();
}
// //// /从出售物品, 主动任务和箱池分别累计玛纳入账 ////
