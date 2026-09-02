// audience: internal
// # personal-service-cn-auth-tests
//
// 该测试使用 CN 客户端的 base64 MessagePack 传输格式验证账号前置协议.

#[path = "support/cn.rs"]
mod cn_support;
mod support;

use cn_support::{
    assert_valid_signup_response, decode_response, encode_request, send_request as send_cn_request,
    send_request_with_resource_version as send_cn_request_with_resource_version, DataHeaders,
    LoadRequest, SignupData, SignupRequest,
};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[derive(Deserialize)]
struct IdentityData {
    #[serde(rename = "idCard")]
    id_card: String,
    age: i64,
    #[serde(rename = "isGuest")]
    is_guest: i64,
    auth: i64,
}

#[derive(Deserialize)]
struct LoginData {
    status: String,
    #[serde(rename = "userId")]
    user_id: String,
    data: IdentityData,
    online_server_check: bool,
    heart_beat_interval: i64,
}

#[derive(Deserialize)]
struct AntiAddictionLimits {
    #[serde(rename = "onlineTime")]
    online_time: i64,
    #[serde(rename = "limitTime")]
    limit_time: i64,
    #[serde(rename = "usableTime")]
    usable_time: i64,
}

#[derive(Deserialize)]
struct AntiAddictionData {
    status: i64,
    message: String,
    data: AntiAddictionLimits,
}

fn send_split_cn_request(port: u16, path: &str, body: &str) -> String {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).expect("service accepts loopback requests");
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .expect("request headers are written");
    stream.flush().expect("request headers are flushed");
    thread::sleep(Duration::from_millis(25));
    stream
        .write_all(body.as_bytes())
        .expect("request body is written");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response is read");
    response
}

fn assert_default_headers(headers: &DataHeaders, viewer_id: i64) {
    assert!(!headers.force_update);
    assert!(!headers.asset_update);
    assert_eq!(headers.short_udid, 0);
    assert_eq!(headers.viewer_id, viewer_id);
    assert!(headers.servertime > 1_700_000_000);
    assert_eq!(headers.result_code, 1);
}

