// audience: internal
// # activity-projection-event-shop-tests
// 用实际商品验证活动重开, 分日上架和关闭时的客户端与服务端窗口.

use super::*;
use crate::activity_projection::{projection_window, MILLIS_PER_DAY};
use tempfile::TempDir;

fn rows(value: &OrderedValue) -> BTreeMap<i64, &[String]> {
    let OrderedValue::Map(entries) = value else {
        panic!("shop has product rows")
    };
    entries
        .iter()
        .map(|(id, value)| {
            let OrderedValue::Row(row) = value else {
                panic!("product has fields")
            };
            (id.parse().unwrap(), row.as_slice())
        })
        .collect()
}

// //// 验证已过期的真实商品随活动开关恢复入口 [@x380kkm 2026-09-26] ////
#[test]
fn projects_managed_products_and_leaves_unrelated_goods_unchanged() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let now = 1_800_000_000_000;
    database.set_virtual_time(true, now, 1.0).unwrap();
    let mut original = decode_ordered_map(master_seed("event_item_shop").unwrap()).unwrap();
    project(&mut original, &active_windows(&database).unwrap()).unwrap();
    let source_rows = rows(&original);
    assert!(parse_master_time(&source_rows[&1][27]).unwrap() < now);
    database.upsert_always_activity("carnival:1").unwrap();
    let mut projected = original.clone();
    project(&mut projected, &active_windows(&database).unwrap()).unwrap();
    let server_periods = managed_item_periods(&database, "carnival:1")
        .unwrap()
        .unwrap();
    assert!(!server_periods.is_empty());
    for (id, row) in rows(&projected) {
        if let Some((start, end)) = server_periods.get(&id) {
            assert!(now >= *start && now <= *end);
            assert_eq!(parse_master_time(&row[26]), Some(*start));
            assert_eq!(parse_master_time(&row[27]), Some(*end));
            for (index, field) in row.iter().enumerate() {
                if index != 26 && index != 27 {
                    assert_eq!(field, &source_rows[&id][index]);
                }
            }
        } else {
            assert_eq!(row, source_rows[&id]);
        }
    }
    database
        .upsert_manual_activity("carnival:1", false)
        .unwrap();
    project(&mut projected, &active_windows(&database).unwrap()).unwrap();
    assert!(parse_master_time(&rows(&projected)[&1][27]).unwrap() < now);
    database.upsert_manual_activity("carnival:1", true).unwrap();
    project(&mut projected, &active_windows(&database).unwrap()).unwrap();
    assert!(parse_master_time(&rows(&projected)[&1][27]).unwrap() > now);
}
// //// /验证已过期的真实商品随活动开关恢复入口 ////

// //// 保留分日商品的切换边界与活动兑换尾期 [@x380kkm 2026-09-26] ////
#[test]
fn preserves_daily_release_order_and_original_reward_tail() {
    let catalog = catalog().unwrap();
    let source = &catalog.events["advent:100010"];
    let duration = source.end.unwrap() - source.start;
    let start = 1_800_000_000_000;
    let mut window = projection_window(start, start + duration, i64::MAX);
    window.quest_times = QuestTimes::Relative;
    let first = item_period(10359, &catalog.items[&10359], source, &window);
    let second = item_period(10360, &catalog.items[&10360], source, &window);
    assert_eq!(first.0, start);
    assert_eq!(first.1 + 1_000, second.0);
    assert_eq!(second.1 - second.0, MILLIS_PER_DAY - 1_000);
    let reward = item_period(10374, &catalog.items[&10374], source, &window);
    assert_eq!(
        reward.1 - window.end_at_ms,
        catalog.items[&10374].period.end.unwrap() - source.end.unwrap()
    );
    assert!(reward.1 > window.end_at_ms);
    assert!(reward.1 <= window.end_at_ms + window.reward_delay_ms);
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    database
        .upsert_activity_schedule("advent:100010", true, window.start_at_ms, window.end_at_ms)
        .unwrap();
    database.set_virtual_time(true, second.0, 1.0).unwrap();
    let periods = managed_item_periods(&database, "advent:100010")
        .unwrap()
        .unwrap();
    assert_eq!(periods[&10359], first);
    assert_eq!(periods[&10360], second);
    assert_eq!(periods[&10374], reward);
    let collect = catalog
        .items
        .values()
        .find(|item| item.activity_id.starts_with("collect-item:"))
        .unwrap();
    assert!(catalog.events.contains_key(&collect.activity_id));
}
// //// /保留分日商品的切换边界与活动兑换尾期 ////

