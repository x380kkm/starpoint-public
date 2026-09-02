// audience: internal
// # personal-service-local-save-tests
//
// 该文件通过真实 HTTP 请求验证本地存档槽和导入导出.
// 该文件验证快照和 revision 恢复.
// 该文件验证 ETag 冲突不覆盖当前 revision.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use local_save_support::{
    activate_local_save, assert_status, authorized_request, authorized_request_with_headers,
    export_local_save, list_local_saves, load, response_body, signup, update_slot_player_snapshot,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 读取并修改本地存档测试状态 [@x380kkm 2026-07-23] ////
fn safety_snapshot_data(root: &Path, slot_id: i64) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let serialized = database
        .query_row(
            "SELECT data_json FROM local_save_snapshots
             WHERE slot_id = ?1 AND label = 'Before restore'
             ORDER BY id DESC LIMIT 1",
            params![slot_id],
            |row| row.get::<_, String>(0),
        )
        .expect("restore safety snapshot is read");
    serde_json::from_str(&serialized).expect("restore safety snapshot is JSON")
}

// //// 读取本地存档槽关联的玩家快照 [@x380kkm 2026-09-01] ////
fn slot_player_data(root: &Path, slot_id: i64) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let serialized = database
        .query_row(
            "SELECT player_snapshots.data_json
             FROM player_snapshots
             JOIN local_save_slots
               ON local_save_slots.account_id = player_snapshots.account_id
             WHERE local_save_slots.id = ?1",
            params![slot_id],
            |row| row.get::<_, String>(0),
        )
        .expect("slot player snapshot is read");
    serde_json::from_str(&serialized).expect("slot player snapshot is JSON")
}
// //// /读取本地存档槽关联的玩家快照 ////

fn create_active_single_quest(root: &Path, slot_id: i64) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    let inserted = database
        .execute(
            "INSERT INTO active_single_quests (
                 account_id, quest_id, category, use_boss_boost_point,
                 use_boost_point, is_auto_start_mode
             )
             SELECT account_id, 1001002, 1, 0, 0, 0
             FROM local_save_slots WHERE id = ?1",
            params![slot_id],
        )
        .expect("active single quest is created");
    assert_eq!(inserted, 1);
}

fn active_single_quest_count(root: &Path, slot_id: i64) -> i64 {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    database
        .query_row(
            "SELECT COUNT(*)
             FROM active_single_quests
             JOIN local_save_slots
               ON local_save_slots.account_id = active_single_quests.account_id
             WHERE local_save_slots.id = ?1",
            params![slot_id],
            |row| row.get(0),
        )
        .expect("active single quest count is read")
}
// //// /读取并修改本地存档测试状态 ////

