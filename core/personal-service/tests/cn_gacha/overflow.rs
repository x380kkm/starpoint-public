// audience: internal
// # personal-service-cn-gacha-overflow-tests
// 真实 HTTP 抽取和兑换覆盖满堆叠转换, 邮件投递, 请求重放和存储故障边界.

use super::*;
use rusqlite::Connection;
use std::path::Path;

// //// 保存用例库存和积分 [@x380kkm 2026-09-26] ////
pub(super) fn save(root: &Path, player: &Value) {
    Connection::open(root.join("personal-service.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();
}

// //// 读取转换余量的实际投递结果 [@x380kkm 2026-09-26] ////
pub(super) fn mails(root: &Path) -> Vec<Value> {
    let db = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let mut statement = db
        .prepare("SELECT rewards_json FROM player_mails ORDER BY id")
        .unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect()
}

// //// 通过正式兑换创建角色并准备满堆叠库存 [@x380kkm 2026-09-26] ////
fn prepare(root: &Path) -> (PersonalService, i64) {
    let service = PersonalService::start(root, 0).unwrap();
    set_virtual_time(&service, "2025-01-02T12:00:00.000Z");
    open_gacha(&service, 1);
    let viewer = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 813 }),
    ))
    .data_headers
    .viewer_id;
    let mut player = replay::stored_state(root).0;
    player["gacha_info_list"] = json!([{"gacha_id":1,"gacha_exchange_point":1000}]);
    save(root, &player);
    let created = exchange(&service, viewer, 0);
    assert_eq!(
        decode_response::<Value>(&created).data["character_list"][0]["character_id"],
        111001
    );
    player = replay::stored_state(root).0;
    player["user_character_list"]["111001"]["stack"] = json!(99);
    player["user_info"]["exp_pool"] = json!(1_999_995_000_i64);
    player["user_info"]["exp_pooled_time"] = json!(1_735_819_200_i64);
    player["item_list"]["990008"] = json!(999989);
    player["user_info"]["free_vmoney"] = json!(3000);
    player["user_info"]["vmoney"] = json!(50);
    save(root, &player);
    (service, viewer)
}

// //// 发送确定角色的积分兑换 [@x380kkm 2026-09-26] ////
fn exchange(service: &PersonalService, viewer: i64, count: i64) -> String {
    cn_support::send_request(
        service.port(),
        "/api/index.php/gacha/exchange_character",
        &encode_request(
            &json!({"viewer_id":viewer,"api_count":count,"gacha_id":1,"character_id":111001}),
        ),
    )
}

// //// 转换余量跨重放和重启保留并在腾出容量后完整领取 [@x380kkm 2026-09-26] ////
#[test]
fn conversion_overflow_survives_replay_restart_and_mail_claim() {
    let root = TempDir::new().unwrap();
    let (service, viewer) = prepare(root.path());
    let response = exchange(&service, viewer, 1);
    let result = decode_response::<Value>(&response).data;
    assert_eq!(result["character_list"][0]["stack"], 99);
    assert_eq!(result["user_info"]["exp_pool"], 2_000_000_000_i64);
    assert_eq!(result["item_list"]["990008"], 999999);
    assert_eq!(result["over_max"][0]["converted_pooled_exp"], 10000);
    assert_eq!(result["over_max"][0]["converted_star_grain"], 30);
    assert_eq!(result["mail_arrived"], true);
    let deliveries = mails(root.path());
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0]["expPool"], 5000);
    assert_eq!(deliveries[0]["itemList"]["990008"], 20);
    let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    for (kind, number) in [(9, 5000), (1, 10)] {
        let recorded: i64 = db.query_row("SELECT SUM(number) FROM player_receive_history WHERE subject='角色转换奖励' AND kind=?1", [kind], |row| row.get(0)).unwrap();
        assert_eq!(recorded, number);
    }
    let settled = replay::stored_state(root.path());
    assert!(settled.0.get("reward_deliveries").is_none());
    assert_eq!(exchange(&service, viewer, 1), response);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(exchange(&service, viewer, 1), response);
    assert_eq!(replay::stored_state(root.path()), settled);
    assert_eq!(mails(root.path()), deliveries);
    let mut player = settled.0;
    player["user_info"]["exp_pool"] = json!(1_999_990_000_i64);
    player["item_list"]["990008"] = json!(999969);
    save(root.path(), &player);
    let request = encode_request(&json!({"viewer_id":viewer}));
    let claimed =
        cn_support::send_request(service.port(), "/api/index.php/mail/receive_all", &request);
    let claimed = decode_response::<Value>(&claimed).data;
    assert_eq!(claimed["user_info"]["exp_pool"], 1_999_995_000_i64);
    assert_eq!(claimed["item_list"]["990008"], 999989);
    let settled = replay::stored_state(root.path());
    cn_support::send_request(service.port(), "/api/index.php/mail/receive_all", &request);
    assert_eq!(replay::stored_state(root.path()), settled);
    service.stop().unwrap();
}
// //// /转换余量跨重放和重启保留并在腾出容量后完整领取 ////

