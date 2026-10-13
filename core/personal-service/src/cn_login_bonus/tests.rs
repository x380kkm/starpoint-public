// audience: internal
// # personal-service-cn-login-date-tests
// 现实日期推进奖励序号, 游戏时间决定活动奖励组的开放窗口.

use super::prepare_for_date;
use crate::cn_player::create_default_player_data;
use crate::database::{parse_iso_timestamp, ServiceDatabase};
use serde_json::{json, Map, Value};
use tempfile::TempDir;

// //// 构造具有完整资产结构的登录存档 [@x380kkm 2026-09-24] ////
fn timestamp(iso: &str) -> i64 {
    parse_iso_timestamp(iso).unwrap() / 1000
}

fn player(now: i64) -> Map<String, Value> {
    serde_json::from_str(&create_default_player_data(now, "2030-01-01 12:00:00").unwrap())
        .unwrap()
}

fn acknowledge(root: &mut Map<String, Value>) {
    for field in ["bonus_index_list", "login_bonus_received_at", "login_bonus_received_date"] {
        root.remove(field);
    }
}

fn index(root: &Map<String, Value>, group: &str) -> i64 {
    root["login_bonus_state"]["groups"][group]["index"].as_i64().unwrap()
}
// //// /构造具有完整资产结构的登录存档 ////

// //// 将现实每日领取与游戏时间往返调整分开 [@x380kkm 2026-09-24] ////
#[test]
fn advances_by_local_date_and_preserves_future_game_receipts() {
    let temporary = TempDir::new().unwrap();
    let database = ServiceDatabase::open(temporary.path()).unwrap();
    let now = timestamp("2030-01-01T12:00:00Z");
    let mut root = player(now);
    root.insert("login_bonus_state".into(), json!({
        "groups": {"normal_2022": {"index": 2, "received_days": [90000]}},
        "last_login_at": timestamp("2099-01-01T12:00:00Z"),
    }));
    let first = prepare_for_date(&mut root, &database, now, "2026-09-24").unwrap();
    assert!(!first.entries.is_empty());
    assert_eq!(index(&root, "normal_2022"), 3);
    assert_eq!(root["login_bonus_state"]["groups"]["normal_2022"]["received_days"], json!([90000]));
    acknowledge(&mut root);
    for game_date in ["2030-01-03T12:00:00Z", "2099-01-01T12:00:00Z", "2029-01-01T12:00:00Z"] {
        let before = (root["item_list"].clone(), root["user_info"].clone());
        let repeated = prepare_for_date(&mut root, &database, timestamp(game_date), "2026-09-24").unwrap();
        assert!(repeated.entries.is_empty());
        assert_eq!(root["item_list"], before.0);
        assert_eq!(root["user_info"], before.1);
    }
    for (date, expected) in [("2026-09-25", 4), ("2026-09-26", 1)] {
        let delivered = prepare_for_date(&mut root, &database, now, date).unwrap();
        assert!(!delivered.entries.is_empty());
        assert_ne!(delivered.event_key, first.event_key);
        assert_eq!(index(&root, "normal_2022"), expected);
        acknowledge(&mut root);
    }
    assert!(prepare_for_date(&mut root, &database, now, "2026-09-24").unwrap().entries.is_empty());
}
// //// /将现实每日领取与游戏时间往返调整分开 ////

// //// 在现实日期推进时遵守有限奖励次数和虚拟活动截止时间 [@x380kkm 2026-09-24] ////
#[test]
fn limited_rewards_follow_game_window_and_real_login_dates() {
    let temporary = TempDir::new().unwrap();
    let database = ServiceDatabase::open(temporary.path()).unwrap();
    let now = timestamp("2023-08-04T12:00:00Z");
    let mut root = player(now);
    for day in 1..=6 {
        prepare_for_date(&mut root, &database, now, &format!("2026-09-{day:02}")).unwrap();
        assert_eq!(index(&root, "hw_v21_02"), day.min(4));
        acknowledge(&mut root);
    }
    let now = timestamp("2022-01-27T12:00:00Z");
    let mut root = player(now);
    prepare_for_date(&mut root, &database, now, "2026-09-24").unwrap();
    assert_eq!(index(&root, "ny22_spring"), 1);
    acknowledge(&mut root);
    prepare_for_date(&mut root, &database, timestamp("2022-02-10T12:00:00Z"), "2026-09-25").unwrap();
    assert_eq!(index(&root, "ny22_spring"), 1);
}
// //// /在现实日期推进时遵守有限奖励次数和虚拟活动截止时间 ////
