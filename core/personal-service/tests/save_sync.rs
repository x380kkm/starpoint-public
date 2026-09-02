// audience: internal
// # personal-service-save-sync-tests
//
// 该文件验证密文服务器配置, 上传版本控制, 容量错误, 隔离下载和事务回滚.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
#[path = "support/save_sync_server.rs"]
mod save_sync_server;
mod support;

use local_save_support::{
    activate_local_save, assert_status, authorized_request, export_local_save, list_local_saves,
    load, response_body, signup,
};
use rusqlite::Connection;
use save_sync_server::MockSaveSyncServer;
use serde_json::json;
use starpoint_personal_service::PersonalService;
use std::time::Duration;
use tempfile::TempDir;

// //// 配置服务器并同步本地存档 [@x380kkm 2026-07-23] ////
#[test]
fn uploads_detects_conflicts_and_downloads_an_isolated_slot() {
    let remote = MockSaveSyncServer::start();
    remote.set_upload_delay(Duration::from_millis(10));
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    assert!(support::request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    let device_id = 109;
    signup(service.port(), device_id);
    let slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup slot has an id");
    let original_name = export_local_save(&service, slot_id)["data"]["user_info"]["name"].clone();

    let insecure_target = authorized_request(
        &service,
        "POST",
        "/v1/save-sync-targets",
        Some(&json!({
            "name": "Unsafe remote",
            "scheme": "http",
            "host": "example.com",
            "port": 80,
            "username": "sync-user",
            "password": "sync-password",
        })),
    );
    assert_status(&insecure_target, "400 Bad Request");

    let oversized_host_name = "a".repeat(254);
    let oversized_host = authorized_request(
        &service,
        "POST",
        "/v1/save-sync-targets",
        Some(&json!({
            "name": "Oversized host",
            "scheme": "https",
            "host": oversized_host_name,
            "port": 443,
            "username": "sync-user",
            "password": "sync-password",
        })),
    );
    assert_status(&oversized_host, "400 Bad Request");

    let created_target = authorized_request(
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
    assert_status(&created_target, "201 Created");
    let target = response_body(&created_target);
    let target_id = target["id"].as_i64().expect("sync target has an id");
    assert_eq!(target["has_credentials"].as_bool(), Some(true));
    assert!(target.get("password").is_none());
    let listed_targets = authorized_request(&service, "GET", "/v1/save-sync-targets", None);
    assert_status(&listed_targets, "200 OK");
    assert_eq!(
        response_body(&listed_targets).as_array().map(Vec::len),
        Some(1)
    );
    assert!(!listed_targets.contains("sync-password"));

    let upload_body = json!({ "target_id": target_id, "object_id": "device-primary" });
    let first_upload = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/sync/upload"),
        Some(&upload_body),
    );
    assert_status(&first_upload, "200 OK");
    let first_etag = response_body(&first_upload)["etag"]
        .as_str()
        .expect("first upload has an ETag")
        .to_owned();
    assert_eq!(remote.etag_revision(), 1);
    let remote_body = remote.encrypted_body();
    assert_eq!(
        remote_body["format"].as_str(),
        Some("starpoint-encrypted-save")
    );
    assert!(!remote_body.to_string().contains("user_info"));
    let first_bindings = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{slot_id}/sync-bindings"),
        None,
    );
    assert_eq!(
        response_body(&first_bindings)[0]["target_id"].as_i64(),
        Some(target_id),
    );

    let second_upload = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/sync/upload"),
        Some(&upload_body),
    );
    assert_status(&second_upload, "200 OK");
    let second_etag = response_body(&second_upload)["etag"]
        .as_str()
        .expect("second upload has an ETag")
        .to_owned();
    assert_ne!(second_etag, first_etag);

    remote.diverge_etag();
    let conflicting_upload = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/sync/upload"),
        Some(&upload_body),
    );
    assert_status(&conflicting_upload, "409 Conflict");
    assert_eq!(
        response_body(&conflicting_upload)["error"].as_str(),
        Some("save_sync_remote_conflict"),
    );

    remote.set_capacity_exceeded(true);
    let full_remote_upload = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/sync/upload"),
        Some(&upload_body),
    );
    assert_status(&full_remote_upload, "409 Conflict");
    assert_eq!(
        response_body(&full_remote_upload)["error"].as_str(),
        Some("save_sync_remote_capacity_exceeded"),
    );
    remote.set_capacity_exceeded(false);

    let download = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/sync/download",
        Some(&json!({
            "target_id": target_id,
            "object_id": "device-primary",
            "name": "Downloaded remote save",
        })),
    );
    assert_status(&download, "201 Created");
    let downloaded_slot_id = response_body(&download)["slot"]["id"]
        .as_i64()
        .expect("download creates a slot");
    assert_ne!(downloaded_slot_id, slot_id);
    assert_eq!(
        list_local_saves(&service)["slots"].as_array().map(Vec::len),
        Some(2),
    );
    let old_bindings = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{slot_id}/sync-bindings"),
        None,
    );
    assert_eq!(
        response_body(&old_bindings).as_array().map(Vec::len),
        Some(0)
    );
    let downloaded_bindings = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{downloaded_slot_id}/sync-bindings"),
        None,
    );
    assert_eq!(
        response_body(&downloaded_bindings)[0]["object_id"].as_str(),
        Some("device-primary"),
    );
    activate_local_save(&service, downloaded_slot_id, device_id);
    let downloaded_signup = signup(service.port(), device_id);
    let downloaded = load(
        service.port(),
        downloaded_signup.data_headers.viewer_id,
        "1.4.99-save-sync",
    );
    assert_eq!(downloaded.data["user_info"]["name"], original_name);

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    database
        .execute_batch(
            "CREATE TRIGGER reject_sync_binding
             BEFORE INSERT ON local_save_sync_bindings
             BEGIN
                 SELECT RAISE(FAIL, 'forced sync binding failure');
             END;",
        )
        .expect("binding failure trigger is installed");
    let failed_download = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/sync/download",
        Some(&json!({
            "target_id": target_id,
            "object_id": "device-primary",
            "name": "Rolled back download",
        })),
    );
    assert_status(&failed_download, "500 Internal Server Error");
    assert_eq!(
        list_local_saves(&service)["slots"].as_array().map(Vec::len),
        Some(2),
    );
    let binding_after_rollback = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{downloaded_slot_id}/sync-bindings"),
        None,
    );
    assert_eq!(
        response_body(&binding_after_rollback)[0]["object_id"].as_str(),
        Some("device-primary"),
    );
    database
        .execute_batch("DROP TRIGGER reject_sync_binding;")
        .expect("binding failure trigger is removed");
    drop(database);

    let changed_identity = authorized_request(
        &service,
        "PUT",
        &format!("/v1/save-sync-targets/{target_id}"),
        Some(&json!({
            "name": "Loopback vault",
            "scheme": "http",
            "host": "127.0.0.1",
            "port": remote.port(),
            "username": "replacement-user",
            "password": "replacement-password",
        })),
    );
    assert_status(&changed_identity, "200 OK");
    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    let binding_count = database
        .query_row("SELECT COUNT(*) FROM local_save_sync_bindings", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("sync binding count is read");
    assert_eq!(binding_count, 0);
    drop(database);

    let deleted_target = authorized_request(
        &service,
        "DELETE",
        &format!("/v1/save-sync-targets/{target_id}"),
        None,
    );
    assert_status(&deleted_target, "200 OK");
    let empty_targets = authorized_request(&service, "GET", "/v1/save-sync-targets", None);
    assert_eq!(
        response_body(&empty_targets).as_array().map(Vec::len),
        Some(0)
    );
    service.stop().expect("service stops cleanly");
}
// //// /配置服务器并同步本地存档 ////
