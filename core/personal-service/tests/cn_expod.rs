// audience: internal
// # personal-service-cn-expod-tests
//
// 该文件验证 CN 角色副本转换和 EXP 注入的持久化协议.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, LoadRequest, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

#[derive(Serialize)]
struct StackToExpRequest {
    character_id: i64,
    api_count: i64,
    number: i64,
    viewer_id: i64,
}

#[derive(Serialize)]
struct InjectExpRequest {
    character_id: i64,
    viewer_id: i64,
    exp: i64,
    api_count: i64,
}

// //// 设置 CN expod 测试角色副本数 [@x380kkm 2026-08-23] ////
fn set_character_stack(root: &Path, character_id: i64, stack: i64) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let (account_id, serialized) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("player snapshot exists");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    player_data["user_character_list"][character_id.to_string()]["stack"] = Value::from(stack);
    database
        .execute(
            "UPDATE player_snapshots SET data_json = ?1 WHERE account_id = ?2",
            params![
                serde_json::to_string(&player_data).expect("player snapshot is encoded"),
                account_id,
            ],
        )
        .expect("player snapshot is updated");
}
// //// /设置 CN expod 测试角色副本数 ////

// //// 验证 CN 角色副本转换和 EXP 注入 [@x380kkm 2026-07-24] ////
#[test]
fn converts_character_stack_and_injects_exp() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 41 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    set_character_stack(root.path(), 1, 1);

    let converted = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/expod/stack_to_exp",
        &encode_request(&StackToExpRequest {
            character_id: 1,
            api_count: 1,
            number: 1,
            viewer_id,
        }),
    ));
    assert_eq!(
        converted.data["user_info"]["exp_pool"].as_i64(),
        Some(2_000)
    );
    assert_eq!(
        converted.data["character_list"][0]["stack"].as_i64(),
        Some(0)
    );
    assert_eq!(converted.data["item_list"]["990008"].as_i64(), Some(10));
    assert_eq!(
        converted.data["converted_exp_info"]["add_exp"].as_i64(),
        Some(2_000)
    );

    let injected = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/expod/inject_exp",
        &encode_request(&InjectExpRequest {
            character_id: 1,
            viewer_id,
            exp: 1_000,
            api_count: 1,
        }),
    ));
    assert_eq!(injected.data["user_info"]["exp_pool"].as_i64(), Some(1_000));
    assert_eq!(
        injected.data["add_exp_list"][0]["after_exp"].as_i64(),
        Some(1_010)
    );
    assert_eq!(
        injected.data["character_list"][0]["exp"].as_i64(),
        Some(1_010)
    );

    let insufficient = cn_support::send_request(
        service.port(),
        "/api/index.php/expod/inject_exp",
        &encode_request(&InjectExpRequest {
            character_id: 1,
            viewer_id,
            exp: 2_000,
            api_count: 1,
        }),
    );
    assert!(insufficient.starts_with("HTTP/1.1 400 Bad Request"));

    let loaded = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/load",
        &encode_request(&LoadRequest {
            keychain: viewer_id,
            viewer_id,
        }),
    ));
    assert_eq!(loaded.data["user_info"]["exp_pool"].as_i64(), Some(1_000));
    assert_eq!(
        loaded.data["user_character_list"]["1"]["exp"].as_i64(),
        Some(1_010)
    );
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 角色副本转换和 EXP 注入 ////
