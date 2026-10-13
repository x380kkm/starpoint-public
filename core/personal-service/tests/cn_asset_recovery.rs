// audience: internal
// # cn-asset-recovery-tests
// 这些检查重放版本查询, 缺失清单读取和按摘要恢复文件的客户端请求链.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use cn_support::{decode_response, encode_request};
use serde_json::json;
use sha2::{Digest, Sha256};
use starpoint_personal_service::PersonalService;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use support::{request, request_bytes, request_with_headers};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

const ENTITY: &str = "production/small_upload/ab/01234567890123456789012345678901234567";

fn digest(body: &[u8]) -> String {
    STANDARD_NO_PAD
        .encode(Sha256::digest(body))
        .replace('+', "_")
        .replace('/', "-")
}

fn write_manifest(root: &Path, name: &str, body: &[u8], digest: &str) {
    fs::create_dir_all(root.join("entities")).unwrap();
    fs::write(
        root.join("entities").join(name),
        format!("{ENTITY},1.4.57,{},{digest},common\n", body.len()),
    )
    .unwrap();
}

fn write_archive(root: &Path, file_name: &str, body: &[u8], compression: zip::CompressionMethod) {
    let directory = root.join("archive-common-diff");
    fs::create_dir_all(&directory).unwrap();
    let mut archive = zip::ZipWriter::new(File::create(directory.join(file_name)).unwrap());
    archive
        .start_file(
            ENTITY,
            SimpleFileOptions::default().compression_method(compression),
        )
        .unwrap();
    archive.write_all(body).unwrap();
    archive.finish().unwrap();
}

fn body(response: &[u8]) -> &[u8] {
    let offset = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    &response[offset..]
}

