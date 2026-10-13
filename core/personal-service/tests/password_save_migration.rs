// audience: internal
// # password-save-migration-tests
// 独立服务实例验证密码备份, 旧恢复包和首次注册前的本机存档选择.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use local_save_support::{
    assert_status, authorized_request, export_local_save, list_local_saves, load, response_body,
    signup, update_slot_player_snapshot,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn key_state(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let connection = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let mut statement = connection
        .prepare("SELECT key_id, key_bytes FROM local_save_encryption_keys")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn post(service: &PersonalService, path: &str, body: Value, status: &str) -> Value {
    let response = authorized_request(service, "POST", path, Some(&body));
    assert_status(&response, status);
    response_body(&response)
}

// //// 在独立实例和空安装中恢复完整密码存档 [@x380kkm 2026-09-26] ////
#[test]
fn migrates_password_and_legacy_backups_without_changing_target_keys() {
    let source_root = TempDir::new().unwrap();
    let source = PersonalService::start(source_root.path(), 0).unwrap();
    signup(source.port(), 91);
    let source_slot = list_local_saves(&source)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    update_slot_player_snapshot(source_root.path(), source_slot, |data| {
        data["user_info"]["name"] = json!("Migrated save");
        data["user_info"]["bond_token"] = json!(77);
    });
    let password = "portable-save-password";
    let backup = post(
        &source,
        &format!("/v1/local-saves/{source_slot}/password-export"),
        json!({"password": password}),
        "200 OK",
    );
    assert_eq!(backup["format"], "starpoint-password-save");
    assert!(!backup.to_string().contains("Migrated save"));
    let legacy_response = authorized_request(
        &source,
        "GET",
        &format!("/v1/local-saves/{source_slot}/encrypted-export"),
        None,
    );
    assert_status(&legacy_response, "200 OK");
    let legacy = response_body(&legacy_response);
    let recovery = post(
        &source,
        &format!("/v1/local-saves/{source_slot}/recovery-export"),
        json!({"password": password}),
        "200 OK",
    )["package"]
        .clone();

    let target_root = TempDir::new().unwrap();
    let target = PersonalService::start(target_root.path(), 0).unwrap();
    signup(target.port(), 92);
    let existing = list_local_saves(&target)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let existing_data = export_local_save(&target, existing)["data"].clone();
    let encrypted = authorized_request(
        &target,
        "GET",
        &format!("/v1/local-saves/{existing}/encrypted-export"),
        None,
    );
    assert_status(&encrypted, "200 OK");
    let keys = key_state(target_root.path());
    assert_ne!(keys, key_state(source_root.path()));
    let before = list_local_saves(&target);
    post(
        &target,
        "/v1/local-saves/import-password",
        json!({"password": "incorrect-password", "package": backup}),
        "400 Bad Request",
    );
    let mut invalid = backup.clone();
    invalid["ciphertext"] = json!("invalid");
    post(
        &target,
        "/v1/local-saves/import-password",
        json!({"password": password, "package": invalid}),
        "400 Bad Request",
    );
    assert_eq!(list_local_saves(&target), before);
    let restored = post(
        &target,
        "/v1/local-saves/import-password",
        json!({"name": "Restored", "password": password, "package": backup}),
        "201 Created",
    );
    let restored_id = restored["id"].as_i64().unwrap();
    assert_eq!(
        export_local_save(&target, restored_id)["data"]["user_info"]["bond_token"],
        77
    );
    let restored_legacy = post(
        &target,
        "/v1/local-saves/import-encrypted-recovery",
        json!({"password": password, "envelope": legacy, "recovery": recovery}),
        "201 Created",
    );
    assert_eq!(
        export_local_save(&target, restored_legacy["id"].as_i64().unwrap())["data"]["user_info"]
            ["bond_token"],
        77
    );
    assert_eq!(key_state(target_root.path()), keys);
    assert_eq!(export_local_save(&target, existing)["data"], existing_data);
    target.stop().unwrap();

    let fresh_root = TempDir::new().unwrap();
    let fresh = PersonalService::start(fresh_root.path(), 0).unwrap();
    assert!(list_local_saves(&fresh)["slots"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(list_local_saves(&fresh)["devices"]
        .as_array()
        .unwrap()
        .is_empty());
    let fresh_keys = key_state(fresh_root.path());
    let imported = post(
        &fresh,
        "/v1/local-saves/import-password",
        json!({"password": password, "package": backup}),
        "201 Created",
    );
    let imported_id = imported["id"].as_i64().unwrap();
    let selected = post(
        &fresh,
        &format!("/v1/local-saves/{imported_id}/activate-local"),
        json!({}),
        "200 OK",
    );
    assert_eq!(selected["local_slot_id"], imported_id);
    assert!(selected["devices"].as_array().unwrap().is_empty());
    assert_eq!(key_state(fresh_root.path()), fresh_keys);
    fresh.stop().unwrap();
    let fresh = PersonalService::start(fresh_root.path(), 0).unwrap();
    assert_eq!(list_local_saves(&fresh)["local_slot_id"], imported_id);
    let account = signup(fresh.port(), 8675309);
    let loaded = load(
        fresh.port(),
        account.data_headers.viewer_id,
        "1.4.99-password-migration",
    );
    assert_eq!(loaded.data["user_info"]["name"], "Migrated save");
    assert_eq!(loaded.data["user_info"]["bond_token"], 77);
    assert_eq!(
        list_local_saves(&fresh)["slots"].as_array().unwrap().len(),
        1
    );
    fresh.stop().unwrap();
    source.stop().unwrap();
}
