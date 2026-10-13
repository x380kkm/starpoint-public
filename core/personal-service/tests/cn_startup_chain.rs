// audience: internal
// # personal-service-cn-startup-chain-tests
//
// 该文件验证 CN 真机启动链的注册路由, 响应结构和重复 SDK 调用.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use starpoint_personal_service::PersonalService;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use support::{request, request_with_body, request_with_headers};
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

struct AssetArchive {
    metadata: Value,
    entity_row: String,
}

// //// 生成包含实体文件的启动资源归档 [@x380kkm 2026-09-23] ////
fn write_archive(root: &Path, directory: &str, name: &str, body: &[u8]) -> AssetArchive {
    let asset_kind = directory
        .strip_prefix("archive-")
        .and_then(|name| name.strip_suffix("-full"))
        .expect("archive directory identifies an asset kind");
    let upload_directory = if asset_kind == "common" {
        "upload".to_owned()
    } else {
        format!("{asset_kind}_upload")
    };
    let digest = Sha256::digest(body);
    let digest_path = format!("{digest:x}");
    let entry_path = format!(
        "production/{upload_directory}/{}/{}",
        &digest_path[..2],
        &digest_path[2..40]
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file(&entry_path, SimpleFileOptions::default())
        .expect("asset entry is created");
    archive.write_all(body).expect("asset entry is written");
    let bytes = archive
        .finish()
        .expect("asset archive is finished")
        .into_inner();
    let path = root.join("cdn").join("cn").join(directory).join(name);
    fs::create_dir_all(path.parent().expect("archive has a parent directory"))
        .expect("archive directory is created");
    fs::write(path, &bytes).expect("archive is written");
    AssetArchive {
        metadata: json!({
            "location": format!("https://retired.invalid/{directory}/{name}"),
            "size": bytes.len(),
            "sha256": STANDARD.encode(Sha256::digest(&bytes)),
        }),
        entity_row: format!(
            "{entry_path},1.4.58,{},{},{asset_kind}\n",
            body.len(),
            URL_SAFE_NO_PAD.encode(digest)
        ),
    }
}
// //// /生成包含实体文件的启动资源归档 ////

// //// 写入启动路径和平台实体清单 [@x380kkm 2026-09-23] ////
fn write_path_manifest(root: &Path, archives: &[AssetArchive]) -> String {
    let path = root.join("cdn").join("cn").join("path");
    let manifest = json!({
        "info": {
            "client_asset_version": "1.4.57",
            "target_asset_version": "1.4.58",
            "eventual_target_asset_version": "1.4.58",
            "is_initial": true,
            "latest_maj_first_version": "1.4.0"
        },
        "full": {
            "version": "1.4.0",
            "archive": archives.iter().map(|archive| &archive.metadata).collect::<Vec<_>>()
        },
        "diff": [],
        "asset_version_hash": "startup-chain"
    });
    fs::write(
        path,
        serde_json::to_vec(&manifest).expect("path manifest is encoded"),
    )
    .expect("path manifest is written");
    let entities = root.join("cdn/cn/entities");
    fs::create_dir_all(&entities).expect("entity directory is created");
    let entity_list = archives
        .iter()
        .map(|archive| archive.entity_row.as_str())
        .collect::<String>();
    fs::write(entities.join("10939-ios_medium.csv"), &entity_list)
        .expect("iOS entity list is written");
    entity_list
}
// //// /写入启动路径和平台实体清单 ////

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .expect("response contains a body")
        .1
}

fn assert_json_response(response: &str, expected: Value) {
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("Content-Type: application/json"));
    assert_eq!(
        serde_json::from_str::<Value>(response_body(response)).expect("response body is JSON"),
        expected,
    );
}

