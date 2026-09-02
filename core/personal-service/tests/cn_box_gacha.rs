// audience: internal
// # personal-service-cn-box-gacha-tests
//
// 该文件验证 CN 箱池目录、活动日历开关和抽取响应契约.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::collections::BTreeSet;
use std::fs;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct BoxListRequest {
    viewer_id: i64,
    box_gacha_id: i64,
}

#[derive(Serialize)]
struct BoxExecRequest {
    viewer_id: i64,
    box_gacha_id: i64,
    box_id: i64,
    number: i64,
    stop_on_featured_rewards: bool,
}

#[derive(Serialize)]
struct BoxResetRequest {
    viewer_id: i64,
    box_gacha_id: i64,
    box_id: i64,
}

// //// 设置 CN 箱池测试货币 [@x380kkm 2026-08-23] ////
fn set_box_currency(root: &TempDir, amount: i64) {
    let database = Connection::open(root.path().join("personal-service.sqlite3"))
        .expect("personal service database opens");
    let (account_id, serialized): (i64, String) = database
        .query_row(
            "SELECT account_id, data_json FROM player_snapshots",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("player snapshot is read");
    let mut player_data =
        serde_json::from_str::<Value>(&serialized).expect("player snapshot is JSON");
    player_data["item_list"]["30101"] = Value::from(amount);
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
// //// /设置 CN 箱池测试货币 ////

// //// 验证 CN 箱池目录和禁用状态 [@x380kkm 2026-08-22] ////
#[test]
fn returns_box_list_and_honors_disabled_activity() {
    let root = TempDir::new().expect("temporary service directory is created");
    let cdn_root = TempDir::new().expect("CN asset root is created");
    fs::write(
        cdn_root.path().join("activity-catalog.json"),
        r#"{
            "format_version": 1,
            "region": "cn",
            "client_version": "1.8.4",
            "asset_version": "1.4.54",
            "generated_at": "2030-01-01T00:00:00Z",
            "activities": [{
                "activity_id": "box-gacha:1",
                "name": "Box Gacha 1",
                "kind": "box-gacha",
                "tags": ["test"],
                "description": "box gacha activity",
                "default_start_at_ms": 1893456000000,
                "default_end_at_ms": 1893715200000
            }]
        }"#,
    )
    .expect("activity manifest is written");
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn_root.path())
        .expect("service starts");
    let signup = decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 71 }),
    ));
    let viewer_id = signup.data_headers.viewer_id;
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let time = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &authorization,
        br#"{"enabled":true,"iso":"2030-01-02T12:00:00.000Z","rate":1.0}"#,
    );
    assert!(time.starts_with("HTTP/1.1 200 OK"));
    let request = BoxListRequest {
        viewer_id,
        box_gacha_id: 1,
    };
    let open = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/get_box_list",
        &encode_request(&request),
    ));
    assert!(!open.data["all_box_info"]
        .as_array()
        .expect("box list is an array")
        .is_empty());

    set_box_currency(&root, 100);
    let draw = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/exec",
        &encode_request(&BoxExecRequest {
            viewer_id,
            box_gacha_id: 1,
            box_id: 1,
            number: 1,
            stop_on_featured_rewards: false,
        }),
    ));
    let draw_keys = draw
        .data
        .as_object()
        .expect("box draw response data is an object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        draw_keys,
        [
            "all_box_info",
            "character_list",
            "drawn_reward_list",
            "equipment_list",
            "item_list",
            "joined_character_id_list",
            "mail_arrived",
            "user_info",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(draw.data["item_list"]["30101"], 90);

    let reset = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/reset",
        &encode_request(&BoxResetRequest {
            viewer_id,
            box_gacha_id: 1,
            box_id: 1,
        }),
    ));
    assert_eq!(reset.data["all_box_info"][0]["reset_times"], 1);
    assert!(reset.data["all_box_info"][0]["all_drawn_reward_list"]
        .as_array()
        .expect("drawn reward list is an array")
        .is_empty());

    let another_pool = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/get_box_list",
        &encode_request(&BoxListRequest {
            viewer_id,
            box_gacha_id: 2,
        }),
    ));
    assert!(!another_pool.data["all_box_info"]
        .as_array()
        .expect("master box list is an array")
        .is_empty());

    let disabled = request_with_headers(
        service.port(),
        "PUT",
        "/v1/activities/calendar/box-gacha:1",
        "application/json",
        &authorization,
        br#"{"enabled":false,"start_at_ms":1893456000000,"end_at_ms":1893715200000}"#,
    );
    assert!(disabled.starts_with("HTTP/1.1 200 OK"));
    let blocked = cn_support::send_request(
        service.port(),
        "/api/index.php/box_gacha/get_box_list",
        &encode_request(&request),
    );
    assert!(blocked.ends_with("{\"error\":\"activity_disabled\"}"));
    service.stop().expect("service stops cleanly");
}
// //// /验证 CN 箱池目录和禁用状态 ////
