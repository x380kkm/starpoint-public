// audience: internal
// # activity-projection-tower-dungeon
// 幽玄域关卡窗口使用选中期别的十关, 其它期别放在父活动窗口之前.

use super::{
    mutate_row, projection_window, OrderedValue, ProjectionMaster, ProjectionWindow, MILLIS_PER_DAY,
};

// //// 将幽玄域开放窗口限定到完整一期 [@x380kkm 2026-09-23] ////
pub(super) fn project_quests(
    value: &mut OrderedValue,
    master: &ProjectionMaster,
    activity_key: &str,
    window: &ProjectionWindow,
    quest_ids: &[i64; 10],
) -> bool {
    let OrderedValue::Map(events) = value else {
        return false;
    };
    let Some((_, OrderedValue::Map(quests))) = events.iter_mut().find(|(id, _)| id == activity_key)
    else {
        return false;
    };
    let closed = projection_window(
        window.start_at_ms.saturating_sub(2 * MILLIS_PER_DAY),
        window.start_at_ms.saturating_sub(MILLIS_PER_DAY),
        window.expires_at_ms,
    );
    let mut changed = false;
    for (_, value) in quests {
        if let OrderedValue::Row(row) = value {
            let selected = row
                .first()
                .and_then(|id| id.parse::<i64>().ok())
                .is_some_and(|id| quest_ids.contains(&id));
            changed |= mutate_row(row, master, if selected { window } else { &closed });
        }
    }
    changed
}
// //// /将幽玄域开放窗口限定到完整一期 ////

#[cfg(test)]
mod tests {
    use super::super::*;
    use tempfile::TempDir;

    // //// 核对常驻和自定义窗口内始终只有当期十关 [@x380kkm 2026-09-23] ////
    #[test]
    fn projects_ten_tower_quests_for_game_time_and_custom_windows() {
        let root = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root.path()).unwrap();
        let master = projection_manifest()
            .unwrap()
            .masters
            .iter()
            .find(|master| master.name == "tower_dungeon_event_quest")
            .unwrap();
        let periods: serde_json::Value =
            serde_json::from_str(include_str!("../../assets/cn-tower-dungeon-periods.json"))
                .unwrap();
        let periods = periods["1"].as_array().unwrap();
        let first_start = periods[0]["starts_at_ms"].as_i64().unwrap();
        let middle_start = periods[24]["starts_at_ms"].as_i64().unwrap();
        let last_start = periods.last().unwrap()["starts_at_ms"].as_i64().unwrap();
        for (time, expected) in [
            (first_start - MILLIS_PER_DAY, 1001),
            (middle_start, 1241),
            (last_start + 400 * MILLIS_PER_DAY, 1471),
        ] {
            database.set_virtual_time(true, time, 1.0).unwrap();
            for custom in [false, true] {
                if custom {
                    database
                        .upsert_activity_schedule(
                            "tower-dungeon:1",
                            true,
                            time - MILLIS_PER_DAY,
                            time + 3 * MILLIS_PER_DAY,
                        )
                        .unwrap();
                } else {
                    database.upsert_always_activity("tower-dungeon:1").unwrap();
                }
                let windows = active_windows(&database).unwrap();
                let window = &windows["tower-dungeon:1"];
                let mut value = decode_ordered_map(master_seed(&master.name).unwrap()).unwrap();
                project_master_value(&mut value, master, "1", window);
                let OrderedValue::Map(events) = value else {
                    panic!("tower events")
                };
                let OrderedValue::Map(quests) = &events[0].1 else {
                    panic!("tower quests")
                };
                let now = format_client_time((time + MASTER_TIMEZONE_OFFSET_MS) / 1000);
                let mut active = quests
                    .iter()
                    .filter_map(|(_, value)| {
                        let OrderedValue::Row(row) = value else {
                            return None;
                        };
                        (row[5] <= now && now <= row[6]).then(|| row[0].parse::<i64>().unwrap())
                    })
                    .collect::<Vec<_>>();
                active.sort();
                assert_eq!(active, (expected..expected + 10).collect::<Vec<_>>());
                assert_eq!(active, window.tower_quest_ids.unwrap().to_vec());
                let enterable = crate::cn_quest::open_quest_keys(&database)
                    .unwrap()
                    .into_iter()
                    .filter_map(|(category, id)| (category == 20).then_some(id))
                    .collect::<Vec<_>>();
                assert_eq!(enterable, active);
            }
        }
        database
            .upsert_activity_schedule("tower-dungeon:1", false, first_start, last_start)
            .unwrap();
        assert!(crate::cn_quest::open_quest_keys(&database)
            .unwrap()
            .iter()
            .all(|(category, _)| *category != 20));
    }
    // //// /核对常驻和自定义窗口内始终只有当期十关 ////
}
