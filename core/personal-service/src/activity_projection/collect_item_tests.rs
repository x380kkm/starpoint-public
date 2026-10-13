// audience: internal
// # personal-service-collect-item-projection-tests
//
// 收集活动任务以行内 event_id 关联父活动, 展示和生效窗口使用父活动开放区间.

use super::ordered_map::decode_ordered_map;
use super::{
    master_seed, project_master_value, projected_time, projection_manifest, projection_window,
    OrderedValue, TimeKind,
};
use std::collections::BTreeSet;

// //// 验证每个收集活动只同步关联任务窗口 [@x380kkm 2026-09-21] ////
#[test]
fn projects_collect_item_mission_windows_by_parent_event() {
    let master = projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "collect_item_event_mission")
        .unwrap();
    let seed = decode_ordered_map(master_seed(&master.name).unwrap()).unwrap();
    let OrderedValue::Map(seed_entries) = &seed else {
        panic!("collect item mission seed is not a map");
    };
    let event_ids = seed_entries
        .iter()
        .map(|(_, value)| match value {
            OrderedValue::Row(row) => row[0].clone(),
            _ => panic!("collect item mission row is not flat"),
        })
        .collect::<BTreeSet<_>>();
    let window = projection_window(2_524_608_000_000, 2_524_694_400_000, i64::MAX);
    for event_id in event_ids {
        let mut projected = seed.clone();
        assert!(project_master_value(
            &mut projected,
            master,
            &event_id,
            &window
        ));
        let OrderedValue::Map(entries) = projected else {
            panic!("projected collect item mission is not a map");
        };
        for ((key, value), (_, original)) in entries.iter().zip(seed_entries) {
            let (OrderedValue::Row(row), OrderedValue::Row(original)) = (value, original) else {
                panic!("projected collect item mission row is not flat");
            };
            for (index, field) in row.iter().enumerate() {
                let expected = if row[0] == event_id {
                    match index {
                        27 | 29 => projected_time(TimeKind::Start, &window),
                        28 | 30 => projected_time(TimeKind::End, &window),
                        _ => original[index].clone(),
                    }
                } else {
                    original[index].clone()
                };
                assert_eq!(
                    *field, expected,
                    "event {event_id}, mission {key}, field {index}"
                );
            }
        }
    }
}
// //// /验证每个收集活动只同步关联任务窗口 ////
