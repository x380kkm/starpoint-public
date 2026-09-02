// audience: internal
// # personal-service-transfer-token-tests
// 此文件通过 HTTP 验证本地壳 token 和槽 token 的签发、作用域、上传和撤销.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use local_save_support::{
    assert_status, authorized_request, export_local_save, list_local_saves, response_body, signup,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn token_request(
    service: &PersonalService,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> String {
    let authorization = format!("Bearer {token}");
    let encoded = body.map_or_else(Vec::new, |body| body.to_string().into_bytes());
    support::request_with_headers(
        service.port(),
        method,
        path,
        "application/json",
        &[("Authorization", authorization.as_str())],
        &encoded,
    )
}

// //// 验证本地壳和槽 transfer token 权限边界 [@x380kkm 2026-07-27] ////
#[test]
fn issues_scopes_transfers_and_revokes_local_transfer_tokens() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    signup(service.port(), 91);
    let first_slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup creates a local save slot");

    let shell_issue = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/transfer-tokens/shell"),
        Some(&json!({
            "deviceName": "Local transfer test",
            "expiresAt": "2030-01-01T00:00:00.000Z",
        })),
    );
    assert_status(&shell_issue, "201 Created");
    let shell = response_body(&shell_issue);
    let shell_token = shell["token"]
        .as_str()
        .expect("shell token is returned once")
        .to_owned();
    assert!(shell_token.starts_with("spt_shell_"));
    assert_eq!(shell["instanceId"].as_str().map(str::len), Some(32));

    let shell_list = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{first_slot_id}/transfer-tokens/shell"),
        None,
    );
    assert_status(&shell_list, "200 OK");
    assert!(!shell_list.contains(&shell_token));
    assert_eq!(
        response_body(&shell_list)["tokens"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let shell_slots = token_request(
        &service,
        "GET",
        "/v1/transfer/v1/shell/slots",
        &shell_token,
        None,
    );
    assert_status(&shell_slots, "200 OK");
    assert_eq!(
        response_body(&shell_slots)["slots"][0]["id"].as_i64(),
        Some(first_slot_id)
    );

    let slot_issue = token_request(
        &service,
        "POST",
        "/v1/transfer/v1/shell/slot-tokens",
        &shell_token,
        Some(&json!({
            "slotId": first_slot_id,
            "permission": "download",
            "deviceName": "Download test",
        })),
    );
    assert_status(&slot_issue, "201 Created");
    let slot = response_body(&slot_issue);
    let download_token = slot["token"]
        .as_str()
        .expect("slot token is returned once")
        .to_owned();
    let download_token_id = slot["metadata"]["id"]
        .as_str()
        .expect("slot token has an id")
        .to_owned();
    assert!(download_token.starts_with("spt_slot_"));
    assert_eq!(slot["metadata"]["permission"].as_str(), Some("download"));

    let downloaded = token_request(
        &service,
        "GET",
        &format!("/v1/transfer/v1/slots/{first_slot_id}"),
        &download_token,
        None,
    );
    assert_status(&downloaded, "200 OK");
    let downloaded_package = response_body(&downloaded);
    assert_eq!(
        downloaded_package["format"].as_str(),
        Some("starpoint-save-package")
    );

    let wrong_permission = token_request(
        &service,
        "PUT",
        &format!("/v1/transfer/v1/slots/{first_slot_id}"),
        &download_token,
        Some(&json!({})),
    );
    assert_status(&wrong_permission, "401 Unauthorized");

    signup(service.port(), 92);
    let foreign_slot_id = list_local_saves(&service)["slots"]
        .as_array()
        .expect("local save slots are listed")
        .iter()
        .find(|slot| slot["name"] == "Save 2")
        .and_then(|slot| slot["id"].as_i64())
        .expect("second account slot exists");
    let foreign_slot_issue = token_request(
        &service,
        "POST",
        "/v1/transfer/v1/shell/slot-tokens",
        &shell_token,
        Some(&json!({
            "slotId": foreign_slot_id,
            "permission": "download",
        })),
    );
    assert_status(&foreign_slot_issue, "404 Not Found");

    let copied = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/copy"),
        Some(&json!({ "name": "Transfer scope copy" })),
    );
    assert_status(&copied, "201 Created");
    let copied_slot_id = response_body(&copied)["id"]
        .as_i64()
        .expect("copied slot has an id");
    let shell_slots_after_copy = token_request(
        &service,
        "GET",
        "/v1/transfer/v1/shell/slots",
        &shell_token,
        None,
    );
    assert_status(&shell_slots_after_copy, "200 OK");
    assert_eq!(
        response_body(&shell_slots_after_copy)["slots"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let cross_slot = token_request(
        &service,
        "GET",
        &format!("/v1/transfer/v1/slots/{copied_slot_id}"),
        &download_token,
        None,
    );
    assert_status(&cross_slot, "401 Unauthorized");

    let copied_slot_issue = token_request(
        &service,
        "POST",
        "/v1/transfer/v1/shell/slot-tokens",
        &shell_token,
        Some(&json!({
            "slotId": copied_slot_id,
            "permission": "download",
        })),
    );
    assert_status(&copied_slot_issue, "201 Created");
    let copied_slot_token = response_body(&copied_slot_issue)["token"]
        .as_str()
        .expect("copied slot token is returned")
        .to_owned();
    let copied_slot_download = token_request(
        &service,
        "GET",
        &format!("/v1/transfer/v1/slots/{copied_slot_id}"),
        &copied_slot_token,
        None,
    );
    assert_status(&copied_slot_download, "200 OK");
    assert!(response_body(&copied_slot_issue)["metadata"]
        .get("accountId")
        .is_none());

    let upload_issue = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/transfer-tokens/slot"),
        Some(&json!({ "permission": "upload" })),
    );
    assert_status(&upload_issue, "201 Created");
    let upload_token = response_body(&upload_issue)["token"]
        .as_str()
        .expect("upload token is returned")
        .to_owned();
    let active_quest_database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    active_quest_database
        .execute(
            "INSERT INTO active_single_quests (
                 account_id, quest_id, category, use_boss_boost_point,
                 use_boost_point, is_auto_start_mode
             )
             SELECT account_id, 1, 1, 0, 0, 0
             FROM local_save_slots WHERE id = ?1",
            params![first_slot_id],
        )
        .expect("active single quest is created");
    active_quest_database
        .close()
        .expect("active quest database closes");
    let blocked_upload = token_request(
        &service,
        "PUT",
        &format!("/v1/transfer/v1/slots/{first_slot_id}"),
        &upload_token,
        Some(&downloaded_package),
    );
    assert_status(&blocked_upload, "409 Conflict");
    let clear_quest_database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    clear_quest_database
        .execute(
            "DELETE FROM active_single_quests
             WHERE account_id = (SELECT account_id FROM local_save_slots WHERE id = ?1)",
            params![first_slot_id],
        )
        .expect("active single quest is cleared");
    clear_quest_database
        .close()
        .expect("active quest database closes");
    let uploaded = token_request(
        &service,
        "PUT",
        &format!("/v1/transfer/v1/slots/{first_slot_id}"),
        &upload_token,
        Some(&downloaded_package),
    );
    assert_status(&uploaded, "200 OK");
    assert_eq!(response_body(&uploaded)["imported"].as_bool(), Some(true));
    assert_eq!(
        export_local_save(&service, first_slot_id)["data"],
        downloaded_package["data"],
    );

    let revoke = token_request(
        &service,
        "DELETE",
        &format!("/v1/transfer/v1/shell/slots/{first_slot_id}/tokens/{download_token_id}"),
        &shell_token,
        None,
    );
    assert_status(&revoke, "200 OK");
    let revoked_download = token_request(
        &service,
        "GET",
        &format!("/v1/transfer/v1/slots/{first_slot_id}"),
        &download_token,
        None,
    );
    assert_status(&revoked_download, "401 Unauthorized");

    let expired_issue = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{first_slot_id}/transfer-tokens/slot"),
        Some(&json!({ "permission": "download" })),
    );
    assert_status(&expired_issue, "201 Created");
    let expired_token = response_body(&expired_issue)["token"]
        .as_str()
        .expect("expired token is returned")
        .to_owned();
    let expired_token_id = response_body(&expired_issue)["metadata"]["id"]
        .as_str()
        .expect("expired token has an id")
        .to_owned();
    let expiration_database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    expiration_database
        .execute(
            "UPDATE local_slot_transfer_tokens
             SET expires_at = '2000-01-01T00:00:00.000Z'
             WHERE id = ?1",
            params![expired_token_id],
        )
        .expect("slot token is expired");
    expiration_database
        .close()
        .expect("expiration database closes");
    let expired_download = token_request(
        &service,
        "GET",
        &format!("/v1/transfer/v1/slots/{first_slot_id}"),
        &expired_token,
        None,
    );
    assert_status(&expired_download, "401 Unauthorized");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    let token_hash = database
        .query_row(
            "SELECT token_hash FROM local_slot_transfer_tokens WHERE id = ?1",
            params![download_token_id],
            |row| row.get::<_, String>(0),
        )
        .expect("slot token hash is stored");
    assert_ne!(token_hash, download_token);
    assert!(!token_hash.contains(&download_token));
    database.close().expect("token database closes");
    service.stop().expect("service stops cleanly");
}
// //// /验证本地壳和槽 transfer token 权限边界 ////
