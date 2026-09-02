// audience: internal
// # transfer-binding-integration
//
// 该测试用两个隔离个人服务实例验证 interval 绑定在重启后执行并保存共同基线.
// 测试进程只连接 loopback, 结束时停止两个服务.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
#[path = "support/mod.rs"]
mod support;

use local_save_support::{authorized_request, list_local_saves, response_body, signup};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

// //// 验证 interval 绑定跨重启上传并保存共同基线 [@x380kkm 2026-08-03] ////
#[test]
fn uploads_due_interval_binding_after_restart() {
    let root = tempdir().expect("temporary service root is created");
    let source_root = root.path().join("source");
    let target_root = root.path().join("target");
    let target = PersonalService::start(&target_root, 0).expect("target service starts");
    let source = PersonalService::start(&source_root, 0).expect("source service starts");
    signup(target.port(), 97001);
    signup(source.port(), 97002);
    let target_slot_id = first_slot_id(&target);
    let source_slot_id = first_slot_id(&source);
    let target_access = issue_target_slot_token(&target, target_slot_id);
    let profile = authorized_request(
        &source,
        "POST",
        "/v1/server-profiles",
        Some(&json!({
            "name": "Interval target",
            "scheme": "http",
            "host": "127.0.0.1",
            "port": target.port(),
        })),
    );
    assert!(
        profile.starts_with("HTTP/1.1 201 Created"),
        "target profile is created"
    );
    let profile_id = response_body(&profile)["id"]
        .as_i64()
        .expect("target profile ID is returned");
    let binding = authorized_request(
        &source,
        "POST",
        &format!("/v1/local-saves/{source_slot_id}/transfer-bindings"),
        Some(&json!({
            "target_profile_id": profile_id,
            "target_instance_kind": "local",
            "target_instance_id": target_access.instance_id,
            "target_slot_id": target_slot_id,
            "target_token": target_access.token,
            "upload_mode": "interval",
            "pull_mode": "manual",
            "conflict_policy": "ask",
            "interval_seconds": 60,
            "enabled": true,
        })),
    );
    let binding_body = response_body(&binding);
    assert!(
        binding.starts_with("HTTP/1.1 201 Created"),
        "interval binding is created: {} {}",
        binding.lines().next().unwrap_or("missing status"),
        binding_body["error"]
    );
    let binding_id = binding_body["binding_id"]
        .as_str()
        .expect("binding ID is returned")
        .to_owned();
    source
        .stop()
        .expect("source service stops before schedule edit");
    let database = Connection::open(source_root.join("personal-service.sqlite3"))
        .expect("source database opens");
    database
        .execute(
            "UPDATE transfer_bindings SET next_run_at = '1970-01-01T00:00:00.000Z' WHERE id = ?1",
            params![binding_id],
        )
        .expect("binding is made due");
    drop(database);

    let source = PersonalService::start(&source_root, 0).expect("source service restarts");
    let binding_path = format!("/v1/local-saves/{source_slot_id}/transfer-bindings/{binding_id}");
    let synchronized = (0..50).find_map(|_| {
        let response = authorized_request(&source, "GET", &binding_path, None);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "binding remains readable"
        );
        let body = response_body(&response);
        if body["last_synced_at"].is_string() {
            Some(body)
        } else {
            thread::sleep(Duration::from_millis(100));
            None
        }
    });
    let synchronized = synchronized.expect("interval binding runs after restart");
    assert_eq!(synchronized["last_error"], Value::Null);
    assert_eq!(synchronized["pending_direction"], "none");
    let source_export = local_save_support::export_local_save(&source, source_slot_id);
    let target_export = download_target_slot(&target, &target_access, target_slot_id);
    assert_eq!(
        synchronized["last_common_etag"],
        source_export["payloadSha256"]
    );
    assert_eq!(
        target_export["payloadSha256"],
        source_export["payloadSha256"]
    );
    source.stop().expect("source service stops");
    target.stop().expect("target service stops");
}
// //// /验证 interval 绑定跨重启上传并保存共同基线 ////

struct TargetAccess {
    instance_id: String,
    token: String,
}

fn first_slot_id(service: &PersonalService) -> i64 {
    list_local_saves(service)["slots"][0]["id"]
        .as_i64()
        .expect("default local slot exists")
}

fn issue_target_slot_token(service: &PersonalService, slot_id: i64) -> TargetAccess {
    let shell = authorized_request(
        service,
        "POST",
        &format!("/v1/local-saves/{slot_id}/transfer-tokens/shell"),
        Some(&json!({ "deviceName": "Interval binding test" })),
    );
    assert!(
        shell.starts_with("HTTP/1.1 201 Created"),
        "target shell token is issued"
    );
    let shell = response_body(&shell);
    let shell_token = shell["token"].as_str().expect("shell token is returned");
    let authorization = format!("Bearer {shell_token}");
    let body = json!({
        "slotId": slot_id,
        "permission": "both",
        "deviceName": "Interval binding slot",
    })
    .to_string();
    let slot = support::request_with_headers(
        service.port(),
        "POST",
        "/v1/transfer/v1/shell/slot-tokens",
        "application/json",
        &[("Authorization", authorization.as_str())],
        body.as_bytes(),
    );
    assert!(
        slot.starts_with("HTTP/1.1 201 Created"),
        "target slot token is issued"
    );
    let slot = response_body(&slot);
    TargetAccess {
        instance_id: slot["instanceId"]
            .as_str()
            .expect("target instance ID is returned")
            .to_owned(),
        token: slot["token"]
            .as_str()
            .expect("target slot token is returned")
            .to_owned(),
    }
}

fn download_target_slot(service: &PersonalService, access: &TargetAccess, slot_id: i64) -> Value {
    let authorization = format!("Bearer {}", access.token);
    let response = support::request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/transfer/v1/slots/{slot_id}"),
        "application/json",
        &[("Authorization", authorization.as_str())],
        &[],
    );
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "target slot remains downloadable"
    );
    response_body(&response)
}