// //// 通过管理 API 管理本地存档槽 [@x380kkm 2026-07-23] ////
#[test]
fn manages_local_save_slots_through_http() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    assert!(support::request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    let device_id = 41;
    let first_signup = signup(service.port(), device_id);

    let state = list_local_saves(&service);
    assert_eq!(state["slots"].as_array().map(Vec::len), Some(1));
    let first_slot_id = state["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a local save slot");
    assert_eq!(state["devices"][0]["device_id"].as_i64(), Some(device_id));
    assert_eq!(
        state["devices"][0]["active_slot_id"].as_i64(),
        Some(first_slot_id),
    );

    let context_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{first_slot_id}/context"),
        None,
    );
    assert_status(&context_response, "200 OK");
    let context = response_body(&context_response);
    assert_eq!(context["slot"]["id"].as_i64(), Some(first_slot_id));
    assert_eq!(
        context["viewer_id"].as_i64(),
        Some(first_signup.data_headers.viewer_id)
    );
    assert_eq!(context["active_device_ids"][0].as_i64(), Some(device_id));

    let mail_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/mails"),
        Some(&json!({
            "title": "Welcome resources",
            "body": "Use these resources for the offline test.",
            "sender": "Starpoint",
            "rewards": { "freeVmoney": 100 },
        })),
    );
    assert_status(&mail_response, "201 Created");
    assert!(response_body(&mail_response)["id"].as_i64().is_some());
    let mails_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{first_slot_id}/mails"),
        None,
    );
    assert_status(&mails_response, "200 OK");
    assert_eq!(
        response_body(&mails_response).as_array().map(Vec::len),
        Some(1)
    );

    let viewer_field_rejected = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/mails"),
        Some(&json!({
            "viewer_id": first_signup.data_headers.viewer_id,
            "title": "Invalid shape",
            "body": "The slot route owns the account mapping.",
            "sender": "Starpoint",
            "rewards": { "freeVmoney": 1 },
        })),
    );
    assert_status(&viewer_field_rejected, "400 Bad Request");

    let direct_list = support::request_with_headers(
        service.port(),
        "GET",
        "/v1/local-saves",
        "application/json",
        &[],
        b"",
    );
    assert_status(&direct_list, "200 OK");
    let direct_context = support::request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/local-saves/{first_slot_id}/context"),
        "application/json",
        &[],
        b"",
    );
    assert_status(&direct_context, "200 OK");
    let direct_mails = support::request_with_headers(
        service.port(),
        "GET",
        &format!("/v1/local-saves/{first_slot_id}/mails"),
        "application/json",
        &[],
        b"",
    );
    assert_status(&direct_mails, "200 OK");
    let bad_token = support::request_with_headers(
        service.port(),
        "GET",
        "/v1/local-saves",
        "application/json",
        &[("Authorization", "Bearer invalid-token")],
        b"",
    );
    assert_status(&bad_token, "401 Unauthorized");

    let first_export = export_local_save(&service, first_slot_id);
    assert_eq!(
        first_export["format"].as_str(),
        Some("starpoint-save-package")
    );
    assert_eq!(first_export["version"].as_i64(), Some(1));
    assert_eq!(first_export["game"].as_str(), Some("starpoint"));
    assert_eq!(first_export["region"].as_str(), Some("cn"));
    assert_eq!(
        first_export["source"]["instanceKind"].as_str(),
        Some("local")
    );
    assert_eq!(
        first_export["source"]["slotId"]
            .as_str()
            .and_then(|value| value.parse::<i64>().ok()),
        Some(first_slot_id)
    );
    assert_eq!(
        first_export["payloadSha256"].as_str().map(str::len),
        Some(64)
    );
    let initial_revision_id = first_export["source"]["revisionId"]
        .as_str()
        .expect("local export contains a revision id")
        .to_owned();
    assert_eq!(initial_revision_id.len(), 32);
    assert!(first_export["data"]["user_info"].is_object());
    assert!(first_export["data"]["user_character_list"].is_object());

    // //// 验证本地 revision 分支和恢复 [@x380kkm 2026-07-27] ////
    update_slot_player_snapshot(root.path(), first_slot_id, |data| {
        data["user_info"]["name"] = Value::from("Local revision branch");
    });
    let branch_export = export_local_save(&service, first_slot_id);
    let branch_revision_id = branch_export["source"]["revisionId"]
        .as_str()
        .expect("changed export contains a revision id")
        .to_owned();
    assert_ne!(branch_revision_id, initial_revision_id);
    let revisions_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{first_slot_id}/revisions"),
        None,
    );
    assert_status(&revisions_response, "200 OK");
    let revisions = response_body(&revisions_response);
    assert_eq!(
        revisions["current_revision_id"].as_str(),
        Some(branch_revision_id.as_str())
    );
    assert_eq!(revisions["revisions"].as_array().map(Vec::len), Some(2));
    assert!(revisions["revisions"]
        .as_array()
        .expect("revision list is an array")
        .iter()
        .any(|revision| {
            revision["id"] == branch_revision_id
                && revision["parent_revision_id"] == initial_revision_id
        }));
    let stale_etag = format!(
        "\"{}\"",
        first_export["payloadSha256"]
            .as_str()
            .expect("initial export contains an etag")
    );
    let stale_restore = authorized_request_with_headers(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/revisions/{initial_revision_id}/restore"),
        &[("If-Match", &stale_etag)],
        None,
    );
    assert_status(&stale_restore, "409 Conflict");
    assert_eq!(
        response_body(&stale_restore)["currentRevisionId"].as_str(),
        Some(branch_revision_id.as_str())
    );
    let restore_revision_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/revisions/{initial_revision_id}/restore"),
        None,
    );
    assert_status(&restore_revision_response, "200 OK");
    let restored_revision = response_body(&restore_revision_response);
    assert_eq!(restored_revision["restored"].as_bool(), Some(true));
    assert_eq!(
        restored_revision["revision"]["parent_revision_id"].as_str(),
        Some(branch_revision_id.as_str())
    );
    assert_eq!(
        export_local_save(&service, first_slot_id)["data"]["user_info"]["name"],
        first_export["data"]["user_info"]["name"],
    );
    // //// /验证本地 revision 分支和恢复 ////

    let invalid_import = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({
            "format": "starpoint-local-save",
            "version": 1,
            "name": "Invalid save",
            "data": {},
        })),
    );
    assert_status(&invalid_import, "400 Bad Request");
    assert_eq!(
        list_local_saves(&service)["slots"].as_array().map(Vec::len),
        Some(1),
    );

    let copy_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/copy"),
        Some(&json!({ "name": "Copied save" })),
    );
    assert_status(&copy_response, "201 Created");
    let copied_slot_id = response_body(&copy_response)["id"]
        .as_i64()
        .expect("copied slot has an id");
    assert_ne!(copied_slot_id, first_slot_id);
    assert_eq!(
        export_local_save(&service, copied_slot_id)["data"],
        first_export["data"],
    );

    let copied_context_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{copied_slot_id}/context"),
        None,
    );
    assert_status(&copied_context_response, "200 OK");
    assert!(response_body(&copied_context_response)["viewer_id"].is_null());
    let copied_mail_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{copied_slot_id}/mails"),
        Some(&json!({
            "title": "Copied slot resources",
            "body": "This mail is addressed by slot account.",
            "sender": "Starpoint",
            "rewards": { "freeVmoney": 5 },
        })),
    );
    assert_status(&copied_mail_response, "201 Created");
    let copied_mails_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{copied_slot_id}/mails"),
        None,
    );
    assert_status(&copied_mails_response, "200 OK");
    assert_eq!(
        response_body(&copied_mails_response)
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    activate_local_save(&service, copied_slot_id, device_id);
    let copied_signup = signup(service.port(), device_id);
    assert_eq!(copied_signup.data.new_account, 0);
    assert_ne!(
        copied_signup.data_headers.viewer_id,
        first_signup.data_headers.viewer_id,
    );
    let copied_load = load(
        service.port(),
        copied_signup.data_headers.viewer_id,
        "1.4.99-local-copy",
    );
    assert_eq!(
        copied_load.data["user_info"]["name"],
        first_export["data"]["user_info"]["name"],
    );

    let mut imported_data = export_local_save(&service, copied_slot_id)["data"].clone();
    imported_data["user_info"]["name"] = Value::from("Imported Player");
    imported_data["user_info"]["bond_token"] = Value::from(7);
    imported_data["associate_token"] = Value::from("legacy-associate-token");
    imported_data["user_tutorial"]["viewer_id"] = Value::from(314);
    imported_data["follow_info"] = json!([{ "viewer_id": 2718 }]);
    imported_data["permissions"] = json!(["manage"]);
    imported_data["large_test_payload"] = Value::from("x".repeat(32 * 1024));
    let import = json!({
        "format": "starpoint-local-save",
        "version": 1,
        "name": "Large imported save",
        "data": imported_data,
    });
    let import_size = import.to_string().len();
    assert!(import_size > 16 * 1024);
    assert!(import_size < 8 * 1024 * 1024);
    let import_response =
        authorized_request(&service, "POST", "/v1/local-saves/import", Some(&import));
    assert_status(&import_response, "201 Created");
    let imported_slot_id = response_body(&import_response)["id"]
        .as_i64()
        .expect("imported slot has an id");
    let imported_export = export_local_save(&service, imported_slot_id);
    assert!(imported_export["data"].get("associate_token").is_none());
    assert!(imported_export["data"]["user_tutorial"]
        .get("viewer_id")
        .is_none());
    assert!(imported_export["data"].get("follow_info").is_none());
    assert!(imported_export["data"].get("permissions").is_none());
    assert_eq!(
        imported_export["data"]["user_info"]["bond_token"].as_i64(),
        Some(7),
    );
    assert_eq!(
        imported_export["data"]["large_test_payload"]
            .as_str()
            .map(str::len),
        Some(32 * 1024),
    );

    activate_local_save(&service, imported_slot_id, device_id);
    let imported_signup = signup(service.port(), device_id);
    let imported_load = load(
        service.port(),
        imported_signup.data_headers.viewer_id,
        "1.4.99-local-import",
    );
    assert_eq!(
        imported_load.data["user_info"]["name"].as_str(),
        Some("Imported Player"),
    );
    assert_eq!(
        imported_load.data["associate_token"].as_str(),
        Some("associate_token"),
    );
    assert!(imported_load.data["user_tutorial"].is_null());
    assert_eq!(
        imported_load.data["large_test_payload"]
            .as_str()
            .map(str::len),
        Some(32 * 1024),
    );
    assert_eq!(
        slot_player_data(root.path(), imported_slot_id)["associate_token"].as_str(),
        Some("associate_token"),
    );

    let snapshot_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{imported_slot_id}/snapshots"),
        Some(&json!({ "label": "Imported checkpoint" })),
    );
    assert_status(&snapshot_response, "201 Created");
    assert_eq!(
        response_body(&snapshot_response)["label"].as_str(),
        Some("Imported checkpoint"),
    );
    let snapshots_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{imported_slot_id}/snapshots"),
        None,
    );
    assert_status(&snapshots_response, "200 OK");
    let snapshots = response_body(&snapshots_response);
    assert_eq!(snapshots.as_array().map(Vec::len), Some(1));
    assert_eq!(snapshots[0]["label"].as_str(), Some("Imported checkpoint"));

    service.stop().expect("service stops cleanly");
}
// //// /通过管理 API 管理本地存档槽 ////

