// audience: internal
// # local-save-deletion-tests
//
// HTTP 请求验证删除确认, 活动槽保护以及版本和 AI 快照的关联清理.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use local_save_support::{
    activate_local_save, assert_status, authorized_request, export_local_save, list_local_saves,
    load, response_body, signup, update_slot_player_snapshot,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 通过管理接口创建待操作的存档副本 [@x380kkm 2026-09-20] ////
fn create_save(service: &PersonalService, source_slot_id: i64, name: &str) -> i64 {
    let response = authorized_request(
        service,
        "POST",
        &format!("/v1/local-saves/{source_slot_id}/copy"),
        Some(&json!({"name": name})),
    );
    assert_status(&response, "201 Created");
    response_body(&response)["id"].as_i64().unwrap()
}

// //// 提交指定名称的存档删除确认 [@x380kkm 2026-09-20] ////
fn delete_save(service: &PersonalService, slot_id: i64, name: &str) -> String {
    authorized_request(
        service,
        "DELETE",
        &format!("/v1/local-saves/{slot_id}"),
        Some(&json!({"confirm_name": name})),
    )
}

// //// 删除确认和占用保护保留存档原文 [@x380kkm 2026-09-20] ////
#[test]
fn requires_confirmation_and_preserves_active_or_last_save() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = signup(service.port(), 61);
    load(service.port(), signup.data_headers.viewer_id, "1.4.56");
    let state = list_local_saves(&service);
    let slot = &state["slots"][0];
    let slot_id = slot["id"].as_i64().unwrap();
    let name = slot["name"].as_str().unwrap();
    let original = export_local_save(&service, slot_id)["data"].clone();

    let missing_confirmation = authorized_request(
        &service,
        "DELETE",
        &format!("/v1/local-saves/{slot_id}"),
        None,
    );
    assert_status(&missing_confirmation, "400 Bad Request");
    assert_eq!(
        response_body(&missing_confirmation)["error"],
        "local_save_delete_confirmation_required"
    );
    let wrong_name = delete_save(&service, slot_id, "different save");
    assert_status(&wrong_name, "409 Conflict");
    assert_eq!(
        response_body(&wrong_name)["error"],
        "local_save_delete_confirmation_mismatch"
    );
    let last = delete_save(&service, slot_id, name);
    assert_status(&last, "409 Conflict");
    assert_eq!(response_body(&last)["error"], "local_save_last_slot");
    let copy_id = create_save(&service, slot_id, "保留的副本");
    let active = delete_save(&service, slot_id, name);
    assert_status(&active, "409 Conflict");
    assert_eq!(response_body(&active)["error"], "local_save_active");
    let second_device = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{copy_id}/activate"),
        Some(&json!({"device_id": 62})),
    );
    assert_status(&second_device, "200 OK");
    let another_device_active = delete_save(&service, copy_id, "保留的副本");
    assert_status(&another_device_active, "409 Conflict");
    assert_eq!(
        response_body(&another_device_active)["error"],
        "local_save_active"
    );
    assert_eq!(export_local_save(&service, slot_id)["data"], original);
    assert_eq!(
        list_local_saves(&service)["slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

// //// 清理版本父链和 AI 快照后重启保持删除结果 [@x380kkm 2026-09-20] ////
#[test]
fn deletes_save_dependencies_without_restoring_them_after_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let registered = signup(service.port(), 71);
    load(service.port(), registered.data_headers.viewer_id, "1.4.56");
    let state = list_local_saves(&service);
    let source_id = state["slots"][0]["id"].as_i64().unwrap();
    let source_name = state["slots"][0]["name"].as_str().unwrap();
    let kept_id = create_save(&service, source_id, "保留存档");
    activate_local_save(&service, kept_id, 71);
    let kept_data = export_local_save(&service, kept_id)["data"].clone();

    for value in ["First revision", "Second revision"] {
        update_slot_player_snapshot(root.path(), source_id, |data| {
            data["user_info"]["name"] = Value::from(value);
        });
        export_local_save(&service, source_id);
    }
    let snapshot = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{source_id}/snapshots"),
        Some(&json!({"label":"本地快照"})),
    );
    assert_status(&snapshot, "201 Created");
    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let account_id: i64 = connection
        .query_row(
            "SELECT account_id FROM local_save_slots WHERE id = ?1",
            [source_id],
            |row| row.get(0),
        )
        .unwrap();
    connection.execute(
        "INSERT INTO ai_team_snapshots (id, slot_id, team_index, party_id, source_revision_id, data_json, created_at)
         SELECT 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', slot_id, 0, 1, revision_id, '{}', updated_at
         FROM local_save_heads WHERE slot_id = ?1", [source_id],
    ).unwrap();
    connection
        .execute(
            "INSERT INTO ai_team_snapshot_heads (slot_id, team_index, snapshot_id)
         VALUES (?1, 0, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')",
            [source_id],
        )
        .unwrap();
    let response = delete_save(&service, source_id, source_name);
    assert_status(&response, "200 OK");
    assert_eq!(
        response_body(&response)["slots"].as_array().unwrap().len(),
        1
    );
    for table in [
        "local_save_snapshots",
        "local_save_revisions",
        "ai_team_snapshots",
    ] {
        let count: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE slot_id = ?1"),
                [source_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "{table} still references the removed save");
    }
    let account_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id = ?1",
            [account_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(account_count, 0);
    assert_eq!(export_local_save(&service, kept_id)["data"], kept_data);
    assert_status(
        &delete_save(&service, source_id, source_name),
        "404 Not Found",
    );
    drop(connection);
    drop(service);

    let restarted = PersonalService::start(root.path(), 0).unwrap();
    let state = list_local_saves(&restarted);
    assert_eq!(state["slots"].as_array().unwrap().len(), 1);
    assert_eq!(state["devices"][0]["active_slot_id"], kept_id);
    let signed_in = signup(restarted.port(), 71);
    let loaded = load(restarted.port(), signed_in.data_headers.viewer_id, "1.4.56");
    assert_eq!(
        loaded.data["user_info"]["name"],
        kept_data["user_info"]["name"]
    );
}

// //// 战斗仍占用的非活动存档保持可恢复 [@x380kkm 2026-09-20] ////
#[test]
fn keeps_inactive_save_with_an_unfinished_battle() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let signup = signup(service.port(), 81);
    load(service.port(), signup.data_headers.viewer_id, "1.4.56");
    let source_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let copy_id = create_save(&service, source_id, "战斗存档");
    let connection = Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    connection.execute(
        "INSERT INTO active_single_quests (
             account_id, quest_id, category, use_boss_boost_point, use_boost_point, is_auto_start_mode
         ) SELECT account_id, 1001002, 1, 0, 0, 0 FROM local_save_slots WHERE id = ?1", params![copy_id],
    ).unwrap();
    let response = delete_save(&service, copy_id, "战斗存档");
    assert_status(&response, "409 Conflict");
    assert_eq!(response_body(&response)["error"], "local_save_busy");
    assert_eq!(
        list_local_saves(&service)["slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
