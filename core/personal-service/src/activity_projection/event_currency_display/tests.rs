// audience: internal
// # event-currency-display-tests
// 使用实际商店币和可下载差分验证活动余额字段及开放日期.

use super::*;
use crate::activity_projection::{
    active_windows, decode_ordered_map, master_seed, projected_time, projection_manifest, sync,
    upgrade_tests::{replay_diff_entries, write_bundle},
    TimeKind,
};
use crate::database::{parse_iso_timestamp, ServiceDatabase};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// //// 按活动编号读取主表字段 [@x380kkm 2026-10-02] ////
fn rows(value: &OrderedValue) -> BTreeMap<&str, &[String]> {
    let OrderedValue::Map(entries) = value else {
        panic!("event map")
    };
    entries
        .iter()
        .map(|(id, value)| {
            let OrderedValue::Row(row) = value else {
                panic!("event row")
            };
            (id.as_str(), row.as_slice())
        })
        .collect()
}

// //// 全部土俑期别显示各自商店消费币且保留其它主表字段 [@x380kkm 2026-10-02] ////
#[test]
fn aligns_all_carnival_display_currencies_with_shop_costs() {
    let original = decode_ordered_map(master_seed("carnival_event").unwrap()).unwrap();
    let mut projected = original.clone();
    project(&mut projected, "carnival_event").unwrap();
    let shops: Value =
        serde_json::from_str(include_str!("../../../../../assets/event_item_shop.json")).unwrap();
    let original_rows = rows(&original);
    let mut changes = 0;
    for (id, row) in rows(&projected) {
        let costs = shops["10"][id]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|item| item["costs"].as_array().unwrap())
            .map(|cost| cost["id"].as_i64().unwrap())
            .collect::<BTreeSet<_>>();
        let displayed = row[15..19]
            .iter()
            .filter_map(|id| id.parse::<i64>().ok())
            .collect::<BTreeSet<_>>();
        assert_eq!(displayed, costs, "carnival:{id}");
        for (index, (before, after)) in original_rows[id].iter().zip(row).enumerate() {
            if before != after {
                assert!((15..=17).contains(&index), "carnival:{id} field {index}");
                changes += 1;
            }
        }
    }
    assert_eq!(changes, 21);
}
// //// /全部土俑期别显示各自商店消费币且保留其它主表字段 ////

// //// 从实际资源差分读取客户端下载的活动主表 [@x380kkm 2026-10-02] ////
fn downloaded_carnival(database: &ServiceDatabase, base: &Path, overrides: &Path) -> OrderedValue {
    sync(database, base, overrides).unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(overrides.join("path")).unwrap()).unwrap();
    let entries = replay_diff_entries(base, overrides, &manifest, "1.4.55");
    let master = projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "carnival_event")
        .unwrap();
    decode_ordered_map(&entries[&master.entry_path]).unwrap()
}

// //// 原始期别与区域和管理日期均下载正确显示币 [@x380kkm 2026-10-02] ////
#[test]
fn downloads_currency_projection_with_native_regional_and_managed_windows() {
    let storage = TempDir::new().unwrap();
    let base = TempDir::new().unwrap();
    let overrides = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(storage.path()).unwrap();
    write_bundle(base.path(), false);
    for (now, dates) in [
        (
            "2023-05-26T04:00:00Z",
            [
                "2023-05-25 12:00:00",
                "2023-06-08 11:59:59",
                "2023-06-16 11:59:59",
            ],
        ),
        (
            "2021-08-01T04:00:00Z",
            [
                "2021-07-31 12:00:00",
                "2021-08-11 20:59:59",
                "2021-08-19 04:59:59",
            ],
        ),
    ] {
        database
            .set_virtual_time(true, parse_iso_timestamp(now).unwrap(), 1.0)
            .unwrap();
        let projected = downloaded_carnival(&database, base.path(), overrides.path());
        let row = rows(&projected)["2"];
        assert_eq!(&row[15..19], ["90010", "90020", "90030", "(None)"]);
        assert_eq!(&row[20..23], dates);
    }
    database.upsert_always_activity("carnival:2").unwrap();
    for enabled in [true, false] {
        if !enabled {
            database
                .upsert_manual_activity("carnival:2", false)
                .unwrap();
        }
        let window = active_windows(&database)
            .unwrap()
            .remove("carnival:2")
            .unwrap();
        let projected = downloaded_carnival(&database, base.path(), overrides.path());
        let row = rows(&projected)["2"];
        assert_eq!(&row[15..19], ["90010", "90020", "90030", "(None)"]);
        assert_eq!(
            &row[20..23],
            [TimeKind::Start, TimeKind::End, TimeKind::Reward]
                .map(|kind| projected_time(kind, &window))
        );
    }
}
// //// /原始期别与区域和管理日期均下载正确显示币 ////