// //// 导入 portable 和 legacy 存档包 [@x380kkm 2026-08-31] ////
#[test]
fn imports_portable_and_legacy_save_packages() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let device_id = 811;
    signup(service.port(), device_id);
    let state = list_local_saves(&service);
    let source_slot_id = state["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a source slot");
    let package = export_local_save(&service, source_slot_id);

    let default_import =
        authorized_request(&service, "POST", "/v1/local-saves/import", Some(&package));
    assert_status(&default_import, "201 Created");
    assert_eq!(
        response_body(&default_import)["name"].as_str(),
        Some("Save 1 (2)"),
    );

    let package = export_local_save(&service, source_slot_id);
    let custom_import = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({ "name": "自定义存档", "data": package })),
    );
    assert_status(&custom_import, "201 Created");
    let custom_import = response_body(&custom_import);
    assert_eq!(custom_import["name"].as_str(), Some("自定义存档"));
    let custom_slot_id = custom_import["id"]
        .as_i64()
        .expect("custom import creates a slot");
    let custom_export = export_local_save(&service, custom_slot_id);
    assert!(custom_export["data"].get("format").is_none());
    assert!(custom_export["data"]["user_info"].is_object());

    let legacy_import = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({
            "name": "",
            "data": {
                "format": "starpoint-local-save",
                "version": 1,
                "name": "Legacy import",
                "data": custom_export["data"],
            },
        })),
    );
    assert_status(&legacy_import, "201 Created");
    assert_eq!(
        response_body(&legacy_import)["name"].as_str(),
        Some("Legacy import"),
    );

    activate_local_save(&service, custom_slot_id, device_id);
    let imported_signup = signup(service.port(), device_id);
    let imported_load = load(
        service.port(),
        imported_signup.data_headers.viewer_id,
        "1.4.99-portable-import",
    );
    assert!(imported_load.data.get("format").is_none());
    assert_eq!(
        imported_load.data["associate_token"].as_str(),
        Some("associate_token"),
    );
    assert!(imported_load.data["user_info"].is_object());
    assert!(imported_load.data["user_character_list"].is_object());

    service.stop().expect("service stops cleanly");
}
// //// /导入 portable 和 legacy 存档包 ////

