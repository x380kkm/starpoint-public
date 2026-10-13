// audience: internal
// # personal-service-cn-payment-rebate-tests
// 存档资格通过真实返利请求核对资产发放, 重放恢复, 失效资格和事务回滚.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "support/local_saves.rs"]
#[allow(dead_code)]
mod local_save_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

const NOW: i64 = 1_704_067_200;
const EXPIRY: i64 = NOW + 86_400;

fn database(root: &Path) -> Connection {
    Connection::open(root.join("personal-service.sqlite3")).unwrap()
}

fn snapshot(root: &Path) -> Value {
    let data: String = database(root)
        .query_row(
            "SELECT data_json FROM player_snapshots ORDER BY account_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

fn update(root: &Path, update: impl FnOnce(&mut Value)) {
    let mut data = snapshot(root);
    update(&mut data);
    database(root).execute(
        "UPDATE player_snapshots SET data_json=?1 WHERE account_id=(SELECT MIN(account_id) FROM player_snapshots)",
        [data.to_string()],
    ).unwrap();
}

fn request(service: &PersonalService, route: &str, body: Value) -> String {
    cn_support::send_request(
        service.port(),
        &format!("/api/index.php/{route}"),
        &encode_request(&body),
    )
}

fn signup(service: &PersonalService, device: i64) -> i64 {
    decode_response::<SignupData>(&request(
        service,
        "tool/signup",
        json!({"device_id": device}),
    ))
    .data_headers
    .viewer_id
}

fn setup() -> (TempDir, PersonalService, i64) {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let authorization = format!("Bearer {}", service.management_token());
    let response = support::request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", &authorization)],
        json!({"enabled":true,"iso":"2024-01-01T00:00:00.000Z","rate":1})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let viewer = signup(&service, 920_039);
    (root, service, viewer)
}

fn receive(service: &PersonalService, viewer: i64) -> Value {
    let response = decode_response::<Value>(&request(
        service,
        "Payment_rebate/rebate",
        json!({"viewer_id": viewer}),
    ));
    assert_eq!(response.data_headers.result_code, 1);
    response.data
}

fn history(root: &Path) -> Vec<(i64, Option<i64>, i64)> {
    let connection = database(root);
    let mut query = connection.prepare("SELECT kind, type_id, number FROM player_receive_history WHERE event_key LIKE 'payment-rebate:%' ORDER BY entry_index").unwrap();
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn mission_state(root: &Path) -> (Vec<(i64, i64, i64)>, Vec<(String, i64)>, i64) {
    let connection = database(root);
    let mut progress = connection.prepare("SELECT category,mission_id,value FROM player_mission_progress ORDER BY category,mission_id").unwrap();
    let progress = progress
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut counters = connection
        .prepare("SELECT pattern,value FROM player_mission_counters ORDER BY pattern")
        .unwrap();
    let counters = counters
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let history = connection
        .query_row("SELECT COUNT(*) FROM player_receive_history", [], |row| {
            row.get(0)
        })
        .unwrap();
    (progress, counters, history)
}

fn offer(rewards: Value) -> Value {
    json!({"status": 1, "start_time": NOW - 60, "expired_time": EXPIRY, "present_list": rewards})
}

// //// 按礼包类型发放全部资产并返回真实客户端增量 [@x380kkm 2026-09-23] ////
#[test]
fn grants_each_supported_rebate_asset_and_records_delivery() {
    let (root, service, viewer) = setup();
    let rewards = json!([
        {"type":1,"type_id":100,"number":3},
        {"type":3,"type_id":null,"number":11},
        {"type":4,"type_id":null,"number":13},
        {"type":5,"type_id":111001,"number":1},
        {"type":6,"type_id":5030037,"number":2},
        {"type":7,"type_id":null,"number":17},
        {"type":8,"type_id":null,"number":19},
        {"type":9,"type_id":null,"number":23},
        {"type":10,"type_id":null,"number":29},
        {"type":11,"type_id":null,"number":2},
        {"type":12,"type_id":null,"number":3},
        {"type":13,"type_id":1,"number":1},
        {"type":14,"type_id":1,"number":5},
        {"type":15,"type_id":null,"number":31}
    ]);
    update(root.path(), |data| {
        data["payment_rebate_info"] = offer(rewards.clone());
        data["payment_rebate_info"]["start_time"] = json!(EXPIRY + 1);
        data["user_info"]["stamina"] = json!(4);
        data["user_info"]["stamina_heal_time"] = json!(NOW - 600);
    });
    let before = snapshot(root.path());
    let response = receive(&service, viewer);
    let after = snapshot(root.path());
    assert_eq!(response["status"], 0);
    assert_eq!(
        response["after_vmoney"],
        before["user_info"]["vmoney"].as_i64().unwrap() + 11
    );
    for (field, amount) in [
        ("vmoney", 11),
        ("free_vmoney", 13),
        ("star_crumb", 17),
        ("free_mana", 19),
        ("exp_pool", 23),
        ("bond_token", 29),
        ("boss_boost_point", 2),
        ("boost_point", 3),
        ("rank_point", 31),
    ] {
        let expected = before["user_info"][field].as_i64().unwrap() + amount;
        assert_eq!(after["user_info"][field], expected, "{field}");
        assert_eq!(response["user_info"][field], expected, "response {field}");
    }
    assert_eq!(after["item_list"]["100"], 3);
    assert_eq!(response["item_list"]["100"], 3);
    assert!(after["user_character_list"]["111001"].is_object());
    assert_eq!(response["character_list"][0]["character_id"], 111001);
    assert_eq!(after["user_equipment_list"]["5030037"]["stack"], 1);
    assert_eq!(response["equipment_list"][0]["stack"], 1);
    assert_eq!(after["user_info"]["stamina"], 30);
    assert_eq!(response["user_info"]["stamina"], 30);
    assert!(after["user_info"]["stamina_heal_time"].as_i64().unwrap() >= NOW);
    assert_eq!(
        response["user_info"]["stamina_heal_time"],
        after["user_info"]["stamina_heal_time"]
    );
    assert!(after["degree_ids"].as_array().unwrap().contains(&json!(1)));
    assert_eq!(response["degree_list"][0]["degree_id"], 1);
    let prior = before["user_daily_challenge_point_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == 1)
        .unwrap()["point"]
        .as_i64()
        .unwrap();
    let challenge = response["user_daily_challenge_point_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == 1)
        .unwrap();
    assert_eq!(challenge["point"], prior + 5);
    assert!(challenge["campaign_list"].is_array());
    assert_eq!(
        challenge["campaign_list"],
        before["user_daily_challenge_point_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == 1)
            .unwrap()["campaign_list"]
    );
    assert_eq!(after["payment_rebate_info"]["status"], 0);
    assert_eq!(
        after["payment_rebate_receipts"].as_object().unwrap().len(),
        1
    );
    let delivered = history(root.path());
    for reward in rewards.as_array().unwrap() {
        assert!(
            delivered.contains(&(
                reward["type"].as_i64().unwrap(),
                reward["type_id"].as_i64(),
                reward["number"].as_i64().unwrap()
            )),
            "missing {reward}"
        );
    }
    let slot = local_save_support::list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let history_page = local_save_support::authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{slot}/reward-history"),
        None,
    );
    assert!(history_page.starts_with("HTTP/1.1 200"), "{history_page}");
    assert!(history_page.contains("返利领取"), "{history_page}");
    let loaded = decode_response::<Value>(&request(
        &service,
        "load",
        json!({"viewer_id":viewer,"keychain":viewer}),
    ))
    .data;
    assert_eq!(loaded["payment_rebate_info"]["status"], 0);
    assert!(loaded.get("payment_rebate_receipts").is_none());
    service.stop().unwrap();
}

// //// 同段位的返利保留现有体力恢复起点 [@x380kkm 2026-09-23] ////
#[test]
fn preserves_stamina_recovery_when_rank_points_stay_in_the_same_rank() {
    let (root, service, viewer) = setup();
    update(root.path(), |data| {
        data["payment_rebate_info"] = offer(json!([{"type":15,"number":1}]));
        data["user_info"]["stamina"] = json!(4);
        data["user_info"]["stamina_heal_time"] = json!(NOW - 600);
    });
    let before = snapshot(root.path());
    let response = receive(&service, viewer);
    let after = snapshot(root.path());
    assert_eq!(after["user_info"]["rank_point"], 11);
    assert_eq!(response["user_info"]["rank_point"], 11);
    assert_eq!(
        after["user_info"]["stamina"],
        before["user_info"]["stamina"]
    );
    assert_eq!(
        after["user_info"]["stamina_heal_time"],
        before["user_info"]["stamina_heal_time"]
    );
    assert!(response["user_info"].get("stamina").is_none());
    assert_eq!(history(root.path()), vec![(15, None, 1)]);
    service.stop().unwrap();
}

// //// 服务重启和同一资格重放保持一次到账 [@x380kkm 2026-09-23] ////
#[test]
fn replays_claim_after_restart_and_ignores_reenabled_same_eligibility() {
    let (root, service, viewer) = setup();
    let rewards = json!([
        {"type":3,"number":5}, {"type":3,"number":6},
        {"type":1,"type_id":100,"number":2}
    ]);
    update(root.path(), |data| {
        data["payment_rebate_info"] = offer(rewards)
    });
    receive(&service, viewer);
    let claimed = snapshot(root.path());
    assert_eq!(
        receive(&service, viewer)["after_vmoney"],
        claimed["user_info"]["vmoney"]
    );
    assert_eq!(snapshot(root.path()), claimed);
    let other_viewer = signup(&service, 920_040);
    assert_eq!(
        receive(&service, other_viewer),
        json!({"status":0,"after_vmoney":0})
    );
    assert_eq!(snapshot(root.path()), claimed);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 920_039);
    update(root.path(), |data| {
        data["payment_rebate_info"]["status"] = json!(1);
        data["payment_rebate_info"]["present_list"] = json!([
            {"type":1,"type_id":100,"number":2}, {"type":3,"number":11}
        ]);
        data["user_info"]["vmoney"] = json!(7);
    });
    let restored = snapshot(root.path());
    assert_eq!(
        receive(&service, viewer),
        json!({"status":0,"after_vmoney":7})
    );
    assert_eq!(snapshot(root.path()), restored);
    assert_eq!(history(root.path()).len(), 2);
    service.stop().unwrap();
}

// //// 无资格和已失效返利保持资产与历史不变 [@x380kkm 2026-09-23] ////
#[test]
fn leaves_missing_expired_and_consumed_offers_unchanged() {
    let (root, service, viewer) = setup();
    let fresh = snapshot(root.path());
    assert_eq!(
        receive(&service, viewer),
        json!({"status":0,"after_vmoney":0})
    );
    assert_eq!(snapshot(root.path()), fresh);
    let valid = json!([{"type":3,"number":11}]);
    let mut expired = offer(valid.clone());
    expired["expired_time"] = json!(NOW);
    let mut consumed = offer(valid);
    consumed["status"] = json!(0);
    for value in [Value::Null, expired, consumed] {
        update(root.path(), |data| {
            data["payment_rebate_info"] = value.clone()
        });
        let before = snapshot(root.path());
        assert_eq!(
            receive(&service, viewer),
            json!({"status":0,"after_vmoney":before["user_info"]["vmoney"]}),
            "{value}"
        );
        assert_eq!(snapshot(root.path()), before, "{value}");
    }
    assert!(history(root.path()).is_empty());
    service.stop().unwrap();
}

// //// 有效期内的错误奖品返回明确错误并保留领取资格 [@x380kkm 2026-09-23] ////
#[test]
fn rejects_malformed_and_overflowing_rewards_without_a_success_result() {
    let (root, service, viewer) = setup();
    for value in [
        offer(json!([])),
        offer(Value::Null),
        offer(json!([{"type":3,"number":0}])),
        offer(json!([{"type":3,"number":-1}])),
        offer(json!([{"type":3,"number":11,"type_id":1}])),
        offer(json!([{"type":1,"number":1}])),
        offer(json!([{"type":2,"number":1}])),
        offer(json!([{"type":3,"number":11},{"type":99,"number":1}])),
    ] {
        update(root.path(), |data| {
            data["payment_rebate_info"] = value.clone()
        });
        let before = snapshot(root.path());
        let response = request(
            &service,
            "Payment_rebate/rebate",
            json!({"viewer_id":viewer}),
        );
        assert!(response.starts_with("HTTP/1.1 400"), "{value}: {response}");
        assert!(response.contains("invalid_rebate_rewards"), "{response}");
        assert_eq!(snapshot(root.path()), before, "{value}");
    }
    update(root.path(), |data| {
        data["user_info"]["vmoney"] = json!(i32::MAX);
        data["payment_rebate_info"] = offer(json!([{"type":3,"number":1}]));
    });
    let full = snapshot(root.path());
    let response = request(
        &service,
        "Payment_rebate/rebate",
        json!({"viewer_id":viewer}),
    );
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(
        response.contains("rebate_reward_capacity_exceeded"),
        "{response}"
    );
    assert_eq!(snapshot(root.path()), full);
    assert!(history(root.path()).is_empty());
    service.stop().unwrap();
}

// //// 领取历史写入失败时完整回滚并支持同一资格重试 [@x380kkm 2026-09-23] ////
#[test]
fn rolls_back_rewards_receipt_eligibility_and_missions_when_history_fails() {
    let (root, service, viewer) = setup();
    update(root.path(), |data| {
        data["payment_rebate_info"] = offer(json!([
            {"type":3,"number":11}, {"type":8,"number":5000}, {"type":1,"type_id":100,"number":3}
        ]))
    });
    let before = snapshot(root.path());
    let missions_before = mission_state(root.path());
    database(root.path()).execute_batch("CREATE TRIGGER reject_rebate_history BEFORE INSERT ON player_receive_history WHEN NEW.event_key LIKE 'payment-rebate:%' BEGIN SELECT RAISE(ABORT,'rebate history failure'); END;").unwrap();
    let result = request(
        &service,
        "Payment_rebate/rebate",
        json!({"viewer_id":viewer}),
    );
    assert!(result.starts_with("HTTP/1.1 500"), "{result}");
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(mission_state(root.path()), missions_before);
    assert!(history(root.path()).is_empty());
    database(root.path())
        .execute_batch("DROP TRIGGER reject_rebate_history;")
        .unwrap();
    assert_eq!(receive(&service, viewer)["after_vmoney"], 11);
    assert_eq!(history(root.path()).len(), 3);
    assert_eq!(snapshot(root.path())["payment_rebate_info"]["status"], 0);
    service.stop().unwrap();
}
