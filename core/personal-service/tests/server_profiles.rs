// audience: internal
// # personal-service-server-profile-tests
//
// 该测试验证管理鉴权, 服务器配置切换, 重启持久化和内置本地配置保护.

use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

mod support;

use support::{request, request_with_headers};

fn authorized_request(
    port: u16,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> String {
    let encoded = body.map_or_else(Vec::new, |value| value.to_string().into_bytes());
    let authorization = format!("Bearer {token}");
    request_with_headers(
        port,
        method,
        path,
        "application/json",
        &[("Authorization", &authorization)],
        &encoded,
    )
}

fn response_body(response: &str) -> Value {
    serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .expect("response contains a body")
            .1,
    )
    .expect("response body is JSON")
}

// //// 管理并持久化多个服务器配置 [@x380kkm 2026-07-23] ////
#[test]
fn manages_server_profiles_across_service_restarts() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let token = service.management_token().to_owned();

    let direct = request(service.port(), "GET", "/v1/server-profiles");
    assert!(direct.starts_with("HTTP/1.1 200 OK"));
    let wrong_token = authorized_request(
        service.port(),
        "wrong-token",
        "GET",
        "/v1/server-profiles",
        None,
    );
    assert!(wrong_token.starts_with("HTTP/1.1 401 Unauthorized"));

    let initial = authorized_request(service.port(), &token, "GET", "/v1/server-profiles", None);
    let initial = response_body(&initial);
    assert_eq!(initial["active_profile_id"].as_i64(), Some(1));
    assert_eq!(initial["profiles"].as_array().map(Vec::len), Some(1));
    assert_eq!(initial["profiles"][0]["mode"], "local");
    assert_eq!(initial["profiles"][0]["is_builtin"], true);

    let remote = json!({
        "name": "Friends",
        "scheme": "http",
        "host": "192.168.1.50",
        "port": 8001,
    });
    let created = authorized_request(
        service.port(),
        &token,
        "POST",
        "/v1/server-profiles",
        Some(&remote),
    );
    assert!(created.starts_with("HTTP/1.1 201 Created"));
    let remote_id = response_body(&created)["id"]
        .as_i64()
        .expect("created profile has an id");

    let duplicate_profile = json!({
        "name": "friends",
        "scheme": "http",
        "host": "192.168.1.51",
        "port": 8001,
    });
    let duplicate = authorized_request(
        service.port(),
        &token,
        "POST",
        "/v1/server-profiles",
        Some(&duplicate_profile),
    );
    assert!(duplicate.starts_with("HTTP/1.1 409 Conflict"));

    let activated = authorized_request(
        service.port(),
        &token,
        "POST",
        &format!("/v1/server-profiles/{remote_id}/activate"),
        None,
    );
    assert_eq!(response_body(&activated)["active_profile_id"], remote_id);
    service.stop().expect("service stops");

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let restarted_token = service.management_token().to_owned();
    assert_ne!(restarted_token, token);
    let old_token = authorized_request(service.port(), &token, "GET", "/v1/server-profiles", None);
    assert!(old_token.starts_with("HTTP/1.1 401 Unauthorized"));
    let restored = authorized_request(
        service.port(),
        &restarted_token,
        "GET",
        "/v1/server-profiles",
        None,
    );
    let restored = response_body(&restored);
    assert_eq!(restored["active_profile_id"], remote_id);
    assert_eq!(restored["profiles"].as_array().map(Vec::len), Some(2));

    let updated = json!({
        "name": "Club",
        "scheme": "https",
        "host": "club.example.test",
        "port": 443,
    });
    let updated_response = authorized_request(
        service.port(),
        &restarted_token,
        "PUT",
        &format!("/v1/server-profiles/{remote_id}"),
        Some(&updated),
    );
    let updated_response = response_body(&updated_response);
    assert_eq!(updated_response["name"], "Club");
    assert_eq!(updated_response["scheme"], "https");
    assert_eq!(updated_response["host"], "club.example.test");

    let delete_active = authorized_request(
        service.port(),
        &restarted_token,
        "DELETE",
        &format!("/v1/server-profiles/{remote_id}"),
        None,
    );
    assert!(delete_active.starts_with("HTTP/1.1 409 Conflict"));

    let activated_local = authorized_request(
        service.port(),
        &restarted_token,
        "POST",
        "/v1/server-profiles/1/activate",
        None,
    );
    assert!(activated_local.starts_with("HTTP/1.1 200 OK"));
    let delete_builtin = authorized_request(
        service.port(),
        &restarted_token,
        "DELETE",
        "/v1/server-profiles/1",
        None,
    );
    assert!(delete_builtin.starts_with("HTTP/1.1 409 Conflict"));
    let update_builtin = authorized_request(
        service.port(),
        &restarted_token,
        "PUT",
        "/v1/server-profiles/1",
        Some(&updated),
    );
    assert!(update_builtin.starts_with("HTTP/1.1 409 Conflict"));

    let deleted = authorized_request(
        service.port(),
        &restarted_token,
        "DELETE",
        &format!("/v1/server-profiles/{remote_id}"),
        None,
    );
    let deleted = response_body(&deleted);
    assert_eq!(deleted["active_profile_id"], 1);
    assert_eq!(deleted["profiles"].as_array().map(Vec::len), Some(1));

    let invalid = json!({
        "name": "Invalid",
        "scheme": "http",
        "host": "http://not-a-host/path",
        "port": 8001,
    });
    let invalid = authorized_request(
        service.port(),
        &restarted_token,
        "POST",
        "/v1/server-profiles",
        Some(&invalid),
    );
    assert!(invalid.starts_with("HTTP/1.1 400 Bad Request"));
}
// //// /管理并持久化多个服务器配置 ////

// //// 从现有个人服务数据库增加服务器配置 [@x380kkm 2026-07-23] ////
#[test]
fn migrates_existing_personal_service_database() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("initial service starts");
    assert!(request(service.port(), "POST", "/v1/state/increment").ends_with("{\"generation\":1}"));
    service.stop().expect("initial service stops");

    let connection = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("existing database opens");
    connection
        .execute_batch(
            "DROP TABLE active_server_profile;
             DROP TABLE server_profiles;
             DROP TABLE management_state;",
        )
        .expect("new management tables are removed to simulate the previous schema");
    drop(connection);

    let service = PersonalService::start(root.path(), 0).expect("existing database migrates");
    assert!(
        request(service.port(), "GET", "/health").ends_with("{\"status\":\"ok\",\"generation\":1}")
    );
    let profiles = authorized_request(
        service.port(),
        service.management_token(),
        "GET",
        "/v1/server-profiles",
        None,
    );
    let profiles = response_body(&profiles);
    assert_eq!(profiles["active_profile_id"], 1);
    assert_eq!(profiles["profiles"].as_array().map(Vec::len), Some(1));
}
// //// /从现有个人服务数据库增加服务器配置 ////
