// audience: internal
// # personal-service-player-sync-tests
// 该文件验证普通玩家的密文同步只访问自己的存档和远端对象作用域.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/save_sync_server.rs"]
mod save_sync_server;
mod support;

use save_sync_server::MockSaveSyncServer;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use support::request_with_headers;
use tempfile::TempDir;

fn response_body(response: &str) -> Value {
    serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .expect("response contains a body")
            .1,
    )
    .expect("response body is JSON")
}

fn admin_request(
    service: &PersonalService,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> String {
    let authorization = format!("Bearer {}", service.management_token());
    let encoded = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
    request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", authorization.as_str())],
        &encoded,
    )
}

fn player_request(
    service: &PersonalService,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> String {
    let authorization = format!("Bearer {token}");
    let encoded = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
    request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", authorization.as_str())],
        &encoded,
    )
}

fn signup(
    service: &PersonalService,
    device_id: i64,
) -> cn_support::Envelope<cn_support::SignupData> {
    let response = cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &cn_support::encode_request(&cn_support::SignupRequest { device_id }),
    );
    let signup = cn_support::decode_response(&response);
    cn_support::assert_valid_signup_response(&signup);
    signup
}

fn issue_player_token(service: &PersonalService, viewer_id: i64) -> String {
    let response = admin_request(
        service,
        "POST",
        "/v1/player-access",
        Some(&json!({ "viewer_id": viewer_id })),
    );
    assert!(response.starts_with("HTTP/1.1 201 Created"), "{response}");
    response_body(&response)["token"]
        .as_str()
        .expect("player token is returned")
        .to_owned()
}

//// 验证玩家密文同步和远端对象隔离 [@x380kkm 2026-07-24] ////
#[test]
fn syncs_only_player_scoped_objects_and_slots() {
    let remote = MockSaveSyncServer::start();
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let first = signup(&service, 601);
    let second = signup(&service, 602);
    let first_token = issue_player_token(&service, first.data_headers.viewer_id);
    let second_token = issue_player_token(&service, second.data_headers.viewer_id);

    let target_response = admin_request(
        &service,
        "POST",
        "/v1/save-sync-targets",
        Some(&json!({
            "name": "Loopback vault",
            "scheme": "http",
            "host": "127.0.0.1",
            "port": remote.port(),
            "username": "sync-user",
            "password": "sync-password",
        })),
    );
    assert!(
        target_response.starts_with("HTTP/1.1 201 Created"),
        "{target_response}"
    );
    let target_id = response_body(&target_response)["id"]
        .as_i64()
        .expect("target id is numeric");

    let first_state = response_body(&player_request(
        &service,
        &first_token,
        "GET",
        "/v1/player/local-saves",
        None,
    ));
    let second_state = response_body(&player_request(
        &service,
        &second_token,
        "GET",
        "/v1/player/local-saves",
        None,
    ));
    let first_slot_id = first_state["slots"][0]["id"]
        .as_i64()
        .expect("first slot id is numeric");
    let second_slot_id = second_state["slots"][0]["id"]
        .as_i64()
        .expect("second slot id is numeric");

    let targets = player_request(
        &service,
        &first_token,
        "GET",
        "/v1/player/save-sync-targets",
        None,
    );
    assert!(targets.starts_with("HTTP/1.1 200 OK"), "{targets}");
    assert!(!targets.contains("sync-user"));
    assert!(!targets.contains("sync-password"));

    let first_upload = player_request(
        &service,
        &first_token,
        "POST",
        &format!("/v1/player/local-saves/{first_slot_id}/sync/upload"),
        Some(&json!({ "target_id": target_id, "object_id": "primary" })),
    );
    assert!(
        first_upload.starts_with("HTTP/1.1 200 OK"),
        "{first_upload}"
    );
    assert_eq!(response_body(&first_upload)["object_id"], "primary");
    assert_eq!(remote.etag_revision(), 1);

    let second_upload = player_request(
        &service,
        &second_token,
        "POST",
        &format!("/v1/player/local-saves/{second_slot_id}/sync/upload"),
        Some(&json!({ "target_id": target_id, "object_id": "primary" })),
    );
    assert!(
        second_upload.starts_with("HTTP/1.1 200 OK"),
        "{second_upload}"
    );
    assert_eq!(response_body(&second_upload)["object_id"], "primary");
    assert_eq!(remote.etag_revision(), 2);

    let denied_upload = player_request(
        &service,
        &second_token,
        "POST",
        &format!("/v1/player/local-saves/{first_slot_id}/sync/upload"),
        Some(&json!({ "target_id": target_id, "object_id": "primary" })),
    );
    assert!(
        denied_upload.starts_with("HTTP/1.1 404 Not Found"),
        "{denied_upload}"
    );

    let first_bindings = player_request(
        &service,
        &first_token,
        "GET",
        &format!("/v1/player/local-saves/{first_slot_id}/sync-bindings"),
        None,
    );
    assert!(
        first_bindings.starts_with("HTTP/1.1 200 OK"),
        "{first_bindings}"
    );
    assert_eq!(response_body(&first_bindings)[0]["object_id"], "primary");

    let first_download = player_request(
        &service,
        &first_token,
        "POST",
        "/v1/player/local-saves/sync/download",
        Some(&json!({
            "target_id": target_id,
            "object_id": "primary",
            "name": "Restored player save",
        })),
    );
    assert!(
        first_download.starts_with("HTTP/1.1 201 Created"),
        "{first_download}"
    );

    let denied_download = player_request(
        &service,
        &second_token,
        "POST",
        "/v1/player/local-saves/sync/download",
        Some(&json!({
            "target_id": target_id,
            "object_id": "missing-for-second",
            "name": "Denied restore",
        })),
    );
    assert!(
        denied_download.starts_with("HTTP/1.1 404 Not Found"),
        "{denied_download}"
    );

    service.stop().expect("service stops cleanly");
}
//// /验证玩家密文同步和远端对象隔离 ////

