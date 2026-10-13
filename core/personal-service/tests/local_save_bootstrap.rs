// audience: internal
// # local-save-bootstrap-tests
// 导入后的首次登录使用持久化默认槽, 服务重启保留选择.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
#[path = "support/save_sync_server.rs"]
mod save_sync_server;
mod support;

use local_save_support::{
    assert_status, authorized_request, export_local_save, list_local_saves, load, response_body,
    signup,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 生成含玩家身份的可导入存档 [@x380kkm 2026-09-26] ////
fn exported_player_data() -> Value {
    let source_root = TempDir::new().expect("source directory is created");
    let source = PersonalService::start(source_root.path(), 0).expect("source starts");
    signup(source.port(), 841);
    let source_slot = list_local_saves(&source)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let mut data = export_local_save(&source, source_slot)["data"].clone();
    data["user_info"]["name"] = json!("Imported adventurer");
    source.stop().expect("source stops");
    data
}

// //// 从首次导入的存档启动游戏并保留后续导入前的选择 [@x380kkm 2026-09-26] ////
#[test]
fn first_import_is_selected_for_login_after_restart() {
    let data = exported_player_data();
    let root = TempDir::new().expect("target directory is created");
    let service = PersonalService::start(root.path(), 0).expect("target starts");
    let response = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({"name": "Imported save", "data": data})),
    );
    assert_status(&response, "201 Created");
    let slot_id = response_body(&response)["id"].as_i64().unwrap();
    assert_eq!(list_local_saves(&service)["local_slot_id"], slot_id);
    let immediate_login = signup(service.port(), 840);
    assert_eq!(immediate_login.data.new_account, 0);
    assert_eq!(
        load(
            service.port(),
            immediate_login.data_headers.viewer_id,
            "1.4.99-import"
        )
        .data["user_info"]["name"],
        "Imported adventurer",
    );
    service.stop().expect("target stops");

    let service = PersonalService::start(root.path(), 0).expect("target restarts");
    let login = signup(service.port(), 842);
    assert_eq!(login.data.new_account, 0);
    let player = load(
        service.port(),
        login.data_headers.viewer_id,
        "1.4.99-import",
    );
    assert_eq!(player.data["user_info"]["name"], "Imported adventurer");
    let state = list_local_saves(&service);
    assert_eq!(state["slots"].as_array().unwrap().len(), 1);
    assert_eq!(state["devices"][0]["active_slot_id"], slot_id);

    let response = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({"name": "Another save", "data": data})),
    );
    assert_status(&response, "201 Created");
    assert_eq!(list_local_saves(&service)["local_slot_id"], slot_id);
    let login = signup(service.port(), 843);
    assert_eq!(login.data.new_account, 0);
    assert_eq!(
        list_local_saves(&service)["slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    service.stop().expect("target stops");
}

// //// 启动时绑定唯一的未选择导入槽 [@x380kkm 2026-09-26] ////
#[test]
fn startup_selects_a_single_unbound_import() {
    let root = TempDir::new().expect("target directory is created");
    let service = PersonalService::start(root.path(), 0).expect("target starts");
    let response = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({"name": "Imported save", "data": exported_player_data()})),
    );
    assert_status(&response, "201 Created");
    let slot_id = response_body(&response)["id"].as_i64().unwrap();
    service.stop().expect("target stops");
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database
        .execute("DELETE FROM local_save_default", [])
        .unwrap();
    drop(database);

    let service = PersonalService::start(root.path(), 0).expect("target restarts");
    assert_eq!(list_local_saves(&service)["local_slot_id"], slot_id);
    let login = signup(service.port(), 844);
    assert_eq!(login.data.new_account, 0);
    let player = load(
        service.port(),
        login.data_headers.viewer_id,
        "1.4.99-import",
    );
    assert_eq!(player.data["user_info"]["name"], "Imported adventurer");
    assert_eq!(
        list_local_saves(&service)["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    service.stop().expect("target stops");
}

// //// 从密文同步下载直接进入首次登录 [@x380kkm 2026-09-27] ////
#[test]
fn first_synced_import_is_selected_without_restart() {
    let remote = save_sync_server::MockSaveSyncServer::start();
    let source_root = TempDir::new().expect("source directory is created");
    let source = PersonalService::start(source_root.path(), 0).expect("source starts");
    let imported = authorized_request(
        &source,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({"name": "Sync source", "data": exported_player_data()})),
    );
    assert_status(&imported, "201 Created");
    let slot_id = response_body(&imported)["id"].as_i64().unwrap();
    let target_settings = json!({
        "name": "Loopback vault",
        "scheme": "http",
        "host": "127.0.0.1",
        "port": remote.port(),
        "username": "sync-user",
        "password": "sync-password",
    });
    let source_target = authorized_request(
        &source,
        "POST",
        "/v1/save-sync-targets",
        Some(&target_settings),
    );
    assert_status(&source_target, "201 Created");
    let uploaded = authorized_request(
        &source,
        "POST",
        &format!("/v1/local-saves/{slot_id}/sync/upload"),
        Some(&json!({
            "target_id": response_body(&source_target)["id"],
            "object_id": "bootstrap-save",
        })),
    );
    assert_status(&uploaded, "200 OK");
    source.stop().expect("source stops");

    let source_database =
        Connection::open(source_root.path().join("personal-service.sqlite3")).unwrap();
    let key = source_database
        .query_row(
            "SELECT key_id, key_bytes, created_at FROM local_save_encryption_keys WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    let root = TempDir::new().expect("target directory is created");
    let service = PersonalService::start(root.path(), 0).expect("target starts");
    let database = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    database.execute(
        "INSERT INTO local_save_encryption_keys (id, key_id, key_bytes, created_at) VALUES (1, ?1, ?2, ?3)",
        rusqlite::params![key.0, key.1, key.2],
    ).unwrap();
    let target = authorized_request(
        &service,
        "POST",
        "/v1/save-sync-targets",
        Some(&target_settings),
    );
    assert_status(&target, "201 Created");
    let downloaded = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/sync/download",
        Some(&json!({
            "target_id": response_body(&target)["id"],
            "object_id": "bootstrap-save",
            "name": "Restored player",
        })),
    );
    assert_status(&downloaded, "201 Created");
    let imported_slot = response_body(&downloaded)["slot"]["id"].as_i64().unwrap();
    assert_eq!(list_local_saves(&service)["local_slot_id"], imported_slot);
    let login = signup(service.port(), 845);
    assert_eq!(login.data.new_account, 0);
    let player = load(
        service.port(),
        login.data_headers.viewer_id,
        "1.4.99-import",
    );
    assert_eq!(player.data["user_info"]["name"], "Imported adventurer");
    let state = list_local_saves(&service);
    assert_eq!(state["slots"].as_array().unwrap().len(), 1);
    assert_eq!(state["devices"][0]["active_slot_id"], imported_slot);
    service.stop().expect("target stops");
}