// //// 持久化账号并在重复注册时轮换 viewer 会话 [@x380kkm 2026-07-22] ////
#[test]
fn persists_cn_signup_and_rotates_viewer_session() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    assert!(support::request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    let first_response = send_cn_request(
        service.port(),
        "/api/index.php/tool/signup",
        "3gABqWRldmljZV9pZGU=",
    );
    let first = decode_response::<SignupData>(&first_response);
    assert_valid_signup_response(&first);

    assert_eq!(first.data.new_account, 1);
    assert_eq!(first.data.role_name, "Player1");
    assert_eq!(first.data.account_name, "Player1");
    let first_viewer_id = first.data_headers.viewer_id;
    let first_create_date = first.data.create_date;
    service.stop().expect("service stops cleanly");

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let second_response = send_cn_request(
        service.port(),
        "/api/index.php/tool/signup",
        "3gABqWRldmljZV9pZGU=",
    );
    let second = decode_response::<SignupData>(&second_response);
    assert_valid_signup_response(&second);

    assert_ne!(second.data_headers.viewer_id, first_viewer_id);
    assert_eq!(second.data.new_account, 0);
    assert_eq!(second.data.role_name, "Player1");
    assert_eq!(second.data.create_date, first_create_date);

    let old_viewer_request = encode_request(&LoadRequest {
        keychain: first_viewer_id,
        viewer_id: first_viewer_id,
    });
    let old_viewer_response =
        send_cn_request(service.port(), "/api/index.php/load", &old_viewer_request);
    assert!(old_viewer_response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(old_viewer_response.ends_with("{\"error\":\"invalid_viewer_session\"}"));

    let current_viewer_request = encode_request(&LoadRequest {
        keychain: second.data_headers.viewer_id,
        viewer_id: second.data_headers.viewer_id,
    });
    let current_viewer_response = send_cn_request_with_resource_version(
        service.port(),
        "/api/index.php/load",
        &current_viewer_request,
        "1.4.99-test",
    );
    let loaded = decode_response::<Value>(&current_viewer_response);
    assert!(loaded.data_headers.asset_update);
    assert_eq!(loaded.data_headers.viewer_id, second.data_headers.viewer_id);
    assert!(loaded.data["user_tutorial"].is_null());
    let triggered_tutorials = loaded.data["user_triggered_tutorial"]
        .as_array()
        .expect("triggered tutorials are an array");
    assert_eq!(
        triggered_tutorials
            .iter()
            .map(|value| value.as_i64().expect("tutorial id is numeric"))
            .collect::<Vec<_>>(),
        vec![
            12, 55, 57, 6, 8, 22, 52, 5, 4, 18, 19, 17, 9, 13, 14, 58, 51, 101, 700, 53, 60, 500,
            2, 10, 20, 11, 31, 54, 29, 70,
        ]
    );
    assert_eq!(
        loaded.data["tutorial_gacha"]["character_id"].as_i64(),
        Some(251001),
    );
    assert_eq!(
        loaded.data["quest_progress"]["1"],
        json!([
            {
                "quest_id": 1_001_001,
                "finished": true,
                "unlocked": false,
                "high_score": 0,
                "clear_rank": 5,
                "best_elapsed_time_ms": null,
            },
            {
                "quest_id": 1_001_002,
                "finished": true,
                "unlocked": false,
                "high_score": 57_076,
                "clear_rank": 5,
                "best_elapsed_time_ms": 30_124,
            },
            {
                "quest_id": 1_001_003,
                "finished": true,
                "unlocked": false,
                "high_score": 0,
                "clear_rank": 5,
                "best_elapsed_time_ms": null,
            },
            {
                "quest_id": 1_002_001,
                "finished": true,
                "unlocked": false,
                "high_score": 61_350,
                "clear_rank": 5,
                "best_elapsed_time_ms": 35_700,
            },
        ]),
    );
    assert!(loaded.data["last_main_quest_id"].is_null());
    assert_eq!(loaded.data["user_info"]["free_vmoney"].as_i64(), Some(1500));
    assert_eq!(
        loaded.data["available_asset_version"].as_str(),
        Some("1.4.99-test"),
    );
    let stamina_heal_time = loaded.data["user_info"]["stamina_heal_time"]
        .as_i64()
        .expect("stamina time is numeric");
    assert!(stamina_heal_time <= loaded.data_headers.servertime);
    assert!(stamina_heal_time >= loaded.data_headers.servertime - 60);
    let first_login_time = loaded.data["user_info"]["last_login_time"]
        .as_str()
        .expect("login time is text")
        .to_owned();
    assert_ne!(first_login_time, "2022-05-02 16:33:34");
    let characters = loaded.data["user_character_list"]
        .as_object()
        .expect("character list is an object");
    assert_eq!(characters.len(), 3);
    for character_id in ["1", "251001", "243001"] {
        let character = characters
            .get(character_id)
            .expect("completed tutorial character exists");
        let join_time = character["join_time"]
            .as_i64()
            .expect("join time is numeric");
        let update_time = character["update_time"]
            .as_i64()
            .expect("update time is numeric");
        assert!(join_time <= loaded.data_headers.servertime);
        assert!(join_time >= loaded.data_headers.servertime - 60);
        assert_eq!(update_time, join_time);
    }
    assert_eq!(
        loaded.data["user_party_group_list"]["1"]["list"]["1"]["character_ids"][0].as_i64(),
        Some(1),
    );
    service.stop().expect("service stops cleanly");
}
// //// /持久化账号并在重复注册时轮换 viewer 会话 ////

// //// UI 教程触发后隐藏主教程状态 [@x380kkm 2026-08-24] ////
#[test]
fn hides_main_tutorial_after_triggering_ui_tutorial_12() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup_body = encode_request(&SignupRequest { device_id: 12 });
    let signup_response =
        send_cn_request(service.port(), "/api/index.php/tool/signup", &signup_body);
    let signup = decode_response::<SignupData>(&signup_response);
    let viewer_id = signup.data_headers.viewer_id;
    service.stop().expect("service stops cleanly");

    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("service database is opened");
    let (account_id, serialized) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("one player snapshot exists");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    player_data["user_tutorial"] = json!({
        "skip_flag": false,
        "tutorial_step": 15,
        "viewer_id": 0,
    });
    player_data["user_triggered_tutorial"] = json!([12]);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id,
            ],
        )
        .expect("player snapshot is updated");
    drop(database);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let load_body = encode_request(&LoadRequest {
        keychain: viewer_id,
        viewer_id,
    });
    let load_response = send_cn_request(service.port(), "/api/index.php/load", &load_body);
    let loaded = decode_response::<Value>(&load_response);
    assert!(loaded.data["user_tutorial"].is_null());
    service.stop().expect("service stops cleanly");
}
// //// /UI 教程触发后隐藏主教程状态 ////

