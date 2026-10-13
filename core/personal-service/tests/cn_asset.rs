// audience: internal
// # personal-service-cn-asset-tests
//
// 该测试验证 CN 资产版本和路径接口只返回本地 CDN 元数据.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

#[path = "cn_asset/download_updates.rs"]
mod download_updates;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use starpoint_personal_service::PersonalService;
use std::collections::BTreeSet;
use std::fs;
use tempfile::TempDir;

use cn_support::{
    decode_response, encode_request, send_request, send_request_with_resource_version,
};
use support::{request, request_with_headers};

// //// 提取 HTTP 响应正文 [@x380kkm 2026-08-29] ////
fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .expect("response contains a header terminator")
        .1
}
// //// /提取 HTTP 响应正文 ////

fn assert_object_keys(value: &Value, expected: &[&str]) {
    let object = value.as_object().expect("value is an object");
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}

fn write_archive(root: &std::path::Path, directory: &str, name: &str, body: &[u8]) {
    let path = root.join("cdn").join("cn").join(directory).join(name);
    fs::create_dir_all(path.parent().expect("archive has a parent directory"))
        .expect("archive directory is created");
    fs::write(path, body).expect("archive is written");
}

// //// 写入临时可写覆盖归档 [@x380kkm 2026-08-29] ////
fn write_override_archive(root: &std::path::Path, directory: &str, name: &str, body: &[u8]) {
    let path = root.join("cdn").join("override").join(directory).join(name);
    fs::create_dir_all(
        path.parent()
            .expect("override archive has a parent directory"),
    )
    .expect("override archive directory is created");
    fs::write(path, body).expect("override archive is written");
}
// //// /写入临时可写覆盖归档 ////

fn write_path_manifest(root: &std::path::Path, data: Value) {
    let path = root.join("cdn").join("cn").join("path");
    fs::create_dir_all(path.parent().expect("path manifest has a parent directory"))
        .expect("path manifest directory is created");
    fs::write(
        path,
        serde_json::to_vec(&data).expect("path manifest is encoded"),
    )
    .expect("path manifest is written");
}

fn send_request_with_device_kind(
    port: u16,
    path: &str,
    body: &str,
    resource_version: &str,
    device_kind: &str,
) -> String {
    request_with_headers(
        port,
        "POST",
        path,
        "application/x-www-form-urlencoded",
        &[("res_ver", resource_version), ("DEVICE", device_kind)],
        body.as_bytes(),
    )
}

