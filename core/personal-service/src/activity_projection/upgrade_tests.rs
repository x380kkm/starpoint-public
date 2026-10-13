// audience: internal
// # activity-projection-upgrade-tests
//
// 该模块用可安装 ZIP 和已有投影验证包内资源升级, 活动规则保留和重启幂等.

use super::*;
use tempfile::TempDir;

// //// 读取虚拟时钟的持久化锚点 [@x380kkm 2026-09-20] ////
fn stored_clock(root: &Path) -> (i64, i64, i64, f64) {
    rusqlite::Connection::open(root.join("personal-service.sqlite3")).unwrap()
        .query_row("SELECT enabled, virtual_anchor_ms, real_anchor_ms, rate FROM virtual_time WHERE id = 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        }).unwrap()
}
// //// /读取虚拟时钟的持久化锚点 ////

// //// 创建带实体清单的包内差分 [@x380kkm 2026-09-20] ////
pub(super) fn write_bundle(root: &Path, repaired: bool) {
    fs::create_dir_all(root.join(COMMON_DIFF_DIRECTORY)).unwrap();
    fs::create_dir_all(root.join("entities")).unwrap();
    let old_archive = build_zip(&[("old.atf".to_owned(), b"old".to_vec())]).unwrap();
    fs::write(
        root.join(COMMON_DIFF_DIRECTORY).join("original.zip"),
        &old_archive,
    )
    .unwrap();
    let mut diffs = vec![json!({
        "version": "1.4.55", "original_version": "1.4.54",
        "archive": [{"location": "archive-common-diff/original.zip", "size": old_archive.len(), "sha256": STANDARD.encode(Sha256::digest(&old_archive))}]
    })];
    let version = if repaired { "1.4.56" } else { "1.4.55" };
    let mut csv = format!(
        "old.atf,1.4.55,3,{},common\n",
        URL_SAFE_NO_PAD.encode(Sha256::digest(b"old"))
    );
    if repaired {
        let master = projection_manifest()
            .unwrap()
            .masters
            .iter()
            .find(|master| master.name == "advent_event")
            .unwrap();
        let repaired = build_zip(&[
            ("old.atf".to_owned(), b"repaired".to_vec()),
            ("new.atf".to_owned(), b"added".to_vec()),
            (
                master.entry_path.clone(),
                master_seed("advent_event").unwrap().to_vec(),
            ),
        ])
        .unwrap();
        fs::write(
            root.join(COMMON_DIFF_DIRECTORY).join("repair.zip"),
            &repaired,
        )
        .unwrap();
        diffs.push(json!({
            "version": version, "original_version": "1.4.55",
            "archive": [{"location": "archive-common-diff/repair.zip", "size": repaired.len(), "sha256": STANDARD.encode(Sha256::digest(&repaired))}]
        }));
        csv = format!(
            "old.atf,1.4.56,8,{},common\nnew.atf,1.4.56,5,{},common\n",
            URL_SAFE_NO_PAD.encode(Sha256::digest(b"repaired")),
            URL_SAFE_NO_PAD.encode(Sha256::digest(b"added"))
        );
    }
    fs::write(root.join("path"), serde_json::to_vec(&json!({
        "info": {"client_asset_version": "1.4.0", "target_asset_version": version, "eventual_target_asset_version": version, "is_initial": true, "latest_maj_first_version": "1.4.0"},
        "full": {"version": "1.4.0", "archive": []}, "diff": diffs, "asset_version_hash": "bundle"
    })).unwrap()).unwrap();
    for name in [
        "PathFile.csv",
        "10939-ios_medium.csv",
        "10939-android_medium.csv",
    ] {
        fs::write(root.join("entities").join(name), &csv).unwrap();
    }
}
// //// /创建带实体清单的包内差分 ////

// //// 按客户端版本链解开未压缩测试归档 [@x380kkm 2026-09-21] ////
pub(super) fn replay_diff_entries(
    base: &Path,
    overrides: &Path,
    path: &Value,
    client_version: &str,
) -> BTreeMap<String, Vec<u8>> {
    let mut entries = BTreeMap::new();
    let groups = path["diff"].as_array().unwrap();
    let mut version = client_version;
    let mut visited = BTreeSet::new();
    while version
        != path["info"]["eventual_target_asset_version"]
            .as_str()
            .unwrap()
    {
        assert!(visited.insert(version));
        let group = groups
            .iter()
            .rev()
            .find(|group| group["original_version"] == version)
            .expect("installed version reaches the target through complete transitions");
        for archive in group["archive"].as_array().unwrap() {
            let relative = archive["location"].as_str().unwrap();
            let file = if overrides.join(relative).is_file() {
                overrides.join(relative)
            } else {
                base.join(relative)
            };
            let bytes = fs::read(file).unwrap();
            let mut offset = 0;
            while bytes[offset..offset + 4] == [0x50, 0x4b, 0x03, 0x04] {
                assert_eq!(
                    u16::from_le_bytes(bytes[offset + 8..offset + 10].try_into().unwrap()),
                    0
                );
                let size = u32::from_le_bytes(bytes[offset + 18..offset + 22].try_into().unwrap())
                    as usize;
                let name_length =
                    u16::from_le_bytes(bytes[offset + 26..offset + 28].try_into().unwrap())
                        as usize;
                let extra_length =
                    u16::from_le_bytes(bytes[offset + 28..offset + 30].try_into().unwrap())
                        as usize;
                let name_start = offset + 30;
                let data_start = name_start + name_length + extra_length;
                let name = String::from_utf8(bytes[name_start..name_start + name_length].to_vec())
                    .unwrap();
                entries.insert(name, bytes[data_start..data_start + size].to_vec());
                offset = data_start + size;
            }
        }
        version = group["version"].as_str().unwrap();
    }
    entries
}
// //// /按客户端版本链解开未压缩测试归档 ////

// //// 验证已有投影接续新包资源并保持活动规则 [@x380kkm 2026-09-20] ////
#[test]
fn upgrades_bundles_after_equal_or_higher_projection_versions() {
    for client_version in ["1.4.56", "1.4.80"] {
        let storage = TempDir::new().unwrap();
        let base = TempDir::new().unwrap();
        let overrides = TempDir::new().unwrap();
        let mut database = ServiceDatabase::open(storage.path()).unwrap();
        database
            .upsert_activity_schedule("advent:1", true, OPEN_RULE_START_MS, OPEN_RULE_END_MS)
            .unwrap();
        database
            .upsert_activity_schedule("daily-week:1", false, OPEN_RULE_START_MS, OPEN_RULE_END_MS)
            .unwrap();
        write_bundle(base.path(), false);
        sync(&database, base.path(), overrides.path()).unwrap();
        let mut old_path: Value =
            serde_json::from_slice(&fs::read(overrides.path().join("path")).unwrap()).unwrap();
        old_path["info"]["target_asset_version"] = json!(client_version);
        old_path["info"]["eventual_target_asset_version"] = json!(client_version);
        old_path["diff"].as_array_mut().unwrap().last_mut().unwrap()["version"] =
            json!(client_version);
        atomic_write_json(&overrides.path().join("path"), &old_path).unwrap();
        let mut old_state: Value = serde_json::from_slice(
            &fs::read(overrides.path().join(PROJECTION_STATE_NAME)).unwrap(),
        )
        .unwrap();
        old_state.as_object_mut().unwrap().remove("base_signature");
        atomic_write_json(&overrides.path().join(PROJECTION_STATE_NAME), &old_state).unwrap();
        let clock = stored_clock(storage.path());
        let schedules = database.list_activity_schedules().unwrap();
        write_bundle(base.path(), true);
        sync(&database, base.path(), overrides.path()).unwrap();
        let path_bytes = fs::read(overrides.path().join("path")).unwrap();
        let path: Value = serde_json::from_slice(&path_bytes).unwrap();
        let repair_version = increment_version(client_version).unwrap();
        let target_version = increment_version(&repair_version).unwrap();
        assert_eq!(path["info"]["target_asset_version"], target_version);
        let last = path["diff"].as_array().unwrap();
        assert_eq!(last[last.len() - 2]["original_version"], client_version);
        assert_eq!(last[last.len() - 2]["version"], repair_version);
        assert_eq!(last.last().unwrap()["original_version"], repair_version);
        let entries = replay_diff_entries(base.path(), overrides.path(), &path, client_version);
        assert_eq!(entries["old.atf"], b"repaired");
        assert_eq!(entries["new.atf"], b"added");
        let master = projection_manifest()
            .unwrap()
            .masters
            .iter()
            .find(|master| master.name == "advent_event")
            .unwrap();
        let mut expected = decode_ordered_map(master_seed("advent_event").unwrap()).unwrap();
        let windows = active_windows(&database).unwrap();
        project_master_value(&mut expected, master, "1", &windows["advent:1"]);
        assert_eq!(
            decode_ordered_map(&entries[&master.entry_path]).unwrap(),
            expected
        );
        for name in [
            "PathFile.csv",
            "10939-ios_medium.csv",
            "10939-android_medium.csv",
        ] {
            let csv = fs::read_to_string(overrides.path().join("entities").join(name)).unwrap();
            assert!(csv.contains(&format!(
                "old.atf,{repair_version},8,{},common",
                URL_SAFE_NO_PAD.encode(Sha256::digest(b"repaired"))
            )));
            assert!(csv.contains(&format!(
                "new.atf,{repair_version},5,{},common",
                URL_SAFE_NO_PAD.encode(Sha256::digest(b"added"))
            )));
            assert!(csv.contains(&format!("{},{target_version},", master.entry_path)));
        }
        assert_eq!(stored_clock(storage.path()), clock);
        assert_eq!(database.list_activity_schedules().unwrap(), schedules);
        let state_bytes = fs::read(overrides.path().join(PROJECTION_STATE_NAME)).unwrap();
        drop(database);
        let database = ServiceDatabase::open(storage.path()).unwrap();
        sync(&database, base.path(), overrides.path()).unwrap();
        assert_eq!(fs::read(overrides.path().join("path")).unwrap(), path_bytes);
        assert_eq!(
            fs::read(overrides.path().join(PROJECTION_STATE_NAME)).unwrap(),
            state_bytes
        );
        for platform in [
            crate::cn_asset::ClientPlatform::Ios,
            crate::cn_asset::ClientPlatform::Android,
        ] {
            let version = crate::cn_asset::available_asset_version(
                &database,
                base.path(),
                overrides.path(),
                Some(&target_version),
                platform,
                &mut crate::cn_asset::ArchiveDigestCache::default(),
            )
            .unwrap();
            assert_eq!(version, target_version);
        }
    }
}
// //// /验证已有投影接续新包资源并保持活动规则 ////

// //// 从运行中的服务读取平台下载清单 [@x380kkm 2026-09-20] ////
fn request_path(service: &crate::PersonalService, client_version: &str, device: &str) -> Value {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", service.port())).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let body = STANDARD.encode(rmp_serde::to_vec_named(&json!({})).unwrap());
    write!(stream, "POST /api/index.php/asset/get_path HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/x-www-form-urlencoded\r\nRES_VER: {client_version}\r\nDEVICE: {device}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", service.port(), body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let encoded = response.split_once("\r\n\r\n").unwrap().1;
    let envelope: Value = rmp_serde::from_slice(&STANDARD.decode(encoded).unwrap()).unwrap();
    envelope["data"].clone()
}
// //// /从运行中的服务读取平台下载清单 ////

// //// 验证未登记平台覆盖归档与升级差分共享连续版本链 [@x380kkm 2026-09-20] ////
#[test]
fn upgrades_after_unregistered_platform_overrides_and_reuses_the_installed_version() {
    let storage = TempDir::new().unwrap();
    let base = TempDir::new().unwrap();
    let overrides = storage.path().join("cdn/override");
    let database = ServiceDatabase::open(storage.path()).unwrap();
    write_bundle(base.path(), false);
    sync(&database, base.path(), &overrides).unwrap();
    for (directory, name) in [
        ("archive-ios-diff", "starpoint-ios-voice-overlay-custom.zip"),
        (
            "archive-android-diff",
            "starpoint-cn-android-custom-overlay.zip",
        ),
    ] {
        fs::create_dir_all(overrides.join(directory)).unwrap();
        fs::write(
            overrides.join(directory).join(name),
            build_zip(&[(format!("{directory}.atf"), b"custom".to_vec())]).unwrap(),
        )
        .unwrap();
    }
    let user_file = overrides.join("custom-player-file.txt");
    fs::write(&user_file, b"preserved").unwrap();
    for platform in [
        crate::cn_asset::ClientPlatform::Ios,
        crate::cn_asset::ClientPlatform::Android,
    ] {
        assert_eq!(
            crate::cn_asset::available_asset_version(
                &database,
                base.path(),
                &overrides,
                Some("1.4.56"),
                platform,
                &mut crate::cn_asset::ArchiveDigestCache::default()
            )
            .unwrap(),
            "1.4.57"
        );
    }
    write_bundle(base.path(), true);
    sync(&database, base.path(), &overrides).unwrap();
    let path: Value = serde_json::from_slice(&fs::read(overrides.join("path")).unwrap()).unwrap();
    assert_eq!(path["info"]["target_asset_version"], "1.4.59");
    assert_eq!(path["diff"][2]["version"], "1.4.57");
    assert_eq!(path["diff"][3]["original_version"], "1.4.57");
    assert_eq!(path["diff"][3]["version"], "1.4.58");
    let replay = replay_diff_entries(base.path(), &overrides, &path, "1.4.57");
    assert_eq!(replay["old.atf"], b"repaired");
    assert_eq!(replay["new.atf"], b"added");
    assert_eq!(fs::read(&user_file).unwrap(), b"preserved");
    drop(database);
    let service =
        crate::PersonalService::start_with_cdn_root(storage.path(), 0, base.path()).unwrap();
    for (device, selected, excluded) in [
        ("1", "archive-ios-diff", "archive-android-diff"),
        ("2", "archive-android-diff", "archive-ios-diff"),
    ] {
        let path = request_path(&service, "1.4.56", device);
        assert!(path["full"].is_null());
        let locations = path["diff"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["archive"].as_array().unwrap())
            .map(|archive| archive["location"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(locations.iter().any(|location| location.contains(selected)));
        assert!(locations
            .iter()
            .all(|location| !location.contains(excluded)));
        assert!(locations
            .iter()
            .any(|location| location.ends_with("repair.zip")));
        let repaired = request_path(&service, "1.4.57", device);
        assert!(repaired["full"].is_null());
        let repairs = repaired["diff"].as_array().unwrap();
        assert_eq!(repairs.len(), 2);
        assert_eq!(repairs[0]["original_version"], "1.4.57");
        assert!(repairs
            .iter()
            .flat_map(|group| group["archive"].as_array().unwrap())
            .all(|archive| !archive["location"].as_str().unwrap().contains(selected)));
        let current = request_path(&service, "1.4.59", device);
        assert!(current["full"].is_null());
        assert!(current["diff"].is_null());
    }
    service.stop().unwrap();
}
// //// /验证未登记平台覆盖归档与升级差分共享连续版本链 ////

// //// 验证同名差分内容变化以更高版本重新提供 [@x380kkm 2026-09-20] ////
#[test]
fn publishes_changed_archive_content_at_a_new_version() {
    let storage = TempDir::new().unwrap();
    let base = TempDir::new().unwrap();
    let overrides = TempDir::new().unwrap();
    let database = ServiceDatabase::open(storage.path()).unwrap();
    write_bundle(base.path(), true);
    sync(&database, base.path(), overrides.path()).unwrap();
    let replacement = build_zip(&[
        ("old.atf".to_owned(), b"corrected-again".to_vec()),
        ("new.atf".to_owned(), b"added".to_vec()),
    ])
    .unwrap();
    fs::write(
        base.path().join(COMMON_DIFF_DIRECTORY).join("repair.zip"),
        &replacement,
    )
    .unwrap();
    let mut manifest = read_bundle_manifest(base.path()).unwrap().value;
    manifest["diff"][1]["archive"][0]["size"] = json!(replacement.len());
    manifest["diff"][1]["archive"][0]["sha256"] =
        json!(STANDARD.encode(Sha256::digest(&replacement)));
    atomic_write_json(&base.path().join("path"), &manifest).unwrap();
    let csv = format!(
        "old.atf,1.4.56,15,{},common\nnew.atf,1.4.56,5,{},common\n",
        URL_SAFE_NO_PAD.encode(Sha256::digest(b"corrected-again")),
        URL_SAFE_NO_PAD.encode(Sha256::digest(b"added"))
    );
    for name in [
        "PathFile.csv",
        "10939-ios_medium.csv",
        "10939-android_medium.csv",
    ] {
        fs::write(base.path().join("entities").join(name), &csv).unwrap();
    }
    sync(&database, base.path(), overrides.path()).unwrap();
    let path: Value =
        serde_json::from_slice(&fs::read(overrides.path().join("path")).unwrap()).unwrap();
    assert_eq!(path["info"]["target_asset_version"], "1.4.59");
    let archives = path["diff"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["archive"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        archives
            .iter()
            .filter(|archive| archive["location"]
                .as_str()
                .unwrap()
                .ends_with("repair.zip"))
            .count(),
        1
    );
    assert_eq!(
        replay_diff_entries(base.path(), overrides.path(), &path, "1.4.57")["old.atf"],
        b"corrected-again"
    );
}
// //// /验证同名差分内容变化以更高版本重新提供 ////

// //// 验证旧同名覆盖保持原文件而新包修复使用独立归档 [@x380kkm 2026-09-20] ////
#[test]
fn delivers_bundle_repairs_without_overwriting_custom_archives() {
    let storage = TempDir::new().unwrap();
    let base = TempDir::new().unwrap();
    let overrides = TempDir::new().unwrap();
    let database = ServiceDatabase::open(storage.path()).unwrap();
    write_bundle(base.path(), false);
    sync(&database, base.path(), overrides.path()).unwrap();
    let custom = build_zip(&[("old.atf".to_owned(), b"custom-old".to_vec())]).unwrap();
    let shadow = overrides
        .path()
        .join(COMMON_DIFF_DIRECTORY)
        .join("repair.zip");
    fs::write(&shadow, &custom).unwrap();
    write_bundle(base.path(), true);
    sync(&database, base.path(), overrides.path()).unwrap();
    let bytes = fs::read(overrides.path().join("path")).unwrap();
    let path: Value = serde_json::from_slice(&bytes).unwrap();
    let replay = replay_diff_entries(base.path(), overrides.path(), &path, "1.4.56");
    assert_eq!(replay["old.atf"], b"repaired");
    assert_eq!(replay["new.atf"], b"added");
    assert_eq!(fs::read(&shadow).unwrap(), custom);
    let repair = &path["diff"][2]["archive"][0];
    assert!(repair["location"]
        .as_str()
        .unwrap()
        .contains("starpoint-cn-bundle-"));
    assert_eq!(repair["bundle_location"], "archive-common-diff/repair.zip");
    sync(&database, base.path(), overrides.path()).unwrap();
    assert_eq!(fs::read(overrides.path().join("path")).unwrap(), bytes);
    assert_eq!(
        crate::cn_asset::available_asset_version(
            &database,
            base.path(),
            overrides.path(),
            Some("1.4.58"),
            crate::cn_asset::ClientPlatform::Ios,
            &mut crate::cn_asset::ArchiveDigestCache::default()
        )
        .unwrap(),
        "1.4.58"
    );
}
// //// /验证旧同名覆盖保持原文件而新包修复使用独立归档 ////
