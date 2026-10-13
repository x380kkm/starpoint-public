// audience: internal
// # activity-projection-character-election-tests
// 投票日期投影与管理窗口一致, 角色候选与活动图片保持原主表内容.

use super::*;
use tempfile::TempDir;

// //// 投票活动按管理日期重新开放并保留原资源引用 [@x380kkm 2026-09-23] ////
#[test]
fn projects_election_window_without_changing_identity_or_artwork() {
    let root = TempDir::new().unwrap();
    let mut database = ServiceDatabase::open(root.path()).unwrap();
    let start = 1_900_000_000_000;
    let end = start + MILLIS_PER_DAY;
    database
        .upsert_activity_schedule("character-election:1", true, start, end)
        .unwrap();
    database.set_virtual_time(true, start + 1_000, 1.0).unwrap();
    let windows = active_windows(&database).unwrap();
    let window = windows.get("character-election:1").unwrap();
    let master = projection_manifest()
        .unwrap()
        .masters
        .iter()
        .find(|master| master.name == "character_election")
        .unwrap();
    let mut projected = decode_ordered_map(master_seed(&master.name).unwrap()).unwrap();
    let original = projected.clone();
    assert!(project_master_value(&mut projected, master, "1", window));
    let OrderedValue::Map(original_rows) = original else {
        panic!("election master contains rows")
    };
    let OrderedValue::Map(projected_rows) = &projected else {
        panic!("projected master contains rows")
    };
    let OrderedValue::Row(before) = &original_rows[0].1 else {
        panic!("election contains fields")
    };
    let OrderedValue::Row(after) = &projected_rows[0].1 else {
        panic!("projected election contains fields")
    };
    assert_eq!(&after[..2], &before[..2]);
    assert_eq!(after[2], projected_time(TimeKind::Start, window));
    assert_eq!(after[3], projected_time(TimeKind::End, window));
    assert_eq!(
        decode_ordered_map(&encode_ordered_map(&projected).unwrap()).unwrap(),
        projected
    );
    database.set_virtual_time(true, end + 1_000, 1.0).unwrap();
    let expired = active_windows(&database).unwrap();
    assert_eq!(expired["character-election:1"].end_at_ms, end);
    assert!(
        expired["character-election:1"].end_at_ms < database.current_server_time_millis().unwrap()
    );
}