// //// 返回 CN 资产版本和下载路径 [@x380kkm 2026-08-10] ////
#[test]
fn returns_local_cn_asset_metadata() {
    let root = TempDir::new().expect("temporary service directory is created");
    fs::create_dir_all(root.path().join("cdn").join("cn").join("entities"))
        .expect("entity directory is created");
    write_archive(
        root.path(),
        "archive-common-full",
        "pinball-1.4.0-1-test.zip",
        b"full",
    );
    write_archive(
        root.path(),
        "archive-common-diff",
        "pinball-1.4.0-1.4.1-1-test.zip",
        b"diff",
    );
    write_archive(
        root.path(),
        "archive-ios-full",
        "pinball-1.4.0-1-ios.zip",
        b"ios-full",
    );
    write_archive(
        root.path(),
        "archive-ios-diff",
        "pinball-1.4.0-1.4.1-1-ios.zip",
        b"ios-diff",
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let version_response = send_request(service.port(), "/api/index.php/asset/version_info", "{}");
    let version = decode_response::<Value>(&version_response);
    assert!(!version.data_headers.asset_update);
    assert!(version.data["files_list"]
        .as_str()
        .expect("files list is text")
        .ends_with("/entities/10939-android_medium.csv"));
    assert_eq!(version.data["total_size"], 8);

    let title_version_response = send_request(
        service.port(),
        "/api/index.php/assetintitle/version_info_in_title",
        "{}",
    );
    let title_version = decode_response::<Value>(&title_version_response);
    assert!(!title_version.data_headers.asset_update);
    assert_eq!(title_version.data["base_url"], version.data["base_url"]);
    assert!(title_version.data["files_list"]
        .as_str()
        .expect("title files list is text")
        .ends_with("/entities/10939-android_medium.csv"));
    assert_eq!(title_version.data["total_size"].as_f64(), Some(8.0));
    assert_eq!(title_version.data["delayed_assets_size"], 0);

    let request_body = encode_request(&json!({ "viewer_id": 123_456_789.0 }));
    let path_response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "",
    );
    let path = decode_response::<Value>(&path_response);
    assert!(path.data_headers.asset_update);
    assert_eq!(path.data_headers.viewer_id, 123_456_789);
    assert_object_keys(&path.data, &["asset_version_hash", "diff", "full", "info"]);
    assert_object_keys(
        &path.data["info"],
        &[
            "client_asset_version",
            "eventual_target_asset_version",
            "is_initial",
            "latest_maj_first_version",
            "target_asset_version",
        ],
    );
    assert_object_keys(&path.data["full"], &["archive", "version"]);
    assert_object_keys(
        &path.data["full"]["archive"][0],
        &["location", "sha256", "size"],
    );
    assert_object_keys(
        &path.data["diff"][0],
        &["archive", "original_version", "version"],
    );
    assert_eq!(path.data["full"]["version"], "1.4.0");
    assert_eq!(path.data["info"]["client_asset_version"], "");
    assert_eq!(path.data["info"]["target_asset_version"], "1.4.1");
    assert_eq!(path.data["info"]["eventual_target_asset_version"], "1.4.1");
    assert_eq!(path.data["info"]["latest_maj_first_version"], "1.4.0");
    assert_eq!(path.data["info"]["is_initial"], true);
    assert_eq!(path.data["diff"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        path.data["full"]["archive"].as_array().map(Vec::len),
        Some(1)
    );
    let expected_sha256 = STANDARD.encode(Sha256::digest(b"full"));
    assert_eq!(path.data["full"]["archive"][0]["sha256"], expected_sha256);
    service.stop().expect("service stops cleanly");
}
// //// /返回 CN 资产版本和下载路径 ////

// //// 为 DEVICE 1 返回 iOS 清单和归档 [@x380kkm 2026-08-18] ////
#[test]
fn returns_ios_asset_metadata_for_device_one() {
    let root = TempDir::new().expect("temporary service directory is created");
    fs::create_dir_all(root.path().join("cdn").join("cn").join("EntityLists"))
        .expect("entity directory is created");
    write_archive(
        root.path(),
        "archive-common-full",
        "pinball-1.4.0-1-common.zip",
        b"common",
    );
    write_archive(
        root.path(),
        "archive-android-full",
        "pinball-1.4.0-1-android.zip",
        b"android-only",
    );
    write_archive(
        root.path(),
        "archive-ios-full",
        "pinball-1.4.0-1-ios.zip",
        b"ios-only",
    );
    write_archive(
        root.path(),
        "archive-common-diff",
        "pinball-1.4.0-1.4.1-1-common.zip",
        b"common-diff",
    );
    write_archive(
        root.path(),
        "archive-android-diff",
        "pinball-1.4.0-1.4.1-1-android.zip",
        b"android-diff",
    );
    write_archive(
        root.path(),
        "archive-ios-diff",
        "pinball-1.4.0-1.4.1-1-ios.zip",
        b"ios-diff",
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let version_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/version_info",
        "{}",
        "1.4.0",
        "1",
    );
    let version = decode_response::<Value>(&version_response);
    assert!(version.data["files_list"]
        .as_str()
        .expect("files list is text")
        .ends_with("/EntityLists/10939-ios_medium.csv"));
    assert_eq!(version.data["total_size"], 33);

    let title_version_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/assetintitle/version_info_in_title",
        "{}",
        "1.4.0",
        "1",
    );
    let title_version = decode_response::<Value>(&title_version_response);
    assert!(title_version.data["files_list"]
        .as_str()
        .expect("title files list is text")
        .ends_with("/EntityLists/10939-ios_medium.csv"));

    let request_body = encode_request(&json!({ "viewer_id": 123_456_789 }));
    let path_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "",
        "1",
    );
    let path = decode_response::<Value>(&path_response).data;
    let full_locations = path["full"]["archive"]
        .as_array()
        .expect("full archive list exists")
        .iter()
        .map(|archive| archive["location"].as_str().expect("location is text"))
        .collect::<Vec<_>>();
    let diff_locations = path["diff"][0]["archive"]
        .as_array()
        .expect("diff archive list exists")
        .iter()
        .map(|archive| archive["location"].as_str().expect("location is text"))
        .collect::<Vec<_>>();

    assert!(full_locations
        .iter()
        .any(|location| location.contains("/archive-ios-full/")));
    assert!(!full_locations
        .iter()
        .any(|location| location.contains("/archive-android-full/")));
    assert!(diff_locations
        .iter()
        .any(|location| location.contains("/archive-ios-diff/")));
    assert!(!diff_locations
        .iter()
        .any(|location| location.contains("/archive-android-diff/")));
    service.stop().expect("service stops cleanly");
}
// //// /为 DEVICE 1 返回 iOS 清单和归档 ////