// //// 重启后恢复快照并隔离其他槽位 [@x380kkm 2026-07-23] ////
#[test]
fn restores_snapshot_after_restart_without_changing_other_slots() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let device_id = 77;
    signup(service.port(), device_id);
    let state = list_local_saves(&service);
    let source_slot_id = state["slots"][0]["id"]
        .as_i64()
        .expect("signup slot has an id");
    let source_export = export_local_save(&service, source_slot_id);
    let original_name = source_export["data"]["user_info"]["name"].clone();

    let snapshot_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{source_slot_id}/snapshots"),
        Some(&json!({ "label": "Baseline" })),
    );
    assert_status(&snapshot_response, "201 Created");
    let snapshot_id = response_body(&snapshot_response)["id"]
        .as_i64()
        .expect("baseline snapshot has an id");

    let copy_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{source_slot_id}/copy"),
        Some(&json!({ "name": "Isolated save" })),
    );
    assert_status(&copy_response, "201 Created");
    let isolated_slot_id = response_body(&copy_response)["id"]
        .as_i64()
        .expect("isolated slot has an id");
    service.stop().expect("service stops before SQLite update");

    update_slot_player_snapshot(root.path(), source_slot_id, |data| {
        data["user_info"]["name"] = Value::from("Changed before restore");
    });
    update_slot_player_snapshot(root.path(), isolated_slot_id, |data| {
        data["user_info"]["name"] = Value::from("Isolated slot change");
    });
    create_active_single_quest(root.path(), source_slot_id);
    assert_eq!(active_single_quest_count(root.path(), source_slot_id), 1);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    assert_eq!(
        export_local_save(&service, source_slot_id)["data"]["user_info"]["name"].as_str(),
        Some("Changed before restore"),
    );
    let restore_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{source_slot_id}/snapshots/{snapshot_id}/restore"),
        None,
    );
    assert_status(&restore_response, "200 OK");
    let restored = response_body(&restore_response);
    assert_eq!(restored["restored"].as_bool(), Some(true));
    assert_eq!(
        restored["safety_snapshot"]["label"].as_str(),
        Some("Before restore"),
    );
    assert_eq!(
        restored["safety_snapshot"]["slot_id"].as_i64(),
        Some(source_slot_id),
    );

    assert_eq!(
        export_local_save(&service, source_slot_id)["data"]["user_info"]["name"],
        original_name,
    );
    assert_eq!(
        export_local_save(&service, isolated_slot_id)["data"]["user_info"]["name"].as_str(),
        Some("Isolated slot change"),
    );
    let snapshots_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{source_slot_id}/snapshots"),
        None,
    );
    assert_status(&snapshots_response, "200 OK");
    let snapshots = response_body(&snapshots_response);
    assert_eq!(snapshots.as_array().map(Vec::len), Some(2));
    assert!(snapshots
        .as_array()
        .expect("snapshot list is an array")
        .iter()
        .any(|snapshot| snapshot["label"] == "Baseline"));
    assert!(snapshots
        .as_array()
        .expect("snapshot list is an array")
        .iter()
        .any(|snapshot| snapshot["label"] == "Before restore"));

    activate_local_save(&service, source_slot_id, device_id);
    let restored_signup = signup(service.port(), device_id);
    let restored_load = load(
        service.port(),
        restored_signup.data_headers.viewer_id,
        "1.4.99-local-restore",
    );
    assert_eq!(restored_load.data["user_info"]["name"], original_name);
    service.stop().expect("service stops cleanly");

    assert_eq!(active_single_quest_count(root.path(), source_slot_id), 0);
    let safety_data = safety_snapshot_data(root.path(), source_slot_id);
    assert_eq!(
        safety_data["user_info"]["name"].as_str(),
        Some("Changed before restore"),
    );
}
// //// /重启后恢复快照并隔离其他槽位 ////

