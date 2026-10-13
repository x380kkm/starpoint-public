// audience: internal
// # boss-release-projection-tests
// 包内领主开放日通过资源差分到达客户端, 已有投影缓存按主表集合更新.

use super::tests::collect_rows;
use super::upgrade_tests::{replay_diff_entries, write_bundle};
use super::*;
use tempfile::TempDir;

// //// 按关卡编号读取领主主表行 [@x380kkm 2026-09-28] ////
fn boss_row<'a>(value: &'a OrderedValue, quest_id: &str) -> &'a [String] {
    let mut rows = Vec::new();
    collect_rows(value, &mut rows);
    rows.into_iter().find(|row| row[0] == quest_id).unwrap()
}

// //// 保存仅含活动主表的客户端缓存 [@x380kkm 2026-09-28] ////
fn cache_activity_projection(database: &ServiceDatabase, base: &Path, overrides: &Path) -> String {
    sync(database, base, overrides).unwrap();
    let mut state = read_state(overrides).unwrap();
    let boss = quest_windows::boss_master().unwrap();
    let mut path: Value =
        serde_json::from_slice(&fs::read(overrides.join("path")).unwrap()).unwrap();
    let entries = replay_diff_entries(base, overrides, &path, "1.4.55")
        .into_iter()
        .filter(|(name, _)| name != &boss.entry_path)
        .collect::<Vec<_>>();
    let archive = build_zip(&entries).unwrap();
    let relative = format!(
        "{COMMON_DIFF_DIRECTORY}/{}",
        state.archive_name.as_ref().unwrap()
    );
    fs::write(overrides.join(relative), &archive).unwrap();
    let descriptor = &mut path["diff"].as_array_mut().unwrap().last_mut().unwrap()["archive"][0];
    descriptor["size"] = json!(archive.len());
    descriptor["sha256"] = json!(STANDARD.encode(Sha256::digest(&archive)));
    atomic_write_json(&overrides.join("path"), &path).unwrap();
    state.touched_master_names.retain(|name| name != &boss.name);
    let touched = state.touched_master_names.iter().cloned().collect();
    state.signature = Some(projection_signature(
        &active_windows(database).unwrap(),
        &touched,
        &read_bundle_manifest(base).unwrap().signature,
        projection_manifest().unwrap(),
    ));
    atomic_write_json(&overrides.join(PROJECTION_STATE_NAME), &state).unwrap();
    state.target_version.unwrap()
}

// //// 在新安装和缓存升级中投递领主日期并保留管理覆盖 [@x380kkm 2026-09-28] ////
#[test]
fn projects_official_boss_dates_on_install_and_cached_upgrade() {
    for has_cached_projection in [false, true] {
        let storage = TempDir::new().unwrap();
        let base = TempDir::new().unwrap();
        let overrides = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(storage.path()).unwrap();
        let release_at = parse_master_time("2023-12-14 11:00:00").unwrap();
        database
            .set_virtual_time(true, release_at - 1_000, 0.0001)
            .unwrap();
        write_bundle(base.path(), false);
        let installed_version = if has_cached_projection {
            cache_activity_projection(&database, base.path(), overrides.path())
        } else {
            "1.4.55".to_owned()
        };
        sync(&database, base.path(), overrides.path()).unwrap();
        let path_bytes = fs::read(overrides.path().join("path")).unwrap();
        let path: Value = serde_json::from_slice(&path_bytes).unwrap();
        assert_eq!(
            path["info"]["target_asset_version"],
            increment_version(&installed_version).unwrap()
        );
        let entries = replay_diff_entries(base.path(), overrides.path(), &path, &installed_version);
        let boss = quest_windows::boss_master().unwrap();
        let master = decode_ordered_map(&entries[&boss.entry_path]).unwrap();
        for (quest_id, date) in [
            ("1006003", "2020-01-15 11:00:00"),
            ("1002004", "2023-05-31 11:00:00"),
            ("1035004", "2023-12-14 11:00:00"),
            ("1060004", "2024-01-09 11:00:00"),
            ("1022004", "2025-02-20 12:00:00"),
        ] {
            assert_eq!(boss_row(&master, quest_id)[5], date);
        }
        assert!(!is_quest_open(&database, 2, 1035004).unwrap());
        database.set_virtual_time(true, release_at, 0.0001).unwrap();
        assert!(is_quest_open(&database, 2, 1035004).unwrap());
        sync(&database, base.path(), overrides.path()).unwrap();
        assert_eq!(fs::read(overrides.path().join("path")).unwrap(), path_bytes);

        database
            .upsert_manual_activity("quest:2:1035004", false)
            .unwrap();
        sync(&database, base.path(), overrides.path()).unwrap();
        let closed_path: Value =
            serde_json::from_slice(&fs::read(overrides.path().join("path")).unwrap()).unwrap();
        let closed_entries = replay_diff_entries(
            base.path(),
            overrides.path(),
            &closed_path,
            path["info"]["target_asset_version"].as_str().unwrap(),
        );
        let closed = decode_ordered_map(&closed_entries[&boss.entry_path]).unwrap();
        let row = boss_row(&closed, "1035004");
        assert_eq!(
            row[5],
            projected_time(TimeKind::Start, &quest_windows::closed_window())
        );
        assert_eq!(
            row[6],
            projected_time(TimeKind::End, &quest_windows::closed_window())
        );
        assert!(!is_quest_open(&database, 2, 1035004).unwrap());
    }
}