// //// 对齐独立自选商品的活动窗口与父活动开关 [@x380kkm 2026-09-26] ////
#[test]
fn projects_selection_campaign_and_respects_parent_closure() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let now = 1_800_000_000_000;
    database.set_virtual_time(true, now, 1.0).unwrap();
    let original = decode_ordered_map(master_seed("event_item_shop").unwrap()).unwrap();
    let source_rows = rows(&original);
    assert!(parse_master_time(&source_rows[&100010][27]).unwrap() < now);
    let mut parent_only = BTreeMap::new();
    parent_only.insert(
        "challenge-dungeon:1".to_owned(),
        projection_window(now - MILLIS_PER_DAY, now + MILLIS_PER_DAY, now),
    );
    let mut parent_projected = original.clone();
    project(&mut parent_projected, &parent_only).unwrap();
    assert!(parse_master_time(&rows(&parent_projected)[&100010][27]).unwrap() < now);
    database
        .upsert_always_activity("event-shop-select-item-campaign:10")
        .unwrap();
    let mut projected = original.clone();
    project(&mut projected, &active_windows(&database).unwrap()).unwrap();
    let projected_rows = rows(&projected);
    let server = managed_item_periods(&database, "challenge-dungeon:1")
        .unwrap()
        .unwrap();
    assert!(parse_master_time(&projected_rows[&100010][26]).unwrap() < now);
    assert!(parse_master_time(&projected_rows[&100010][27]).unwrap() > now);
    assert!(server[&100010].0 < now && server[&100010].1 > now);
    for (id, item) in &catalog().unwrap().items {
        if item.campaign_id.as_deref() == Some("event-shop-select-item-campaign:10") {
            assert_eq!(
                parse_master_time(&projected_rows[id][26]),
                Some(server[id].0)
            );
            assert_eq!(
                parse_master_time(&projected_rows[id][27]),
                Some(server[id].1)
            );
        }
    }
    database
        .upsert_manual_activity("event-shop-select-item-campaign:10", false)
        .unwrap();
    project(&mut projected, &active_windows(&database).unwrap()).unwrap();
    assert!(parse_master_time(&rows(&projected)[&100010][27]).unwrap() < now);
    database
        .upsert_manual_activity("event-shop-select-item-campaign:10", true)
        .unwrap();
    database
        .upsert_manual_activity("challenge-dungeon:1", false)
        .unwrap();
    project(&mut projected, &active_windows(&database).unwrap()).unwrap();
    assert!(parse_master_time(&rows(&projected)[&100010][27]).unwrap() < now);
}
// //// /对齐独立自选商品的活动窗口与父活动开关 ////