// //// 验证单人战斗期间不能切换当前存档槽 [@x380kkm 2026-08-18] ////
#[test]
fn rejects_slot_activation_while_current_slot_has_active_single_battle() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let device_id = 91;
    signup(service.port(), device_id);
    let state = list_local_saves(&service);
    let source_slot_id = state["slots"][0]["id"]
        .as_i64()
        .expect("signup slot has an id");
    let copy_response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{source_slot_id}/copy"),
        Some(&json!({ "name": "Battle protected copy" })),
    );
    assert_status(&copy_response, "201 Created");
    let target_slot_id = response_body(&copy_response)["id"]
        .as_i64()
        .expect("copied slot has an id");
    service.stop().expect("service stops before SQLite update");

    create_active_single_quest(root.path(), source_slot_id);
    assert_eq!(active_single_quest_count(root.path(), source_slot_id), 1);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let response = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{target_slot_id}/activate"),
        Some(&json!({ "device_id": device_id })),
    );
    assert_status(&response, "409 Conflict");
    assert_eq!(response_body(&response)["error"], "local_save_busy");
    assert_eq!(
        list_local_saves(&service)["devices"][0]["active_slot_id"].as_i64(),
        Some(source_slot_id),
    );

    activate_local_save(&service, source_slot_id, device_id);
    service.stop().expect("service stops cleanly");
}
// //// /验证单人战斗期间不能切换当前存档槽 ////