// //// 接受不携带 viewer_id 的资产路径请求 [@x380kkm 2026-08-10] ////
#[test]
fn accepts_an_asset_path_request_without_a_viewer_id() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let request_body = encode_request(&json!({}));

    let response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "",
    );
    let path = decode_response::<Value>(&response);

    assert_eq!(path.data_headers.viewer_id, 0);
    assert!(path.data_headers.asset_update);
    service.stop().expect("service stops cleanly");
}
// //// /接受不携带 viewer_id 的资产路径请求 ////

// //// 按本地已安装版本复用资源归档 [@x380kkm 2026-09-07] ////
#[test]
fn omits_archives_when_the_client_reports_the_current_target_version() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_archive(
        root.path(),
        "archive-common-full",
        "pinball-1.4.0-1-test.zip",
        b"full",
    );
    write_archive(
        root.path(),
        "archive-common-diff",
        "pinball-1.4.0-1.4.1-1-test.zip",
        b"diff",
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let request_body = encode_request(&json!({
        "viewer_id": 42,
        "target_asset_version": "1.4.1"
    }));

    let response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.1",
    );
    let path = decode_response::<Value>(&response);

    assert!(!path.data_headers.asset_update);
    assert_eq!(path.data["info"]["client_asset_version"], "1.4.1");
    assert_eq!(path.data["info"]["is_initial"], false);
    assert!(path.data["full"].is_null());
    assert!(path.data["diff"].is_null());
    service.stop().expect("service stops cleanly");
}
// //// /按本地已安装版本复用资源归档 ////

