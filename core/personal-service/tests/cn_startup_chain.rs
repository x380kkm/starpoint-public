// audience: internal
// # personal-service-cn-startup-chain-tests
//
// 该文件验证 CN 真机启动链的注册路由, 响应结构和重复 SDK 调用.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::fs;
use std::path::Path;
use support::{request, request_with_body, request_with_headers};
use tempfile::TempDir;

fn write_archive(root: &Path, directory: &str, name: &str, body: &[u8]) {
    let path = root.join("cdn").join("cn").join(directory).join(name);
    fs::create_dir_all(path.parent().expect("archive has a parent directory"))
        .expect("archive directory is created");
    fs::write(path, body).expect("archive is written");
}

fn write_path_manifest(root: &Path) {
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
            "archive": [{
                "location": "https://retired.invalid/archive-common-full/pinball-1.4.0-1-common.zip",
                "size": 6,
                "sha256": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            }, {
                "location": "https://retired.invalid/archive-ios-full/pinball-1.4.0-1-ios.zip",
                "size": 3,
                "sha256": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            }]
        },
        "diff": [],
        "asset_version_hash": "startup-chain"
    });
    fs::write(
        path,
        serde_json::to_vec(&manifest).expect("path manifest is encoded"),
    )
    .expect("path manifest is written");
}

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
    fs::create_dir_all(root.path().join("cdn").join("cn").join("entities"))
        .expect("entity directory is created");
    write_archive(
        root.path(),
        "archive-common-full",
        "pinball-1.4.0-1-common.zip",
        b"common",
    );
    write_archive(
        root.path(),
        "archive-ios-full",
        "pinball-1.4.0-1-ios.zip",
        b"ios",
    );
    write_path_manifest(root.path());
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
    assert_eq!(loaded.data["available_asset_version"], "1.4.58");
    assert!(loaded.data["unfinished_quest_list"].is_array());
    assert!(loaded.data["unfinished_multi_quest_list"].is_array());

    let version_response = cn_support::send_request(
        service.port(),
        "/api/index.php/asset/version_info",
        &encode_request(&json!({"viewer_id": viewer_id})),
    );
    assert!(version_response.contains("Content-Type: application/x-msgpack"));
    let version = decode_response::<Value>(&version_response);
    assert_eq!(version.data_headers.viewer_id, 0);
    assert!(!version.data_headers.asset_update);
    assert!(version.data["base_url"].as_str().is_some());
    assert!(version.data["files_list"]
        .as_str()
        .is_some_and(|path| path.ends_with("/entities/empty.csv")));
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
    assert_eq!(path.data["info"]["target_asset_version"], "1.4.58");
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
            "not-messagepack",
        ));
        assert_eq!(order.data_headers.viewer_id, 0);
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
    for (path, expected) in [
        (
            "/api/index.php/channels/channel_leiting_pay/query_purcharge",
            json!({"status": 3}),
        ),
        (
            "/api/index.php/channels/channel_leiting_pay/set_unfinish_order_status",
            json!({}),
        ),
    ] {
        let response = decode_response::<Value>(&cn_support::send_request(
            service.port(),
            path,
            "not-messagepack",
        ));
        assert_eq!(response.data_headers.viewer_id, 0);
        assert_eq!(response.data, expected);
    }
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 启动链与 SDK 重复调用 ////
