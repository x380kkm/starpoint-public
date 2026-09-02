// audience: internal
// # personal-service-player-access-tests
//
// 该文件验证管理员签发的玩家 token 只允许访问被授予的本地存档槽.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use rusqlite::{params, Connection};
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

// //// 验证玩家 token 的存档权限范围 [@x380kkm 2026-07-24] ////
#[test]
fn scopes_player_save_access_and_supports_revoke() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let first = signup(&service, 501);
    let second = signup(&service, 502);
    let first_viewer_id = first.data_headers.viewer_id;
    let second_viewer_id = second.data_headers.viewer_id;

    let first_token = issue_player_token(&service, first_viewer_id);
    let second_token = issue_player_token(&service, second_viewer_id);
    let connection = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let management_token_hash: String = connection
        .query_row(
            "SELECT token_hash FROM management_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("management token digest exists");
    assert_eq!(management_token_hash.len(), 64);
    assert_ne!(management_token_hash, service.management_token());
    let player_token_hash: String = connection
        .query_row(
            "SELECT token_hash FROM player_access_tokens WHERE account_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("player token digest exists");
    assert_eq!(player_token_hash.len(), 64);
    assert_ne!(player_token_hash, first_token);
    drop(connection);
    let invalid = admin_request(
        &service,
        "POST",
        "/v1/player-access",
        Some(&json!({ "viewer_id": 999_999_999 })),
    );
    assert!(invalid.starts_with("HTTP/1.1 404 Not Found"));
    let player_cannot_issue = player_request(
        &service,
        &first_token,
        "POST",
        "/v1/player-access",
        Some(&json!({ "viewer_id": second_viewer_id })),
    );
    assert!(player_cannot_issue.starts_with("HTTP/1.1 401 Unauthorized"));

    let all_saves = response_body(&admin_request(&service, "GET", "/v1/local-saves", None));
    let slots = all_saves["slots"]
        .as_array()
        .expect("admin slots are an array");
    assert_eq!(slots.len(), 2);
    let first_slot_id = slots
        .iter()
        .find(|slot| slot["name"] == "Save 1")
        .and_then(|slot| slot["id"].as_i64())
        .expect("first account slot exists");
    let second_slot_id = slots
        .iter()
        .find(|slot| slot["name"] == "Save 2")
        .and_then(|slot| slot["id"].as_i64())
        .expect("second account slot exists");

    let first_saves = player_request(
        &service,
        &first_token,
        "GET",
        "/v1/player/local-saves",
        None,
    );
    assert!(first_saves.starts_with("HTTP/1.1 200 OK"));
    let first_saves_body = response_body(&first_saves);
    assert_eq!(first_saves_body["slots"].as_array().map(Vec::len), Some(1));
    assert_eq!(first_saves_body["slots"][0]["id"], first_slot_id);

    let second_saves = player_request(
        &service,
        &second_token,
        "GET",
        "/v1/player/local-saves",
        None,
    );
    assert!(second_saves.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(
        response_body(&second_saves)["slots"][0]["id"],
        second_slot_id
    );

    let denied_activation = player_request(
        &service,
        &second_token,
        "POST",
        &format!("/v1/player/local-saves/{first_slot_id}/activate"),
        Some(&json!({ "device_id": 502 })),
    );
    assert!(denied_activation.starts_with("HTTP/1.1 404 Not Found"));

    let denied_export = player_request(
        &service,
        &second_token,
        "GET",
        &format!("/v1/player/local-saves/{first_slot_id}/export"),
        None,
    );
    assert!(denied_export.starts_with("HTTP/1.1 404 Not Found"));

    let exported = player_request(
        &service,
        &first_token,
        "GET",
        &format!("/v1/player/local-saves/{first_slot_id}/export"),
        None,
    );
    assert!(exported.starts_with("HTTP/1.1 200 OK"));
    let mut import = response_body(&exported);
    import["name"] = Value::String("Player copy".to_owned());
    let imported = player_request(
        &service,
        &first_token,
        "POST",
        "/v1/player/local-saves/import",
        Some(&import),
    );
    assert!(imported.starts_with("HTTP/1.1 201 Created"), "{imported}");
    let imported_slot_id = response_body(&imported)["id"]
        .as_i64()
        .expect("imported slot id is numeric");
    assert_ne!(imported_slot_id, first_slot_id);

    let encrypted_export = player_request(
        &service,
        &first_token,
        "GET",
        &format!("/v1/player/local-saves/{first_slot_id}/encrypted-export"),
        None,
    );
    assert!(encrypted_export.starts_with("HTTP/1.1 200 OK"));
    let encrypted_envelope = response_body(&encrypted_export);
    let encrypted_import = player_request(
        &service,
        &first_token,
        "POST",
        "/v1/player/local-saves/import-encrypted",
        Some(&json!({
            "name": "Encrypted copy",
            "envelope": encrypted_envelope.clone(),
        })),
    );
    assert!(
        encrypted_import.starts_with("HTTP/1.1 201 Created"),
        "{encrypted_import}"
    );
    assert_eq!(
        response_body(&encrypted_import)["name"].as_str(),
        Some("Encrypted copy")
    );
    let default_encrypted_import = player_request(
        &service,
        &first_token,
        "POST",
        "/v1/player/local-saves/import-encrypted",
        Some(&json!({
            "name": "",
            "envelope": encrypted_envelope,
        })),
    );
    assert!(
        default_encrypted_import.starts_with("HTTP/1.1 201 Created"),
        "{default_encrypted_import}"
    );
    assert_eq!(
        response_body(&default_encrypted_import)["name"].as_str(),
        Some("Imported save")
    );

    let first_after_import = response_body(&player_request(
        &service,
        &first_token,
        "GET",
        "/v1/player/local-saves",
        None,
    ));
    assert_eq!(
        first_after_import["slots"].as_array().map(Vec::len),
        Some(4)
    );
    let activated = player_request(
        &service,
        &first_token,
        "POST",
        &format!("/v1/player/local-saves/{imported_slot_id}/activate"),
        Some(&json!({ "device_id": 501 })),
    );
    assert!(activated.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(
        response_body(&activated)["devices"]
            .as_array()
            .and_then(|devices| devices.iter().find(|device| device["device_id"] == 501))
            .and_then(|device| device["active_slot_id"].as_i64()),
        Some(imported_slot_id)
    );

    let revoked = admin_request(
        &service,
        "DELETE",
        &format!("/v1/player-access/{first_viewer_id}"),
        None,
    );
    assert!(revoked.starts_with("HTTP/1.1 200 OK"));
    let rejected_after_revoke = player_request(
        &service,
        &first_token,
        "GET",
        "/v1/player/local-saves",
        None,
    );
    assert!(rejected_after_revoke.starts_with("HTTP/1.1 401 Unauthorized"));
    service.stop().expect("service stops cleanly");
}
// //// /验证玩家 token 的存档权限范围 ////

// //// 迁移旧版明文 token 并保留玩家访问 [@x380kkm 2026-07-27] ////
#[test]
fn migrates_legacy_credential_tokens_to_digests() {
    let root = TempDir::new().expect("temporary service directory is created");
    let database_path = root.path().join("personal-service.sqlite3");
    let legacy_management_token = "m".repeat(43);
    let legacy_player_token = "p".repeat(43);
    let connection = Connection::open(&database_path).expect("legacy database opens");
    connection
        .execute_batch(
            "CREATE TABLE management_state (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 token TEXT NOT NULL
             );
             CREATE TABLE accounts (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 app_id TEXT NOT NULL,
                 first_login_time TEXT NOT NULL,
                 idp_alias TEXT NOT NULL,
                 idp_code TEXT NOT NULL,
                 idp_id TEXT NOT NULL UNIQUE,
                 reg_time TEXT NOT NULL,
                 last_login_time TEXT NOT NULL,
                 status TEXT NOT NULL
             );
             CREATE TABLE player_access_tokens (
                 token TEXT PRIMARY KEY NOT NULL,
                 account_id INTEGER NOT NULL UNIQUE,
                 created_at TEXT NOT NULL,
                 FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE CASCADE
             );",
        )
        .expect("legacy token tables are created");
    connection
        .execute(
            "INSERT INTO management_state (id, token) VALUES (1, ?1)",
            params![legacy_management_token],
        )
        .expect("legacy management token is inserted");
    connection
        .execute(
            "INSERT INTO accounts (
                 id, app_id, first_login_time, idp_alias, idp_code, idp_id,
                 reg_time, last_login_time, status
             ) VALUES (1, 'wf_cn', ?1, 'wf_cn:777:android', 'leiting', 'cn:777', ?1, ?1, 'normal')",
            params!["2026-07-27T00:00:00.000Z"],
        )
        .expect("legacy account is inserted");
    connection
        .execute(
            "INSERT INTO player_access_tokens (token, account_id, created_at) VALUES (?1, 1, ?2)",
            params![legacy_player_token, "2026-07-27T00:00:00.000Z"],
        )
        .expect("legacy player token is inserted");
    drop(connection);

    let service = PersonalService::start(root.path(), 0).expect("legacy database migrates");
    assert_ne!(service.management_token(), legacy_management_token);
    let player_access = player_request(
        &service,
        &legacy_player_token,
        "GET",
        "/v1/player/local-saves",
        None,
    );
    assert!(
        player_access.starts_with("HTTP/1.1 200 OK"),
        "{player_access}"
    );

    let connection = Connection::open(&database_path).expect("migrated database opens");
    let management_token_hash: String = connection
        .query_row(
            "SELECT token_hash FROM management_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("management digest exists");
    assert_eq!(management_token_hash.len(), 64);
    assert_ne!(management_token_hash, legacy_management_token);
    let player_token_hash: String = connection
        .query_row(
            "SELECT token_hash FROM player_access_tokens WHERE account_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("player digest exists");
    assert_eq!(player_token_hash.len(), 64);
    assert_ne!(player_token_hash, legacy_player_token);
    service.stop().expect("service stops cleanly");
}
// //// /迁移旧版明文 token 并保留玩家访问 ////