// //// 为首次安装和未知已装版本提供完整下载链 [@x380kkm 2026-09-21] ////
#[test]
fn provides_initial_archives_when_only_the_requested_target_is_current() {
    let root = TempDir::new().expect("temporary service directory is created");
    write_archive(
        root.path(),
        "archive-common-full",
        "pinball-1.4.0-1-test.zip",
        b"full",
    );
    write_archive(
        root.path(),
        "archive-common-diff",
        "pinball-1.4.0-1.4.1-1-test.zip",
        b"diff",
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let request_body = encode_request(&json!({
        "viewer_id": 42,
        "target_asset_version": "1.4.1"
    }));

    for installed_version in [None, Some(""), Some("0"), Some("1.4.99")] {
        let headers = installed_version
            .map(|version| vec![("RES_VER", version)])
            .unwrap_or_default();
        let response = request_with_headers(
            service.port(),
            "POST",
            "/api/index.php/asset/get_path",
            "application/x-www-form-urlencoded",
            &headers,
            request_body.as_bytes(),
        );
        let path = decode_response::<Value>(&response);

        assert!(path.data_headers.asset_update);
        assert_eq!(
            path.data["info"]["client_asset_version"],
            installed_version.unwrap_or_default()
        );
        assert_eq!(path.data["info"]["eventual_target_asset_version"], "1.4.1");
        assert_eq!(path.data["info"]["is_initial"], true);
        assert_eq!(path.data["full"]["version"], "1.4.0");
        assert_eq!(
            path.data["full"]["archive"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(path.data["diff"][0]["original_version"], "1.4.0");
        assert_eq!(path.data["diff"][0]["version"], "1.4.1");
        assert_eq!(
            path.data["diff"][0]["archive"].as_array().map(Vec::len),
            Some(1)
        );
    }
    service.stop().expect("service stops cleanly");
}
// //// /为首次安装和未知已装版本提供完整下载链 ////

// //// 保留平台空归档过渡并复用完整差分链的终点版本 [@x380kkm 2026-09-08] ////
#[test]
fn preserves_platform_empty_diff_steps_before_an_android_override() {
    let root = TempDir::new().expect("temporary service directory is created");
    let full_name = "pinball-1.4.0-1-test.zip";
    write_archive(root.path(), "archive-common-full", full_name, b"full");
    write_override_archive(
        root.path(),
        "archive-android-diff",
        "starpoint-cn-activity-overlay.zip",
        b"activity overlay",
    );
    write_path_manifest(
        root.path(),
        json!({
            "info": {
                "client_asset_version": "",
                "target_asset_version": "1.4.1",
                "eventual_target_asset_version": "1.4.1",
                "is_initial": true,
                "latest_maj_first_version": "1.4.0"
            },
            "full": {
                "version": "1.4.0",
                "archive": [{
                    "location": format!("https://retired.invalid/archive-common-full/{full_name}"),
                    "size": 4,
                    "sha256": STANDARD.encode(Sha256::digest(b"full"))
                }]
            },
            "diff": [{
                "original_version": "1.4.0",
                "version": "1.4.1",
                "archive": [{
                    "location": "https://retired.invalid/archive-ios-diff/voice.zip",
                    "size": 5,
                    "sha256": STANDARD.encode(Sha256::digest(b"voice"))
                }]
            }],
            "asset_version_hash": "fixture-hash"
        }),
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let body = encode_request(&json!({"target_asset_version": "1.4.2"}));
    let response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &body,
        "",
        "2",
    );
    let path = decode_response::<Value>(&response).data;
    let diffs = path["diff"].as_array().expect("diff chain exists");
    assert_eq!(diffs.len(), 2);
    assert_eq!(diffs[0]["archive"], json!([]));
    assert_eq!(diffs[1]["archive"].as_array().map(Vec::len), Some(1));

    let mut installed_version = path["full"]["version"]
        .as_str()
        .expect("initial version exists");
    for _ in 0..diffs.len() {
        let next = diffs
            .iter()
            .find(|group| group["original_version"] == installed_version)
            .expect("each version has a next diff step");
        installed_version = next["version"].as_str().expect("diff version exists");
    }
    assert_eq!(installed_version, "1.4.2");
    assert_eq!(
        path["info"]["eventual_target_asset_version"],
        installed_version
    );

    let repeated_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &body,
        installed_version,
        "2",
    );
    let repeated = decode_response::<Value>(&repeated_response);
    assert!(!repeated.data_headers.asset_update);
    assert!(repeated.data["full"].is_null());
    assert!(repeated.data["diff"].is_null());
    service.stop().expect("service stops cleanly");
}
// //// /保留平台空归档过渡并复用完整差分链的终点版本 ////

// //// 本地化已有 CN 资产路径清单 [@x380kkm 2026-08-10] ////
#[test]
fn localizes_an_existing_cn_asset_path_manifest() {
    let root = TempDir::new().expect("temporary service directory is created");
    let archive_name = "pinball-1.4.0-1-test.zip";
    let ios_archive_name = "pinball-1.4.0-1-ios.zip";
    write_archive(root.path(), "archive-common-full", archive_name, b"full");
    write_archive(
        root.path(),
        "archive-ios-full",
        ios_archive_name,
        b"ios-full",
    );
    let manifest_sha256 = STANDARD.encode([0_u8; 32]);
    write_path_manifest(
        root.path(),
        json!({
            "info": {
                "client_asset_version": "manifest-client",
                "target_asset_version": "1.4.58",
                "eventual_target_asset_version": "1.4.58",
                "is_initial": true,
                "latest_maj_first_version": "1.4.0"
            },
            "full": {
                "version": "1.4.0",
                "archive": [{
                    "location": format!("https://retired.invalid/archive-common-full/{archive_name}"),
                    "size": 4,
                    "sha256": manifest_sha256.clone()
                }, {
                    "location": format!("https://retired.invalid/archive-ios-full/{ios_archive_name}"),
                    "size": 8,
                    "sha256": manifest_sha256.clone()
                }]
            },
            "diff": [],
            "asset_version_hash": "fixture-hash"
        }),
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let request_body = encode_request(&json!({ "viewer_id": 987_654_321 }));
    let response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.57",
    );
    let response = decode_response::<Value>(&response);
    assert_eq!(response.data_headers.viewer_id, 987_654_321);
    let path = response.data;

    assert_eq!(path["info"]["client_asset_version"], "1.4.57");
    assert_eq!(path["info"]["target_asset_version"], "1.4.58");
    assert_eq!(path["info"]["eventual_target_asset_version"], "1.4.58");
    assert_eq!(path["asset_version_hash"], "fixture-hash");
    assert_eq!(path["full"]["archive"][0]["sha256"], manifest_sha256);
    assert!(path["full"]["archive"][0]["location"]
        .as_str()
        .expect("archive location is text")
        .ends_with(&format!("/patch/cn/archive-common-full/{archive_name}")));
    assert_eq!(path["full"]["archive"].as_array().map(Vec::len), Some(1));

    let ios_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.57",
        "1",
    );
    let ios_path = decode_response::<Value>(&ios_response).data;
    let ios_locations = ios_path["full"]["archive"]
        .as_array()
        .expect("iOS archive list exists")
        .iter()
        .map(|archive| archive["location"].as_str().expect("location is text"))
        .collect::<Vec<_>>();
    assert!(ios_locations
        .iter()
        .any(|location| location.contains("/archive-ios-full/")));
    assert!(ios_locations
        .iter()
        .any(|location| location.contains("/archive-common-full/")));
    assert_eq!(ios_locations.len(), 2);
    service.stop().expect("service stops cleanly");
}
// //// /本地化已有 CN 资产路径清单 ////

// //// 使用覆盖归档的实际下载元数据 [@x380kkm 2026-08-22] ////
#[test]
fn uses_override_archive_metadata_in_path_manifest() {
    let root = TempDir::new().expect("temporary service directory is created");
    let directory = "archive-common-full";
    let archive_name = "pinball-1.4.0-1-override.zip";
    let base_body = b"base";
    let override_body = b"writable override archive";
    write_archive(root.path(), directory, archive_name, base_body);

    let override_path = root
        .path()
        .join("cdn")
        .join("override")
        .join(directory)
        .join(archive_name);
    fs::create_dir_all(
        override_path
            .parent()
            .expect("override archive has a parent directory"),
    )
    .expect("override archive directory is created");
    fs::write(&override_path, override_body).expect("override archive is written");

    write_path_manifest(
        root.path(),
        json!({
            "info": {
                "client_asset_version": "manifest-client",
                "target_asset_version": "1.4.54",
                "eventual_target_asset_version": "1.4.54",
                "is_initial": true,
                "latest_maj_first_version": "1.4.0"
            },
            "full": {
                "version": "1.4.0",
                "archive": [{
                    "location": format!("https://retired.invalid/{directory}/{archive_name}"),
                    "size": base_body.len(),
                    "sha256": STANDARD.encode(Sha256::digest(base_body))
                }]
            },
            "diff": [],
            "asset_version_hash": "fixture-hash"
        }),
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let version_response = send_request(service.port(), "/api/index.php/asset/version_info", "{}");
    let version = decode_response::<Value>(&version_response);
    assert_eq!(
        version.data["total_size"].as_u64(),
        Some(override_body.len() as u64)
    );

    let request_body = encode_request(&json!({ "viewer_id": 123_456_789.0 }));
    let response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.0",
    );
    let path = decode_response::<Value>(&response).data;
    let archive = &path["full"]["archive"][0];

    assert_eq!(archive["size"].as_u64(), Some(override_body.len() as u64));
    assert_eq!(
        archive["sha256"],
        STANDARD.encode(Sha256::digest(override_body))
    );

    let updated_override_body = b"updated writable override archive";
    fs::write(&override_path, updated_override_body).expect("override archive is updated");
    let updated_version_response =
        send_request(service.port(), "/api/index.php/asset/version_info", "{}");
    let updated_version = decode_response::<Value>(&updated_version_response);
    assert_eq!(
        updated_version.data["total_size"].as_u64(),
        Some(updated_override_body.len() as u64)
    );
    let updated_response = send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.0",
    );
    let updated_path = decode_response::<Value>(&updated_response).data;
    assert_eq!(
        updated_path["full"]["archive"][0]["sha256"],
        STANDARD.encode(Sha256::digest(updated_override_body))
    );

    fs::write(&override_path, b"").expect("override archive is truncated");
    let fallback_version = decode_response::<Value>(&send_request(
        service.port(),
        "/api/index.php/asset/version_info",
        "{}",
    ));
    assert_eq!(fallback_version.data["total_size"], base_body.len());
    let fallback = decode_response::<Value>(&send_request_with_resource_version(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.0",
    ));
    let archive = &fallback.data["full"]["archive"][0];
    assert_eq!(archive["size"], base_body.len());
    assert_eq!(archive["sha256"], STANDARD.encode(Sha256::digest(base_body)));
    service.stop().expect("service stops cleanly");
}
// //// /使用覆盖归档的实际下载元数据 ////

