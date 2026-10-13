// audience: internal
// # personal-service-player-history-projection
// 回忆录统计截止时间保持与展示开始时间的主表间隔. 客户端结束时间包含最后一个整秒.

use super::{format_master_time, ProjectionWindow, MASTER_TIMEZONE_OFFSET_MS};
use crate::database::parse_iso_timestamp;

// //// 将回忆录展示窗口和统计截止时间投影到主表 [@x380kkm 2026-09-25] ////
pub(super) fn project_period(row: &mut [String], window: &ProjectionWindow) -> bool {
    if row.len() < 4 {
        return false;
    }
    let Some(aggregation) = row.get(1).and_then(|value| parse_master_time(value)) else {
        return false;
    };
    let Some(start) = row.get(2).and_then(|value| parse_master_time(value)) else {
        return false;
    };
    let offset = aggregation.saturating_sub(start);
    let (start, end) = if window.start_at_ms < window.end_at_ms {
        (window.start_at_ms, window.end_at_ms.saturating_sub(1_000))
    } else {
        (0, 0)
    };
    let replacements = [
        format_master_time(start.saturating_add(offset)),
        format_master_time(start),
        format_master_time(end),
    ];
    let mut changed = false;
    for (value, replacement) in row[1..4].iter_mut().zip(replacements) {
        changed |= *value != replacement;
        *value = replacement;
    }
    changed
}

fn parse_master_time(value: &str) -> Option<i64> {
    parse_iso_timestamp(&format!("{}Z", value.replace(' ', "T")))
        .map(|timestamp| timestamp.saturating_sub(MASTER_TIMEZONE_OFFSET_MS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_projection::{
        active_windows, decode_ordered_map, master_seed, project_master_value, projection_manifest,
        OrderedValue,
    };
    use crate::database::ServiceDatabase;
    use tempfile::TempDir;

    // //// 独立改期保留统计间隔及原始回忆录标识 [@x380kkm 2026-09-25] ////
    #[test]
    fn projects_history_independently_with_its_original_aggregation_offset() {
        let root = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(root.path()).unwrap();
        let start = 1_893_456_000_000;
        let end = start + 10 * 86_400_000;
        database.set_virtual_time(true, start, 0.0001).unwrap();
        assert!(!active_windows(&database)
            .unwrap()
            .contains_key("player-history:1"));
        database
            .upsert_activity_schedule("player-history:1", true, start, end)
            .unwrap();
        let windows = active_windows(&database).unwrap();
        assert!(!windows.contains_key("collect-item:100009"));
        let master = projection_manifest()
            .unwrap()
            .masters
            .iter()
            .find(|master| master.name == "player_history")
            .unwrap();
        let mut value = decode_ordered_map(master_seed(&master.name).unwrap()).unwrap();
        assert!(project_master_value(
            &mut value,
            master,
            "1",
            &windows["player-history:1"]
        ));
        let OrderedValue::Map(entries) = value else {
            panic!("history map is missing")
        };
        let (_, OrderedValue::Row(row)) = entries.iter().find(|(key, _)| key == "1").unwrap()
        else {
            panic!("history row is missing")
        };
        assert_eq!(row[0], "player_history");
        assert_eq!(row[1], format_master_time(start - 28 * 86_400_000));
        assert_eq!(row[2], format_master_time(start));
        assert_eq!(row[3], format_master_time(end - 1_000));
        let mut row = row.clone();
        let empty = super::super::projection_window(start, start, i64::MAX);
        assert!(project_period(&mut row, &empty));
        assert_eq!(row[2], format_master_time(0));
        assert_eq!(row[3], format_master_time(0));
    }
}