// //// 验证 CN 启动链与 SDK 重复调用 [@x380kkm 2026-08-22] ////
#[test]
fn completes_the_registered_cn_startup_chain() {
    let root = TempDir::new().expect("temporary service directory is created");
    let common_archive = write_archive(
        root.path(),
        "archive-common-full",
        "pinball-1.4.0-1-common.zip",
        b"common",
    );
    let ios_archive = write_archive(
        root.path(),
        "archive-ios-full",
        "pinball-1.4.0-1-ios.zip",
        b"ios",
    );
    let entity_list = write_path_manifest(root.path(), &[common_archive, ios_archive]);
    let service = PersonalService::start(root.path(), 0).expect("service starts");

    let discovery = request(
        service.port(),
        "GET",
        "/shijtswy/version/client_release_ios.dis?cache=1",
    );
    assert!(discovery.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&discovery).contains("\"apiScheme\":\"http\""));
    assert!(response_body(&discovery)
        .contains(&format!("\"apiPath\":\"127.0.0.1:{}\"", service.port())));

    let leiting = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/channels/channel_leiting/leiting_login",
        &encode_request(&json!({"userId": "local-user", "game": "wf"})),
    ));
    assert_eq!(leiting.data_headers.viewer_id, 0);
    assert_eq!(leiting.data["status"], "success");
    assert_eq!(leiting.data["userId"], "local-user");
    assert_eq!(leiting.data["data"]["isGuest"], 0);
    assert_eq!(leiting.data["online_server_check"], true);
    assert_eq!(leiting.data["heart_beat_interval"], 240);

    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 77 }),
    ));
    cn_support::assert_valid_signup_response(&signup);
    assert_eq!(signup.data.new_account, 1);
    let viewer_id = signup.data_headers.viewer_id;

    let header = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/get_header_response",
        &encode_request(&json!({"viewer_id": viewer_id})),
    ));
    assert_eq!(header.data_headers.viewer_id, viewer_id);
    assert!(header.data.is_array());
    for (path, expected) in [
        ("/api/index.php/tool/auth", json!({})),
        (
            "/api/index.php/tool/check_social_link_enable",
            json!({"enable": false}),
        ),
        (
            "/api/index.php/tool/check_enable_gift",
            json!({"enable_gift": true}),
        ),
        (
            "/api/index.php/tool/contact_active",
            json!({"enable_customer_service": false}),
        ),
        ("/api/index.php/tool/custom_notify", json!({})),
        (
            "/api/index.php/channels/channel_leiting/leiting_antiaddiction_logout",
            json!({}),
        ),
        (
            "/api/index.php/channels/channel_leiting/leiting_update",
            json!({}),
        ),
    ] {
        let response = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            path,
            "not-messagepack",
        ));
        assert_eq!(response.data, expected, "unexpected response for {path}");
    }

    let loaded = decode_response::<Value>(&cn_support::send_request_with_resource_version(
        service.port(),
        "/api/index.php/load",
        &encode_request(&json!({"keychain": viewer_id, "viewer_id": 0})),
        "1.4.57",
    ));
    assert_eq!(loaded.data_headers.viewer_id, viewer_id);
    assert!(loaded.data_headers.asset_update);
    let available_asset_version = loaded.data["available_asset_version"]
        .as_str()
        .expect("available asset version is text");
    assert_ne!(available_asset_version, "1.4.57");
    assert!(loaded.data["unfinished_quest_list"].is_array());
    assert!(loaded.data["unfinished_multi_quest_list"].is_array());

    let version_response = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/asset/version_info",
        "application/x-www-form-urlencoded",
        &[("DEVICE", "1")],
        encode_request(&json!({"viewer_id": viewer_id})).as_bytes(),
    );
    assert!(version_response.contains("Content-Type: application/x-msgpack"));
    let version = decode_response::<Value>(&version_response);
    assert_eq!(version.data_headers.viewer_id, 0);
    assert!(!version.data_headers.asset_update);
    assert!(version.data["base_url"].as_str().is_some());
    let files_list = url::Url::parse(
        version.data["files_list"]
            .as_str()
            .expect("entity list URL is text"),
    )
    .expect("entity list URL parses");
    assert!(files_list
        .path()
        .ends_with("/entities/10939-ios_medium.csv"));
    let entities_response = request(service.port(), "GET", files_list.path());
    assert!(entities_response.starts_with("HTTP/1.1 200 OK"));
    assert!(entities_response.contains("Content-Type: text/csv"));
    for entity in entity_list.lines() {
        assert!(
            response_body(&entities_response)
                .lines()
                .any(|line| line == entity),
            "missing entity: {entity}"
        );
    }
    assert_eq!(version.data["delayed_assets_size"], 0);

    let path_response = request_with_headers(
        service.port(),
        "POST",
        "/api/index.php/asset/get_path",
        "application/x-www-form-urlencoded",
        &[("res_ver", "1.4.57"), ("DEVICE", "1")],
        encode_request(&json!({"viewer_id": viewer_id})).as_bytes(),
    );
    let path = decode_response::<Value>(&path_response);
    assert_eq!(path.data_headers.viewer_id, viewer_id);
    assert!(path.data_headers.asset_update);
    assert_eq!(path.data["info"]["client_asset_version"], "1.4.57");
    assert_eq!(
        path.data["info"]["target_asset_version"],
        available_asset_version
    );
    assert_eq!(
        path.data["info"]["eventual_target_asset_version"],
        available_asset_version
    );
    let updates = path.data["diff"]
        .as_array()
        .expect("asset updates are an array");
    assert_eq!(
        updates.first().expect("activity projection adds a diff")["original_version"],
        "1.4.58"
    );
    assert_eq!(updates.last().unwrap()["version"], available_asset_version);
    let archives = path.data["full"]["archive"]
        .as_array()
        .expect("full archive list is an array");
    assert!(archives.iter().any(|archive| archive["location"]
        .as_str()
        .is_some_and(|location| location.contains("archive-ios-full"))));
    assert!(archives.iter().all(|archive| !archive["location"]
        .as_str()
        .is_some_and(|location| location.contains("archive-android-full"))));

    for _ in 0..2 {
        let order = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            "/api/index.php/channels/channel_leiting_pay/query_unfinish_order",
            &encode_request(&json!({"viewer_id": viewer_id})),
        ));
        assert_eq!(order.data_headers.viewer_id, viewer_id);
        assert_eq!(order.data_headers.result_code, 1);
        assert_eq!(order.data, json!({"order_id": ""}));

        assert_json_response(
            &request_with_body(
                service.port(),
                "POST",
                "/sync_data",
                "application/json",
                br#"{}"#,
            ),
            json!({"code": 0}),
        );
        let sdk_login = request_with_body(
            service.port(),
            "POST",
            "/sdk/v3-3/check_login.do",
            "application/json",
            br#"{}"#,
        );
        assert!(sdk_login.contains("Cache-Control: no-store"));
        let sdk_login: Value =
            serde_json::from_str(response_body(&sdk_login)).expect("SDK login body is JSON");
        assert_eq!(sdk_login["status"], "0");
        assert_eq!(sdk_login["type"], "0");
        assert!(sdk_login["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty()));

        let report = request_with_body(
            service.port(),
            "POST",
            "/api/device/report",
            "application/json",
            br#"{}"#,
        );
        assert!(report.starts_with("HTTP/1.1 204 No Content"));
        assert!(response_body(&report).is_empty());
    }
    for (path, result_code) in [
        (
            "/api/index.php/channels/channel_leiting_pay/query_purcharge",
            20001,
        ),
        (
            "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_status",
            1,
        ),
    ] {
        let response = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            path,
            &encode_request(&json!({"viewer_id": viewer_id})),
        ));
        assert_eq!(response.data_headers.viewer_id, viewer_id);
        assert_eq!(response.data_headers.result_code, result_code);
        assert_eq!(response.data, json!({}));
    }
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 启动链与 SDK 重复调用 ////