// //// 从当前 iOS 差分组发现可写覆盖归档 [@x380kkm 2026-08-29] ////
#[test]
fn discovers_an_additive_ios_override_archive() {
    let root = TempDir::new().expect("temporary service directory is created");
    let directory = "archive-ios-diff";
    let old_name = "pinball-1.4.56-1.4.57-1-base.zip";
    let base_name = "pinball-1.4.57-1.4.58-1-base.zip";
    let overlay_name = "starpoint-cn-voice-overlay-ios.zip";
    let old_body = b"older diff";
    let base_body = b"base diff";
    let overlay_body = b"voice overlay";
    write_archive(root.path(), directory, old_name, old_body);
    write_archive(root.path(), directory, base_name, base_body);
    write_override_archive(root.path(), directory, overlay_name, overlay_body);
    write_path_manifest(
        root.path(),
        json!({
            "info": {
                "client_asset_version": "manifest-client",
                "target_asset_version": "1.4.58",
                "eventual_target_asset_version": "1.4.58",
                "is_initial": true,
                "latest_maj_first_version": "1.4.0"
            },
            "full": {
                "version": "1.4.0",
                "archive": []
            },
            "diff": [{
                "version": "1.4.56",
                "original_version": "1.4.55",
                "archive": []
            }, {
                "version": "1.4.57",
                "original_version": "1.4.56",
                "archive": [{
                    "location": format!("https://retired.invalid/{directory}/{old_name}"),
                    "size": old_body.len(),
                    "sha256": STANDARD.encode(Sha256::digest(old_body))
                }]
            }, {
                "version": "1.4.58",
                "original_version": "1.4.57",
                "archive": [{
                    "location": format!("https://retired.invalid/{directory}/{base_name}"),
                    "size": base_body.len(),
                    "sha256": STANDARD.encode(Sha256::digest(base_body))
                }]
            }],
            "asset_version_hash": "fixture-hash"
        }),
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let request_body = encode_request(&json!({ "viewer_id": 42 }));
    let response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.58",
        "1",
    );
    let path = decode_response::<Value>(&response).data;
    assert_eq!(path["info"]["client_asset_version"], "1.4.58");
    assert_eq!(path["info"]["target_asset_version"], "1.4.59");
    assert_eq!(path["info"]["eventual_target_asset_version"], "1.4.59");
    let versions = path["diff"]
        .as_array()
        .expect("iOS diff groups exist")
        .iter()
        .filter_map(|group| group["version"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(versions, ["1.4.59"]);
    assert!(path["full"].is_null());
    let target_group = path["diff"]
        .as_array()
        .expect("iOS diff groups exist")
        .iter()
        .find(|group| group["version"] == "1.4.59")
        .expect("target diff group exists");
    assert_eq!(target_group["original_version"], "1.4.58");
    let archives = target_group["archive"]
        .as_array()
        .expect("target iOS diff archives exist");
    let overlays = archives
        .iter()
        .filter(|archive| {
            archive["location"]
                .as_str()
                .is_some_and(|location| location.ends_with(&format!("/{directory}/{overlay_name}")))
        })
        .collect::<Vec<_>>();
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0]["size"], overlay_body.len());
    assert_eq!(
        overlays[0]["sha256"],
        STANDARD.encode(Sha256::digest(overlay_body))
    );
    let locations = archives
        .iter()
        .filter_map(|archive| archive["location"].as_str())
        .collect::<Vec<_>>();
    assert!(locations.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(!path["diff"]
        .as_array()
        .expect("iOS diff groups exist")
        .iter()
        .filter(|group| group["version"] != "1.4.59")
        .flat_map(|group| group["archive"].as_array().into_iter().flatten())
        .any(|archive| archive["location"]
            .as_str()
            .is_some_and(|location| location.ends_with(&format!("/{overlay_name}")))));

    let archive_response = request(
        service.port(),
        "GET",
        &format!("/patch/cn/{directory}/{overlay_name}"),
    );
    assert!(archive_response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(response_body(&archive_response).as_bytes(), overlay_body);
    service.stop().expect("service stops cleanly");
}
// //// /从当前 iOS 差分组发现可写覆盖归档 ////

// //// 验证安装器命名的 iOS 语音归档可被覆盖层发现 ////
#[test]
fn discovers_installer_named_ios_voice_override_archive() {
    let root = TempDir::new().expect("temporary service directory is created");
    let directory = "archive-ios-diff";
    let archive_name = "starpoint-ios-voice-overlay-1.4.61-1.4.62.zip";
    let archive_body = b"voice overlay installer name";
    write_override_archive(root.path(), directory, archive_name, archive_body);
    write_path_manifest(
        root.path(),
        json!({
            "info": {
                "client_asset_version": "1.4.61",
                "target_asset_version": "1.4.61",
                "eventual_target_asset_version": "1.4.61",
                "is_initial": true,
                "latest_maj_first_version": "1.4.0"
            },
            "full": {"version": "1.4.0", "archive": []},
            "diff": [],
            "asset_version_hash": "fixture-hash"
        }),
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let request_body = encode_request(&json!({ "viewer_id": 42 }));
    let response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.61",
        "1",
    );
    let path = decode_response::<Value>(&response).data;
    let target_group = path["diff"]
        .as_array()
        .expect("iOS diff groups exist")
        .iter()
        .find(|group| group["version"] == "1.4.62")
        .expect("installer-named override creates target group");
    assert!(target_group["archive"][0]["location"]
        .as_str()
        .is_some_and(|location| location.ends_with(archive_name)));
    service.stop().expect("service stops cleanly");
}
// //// /验证安装器命名的 iOS 语音归档可被覆盖层发现 ////

// //// 在没有 path 清单时生成可触发的覆盖差分组 [@x380kkm 2026-08-29] ////
#[test]
fn discovers_an_additive_override_without_a_path_manifest() {
    let root = TempDir::new().expect("temporary service directory is created");
    let directory = "archive-ios-diff";
    let overlay_name = "starpoint-cn-voice-overlay-ios.zip";
    let overlay_body = b"voice overlay without manifest";
    write_override_archive(root.path(), directory, overlay_name, overlay_body);
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let request_body = encode_request(&json!({}));

    let response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.54",
        "1",
    );
    let path = decode_response::<Value>(&response).data;
    assert_eq!(path["info"]["client_asset_version"], "1.4.54");
    assert_eq!(path["info"]["target_asset_version"], "1.4.55");
    assert_eq!(path["info"]["eventual_target_asset_version"], "1.4.55");
    assert_eq!(path["diff"].as_array().map(Vec::len), Some(1));
    assert_eq!(path["diff"][0]["version"], "1.4.55");
    assert_eq!(path["diff"][0]["original_version"], "1.4.54");
    let archive = &path["diff"][0]["archive"][0];
    assert_eq!(archive["size"], overlay_body.len());
    assert_eq!(
        archive["sha256"],
        STANDARD.encode(Sha256::digest(overlay_body))
    );

    let version_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/version_info",
        "{}",
        "",
        "1",
    );
    let version = decode_response::<Value>(&version_response).data;
    assert_eq!(
        version["total_size"].as_u64(),
        Some(overlay_body.len() as u64)
    );
    service.stop().expect("service stops cleanly");
}
// //// /在没有 path 清单时生成可触发的覆盖差分组 ////