// //// 从两平台恢复清单补取已从客户端缓存丢失的实际内容 [@x380kkm 2026-09-20] ////
#[test]
fn recovers_a_missing_entity_through_the_client_protocol() {
    let root = TempDir::new().unwrap();
    let cdn = root.path().join("cdn/cn");
    let bytes = b"\x89png\r\n\x1a\nmissing cached image\x00\xff";
    let hash = digest(bytes);
    for name in ["10939-ios_medium.csv", "10939-android_medium.csv"] {
        write_manifest(&cdn, name, bytes, &hash);
    }
    write_archive(
        &cdn,
        "older.zip",
        &vec![b'x'; bytes.len()],
        zip::CompressionMethod::Stored,
    );
    write_archive(&cdn, "current.zip", bytes, zip::CompressionMethod::Deflated);
    fs::write(
        cdn.join("archive-common-diff/unused-backup.zip"),
        b"invalid ZIP outside the published list",
    )
    .unwrap();
    let archives: Vec<_> = ["older.zip", "current.zip"]
        .into_iter()
        .map(|name| {
            let archive = fs::read(cdn.join("archive-common-diff").join(name)).unwrap();
            json!({
                "location": format!("archive-common-diff/{name}"),
                "size": archive.len(),
                "sha256": STANDARD.encode(Sha256::digest(&archive))
            })
        })
        .collect();
    fs::write(
        cdn.join("path"),
        serde_json::to_vec(&json!({
            "info": {
                "client_asset_version": "",
                "target_asset_version": "1.4.57",
                "eventual_target_asset_version": "1.4.57",
                "is_initial": true,
                "latest_maj_first_version": "1.4.57"
            },
            "full": {"version": "1.4.57", "archive": archives},
            "diff": [],
            "asset_version_hash": "recovery-fixture"
        }))
        .unwrap(),
    )
    .unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    for (device, file_name) in [
        ("1", "10939-ios_medium.csv"),
        ("2", "10939-android_medium.csv"),
    ] {
        let encoded = encode_request(&json!({"version": "1.4.57"}));
        let response = request_with_headers(
            service.port(),
            "POST",
            "/api/index.php/asset/version_info",
            "application/x-www-form-urlencoded",
            &[("DEVICE", device)],
            encoded.as_bytes(),
        );
        let version = decode_response::<serde_json::Value>(&response);
        let list_url = url::Url::parse(version.data["files_list"].as_str().unwrap()).unwrap();
        assert!(list_url.path().ends_with(file_name));
        let list = request(service.port(), "GET", list_url.path());
        assert!(list.contains(ENTITY));
        let entity_url = format!("{}{}", version.data["base_url"].as_str().unwrap(), hash);
        let entity_url = url::Url::parse(&entity_url).unwrap();
        let recovered = request_bytes(service.port(), "GET", entity_url.path());
        assert!(recovered.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert_eq!(body(&recovered), bytes);
        let path_url = format!("/patch/cn/entities/by-path/{file_name}/{ENTITY}");
        let recovered = request_bytes(service.port(), "GET", &path_url);
        assert!(recovered.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert_eq!(body(&recovered), bytes);
    }
    let unknown = request(
        service.port(),
        "GET",
        &format!("/patch/cn/entities/{}", digest(b"unknown")),
    );
    assert!(unknown.starts_with("HTTP/1.1 404 Not Found"));
    service.stop().unwrap();
}

// //// 在资源覆盖更新后按当前摘要取回新文件 [@x380kkm 2026-09-20] ////
#[test]
fn refreshes_recovery_sources_after_an_overlay_change() {
    let root = TempDir::new().unwrap();
    let cdn = root.path().join("cdn/cn");
    let overlay = root.path().join("cdn/override");
    let old = b"base resource";
    write_manifest(&cdn, "PathFile.csv", old, &digest(old));
    write_archive(&cdn, "base.zip", old, zip::CompressionMethod::Stored);
    let service = PersonalService::start(root.path(), 0).unwrap();
    let old_path = format!("/patch/cn/EntityLists/{}", digest(old));
    assert_eq!(body(&request_bytes(service.port(), "GET", &old_path)), old);
    fs::create_dir_all(overlay.join("entities")).unwrap();
    fs::write(overlay.join("entities/PathFile.csv"), b"").unwrap();
    fs::create_dir_all(overlay.join("archive-common-diff")).unwrap();
    fs::write(overlay.join("archive-common-diff/base.zip"), b"").unwrap();
    let empty_entity = overlay.join(ENTITY);
    fs::create_dir_all(empty_entity.parent().unwrap()).unwrap();
    fs::write(empty_entity, b"").unwrap();
    assert_eq!(body(&request_bytes(service.port(), "GET", &old_path)), old);
    let new = b"updated overlay resource";
    let new_digest = URL_SAFE_NO_PAD.encode(Sha256::digest(new));
    write_manifest(&overlay, "PathFile.csv", new, &new_digest);
    write_archive(&overlay, "updated.zip", new, zip::CompressionMethod::Stored);
    let new_path = format!("/patch/cn/EntityLists/{new_digest}");
    assert_eq!(body(&request_bytes(service.port(), "GET", &new_path)), new);
    assert!(request(service.port(), "GET", &old_path).starts_with("HTTP/1.1 404 Not Found"));
    service.stop().unwrap();
}

// //// 按原位补回的归档更新恢复索引 [@x380kkm 2026-09-26] ////
#[test]
fn refreshes_recovery_after_an_archive_is_restored_in_place() {
    let root = TempDir::new().unwrap();
    let cdn = root.path().join("cdn/cn");
    let resource = b"restored archive content";
    write_manifest(&cdn, "PathFile.csv", resource, &digest(resource));
    let directory = cdn.join("archive-common-diff");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("restored.zip"), b"").unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let path = format!("/patch/cn/entities/{}", digest(resource));
    assert!(request(service.port(), "GET", &path).starts_with("HTTP/1.1 500 Internal Server Error"));
    write_archive(
        &cdn,
        "restored.zip",
        resource,
        zip::CompressionMethod::Deflated,
    );
    let recovered = request_bytes(service.port(), "GET", &path);
    assert!(recovered.starts_with(b"HTTP/1.1 200 OK"));
    assert_eq!(body(&recovered), resource);
    fs::write(directory.join("restored.zip"), b"").unwrap();
    assert!(request(service.port(), "GET", &path).starts_with("HTTP/1.1 500 Internal Server Error"));
    write_archive(
        &cdn,
        "restored.zip",
        resource,
        zip::CompressionMethod::Deflated,
    );
    assert_eq!(body(&request_bytes(service.port(), "GET", &path)), resource);
    service.stop().unwrap();
}

// //// 同一逻辑路径的两平台资源保留各自恢复摘要 [@x380kkm 2026-09-20] ////
#[test]
fn recovers_distinct_platform_entities_at_the_same_path() {
    let root = TempDir::new().unwrap();
    let cdn = root.path().join("cdn/cn");
    let ios = b"ios platform master";
    let android = b"android platform master";
    write_manifest(&cdn, "10939-ios_medium.csv", ios, &digest(ios));
    write_manifest(&cdn, "10939-android_medium.csv", android, &digest(android));
    write_archive(&cdn, "ios.zip", ios, zip::CompressionMethod::Stored);
    write_archive(&cdn, "android.zip", android, zip::CompressionMethod::Stored);
    let service = PersonalService::start(root.path(), 0).unwrap();
    for (manifest, bytes) in [
        ("10939-ios_medium.csv", ios.as_slice()),
        ("10939-android_medium.csv", android.as_slice()),
    ] {
        let path = format!("/patch/cn/entities/{}", digest(bytes));
        assert_eq!(body(&request_bytes(service.port(), "GET", &path)), bytes);
        let path = format!("/patch/cn/entities/by-path/{manifest}/{ENTITY}");
        assert_eq!(body(&request_bytes(service.port(), "GET", &path)), bytes);
    }
    service.stop().unwrap();
}

// //// 按同路径最终有效清单记录恢复实体 [@x380kkm 2026-09-27] ////
#[test]
fn resolves_the_last_valid_manifest_record_at_each_path() {
    let root = TempDir::new().unwrap();
    let cdn = root.path().join("cdn/cn");
    let manifest = "10939-android_medium.csv";
    let old = b"previous resource";
    let current = b"current resource";
    write_manifest(&cdn, manifest, current, &digest(current));
    fs::write(
        cdn.join("entities").join(manifest),
        format!(
            "\u{feff}{ENTITY},1.4.56,{},{},common\r\n{ENTITY},1.4.57,{},{},common\r\n{ENTITY},1.4.58,invalid,{},common\r\n",
            old.len(), digest(old), current.len(), digest(current), digest(old),
        ),
    ).unwrap();
    write_archive(&cdn, "current.zip", current, zip::CompressionMethod::Stored);
    let service = PersonalService::start(root.path(), 0).unwrap();
    let path = format!("/patch/cn/entities/by-path/{manifest}/{ENTITY}");
    assert_eq!(body(&request_bytes(service.port(), "GET", &path)), current);
    let current_path = format!("/patch/cn/entities/{}", digest(current));
    assert_eq!(
        body(&request_bytes(service.port(), "GET", &current_path)),
        current
    );
    let old_path = format!("/patch/cn/entities/{}", digest(old));
    assert!(request(service.port(), "GET", &old_path).starts_with("HTTP/1.1 404 Not Found"));
    service.stop().unwrap();
}

// //// 目标归档原位重排成员后按当前中央目录读取实体 [@x380kkm 2026-09-27] ////
#[test]
fn refreshes_a_cached_archive_when_members_move_in_place() {
    let root = TempDir::new().unwrap();
    let cdn = root.path().join("cdn/cn");
    let directory = cdn.join("archive-common-diff");
    let resource = b"resource at a different archive offset";
    write_manifest(&cdn, "PathFile.csv", resource, &digest(resource));
    fs::create_dir_all(&directory).unwrap();
    let archive_path = directory.join("reordered.zip");
    let service = PersonalService::start(root.path(), 0).unwrap();
    let path = format!("/patch/cn/entities/{}", digest(resource));
    let entries = [
        (ENTITY, resource.as_slice()),
        ("production/other", b"other entry".as_slice()),
    ];
    let mut first_size = 0;
    for reverse in [false, true] {
        let mut ordered = entries.to_vec();
        if reverse {
            ordered.reverse();
        }
        let mut archive = zip::ZipWriter::new(File::create(&archive_path).unwrap());
        for (name, bytes) in ordered {
            archive
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        let current_size = fs::metadata(&archive_path).unwrap().len();
        if reverse {
            assert_eq!(current_size, first_size);
        } else {
            first_size = current_size;
        }
        let recovered = request_bytes(service.port(), "GET", &path);
        assert!(recovered.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert_eq!(body(&recovered), resource);
    }
    service.stop().unwrap();
}

// //// 从完整归档目录连续处理客户端的并行补取请求 [@x380kkm 2026-09-27] ////
#[test]
fn recovers_a_burst_from_a_published_archive_catalog() {
    let root = TempDir::new().unwrap();
    let cdn = root.path().join("cdn/cn");
    let directory = cdn.join("archive-common-diff");
    let resource = vec![b'a'; 16 * 1024];
    write_manifest(&cdn, "10939-ios_medium.csv", &resource, &digest(&resource));
    fs::create_dir_all(&directory).unwrap();
    let mut locations = Vec::new();
    for index in 0..256 {
        let name = format!("{index:04}.zip");
        let mut archive = zip::ZipWriter::new(File::create(directory.join(&name)).unwrap());
        let entries = if index == 0 { 2048 } else { 1 };
        for entry in 0..entries {
            archive
                .start_file(
                    format!("production/noise/{index:04}/{entry:04}"),
                    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            archive.write_all(b"catalog member").unwrap();
        }
        if index == 0 {
            archive
                .start_file(
                    ENTITY,
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            archive.write_all(&resource).unwrap();
        }
        archive.finish().unwrap();
        locations.push(json!({"location": format!("archive-common-diff/{name}")}));
    }
    fs::write(
        cdn.join("path"),
        serde_json::to_vec(&json!({"full": {"archive": locations}, "diff": []})).unwrap(),
    )
    .unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let path = format!("/patch/cn/entities/{}", digest(&resource));
    assert_eq!(body(&request_bytes(service.port(), "GET", &path)), resource);
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..32 {
            scope.spawn(|| {
                for _ in 0..2 {
                    let recovered = request_bytes(service.port(), "GET", &path);
                    assert!(recovered.starts_with(b"HTTP/1.1 200 OK\r\n"));
                    assert_eq!(body(&recovered), resource);
                }
            });
        }
    });
    println!(
        "64 次 16 KiB 资源恢复耗时 {:?}, 已发布归档 256 个.",
        started.elapsed()
    );
    service.stop().unwrap();
}

// //// 使用本机完整 CDN 测量冷启动和相邻实体恢复 [@x380kkm 2026-09-27] ////
#[test]
#[ignore = "使用 STARPOINT_CN_CDN_ROOT 与 STARPOINT_RECOVERY_ENTITY 指定只读资源"]
fn recovers_cold_and_neighboring_entities_from_a_local_cdn() {
    let cdn = std::env::var("STARPOINT_CN_CDN_ROOT").unwrap();
    let entity = std::env::var("STARPOINT_RECOVERY_ENTITY").unwrap();
    let root = TempDir::new().unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, &cdn).unwrap();
    let manifest = "10939-android_medium.csv";
    let path = format!("/patch/cn/entities/by-path/{manifest}/{entity}");
    let started = Instant::now();
    let recovered = request_bytes(service.port(), "GET", &path);
    let cold = started.elapsed();
    assert!(recovered.starts_with(b"HTTP/1.1 200 OK\r\n"));
    let contents = fs::read_to_string(Path::new(&cdn).join("entities").join(manifest)).unwrap();
    let record: Vec<_> = contents
        .lines()
        .find(|line| line.split(',').next() == Some(entity.as_str()))
        .unwrap()
        .split(',')
        .collect();
    assert_eq!(body(&recovered).len(), record[2].parse::<usize>().unwrap());
    assert_eq!(digest(body(&recovered)), record[3]);
    let prefix = entity.rsplit_once('/').unwrap().0;
    let neighbors: Vec<Vec<_>> = contents
        .lines()
        .map(|line| line.split(',').collect::<Vec<_>>())
        .filter(|fields| {
            fields.len() == 5
                && fields[0].starts_with(prefix)
                && fields[2]
                    .parse::<usize>()
                    .is_ok_and(|size| size <= 32 * 1024)
        })
        .take(32)
        .collect();
    let started = Instant::now();
    for fields in &neighbors {
        let response = request_bytes(
            service.port(),
            "GET",
            &format!("/patch/cn/entities/{}", fields[3]),
        );
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert_eq!(digest(body(&response)), fields[3]);
    }
    println!(
        "完整 CDN 冷请求 {:?}, {} 个相邻实体 {:?}.",
        cold,
        neighbors.len(),
        started.elapsed()
    );
    service.stop().unwrap();
}