// //// 全量投影区域商品日期并保持 CN 商品契约 [@x380kkm 2026-09-28] ////
#[test]
fn projects_all_regional_shop_windows_without_changing_cn_contracts() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let original = decode_ordered_map(master_seed("event_item_shop").unwrap()).unwrap();
    let source = rows(&original);
    let periods: serde_json::Value = serde_json::from_str(super::super::regional::CATALOG).unwrap();
    let expected_count: usize = periods
        .as_object()
        .unwrap()
        .values()
        .map(|period| period["items"].as_object().unwrap().len())
        .sum();
    let mut projected_count = 0;
    for (activity_id, data) in periods.as_object().unwrap() {
        if data["items"].as_object().unwrap().is_empty() {
            continue;
        }
        let now = data["start_at_ms"].as_i64().unwrap() + 1000;
        database.set_virtual_time(true, now, 1.0).unwrap();
        let windows = active_windows(&database).unwrap();
        let period = windows[activity_id].regional_period.unwrap();
        let mut projected = original.clone();
        project(
            &mut projected,
            &BTreeMap::from([(activity_id.clone(), windows[activity_id].clone())]),
        )
        .unwrap();
        let client = rows(&projected);
        let server = managed_item_periods(&database, activity_id)
            .unwrap()
            .unwrap();
        for (id, [start, end]) in &period.items {
            assert_eq!(server[id], (*start, *end), "server {activity_id}:{id}");
            assert_eq!(
                parse_master_time(&client[id][26]),
                Some(*start),
                "client start {activity_id}:{id}"
            );
            assert_eq!(
                parse_master_time(&client[id][27]),
                Some(*end),
                "client end {activity_id}:{id}"
            );
            for (index, value) in client[id].iter().enumerate() {
                if !matches!(index, 26 | 27) {
                    if index == 1 && data["canonical_activity_id"].as_str().is_some() {
                        assert_eq!(value, activity_id.split_once(':').unwrap().1);
                    } else {
                        assert_eq!(
                            value, &source[id][index],
                            "contract {activity_id}:{id}:{index}"
                        );
                    }
                }
            }
            projected_count += 1;
        }
    }
    assert_eq!(projected_count, expected_count);
}
// //// /全量投影区域商品日期并保持 CN 商品契约 ////

// //// 校准原期与管理重开商品并保留分日上架 [@x380kkm 2026-09-29] ////
#[test]
fn canonical_shop_dates_apply_to_native_and_managed_periods() {
    let catalog = catalog().unwrap();
    let original = decode_ordered_map(master_seed("event_item_shop").unwrap()).unwrap();
    let mut projected = original.clone();
    project(&mut projected, &BTreeMap::new()).unwrap();
    let client = rows(&projected);
    for (id, [start, end]) in &catalog.canonical_periods {
        assert_eq!(parse_master_time(&client[id][26]), Some(*start));
        assert_eq!(parse_master_time(&client[id][27]), Some(*end));
        let item = &catalog.items[id];
        let source = &catalog.events[&item.activity_id];
        let shift = 100 * MILLIS_PER_DAY;
        let mut managed =
            projection_window(source.start + shift, source.end.unwrap() + shift, i64::MAX);
        managed.quest_times = QuestTimes::Relative;
        assert_eq!(
            item_period(*id, item, source, &managed),
            (start + shift, end + shift)
        );
    }
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let period = &catalog.events["advent:100001"];
    database
        .set_virtual_time(true, period.start + 1_000, 1.0)
        .unwrap();
    let server = managed_item_periods(&database, "advent:100001")
        .unwrap()
        .unwrap();
    for (id, item) in &catalog.items {
        if item.activity_id == "advent:100001" {
            assert_eq!(server[id], (item.period.start, item.period.end.unwrap()));
        }
    }
}
// //// /校准原期与管理重开商品并保留分日上架 ////

// //// 常驻崩坏域保留后续商品和自选活动的实际开放日 [@x380kkm 2026-09-29] ////
#[test]
fn permanent_challenge_preserves_later_shop_dates() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    database
        .set_virtual_time(true, 1_577_836_800_000, 1.0)
        .unwrap();
    let catalog = catalog().unwrap();
    let source = &catalog.events["challenge-dungeon:1"];
    let periods = managed_item_periods(&database, "challenge-dungeon:1")
        .unwrap()
        .unwrap();
    for (id, item) in &catalog.items {
        if item.activity_id == "challenge-dungeon:1" && item.period.start > source.start {
            assert_eq!(periods[id].0, item.period.start);
            if let Some(end) = item.period.end {
                assert_eq!(periods[id].1, end);
            }
        }
    }
}
// //// /常驻崩坏域保留后续商品和自选活动的实际开放日 ////
