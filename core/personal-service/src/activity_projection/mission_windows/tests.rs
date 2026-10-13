// audience: internal
// # activity-mission-window-tests
// 团队战的任务时间与原始区域字段一致, 其他任务保持 CN 定义.

use super::*;
use crate::activity_projection::{
    active_windows, project_master_value, projection_window, regional,
};
use std::collections::BTreeSet;
use tempfile::TempDir;

fn rows(value: &OrderedValue) -> BTreeMap<String, &[String]> {
    let OrderedValue::Map(entries) = value else {
        panic!("mission master has rows");
    };
    entries
        .iter()
        .map(|(key, value)| {
            let OrderedValue::Row(row) = value else {
                panic!("mission definition is flat");
            };
            (key.clone(), row.as_slice())
        })
        .collect()
}

// //// 完整核对六期团队战的任务参与和展示日期 [@x380kkm 2026-09-28] ////
#[test]
fn projects_all_regional_raid_missions_without_changing_other_definitions() {
    let source = decode_ordered_map(master_seed("event_mission").unwrap()).unwrap();
    let original_rows = rows(&source);
    let periods: serde_json::Value = serde_json::from_str(regional::CATALOG).unwrap();
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let mut covered = BTreeSet::new();
    for event in 1..=6 {
        let activity_id = format!("raid:{event}");
        let now = periods[&activity_id]["start_at_ms"].as_i64().unwrap() + 1000;
        database.set_virtual_time(true, now, 1.0).unwrap();
        let period = regional::regional_activity_period(&activity_id, now)
            .unwrap()
            .unwrap();
        let mut projected = source.clone();
        project(
            &mut projected,
            "event_mission",
            &active_windows(&database).unwrap(),
        )
        .unwrap();
        let captured = event_mission_windows(&database, now).unwrap();
        for (id, row) in rows(&projected) {
            let Some(fields) = period.master_fields("event_mission", &id) else {
                assert_eq!(row, original_rows[&id], "unrelated mission {id}");
                continue;
            };
            covered.insert(id.clone());
            for (index, value) in row.iter().enumerate() {
                assert_eq!(
                    value,
                    fields.get(&index).unwrap_or(&original_rows[&id][index]),
                    "mission {id}, field {index}"
                );
            }
            let expected = [
                parse_master_time(&fields[&25]).unwrap(),
                parse_master_time(&fields[&26]).unwrap(),
            ];
            assert_eq!(captured[&id.parse::<i64>().unwrap()], expected);
        }
    }
    assert_eq!(covered.len(), 92);
    database.set_virtual_time(true, 1639659660000, 1.0).unwrap();
    assert!(!event_mission_windows(&database, 1639659660000)
        .unwrap()
        .contains_key(&400001));
}
// //// /完整核对六期团队战的任务参与和展示日期 ////

// //// 显式团队战开关只投影关联的团队战任务 [@x380kkm 2026-09-28] ////
#[test]
fn filters_raid_missions_before_managed_projection() {
    let catalog = catalog("event_mission").unwrap();
    let source = decode_ordered_map(master_seed("event_mission").unwrap()).unwrap();
    let original_rows = rows(&source);
    let mut projected = source.clone();
    let window = projection_window(1_900_000_000_000, 1_900_086_400_000, i64::MAX);
    assert!(project_master_value(
        &mut projected,
        catalog.master,
        "1",
        &window
    ));
    let windows = BTreeMap::from([("raid:1".to_owned(), window)]);
    project(&mut projected, "event_mission", &windows).unwrap();
    let mut changed = 0;
    for (id, row) in rows(&projected) {
        if row[7] == "16" && row[8] == "1" {
            changed += 1;
            assert_eq!(parse_master_time(&row[25]), Some(1_900_000_000_000));
            assert_eq!(parse_master_time(&row[26]), Some(1_900_086_400_000));
        } else {
            assert_eq!(row, original_rows[&id], "unrelated mission {id}");
        }
    }
    assert_eq!(changed, 12);
}
// //// /显式团队战开关只投影关联的团队战任务 ////
