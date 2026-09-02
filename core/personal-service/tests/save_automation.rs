// audience: internal
// # personal-service-save-automation-tests
//
// 该文件验证自动快照配置, 前台恢复, 保留数量, 密文上传冲突和重启恢复.

#[path = "support/save_sync_server.rs"]
mod save_sync_server;

use rusqlite::{params, Connection};
use save_sync_server::MockSaveSyncServer;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

// //// 配置自动快照并在到期后上传密文 [@x380kkm 2026-07-23] ////
#[test]
fn persists_automatic_snapshots_and_reports_upload_conflicts() {
    let remote = MockSaveSyncServer::start();
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let slot_id = import_local_save(&service);
    let automation_path = format!("/v1/local-saves/{slot_id}/automation");

    let direct = http_request(service.port(), "GET", &automation_path, None, None);
    assert_status(&direct, "200 OK");
    let default_config = authorized_request(&service, "GET", &automation_path, None);
    assert_status(&default_config, "200 OK");
    let default_body = response_body(&default_config);
    assert_eq!(default_body["enabled"].as_bool(), Some(false));
    assert_eq!(default_body["interval_seconds"].as_i64(), Some(900));
    assert!(default_body["next_run_at"].is_null());

    let invalid_interval = authorized_request(
        &service,
        "PUT",
        &automation_path,
        Some(&json!({
            "enabled": true,
            "interval_seconds": 59,
            "target_id": null,
            "object_id": null,
        })),
    );
    assert_status(&invalid_interval, "400 Bad Request");

    let created_target = authorized_request(
        &service,
        "POST",
        "/v1/save-sync-targets",
        Some(&json!({
            "name": "Automatic vault",
            "scheme": "http",
            "host": "127.0.0.1",
            "port": remote.port(),
            "username": "sync-user",
            "password": "sync-password",
        })),
    );
    assert_status(&created_target, "201 Created");
    let target_id = response_body(&created_target)["id"]
        .as_i64()
        .expect("sync target has an id");
    let enabled = authorized_request(
        &service,
        "PUT",
        &automation_path,
        Some(&json!({
            "enabled": true,
            "interval_seconds": 60,
            "target_id": target_id,
            "object_id": "device-primary",
        })),
    );
    assert_status(&enabled, "200 OK");

    make_automation_due(root.path(), slot_id);
    let first_result = wait_for_automation(&service, &automation_path, |body| {
        body["last_snapshot_at"].is_string() && body["last_upload_at"].is_string()
    });
    assert_eq!(first_result["pending_upload"].as_bool(), Some(false));
    assert!(first_result["last_error"].is_null());
    let first_upload_at = first_result["last_upload_at"]
        .as_str()
        .expect("first upload time is recorded")
        .to_owned();
    let snapshots = list_snapshots(&service, slot_id);
    assert_eq!(snapshots.as_array().map(Vec::len), Some(1));
    assert_eq!(snapshots[0]["label"].as_str(), Some("Automatic snapshot"));
    let remote_body = remote.encrypted_body();
    assert_eq!(
        remote_body["format"].as_str(),
        Some("starpoint-encrypted-save")
    );
    assert!(!remote_body.to_string().contains("user_info"));

    service.stop().expect("service stops cleanly");
    drop(remote);
    let remote = MockSaveSyncServer::start();
    move_target(root.path(), target_id, remote.port());
    make_automation_due(root.path(), slot_id);
    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    wait_for_snapshot_count(&service, slot_id, 2);
    wait_for_automation(&service, &automation_path, |body| {
        body["last_upload_at"].as_str() != Some(first_upload_at.as_str())
            && body["last_error"].is_null()
            && body["pending_upload"].as_bool() == Some(false)
    });

    let revision_before_overlapping_uploads = remote.etag_revision();
    remote.set_upload_delay(Duration::from_millis(1_500));
    make_automation_due(root.path(), slot_id);
    wait_for_snapshot_count(&service, slot_id, 3);
    make_automation_due(root.path(), slot_id);
    wait_for_snapshot_count(&service, slot_id, 4);
    remote.set_upload_delay(Duration::ZERO);
    wait_for_automation(&service, &automation_path, |body| {
        remote.etag_revision() >= revision_before_overlapping_uploads + 2
            && body["last_error"].is_null()
            && body["pending_upload"].as_bool() == Some(false)
    });

    remote.set_capacity_exceeded(true);
    make_automation_due(root.path(), slot_id);
    let capacity_error = wait_for_automation(&service, &automation_path, |body| {
        body["last_error"].as_str() == Some("save_sync_remote_capacity_exceeded")
    });
    assert_eq!(capacity_error["pending_upload"].as_bool(), Some(false));
    assert_eq!(
        list_snapshots(&service, slot_id).as_array().map(Vec::len),
        Some(5)
    );
    remote.set_capacity_exceeded(false);

    remote.diverge_etag();
    make_automation_due(root.path(), slot_id);
    let conflict = wait_for_automation(&service, &automation_path, |body| {
        body["last_error"].as_str() == Some("save_sync_remote_conflict")
    });
    assert_eq!(conflict["pending_upload"].as_bool(), Some(false));
    assert_eq!(
        list_snapshots(&service, slot_id).as_array().map(Vec::len),
        Some(6)
    );

    let manual_same_label = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/snapshots"),
        Some(&json!({ "label": "Automatic snapshot" })),
    );
    assert_status(&manual_same_label, "201 Created");
    seed_automatic_snapshots(root.path(), slot_id, 50);
    make_automation_due(root.path(), slot_id);
    wait_for_snapshot_count(&service, slot_id, 49);

    let disabled = authorized_request(
        &service,
        "PUT",
        &automation_path,
        Some(&json!({
            "enabled": false,
            "interval_seconds": 60,
            "target_id": target_id,
            "object_id": "device-primary",
        })),
    );
    assert_status(&disabled, "200 OK");
    make_automation_due(root.path(), slot_id);
    thread::sleep(Duration::from_millis(1_200));
    assert_eq!(
        list_snapshots(&service, slot_id).as_array().map(Vec::len),
        Some(49)
    );

    let deleted_target = authorized_request(
        &service,
        "DELETE",
        &format!("/v1/save-sync-targets/{target_id}"),
        None,
    );
    assert_status(&deleted_target, "200 OK");
    let cleared = response_body(&authorized_request(&service, "GET", &automation_path, None));
    assert!(cleared["target_id"].is_null());
    assert!(cleared["object_id"].is_null());
    service.stop().expect("service stops cleanly");
}
// //// /配置自动快照并在到期后上传密文 ////

