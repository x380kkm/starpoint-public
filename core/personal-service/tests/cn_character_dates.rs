// audience: internal
// # personal-service-cn-character-dates-tests
// 角色操作和请求重放返回客户端可解析的日期, 旧存档的原始时间和资产分别保留.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use std::path::Path;
use tempfile::TempDir;

// //// 读取隔离玩家快照 [@x380kkm 2026-09-30] ////
fn snapshot(root: &Path) -> Value {
    let database = Connection::open(root.join("personal-service.sqlite3")).unwrap();
    let data: String = database
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

// //// 调用真实角色接口并返回成功数据 [@x380kkm 2026-09-30] ////
fn request(service: &PersonalService, path: &str, body: Value) -> Value {
    let response = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        path,
        &encode_request(&body),
    ));
    assert_eq!(response.data_headers.result_code, 1);
    response.data
}

// //// 已学习节点和副本转换统一使用旧角色的实际日期 [@x380kkm 2026-09-30] ////
#[test]
fn formats_legacy_times_on_mana_retry_conversion_and_receipt_replay() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer_id = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 930201 }),
    ))
    .data_headers
    .viewer_id;
    let mut player = snapshot(root.path());
    player["user_character_list"]["1"]["join_time"] = json!("1700000000");
    player["user_character_list"]["1"]["update_time"] = json!("2024-07-20T09:30:00Z");
    player["user_character_list"]["1"]["stack"] = json!(2);
    player["user_character_mana_node_list"]["1"] = json!([2201]);
    Connection::open(root.path().join("personal-service.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE player_snapshots SET data_json=?1",
            [player.to_string()],
        )
        .unwrap();

    let learned = request(
        &service,
        "/api/index.php/character/learn_mana_node",
        json!({
            "viewer_id":viewer_id,"api_count":1,"character_id":1,"mana_node_multiplied_id_list":[2201],
        }),
    );
    let conversion = json!({"viewer_id":viewer_id,"api_count":2,"character_id":1,"number":1});
    let converted = request(
        &service,
        "/api/index.php/expod/stack_to_exp",
        conversion.clone(),
    );
    let replayed = request(&service, "/api/index.php/expod/stack_to_exp", conversion);
    for response in [&learned, &converted, &replayed] {
        let character = &response["character_list"][0];
        assert_eq!(character["join_time"], "2023-11-14 22:13:20");
        assert_eq!(character["update_time"], "2024-07-20 09:30:00");
    }
    let loaded = request(
        &service,
        "/api/index.php/load",
        json!({"viewer_id":viewer_id,"keychain":viewer_id}),
    );
    assert_eq!(
        loaded["user_character_list"]["1"]["join_time"],
        1_700_000_000
    );
    assert_eq!(
        loaded["user_character_list"]["1"]["update_time"],
        1_721_467_800
    );
    let saved = snapshot(root.path());
    assert_eq!(saved["user_character_list"]["1"]["join_time"], "1700000000");
    assert_eq!(
        saved["user_character_list"]["1"]["update_time"],
        "2024-07-20T09:30:00Z"
    );
    assert_eq!(saved["user_character_list"]["1"]["stack"], 1);
    assert_eq!(
        converted["user_info"]["exp_pool"],
        replayed["user_info"]["exp_pool"]
    );
    service.stop().unwrap();
}
