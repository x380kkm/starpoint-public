// audience: internal
// # regional-activity-period-tests
// 同 ID 活动的区域时间, CN 时间和显式管理规则通过同一目录与入场接口验证.

use super::*;
use crate::activity_projection::{
    active_windows, is_quest_open, managed_item_periods, master_seed,
    ordered_map::decode_ordered_map,
};
use crate::database::{parse_iso_timestamp, ServiceDatabase};
use tempfile::TempDir;

fn at(database: &mut ServiceDatabase, iso: &str) {
    database
        .set_virtual_time(true, parse_iso_timestamp(iso).unwrap(), 1.0)
        .unwrap();
}

// //// 验证复刻的关卡和商品按区域时间开放并保留兑换尾期 [@x380kkm 2026-09-27] ////
#[test]
fn saga_preserves_regional_play_shop_and_cn_periods() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let period = &catalog().unwrap()["advent:100003"];
    at(&mut database, "2021-04-24T03:59:00Z");
    assert!(!is_quest_open(&database, 7, 100003001).unwrap());
    at(&mut database, "2021-04-25T04:00:00Z");
    for suffix in 1..=8 {
        assert!(is_quest_open(&database, 7, 100003000 + suffix).unwrap());
    }
    let items = managed_item_periods(&database, "advent:100003")
        .unwrap()
        .unwrap();
    assert_eq!(items.len(), 57);
    assert!(items
        .values()
        .all(|(start, end)| *start == period.start_at_ms && *end == period.reward_end_at_ms));
    let mut master = decode_ordered_map(master_seed("advent_event").unwrap()).unwrap();
    project_master(&mut master, "advent_event", period);
    let row = row_at_path(&mut master, "100003").unwrap();
    assert_eq!(row[24], "2021-04-24 12:00:00");
    assert_eq!(row[25], "2021-04-30 20:59:59");
    assert_eq!(row[26], "2021-05-10 04:59:59");

    at(&mut database, "2021-05-01T04:00:00Z");
    assert!(!is_quest_open(&database, 7, 100003001).unwrap());
    assert_eq!(
        managed_item_periods(&database, "advent:100003")
            .unwrap()
            .unwrap(),
        items
    );
    at(&mut database, "2021-05-10T04:00:00Z");
    assert!(managed_item_periods(&database, "advent:100003")
        .unwrap()
        .is_none());

    at(&mut database, "2023-03-25T04:00:00Z");
    for suffix in 1..=8 {
        assert!(is_quest_open(&database, 7, 100003000 + suffix).unwrap());
    }
    assert!(!active_windows(&database)
        .unwrap()
        .contains_key("advent:100003"));
}
// //// /验证复刻的关卡和商品按区域时间开放并保留兑换尾期 ////

// //// 验证显式关闭和轮换时间覆盖自动区域时间 [@x380kkm 2026-09-27] ////
#[test]
fn explicit_management_owns_regional_event_availability() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    at(&mut database, "2021-04-25T04:00:00Z");
    database
        .upsert_manual_activity("advent:100003", false)
        .unwrap();
    assert!(!is_quest_open(&database, 7, 100003001).unwrap());
    database
        .upsert_manual_activity("advent:100003", true)
        .unwrap();
    assert!(is_quest_open(&database, 7, 100003001).unwrap());
    assert!(active_windows(&database).unwrap()["advent:100003"]
        .regional_period
        .is_none());
    let start = parse_iso_timestamp("2030-01-01T00:00:00Z").unwrap();
    database
        .upsert_activity_schedule("advent:100003", true, start, start + 86_400_000)
        .unwrap();
    assert!(!is_quest_open(&database, 7, 100003001).unwrap());
    at(&mut database, "2030-01-01T01:00:00Z");
    assert!(is_quest_open(&database, 7, 100003001).unwrap());
}
// //// /验证显式关闭和轮换时间覆盖自动区域时间 ////