// //// 邮件和领取历史写入失败时同时回滚积分与转换奖励 [@x380kkm 2026-09-26] ////
#[test]
fn conversion_overflow_rolls_back_delivery_snapshot_and_history() {
    for (table, operation) in [
        ("player_mails", "INSERT"),
        ("player_snapshots", "UPDATE"),
        ("player_receive_history", "INSERT"),
    ] {
        let root = TempDir::new().unwrap();
        let (service, viewer) = prepare(root.path());
        let before = replay::stored_state(root.path());
        let db = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
        db.execute_batch(&format!("CREATE TRIGGER reject_conversion BEFORE {operation} ON {table} BEGIN SELECT RAISE(ABORT,'write unavailable'); END;")).unwrap();
        let failed = exchange(&service, viewer, 1);
        assert!(failed.starts_with("HTTP/1.1 500"), "{failed}");
        assert_eq!(replay::stored_state(root.path()), before);
        assert!(mails(root.path()).is_empty());
        db.execute_batch("DROP TRIGGER reject_conversion").unwrap();
        let response = exchange(&service, viewer, 1);
        assert_eq!(
            decode_response::<Value>(&response).data["mail_arrived"],
            true
        );
        assert_eq!(mails(root.path()).len(), 1);
        assert_eq!(exchange(&service, viewer, 1), response);
        assert_eq!(mails(root.path()).len(), 1);
        service.stop().unwrap();
    }
}
// //// /邮件和领取历史写入失败时同时回滚积分与转换奖励 ////

// //// 十连和每日抽取在满容量时保留每份转换资产且只扣费一次 [@x380kkm 2026-09-26] ////
#[test]
fn conversion_overflow_handles_bulk_and_daily_draws() {
    for (payment_type, draw_type, pulls, free_cost, paid_cost) in
        [(1, 2, 10, 1500, 0), (2, 5, 1, 0, 50)]
    {
        let root = TempDir::new().unwrap();
        let (service, viewer) = prepare(root.path());
        let mut player = replay::stored_state(root.path()).0;
        let character = player["user_character_list"]["111001"].clone();
        let assets: Value =
            serde_json::from_str(include_str!("../../../../assets/character.json")).unwrap();
        for id in assets.as_object().unwrap().keys() {
            player["user_character_list"][id] = character.clone();
        }
        player["user_info"]["exp_pool"] = json!(2_000_000_000_i64);
        player["item_list"]["990008"] = json!(999999);
        save(root.path(), &player);
        let request = encode_request(
            &json!({"viewer_id":viewer,"api_count":10,"gacha_id":1,"payment_type":payment_type,"type":draw_type,"number_of_exec":1}),
        );
        let response =
            cn_support::send_request(service.port(), "/api/index.php/gacha/exec", &request);
        let result = decode_response::<Value>(&response).data;
        assert_eq!(result["draw"].as_array().unwrap().len(), pulls);
        assert_eq!(result["user_info"]["free_vmoney"], 3000 - free_cost);
        assert_eq!(result["user_info"]["vmoney"], 50 - paid_cost);
        assert_eq!(result["user_info"]["exp_pool"], 2_000_000_000_i64);
        assert!(result["character_list"]
            .as_array()
            .unwrap()
            .iter()
            .all(|character| character["stack"] == 99));
        let conversions: Vec<_> = result["over_max"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["process_type"] == 5)
            .collect();
        assert_eq!(conversions.len(), pulls);
        let delivered = mails(root.path());
        assert_eq!(delivered.len(), pulls);
        assert_eq!(
            delivered
                .iter()
                .map(|mail| mail["expPool"].as_i64().unwrap())
                .sum::<i64>(),
            conversions
                .iter()
                .map(|entry| entry["converted_pooled_exp"].as_i64().unwrap())
                .sum::<i64>()
        );
        assert_eq!(
            delivered
                .iter()
                .map(|mail| mail["itemList"]["990008"].as_i64().unwrap_or_default())
                .sum::<i64>(),
            conversions
                .iter()
                .map(|entry| entry["converted_star_grain"].as_i64().unwrap())
                .sum::<i64>()
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
// //// /十连和每日抽取在满容量时保留每份转换资产且只扣费一次 ////