// //// 返回雷霆登录兼容数据 [@x380kkm 2026-07-22] ////
#[test]
fn returns_leiting_login_compatibility_data() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let response = send_cn_request(
        service.port(),
        "/api/index.php/channels/channel_leiting/leiting_login",
        "3gABpnVzZXJJZKdjbi11c2Vy",
    );
    let login = decode_response::<LoginData>(&response);

    assert_default_headers(&login.data_headers, 0);
    assert_eq!(login.data.status, "success");
    assert_eq!(login.data.user_id, "cn-user");
    assert_eq!(login.data.data.id_card, "123456");
    assert_eq!(login.data.data.age, 18);
    assert_eq!(login.data.data.is_guest, 0);
    assert_eq!(login.data.data.auth, 1);
    assert!(login.data.online_server_check);
    assert_eq!(login.data.heart_beat_interval, 240);
    service.stop().expect("service stops cleanly");
}
// //// /返回雷霆登录兼容数据 ////

// //// 返回雷霆防沉迷登录兼容数据 [@x380kkm 2026-08-18] ////
#[test]
fn returns_leiting_antiaddiction_login_compatibility_data() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let body = encode_request(&serde_json::json!({}));
    let response = send_cn_request(
        service.port(),
        "/api/index.php/channels/channel_leiting/leiting_antiaddiction_login",
        &body,
    );
    let login = decode_response::<AntiAddictionData>(&response);

    assert_default_headers(&login.data_headers, 0);
    assert_eq!(login.data.status, 0);
    assert_eq!(login.data.message, "success");
    assert_eq!(login.data.data.online_time, 0);
    assert_eq!(login.data.data.limit_time, 999_999);
    assert_eq!(login.data.data.usable_time, 999_999);
    service.stop().expect("service stops cleanly");
}
// //// /返回雷霆防沉迷登录兼容数据 ////

// //// 拒绝无效的 CN 注册正文 [@x380kkm 2026-07-22] ////
#[test]
fn rejects_invalid_cn_signup_body() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let invalid_device = encode_request(&SignupRequest { device_id: 0 });
    let response = send_cn_request(
        service.port(),
        "/api/index.php/tool/signup",
        &invalid_device,
    );

    assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(response.ends_with("{\"error\":\"invalid_device_id\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /拒绝无效的 CN 注册正文 ////

// //// 接收分开发送的 HTTP 头和 MessagePack 正文 [@x380kkm 2026-07-22] ////
#[test]
fn accepts_cn_body_after_request_headers() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let response = send_split_cn_request(
        service.port(),
        "/api/index.php/tool/signup",
        "3gABqWRldmljZV9pZGU=",
    );
    let signup = decode_response::<SignupData>(&response);

    assert_eq!(signup.data.new_account, 1);
    assert_eq!(signup.data.account_name, "Player1");
    service.stop().expect("service stops cleanly");
}
// //// /接收分开发送的 HTTP 头和 MessagePack 正文 ////