//// 验证跨数据库根恢复玩家密文存档 [@x380kkm 2026-07-24] ////
#[test]
fn restores_player_sync_after_importing_recovery_package() {
    let remote = MockSaveSyncServer::start();
    let source_root = TempDir::new().expect("source service directory is created");
    let source = PersonalService::start(source_root.path(), 0).expect("source service starts");
    let source_signup = signup(&source, 701);
    let source_token = issue_player_token(&source, source_signup.data_headers.viewer_id);
    let source_slot_id = response_body(&player_request(
        &source,
        &source_token,
        "GET",
        "/v1/player/local-saves",
        None,
    ))["slots"][0]["id"]
        .as_i64()
        .expect("source slot id is numeric");
    let source_target = admin_request(
        &source,
        "POST",
        "/v1/save-sync-targets",
        Some(&json!({
            "name": "Loopback vault",
            "scheme": "http",
            "host": "127.0.0.1",
            "port": remote.port(),
            "username": "sync-user",
            "password": "sync-password",
        })),
    );
    assert!(
        source_target.starts_with("HTTP/1.1 201 Created"),
        "{source_target}"
    );
    let target_id = response_body(&source_target)["id"]
        .as_i64()
        .expect("source target id is numeric");
    let upload = player_request(
        &source,
        &source_token,
        "POST",
        &format!("/v1/player/local-saves/{source_slot_id}/sync/upload"),
        Some(&json!({ "target_id": target_id, "object_id": "primary" })),
    );
    assert!(upload.starts_with("HTTP/1.1 200 OK"), "{upload}");

    let recovery = player_request(
        &source,
        &source_token,
        "POST",
        "/v1/player/recovery/export",
        Some(&json!({ "password": "cross-device-password" })),
    );
    assert!(recovery.starts_with("HTTP/1.1 200 OK"), "{recovery}");
    let recovery_package = response_body(&recovery)["package"].clone();
    assert_eq!(recovery_package["algorithm"], "AES-256-GCM");
    assert_eq!(recovery_package["kdf"], "PBKDF2-HMAC-SHA256");
    let recovery_json = recovery_package.to_string();
    assert!(!recovery_json.contains("key_bytes"));
    assert!(!recovery_json.contains("cross-device-password"));
    let admin_recovery = admin_request(
        &source,
        "POST",
        &format!(
            "/v1/player-recovery/{}/export",
            source_signup.data_headers.viewer_id
        ),
        Some(&json!({ "password": "cross-device-password" })),
    );
    assert!(
        admin_recovery.starts_with("HTTP/1.1 200 OK"),
        "{admin_recovery}"
    );
    let player_admin_attempt = player_request(
        &source,
        &source_token,
        "POST",
        &format!(
            "/v1/player-recovery/{}/export",
            source_signup.data_headers.viewer_id
        ),
        Some(&json!({ "password": "cross-device-password" })),
    );
    assert!(
        player_admin_attempt.starts_with("HTTP/1.1 401 Unauthorized"),
        "{player_admin_attempt}"
    );
    let wrong_password = player_request(
        &source,
        &source_token,
        "POST",
        "/v1/player/recovery/import",
        Some(&json!({
            "password": "wrong-password",
            "package": recovery_package.clone(),
        })),
    );
    assert!(
        wrong_password.starts_with("HTTP/1.1 400 Bad Request"),
        "{wrong_password}"
    );
    let mut tampered_package = recovery_package.clone();
    let ciphertext = tampered_package["ciphertext"]
        .as_str()
        .expect("recovery ciphertext is text");
    let replacement = if ciphertext.starts_with('A') {
        "B"
    } else {
        "A"
    };
    tampered_package["ciphertext"] = Value::from(format!("{replacement}{}", &ciphertext[1..]));
    let tampered = player_request(
        &source,
        &source_token,
        "POST",
        "/v1/player/recovery/import",
        Some(&json!({
            "password": "cross-device-password",
            "package": tampered_package,
        })),
    );
    assert!(
        tampered.starts_with("HTTP/1.1 400 Bad Request"),
        "{tampered}"
    );
    source.stop().expect("source service stops cleanly");

    let target_root = TempDir::new().expect("target service directory is created");
    let target = PersonalService::start(target_root.path(), 0).expect("target service starts");
    let target_signup = signup(&target, 702);
    let target_token = issue_player_token(&target, target_signup.data_headers.viewer_id);
    let target_config = admin_request(
        &target,
        "POST",
        "/v1/save-sync-targets",
        Some(&json!({
            "name": "Loopback vault",
            "scheme": "http",
            "host": "127.0.0.1",
            "port": remote.port(),
            "username": "sync-user",
            "password": "sync-password",
        })),
    );
    assert!(
        target_config.starts_with("HTTP/1.1 201 Created"),
        "{target_config}"
    );
    let target_id = response_body(&target_config)["id"]
        .as_i64()
        .expect("target target id is numeric");
    let imported = admin_request(
        &target,
        "POST",
        &format!(
            "/v1/player-recovery/{}/import",
            target_signup.data_headers.viewer_id
        ),
        Some(&json!({
            "password": "cross-device-password",
            "package": recovery_package,
        })),
    );
    assert!(imported.starts_with("HTTP/1.1 200 OK"), "{imported}");
    let downloaded = player_request(
        &target,
        &target_token,
        "POST",
        "/v1/player/local-saves/sync/download",
        Some(&json!({
            "target_id": target_id,
            "object_id": "primary",
            "name": "跨设备恢复",
        })),
    );
    assert!(
        downloaded.starts_with("HTTP/1.1 201 Created"),
        "{downloaded}"
    );
    assert_eq!(response_body(&downloaded)["slot"]["name"], "跨设备恢复");
    target.stop().expect("target service stops cleanly");
}
//// /验证跨数据库根恢复玩家密文存档 ////