// //// 保持覆盖归档的平台隔离和同名去重 [@x380kkm 2026-08-29] ////
#[test]
fn keeps_override_archives_platform_scoped_and_deduplicated() {
    let root = TempDir::new().expect("temporary service directory is created");
    let name = "starpoint-cn-voice-overlay-ios.zip";
    let base_body = b"base overlay";
    let override_body = b"replacement overlay";
    write_archive(root.path(), "archive-ios-diff", name, base_body);
    write_override_archive(root.path(), "archive-ios-diff", name, override_body);
    write_override_archive(
        root.path(),
        "archive-android-diff",
        name,
        b"android overlay",
    );
    write_path_manifest(
        root.path(),
        json!({
            "info": {
                "client_asset_version": "1.4.57",
                "target_asset_version": "1.4.58",
                "eventual_target_asset_version": "1.4.58",
                "is_initial": true,
                "latest_maj_first_version": "1.4.0"
            },
            "full": { "version": "1.4.0", "archive": [] },
            "diff": [{
                "version": "1.4.58",
                "original_version": "1.4.57",
                "archive": [{
                    "location": format!("https://retired.invalid/archive-ios-diff/{name}"),
                    "size": base_body.len(),
                    "sha256": STANDARD.encode(Sha256::digest(base_body))
                }]
            }],
            "asset_version_hash": "fixture-hash"
        }),
    );
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let request_body = encode_request(&json!({}));

    let ios_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.57",
        "1",
    );
    let ios_path = decode_response::<Value>(&ios_response).data;
    assert_eq!(ios_path["info"]["target_asset_version"], "1.4.58");
    let ios_target_group = ios_path["diff"]
        .as_array()
        .expect("iOS diff groups exist")
        .iter()
        .find(|group| group["version"] == "1.4.58")
        .expect("iOS target group exists");
    let ios_archives = ios_target_group["archive"]
        .as_array()
        .expect("iOS diff archives exist");
    assert_eq!(
        ios_archives
            .iter()
            .filter(|archive| archive["location"]
                .as_str()
                .is_some_and(|location| location.ends_with(&format!("/{name}"))))
            .count(),
        1
    );
    let ios_overlay = ios_archives
        .iter()
        .find(|archive| {
            archive["location"]
                .as_str()
                .is_some_and(|location| location.ends_with(&format!("/{name}")))
        })
        .expect("iOS overlay is listed");
    assert_eq!(ios_overlay["size"], override_body.len());
    assert_eq!(
        ios_overlay["sha256"],
        STANDARD.encode(Sha256::digest(override_body))
    );

    let android_response = send_request_with_device_kind(
        service.port(),
        "/api/index.php/asset/get_path",
        &request_body,
        "1.4.57",
        "2",
    );
    let android_path = decode_response::<Value>(&android_response).data;
    let android_locations = android_path["diff"]
        .as_array()
        .expect("Android diff groups exist")
        .iter()
        .flat_map(|group| group["archive"].as_array().into_iter().flatten())
        .filter_map(|archive| archive["location"].as_str())
        .collect::<Vec<_>>();
    assert!(!android_locations
        .iter()
        .any(|location| location.contains("archive-ios-diff")));
    service.stop().expect("service stops cleanly");
}
// //// /保持覆盖归档的平台隔离和同名去重 ////