// //// 为现有快照表补充自动来源标记 [@x380kkm 2026-07-23] ////
#[test]
fn migrates_a_snapshot_table_created_before_save_automation() {
    let root = TempDir::new().expect("temporary service directory is created");
    PersonalService::start(root.path(), 0)
        .expect("initial service starts")
        .stop()
        .expect("initial service stops");
    let connection = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    connection
        .execute_batch(
            "DROP TABLE local_save_automations;
             ALTER TABLE local_save_snapshots DROP COLUMN is_automatic;",
        )
        .expect("automation schema is removed to simulate an existing database");
    drop(connection);

    let service = PersonalService::start(root.path(), 0).expect("existing database migrates");
    let connection = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("migrated database is opened");
    let has_automatic_marker = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('local_save_snapshots')
                 WHERE name = 'is_automatic'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("snapshot marker is inspected");
    assert!(has_automatic_marker);
    service.stop().expect("migrated service stops");
}
// //// /为现有快照表补充自动来源标记 ////

fn import_local_save(service: &PersonalService) -> i64 {
    let response = authorized_request(
        service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({
            "name": "Automatic save",
            "data": {
                "user_info": { "name": "Automation" },
                "user_character_list": {},
            },
        })),
    );
    assert_status(&response, "201 Created");
    response_body(&response)["id"]
        .as_i64()
        .expect("imported slot has an id")
}

fn authorized_request(
    service: &PersonalService,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> String {
    http_request(
        service.port(),
        method,
        path,
        Some(service.management_token()),
        body,
    )
}

fn http_request(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> String {
    let body = body
        .map(serde_json::to_vec)
        .transpose()
        .expect("request body is encoded")
        .unwrap_or_default();
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("service accepts loopback requests");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n",
    )
    .expect("request headers are written");
    if let Some(token) = token {
        write!(stream, "Authorization: Bearer {token}\r\n")
            .expect("authorization header is written");
    }
    write!(stream, "Content-Length: {}\r\n\r\n", body.len()).expect("request length is written");
    stream.write_all(&body).expect("request body is written");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response is read");
    response
}

fn assert_status(response: &str, status: &str) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status}")),
        "unexpected response: {response}"
    );
}

fn response_body(response: &str) -> Value {
    serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("response contains a body separator"),
    )
    .expect("response body is JSON")
}

fn make_automation_due(root: &std::path::Path, slot_id: i64) {
    let connection = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    connection
        .execute(
            "UPDATE local_save_automations
             SET next_run_at = '2000-01-01T00:00:00.000Z', pending_upload = 0
             WHERE slot_id = ?1",
            params![slot_id],
        )
        .expect("automation is made due");
}

fn move_target(root: &std::path::Path, target_id: i64, port: u16) {
    let connection = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    connection
        .execute(
            "UPDATE save_sync_targets SET port = ?2 WHERE id = ?1",
            params![target_id, i64::from(port)],
        )
        .expect("sync target port is replaced");
    connection
        .execute(
            "DELETE FROM local_save_sync_bindings WHERE target_id = ?1",
            params![target_id],
        )
        .expect("old sync binding is removed");
}

fn seed_automatic_snapshots(root: &std::path::Path, slot_id: i64, count: usize) {
    let mut connection = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let transaction = connection
        .transaction()
        .expect("snapshot seed transaction starts");
    for _ in 0..count {
        transaction
            .execute(
                "INSERT INTO local_save_snapshots (
                     slot_id, label, data_json, created_at, is_automatic
                 )
                 SELECT slots.id, 'Automatic snapshot', player_snapshots.data_json,
                        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 1
                 FROM local_save_slots AS slots
                 JOIN player_snapshots ON player_snapshots.account_id = slots.account_id
                 WHERE slots.id = ?1",
                params![slot_id],
            )
            .expect("automatic snapshot is seeded");
    }
    transaction
        .commit()
        .expect("snapshot seed transaction commits");
}

fn list_snapshots(service: &PersonalService, slot_id: i64) -> Value {
    let response = authorized_request(
        service,
        "GET",
        &format!("/v1/local-saves/{slot_id}/snapshots"),
        None,
    );
    assert_status(&response, "200 OK");
    response_body(&response)
}

fn wait_for_snapshot_count(service: &PersonalService, slot_id: i64, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut actual = None;
    while Instant::now() < deadline {
        actual = list_snapshots(service, slot_id).as_array().map(Vec::len);
        if actual == Some(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("automatic snapshot count did not reach {expected}; actual={actual:?}");
}

fn wait_for_automation(
    service: &PersonalService,
    path: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last_body = Value::Null;
    while Instant::now() < deadline {
        let response = authorized_request(service, "GET", path, None);
        assert_status(&response, "200 OK");
        let body = response_body(&response);
        if predicate(&body) {
            return body;
        }
        last_body = body;
        thread::sleep(Duration::from_millis(20));
    }
    panic!("automatic save state did not reach the expected value: {last_body}");
}
